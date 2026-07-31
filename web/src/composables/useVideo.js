// WebRTC viewer. CLASP carries only presence and signaling; the video itself
// flows over a native RTCPeerConnection media track. The robot is the offerer:
// once it sees our presence it sends an offer, we answer, and its camera track
// arrives on `remoteStream`.
//
// Several operators watch the same robot at once. The robot encodes once and
// fans the stream out to everyone, which means it rebuilds its pipeline when
// somebody new joins and sends every viewer a fresh offer. So an offer arriving
// while we already have live video is normal, not an error: we drop the old
// peer connection and negotiate again.
//
// Presence is a heartbeat rather than a one-shot announcement. The robot drops
// viewers it has not heard from, so a closed tab or a dead network frees its
// slot without waiting on an ICE timeout, and a robot that restarts picks
// everyone up again on its own.

import { ref, shallowRef, watch, onUnmounted } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses, SignalKind } from '../protocol.js'

const DEFAULT_ICE = [{ urls: 'stun:stun.l.google.com:19302' }]

// Comfortably inside the robot's viewer timeout, without chattering.
const HELLO_INTERVAL_MS = 3000

export function useVideo(robotId, iceServers = DEFAULT_ICE) {
  const { client, connected, sessionId } = useClasp()

  const remoteStream = shallowRef(null)
  const state = ref('idle') // idle | waiting | connecting | live | failed
  let pc = null
  // The robot's session, remembered past teardown so we can still say goodbye
  // after a failed connection.
  let robotSession = null
  let unsub = null
  let helloTimer = null
  // Candidates that arrived before the offer finished being applied. Signaling
  // callbacks are not awaited, so the robot's trickled ICE can overtake our own
  // setRemoteDescription; adding a candidate before then throws and the
  // candidate is lost, which on some networks is the difference between
  // connecting and not.
  let pendingIce = []

  function addr() {
    return addresses(robotId.value)
  }

  function send(to, message) {
    const c = client.value
    if (!c || !to) return
    c.emit(addr().videoSignal(to), { ...message, from: sessionId.value })
  }

  function closePeer() {
    if (pc) {
      try {
        pc.close()
      } catch {
        // A closed peer connection needs no further handling.
      }
      pc = null
    }
    pendingIce = []
    remoteStream.value = null
  }

  function newPeer() {
    closePeer()
    pc = new RTCPeerConnection({ iceServers })
    const mine = pc
    pc.ontrack = (event) => {
      if (pc !== mine) return // a later negotiation already replaced us
      remoteStream.value = event.streams[0] || new MediaStream([event.track])
      state.value = 'live'
    }
    pc.onicecandidate = (event) => {
      if (pc === mine && event.candidate && robotSession) {
        send(robotSession, {
          kind: SignalKind.Ice,
          candidate: event.candidate.candidate,
          sdpMLineIndex: event.candidate.sdpMLineIndex ?? 0,
        })
      }
    }
    pc.onconnectionstatechange = () => {
      if (pc !== mine) return
      if (pc.connectionState === 'failed' || pc.connectionState === 'disconnected') {
        // Say nothing to the robot: it sees the same failure and frees our
        // slot. Go back to waiting so the heartbeat re-establishes.
        state.value = 'waiting'
        closePeer()
      }
    }
    return pc
  }

  async function onOffer(message) {
    state.value = 'connecting'
    const peer = newPeer()
    robotSession = message.from
    try {
      await peer.setRemoteDescription({ type: 'offer', sdp: message.sdp })
      const answer = await peer.createAnswer()
      await peer.setLocalDescription(answer)
    } catch (err) {
      console.warn('failed to answer offer', err)
      state.value = 'waiting'
      closePeer()
      return
    }
    if (pc !== peer) return // superseded while we were negotiating
    send(robotSession, { kind: SignalKind.Answer, sdp: peer.localDescription.sdp })
    await flushPendingIce(peer)
  }

  async function flushPendingIce(peer) {
    const queued = pendingIce
    pendingIce = []
    for (const candidate of queued) {
      if (pc !== peer) return
      await addCandidate(peer, candidate)
    }
  }

  async function onIce(message) {
    if (!pc) return
    const candidate = {
      candidate: message.candidate,
      sdpMLineIndex: message.sdpMLineIndex ?? 0,
    }
    if (!pc.remoteDescription) {
      pendingIce.push(candidate)
      return
    }
    await addCandidate(pc, candidate)
  }

  async function addCandidate(peer, candidate) {
    try {
      await peer.addIceCandidate(candidate)
    } catch (err) {
      console.warn('failed to add ICE candidate', err)
    }
  }

  async function handleSignal(value) {
    if (!value || typeof value !== 'object') return
    if (value.from && value.from === sessionId.value) return // ignore our echoes
    switch (value.kind) {
      case SignalKind.Offer:
        await onOffer(value)
        break
      case SignalKind.Ice:
        await onIce(value)
        break
      case SignalKind.Bye:
        state.value = 'waiting'
        closePeer()
        break
    }
  }

  function hello() {
    const c = client.value
    if (!c || !sessionId.value) return
    c.emit(addr().videoHello, { session: sessionId.value, role: 'viewer' })
  }

  // Best effort: tells the robot to drop us now rather than after a timeout, so
  // the next viewer does not wait on us.
  function sayGoodbye() {
    if (!robotSession) return
    send(robotSession, { kind: SignalKind.Bye })
  }

  function start() {
    stop()
    const c = client.value
    if (!c || !connected.value || !sessionId.value || !robotId.value) return
    state.value = 'waiting'
    // Listen for offers/ICE addressed to us.
    unsub = c.on(addr().videoSignal(sessionId.value), (value) => {
      handleSignal(value)
    })
    hello()
    helloTimer = setInterval(hello, HELLO_INTERVAL_MS)
  }

  function stop() {
    if (helloTimer) {
      clearInterval(helloTimer)
      helloTimer = null
    }
    sayGoodbye()
    if (unsub) {
      try {
        unsub()
      } catch {
        // best-effort
      }
      unsub = null
    }
    closePeer()
    robotSession = null
    state.value = 'idle'
  }

  // A closed tab never runs onUnmounted. pagehide is the one event that fires
  // for every way a page goes away, including the back/forward cache.
  const onPageHide = () => sayGoodbye()
  window.addEventListener('pagehide', onPageHide)

  watch([connected, sessionId, robotId], start, { immediate: true })
  onUnmounted(() => {
    window.removeEventListener('pagehide', onPageHide)
    stop()
  })

  return { remoteStream, state, start, stop }
}
