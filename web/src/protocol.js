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
    telPattern: `${base}/tel/**`,
    videoHello: `${base}/video/hello`,
    videoSignal: (session) => `${base}/video/signal/${session}`,
  }
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
