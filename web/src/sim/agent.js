// The robot half of the protocol, in a browser tab.
//
// It answers everything the real agent answers: latched status, the driving
// lease, the e-stop, viewer presence, motor telemetry, and WebRTC offers
// carrying a real video track captured from a canvas. From the console's side
// it is indistinguishable from the Pi, minus the Pi.
//
// This is a test double, not a specification. `robot/src/control.rs` and
// `robot/src/video.rs` are the real thing; where the two disagree, this file is
// the one that is wrong.

import clasp from '@clasp-to/sdk'
import { addresses, SignalKind, PROTOCOL_VERSION } from '../protocol.js'

// Mirrors of the constants in the Rust agent. Keep them in step.
const LEASE_MS = 1500
const VIEWER_TIMEOUT_MS = 20000
const MAX_VIEWERS = 4

// Telemetry cadence. Counted in simulation ticks rather than milliseconds
// because the worker owns the clock; at 20 Hz, every fourth tick is 5 Hz, the
// rate the robot publishes at.
const TICKS_PER_TELEMETRY = 4

const ICE = [{ urls: 'stun:stun.l.google.com:19302' }]

export async function startAgent({ robotId, url, token, stream, onDemand, onEstop, onState }) {
  const addr = addresses(robotId)
  const client = await clasp(url, {
    name: `sim-${robotId}`,
    token: token || undefined,
    reconnect: true,
  })
  // Read live, never captured: the relay issues a new session on every
  // reconnect, and signalling addressed to the old one goes nowhere. The real
  // agent had exactly this bug.
  const me = () => client.session

  // Arbitration, the same rules as robot/src/control.rs.
  let driver = null // { session, name }
  let touched = 0
  let publishedDriver = undefined

  let estopped = false
  let wheels = { left: 0, right: 0 }
  let ticks = 0

  const viewers = new Map() // session -> { seen, pc }
  let publishedViewers = -1

  await client.set(addr.status('online'), true)
  await client.set(addr.status('mode'), 'manual')
  await client.set(addr.status('estop'), false)
  await client.set(addr.status('protocol'), PROTOCOL_VERSION)
  await client.set(addr.status('driver'), null)
  await client.set(addr.status('viewers'), 0)

  function publishDriver() {
    const key = driver ? `${driver.session}:${driver.name}` : null
    if (key === publishedDriver) return
    publishedDriver = key
    client.set(addr.status('driver'), driver ? { ...driver } : null)
  }

  function expireLease() {
    if (driver && Date.now() - touched >= LEASE_MS) {
      driver = null
      publishDriver()
    }
  }

  // Returns whether this drive command should be obeyed, taking a free wheel
  // on the sender's behalf and naming it from the command itself.
  function accepts(sender, name) {
    expireLease()
    if (!driver) {
      driver = {
        session: sender,
        name: (name || '').trim().slice(0, 32) || 'operator',
      }
      touched = Date.now()
      publishDriver()
      return true
    }
    if (driver.session === sender) {
      touched = Date.now()
      return true
    }
    return false
  }

  const unsubs = []

  unsubs.push(
    client.on(addr.cmdPattern, (value, address) => {
      if (address === addr.drive) {
        if (!accepts(value?.session || '', value?.name)) return
        // Straight through to the simulation, which applies the mix and the
        // drive watchdog on its own clock.
        onDemand?.(num(value?.throttle), num(value?.steer))
      } else if (address === addr.estop) {
        // Never arbitrated, like the real robot.
        estopped = Boolean(value)
        onEstop?.(estopped)
        client.set(addr.status('estop'), estopped)
      } else if (address === addr.control) {
        if (value?.action === 'claim' && value.session) {
          driver = {
            session: value.session,
            name: (value.name || '').trim().slice(0, 32) || 'operator',
          }
          touched = Date.now()
          publishDriver()
        } else if (value?.action === 'release' && driver?.session === value.session) {
          driver = null
          publishDriver()
        }
      }
    }),
  )

  unsubs.push(
    client.on(addr.videoHello, (value) => {
      const viewer = value?.session
      if (!viewer) return
      const known = viewers.get(viewer)
      if (known) {
        known.seen = Date.now()
        return
      }
      if (viewers.size >= MAX_VIEWERS) return
      viewers.set(viewer, { seen: Date.now(), pc: null })
      offerTo(viewer)
    }),
  )

  unsubs.push(
    client.on(`${addr.base}/video/signal/**`, (value, address) => {
      const session = me()
      if (address.slice(address.lastIndexOf('/') + 1) !== session) return
      if (!value || value.from === session) return
      const entry = viewers.get(value.from)
      switch (value.kind) {
        case SignalKind.Answer:
          entry?.pc?.setRemoteDescription({ type: 'answer', sdp: value.sdp }).catch(warn)
          break
        case SignalKind.Ice:
          entry?.pc
            ?.addIceCandidate({
              candidate: value.candidate,
              sdpMLineIndex: value.sdpMLineIndex ?? 0,
            })
            .catch(warn)
          break
        case SignalKind.Bye:
          dropViewer(value.from)
          break
      }
    }),
  )

  // The robot is the offerer, exactly as in the Rust agent: a viewer says hello
  // and gets an offer back. Every viewer gets its own peer connection carrying
  // the same canvas track, which is the browser's version of one encoder
  // feeding a tee.
  async function offerTo(viewer) {
    const entry = viewers.get(viewer)
    if (!entry) return
    const pc = new RTCPeerConnection({ iceServers: ICE })
    entry.pc = pc

    for (const track of stream.getVideoTracks()) pc.addTrack(track, stream)

    pc.onicecandidate = (event) => {
      if (!event.candidate) return
      client.emit(addr.videoSignal(viewer), {
        kind: SignalKind.Ice,
        from: me(),
        candidate: event.candidate.candidate,
        sdpMLineIndex: event.candidate.sdpMLineIndex ?? 0,
      })
    }
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed' || pc.connectionState === 'closed') dropViewer(viewer)
      report()
    }

    try {
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      client.emit(addr.videoSignal(viewer), {
        kind: SignalKind.Offer,
        from: me(),
        sdp: pc.localDescription.sdp,
      })
    } catch (err) {
      warn(err)
      dropViewer(viewer)
    }
    report()
  }

  function dropViewer(viewer) {
    const entry = viewers.get(viewer)
    if (!entry) return
    try {
      entry.pc?.close()
    } catch {
      // Closing a dead peer needs no handling.
    }
    viewers.delete(viewer)
    report()
  }

  // Driven by the simulation's clock rather than a timer of our own: a timer
  // here would be throttled to once a second whenever this page is in the
  // background, which would stall telemetry and stretch the driving lease.
  function tick(nextWheels) {
    wheels = nextWheels
    expireLease()

    if (++ticks % TICKS_PER_TELEMETRY === 0) {
      client.stream(addr.tel('motors'), wheels)

      for (const [viewer, entry] of viewers) {
        if (Date.now() - entry.seen > VIEWER_TIMEOUT_MS) dropViewer(viewer)
      }
      if (viewers.size !== publishedViewers) {
        publishedViewers = viewers.size
        client.set(addr.status('viewers'), viewers.size)
      }
      report()
    }
  }

  function report() {
    onState?.({
      session: me(),
      driver,
      estopped,
      wheels,
      viewers: [...viewers.keys()],
      peers: [...viewers.values()].map((v) => v.pc?.connectionState || 'new'),
    })
  }

  report()

  return {
    get session() {
      return me()
    },
    tick,
    async stop() {
      for (const viewer of [...viewers.keys()]) dropViewer(viewer)
      for (const un of unsubs) {
        try {
          un?.()
        } catch {
          // best effort
        }
      }
      try {
        await client.set(addr.status('online'), false)
        await client.set(addr.status('driver'), null)
        await client.set(addr.status('viewers'), 0)
        await client.close?.()
      } catch {
        // Shutting down a dead socket is not worth surfacing.
      }
    },
  }
}

function num(v) {
  const n = Number(v)
  return Number.isFinite(n) ? n : 0
}

function warn(err) {
  console.warn('[sim]', err)
}
