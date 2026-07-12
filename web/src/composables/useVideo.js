// WebRTC viewer. CLASP carries only presence and signaling; the video itself
// flows over a native RTCPeerConnection media track. The robot is the offerer:
// once it sees our presence it sends an offer, we answer, and its camera track
// arrives on `remoteStream`.

import { ref, shallowRef, watch, onUnmounted } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses, SignalKind } from '../protocol.js'

const DEFAULT_ICE = [{ urls: 'stun:stun.l.google.com:19302' }]

export function useVideo(robotId, iceServers = DEFAULT_ICE) {
  const { client, connected, sessionId } = useClasp()

  const remoteStream = shallowRef(null)
  const state = ref('idle') // idle | waiting | connecting | live | failed
  let pc = null
  let robotSession = null
  let unsub = null
  let helloTimer = null

  function addr() {
    return addresses(robotId.value)
  }

  function send(to, message) {
    const c = client.value
    if (!c || !to) return
    c.emit(addr().videoSignal(to), { ...message, from: sessionId.value })
  }

  function teardown() {
    if (pc) {
      try {
        pc.close()
      } catch {
        // A closed peer connection needs no further handling.
      }
      pc = null
    }
    robotSession = null
    remoteStream.value = null
  }

  function newPeer() {
    teardown()
    pc = new RTCPeerConnection({ iceServers })
    pc.ontrack = (event) => {
      remoteStream.value = event.streams[0] || new MediaStream([event.track])
      state.value = 'live'
    }
    pc.onicecandidate = (event) => {
      if (event.candidate && robotSession) {
        send(robotSession, {
          kind: SignalKind.Ice,
          candidate: event.candidate.candidate,
          sdpMLineIndex: event.candidate.sdpMLineIndex ?? 0,
        })
      }
    }
    pc.onconnectionstatechange = () => {
      if (!pc) return
      if (pc.connectionState === 'failed' || pc.connectionState === 'disconnected') {
        state.value = 'failed'
      }
    }
    return pc
  }

  async function onOffer(message) {
    state.value = 'connecting'
    // newPeer() tears down any prior peer and clears robotSession, so set it
    // after building the fresh connection.
    const peer = newPeer()
    robotSession = message.from
    await peer.setRemoteDescription({ type: 'offer', sdp: message.sdp })
    const answer = await peer.createAnswer()
    await peer.setLocalDescription(answer)
    send(robotSession, { kind: SignalKind.Answer, sdp: answer.sdp })
  }

  async function onIce(message) {
    if (!pc) return
    try {
      await pc.addIceCandidate({
        candidate: message.candidate,
        sdpMLineIndex: message.sdpMLineIndex ?? 0,
      })
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
        teardown()
        break
    }
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
    // Say hello until we have a live stream. Repeating handles a robot that
    // starts after us, a lost hello, or a robot restart mid-session.
    const hello = () => {
      if (state.value === 'live') return
      c.emit(addr().videoHello, { session: sessionId.value, role: 'viewer' })
    }
    hello()
    helloTimer = setInterval(hello, 2000)
  }

  function stop() {
    if (helloTimer) {
      clearInterval(helloTimer)
      helloTimer = null
    }
    if (unsub) {
      try {
        unsub()
      } catch {
        // best-effort
      }
      unsub = null
    }
    teardown()
    state.value = 'idle'
  }

  watch([connected, sessionId, robotId], start, { immediate: true })
  onUnmounted(stop)

  return { remoteStream, state, start, stop }
}
