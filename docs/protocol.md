# CLASP protocol contract

Both the robot (`robot/src/protocol.rs`) and the web console (`web/src/protocol.js`)
implement this contract. Keep them in sync.

All addresses are rooted at `/robot/<robot_id>`. The robot subscribes to its
command and config subtrees; the operator subscribes to status, telemetry, and
its own video signaling address.

## Signal type choice

CLASP distinguishes Params (stateful, persisted, re-sent to late joiners),
Events (confirmed, one-shot), and Streams (best-effort, high rate). The rule
applied here: anything the UI must render correctly the instant it connects is a
Param; anything transient or high rate is an Event or Stream. Safety never rides
a single lossy frame.

| Address | Type | Payload | Notes |
|---|---|---|---|
| `cmd/drive` | Stream | `{ throttle, steer, seq, ts, session }` | 10 to 20 Hz teleop. Lossy is fine because it is continuous and watchdog-backed. Obeyed only from the session holding the wheel. |
| `cmd/estop` | Param | `bool` | Latched. `true` holds the motors stopped. Re-syncs to a late-joining second operator. Never arbitrated: any operator may stop the robot. |
| `cmd/control` | Event | `ControlCommand` | Claim or release the driving lease. |
| `cfg/max_speed` | Param | `number` 0..1 | Speed ceiling. Survives reconnect. |
| `status/protocol` | Param | `number` | Wire contract version, currently 2. Absent on robots predating multi-operator support. |
| `status/online` | Param | `bool` | Set true on connect, false on clean shutdown. |
| `status/mode` | Param | `string` | Current drive mode. |
| `status/estop` | Param | `bool` | Robot's mirror of the e-stop state, for the console. |
| `status/driver` | Param | `Driver` or null | Who holds the wheel. Null when it is free. |
| `status/viewers` | Param | `number` | How many operators are currently watching. |
| `status/battery` | Param | `number` 0..1 | Optional. Rendered when present. |
| `tel/motors` | Stream | `{ left, right }` | Applied wheel demand, ~5 Hz. |
| `video/hello` | Event | `{ session, role }` | A viewer's presence heartbeat, repeated every few seconds for as long as it wants video. |
| `video/signal/<session>` | Event | `SignalMessage` | SDP/ICE, keyed by recipient session. |

## DriveCommand

```json
{ "throttle": 0.5, "steer": -0.2, "seq": 1234, "ts": 1717000000000, "session": "..." }
```

`throttle` and `steer` are each normalized to -1..1. Positive `steer` turns the
robot to its right. `seq` is monotonic per operator; `ts` is the operator's send
time in milliseconds. `session` is the sender's CLASP session, which the robot
checks against the driving lease before the command reaches the motors.

## Control and the driving lease

Any number of operators may watch. Exactly one may drive. The robot owns that
decision and publishes it as `status/driver`; consoles render the robot's answer
rather than deciding locally, so two consoles can never both believe they are
driving.

```json
{ "action": "claim",   "session": "...", "name": "Ada" }
{ "action": "release", "session": "..." }
```

```json
// status/driver
{ "session": "...", "name": "Ada" }
```

The rules:

- A claim always succeeds, displacing the current holder. Taking over from
  someone is a social problem, not a protocol one, and on a shared robot a lease
  nobody can break is worse than an occasional rude handoff.
- Driving while the wheel is free claims it implicitly, so a lone operator never
  has to ask.
- The lease lapses after 8 seconds without a drive command from its holder. A
  console that holds the wheel without driving re-sends its claim every few
  seconds, so an open console keeps control while a closed one frees it.
- The e-stop is not arbitrated at all.

`status/protocol` is what tells a console the robot arbitrates. When it is
absent the robot is an older single-viewer build that ignores `cmd/control`
entirely, and the console must let whoever is present drive rather than waiting
for a lease that will never be granted.

## SignalMessage

A tagged union keyed by `kind`, always carrying `from` (the sender's CLASP
session id) so a peer can reply and can ignore echoes of its own messages.

```json
{ "kind": "offer",  "from": "<session>", "sdp": "..." }
{ "kind": "answer", "from": "<session>", "sdp": "..." }
{ "kind": "ice",    "from": "<session>", "candidate": "...", "sdpMLineIndex": 0 }
{ "kind": "bye",    "from": "<session>" }
```

The robot is the offerer. A viewer emits a `hello` Event and keeps repeating it
every few seconds for as long as it wants video; the robot sends an `offer` to
`video/signal/<viewerSession>`. The viewer replies with an `answer` and both
trickle `ice` candidates to each other's signaling address. Media flows over the
resulting native WebRTC track, never over CLASP. On its way out a viewer sends
`bye` so its slot frees immediately rather than after a timeout.

Up to four viewers are served at once. The robot captures and encodes once and
fans the encoded stream out to one WebRTC peer each, so the expensive half of
the work does not scale with the audience.

Adding a viewer rebuilds the pipeline, which briefly interrupts the people
already watching. That is deliberate: splicing a branch into a live GStreamer
pipeline risks wedging the streaming thread and taking the camera down for
everyone, while a rebuild cannot get stuck and, as a bonus, makes the new
encoder emit a keyframe immediately so the joiner sees a picture at once. A
viewer *leaving* does not rebuild anything.

`hello` is an Event rather than a persistent presence Param on purpose. A
persistent per-session Param would accumulate stale entries (every past console
session), and the robot would try to start a stream for each ghost on connect.
An Event is never re-sent to late joiners, so the robot only ever sees live
viewers. Because it repeats, it doubles as the liveness signal: a viewer the
robot has not heard from in 20 seconds is dropped, which is how a closed tab
frees its slot without waiting on an ICE timeout, and it is also what lets every
viewer recover on its own after a robot restart.

## Safety model

Motion safety lives on the robot, not in the transport. Drive commands are
continuous Streams; if none arrives within the watchdog window (default 400 ms)
the motors coast. The e-stop is a latched Param, so losing the relay leaves the
robot stopped rather than holding its last command, and a reconnecting operator
sees the true state immediately.

Arbitration is part of that model. A command from an operator who does not hold
the wheel is dropped at the link layer, before it can reach the motors or reset
the watchdog, so a second console cannot fight the driver for the robot. The
e-stop deliberately sits outside all of it: the person who can see the robot
about to hit something is not always the person holding the wheel.
