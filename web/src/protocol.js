// CLASP address contract shared with the robot. This mirrors robot/src/protocol.rs.
// Keep the two in sync: the same paths, the same payload shapes.

/// Wire contract version this console speaks. A robot publishes its own as the
/// `status/protocol` Param; when that is missing the robot predates
/// multi-operator support, serves one viewer, and ignores control messages, so
/// the console must not gate driving on a lease it will never be granted.
export const PROTOCOL_VERSION = 2

export function addresses(robotId) {
  const base = `/robot/${robotId}`
  return {
    base,
    cmdPattern: `${base}/cmd/**`,
    drive: `${base}/cmd/drive`,
    estop: `${base}/cmd/estop`,
    control: `${base}/cmd/control`,
    cfg: (name) => `${base}/cfg/${name}`,
    cfgPattern: `${base}/cfg/**`,
    status: (name) => `${base}/status/${name}`,
    statusPattern: `${base}/status/**`,
    // The console only ever subscribes to telemetry, but the simulator
    // publishes it, and both speak the same contract.
    tel: (name) => `${base}/tel/${name}`,
    telPattern: `${base}/tel/**`,
    videoHello: `${base}/video/hello`,
    videoSignal: (session) => `${base}/video/signal/${session}`,
    // The encoded stream, published once and fanned out by the relay. See
    // "Proving the link works" and the broadcast section in docs/protocol.md.
    videoBroadcast: `${base}/video/broadcast`,
    // The robot's own liveness token. The console reads it to tell a robot that
    // is talking from one that is also listening.
    health: `${base}/health`,
  }
}

// Role a console asks for in its `video/hello`. A peer connection is the
// low-latency path and the robot only has a few; the broadcast costs the robot
// nothing per viewer, so everyone who is not driving should be on it.
export const ViewerRole = {
  Peer: 'viewer',
  Broadcast: 'broadcast',
}

// Signal message kinds exchanged on the video/signal path. Mirrors SignalMessage
// in the Rust protocol: a tagged union keyed by `kind`, with `from` carrying the
// sender's session id.
export const SignalKind = {
  Offer: 'offer',
  Answer: 'answer',
  Ice: 'ice',
  Bye: 'bye',
}

// Actions on cmd/control. Mirrors ControlCommand in the Rust protocol.
export const ControlAction = {
  Claim: 'claim',
  Release: 'release',
}
