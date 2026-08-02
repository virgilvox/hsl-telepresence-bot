# CLASP protocol contract

Both the robot (`robot/src/protocol.rs`) and the web console (`web/src/protocol.js`)
implement this contract. Keep them in sync.

All addresses are rooted at `/robot/<robot_id>`. The robot subscribes to its
command and config subtrees; the operator subscribes to status, telemetry, and
the video broadcast.

## Signal type choice

CLASP distinguishes Params (stateful, persisted, re-sent to late joiners),
Events (confirmed, one-shot), and Streams (best-effort, high rate). The rule
applied here: anything the UI must render correctly the instant it connects is a
Param; anything transient or high rate is an Event or Stream. Safety never rides
a single lossy frame.

| Address | Type | Payload | Notes |
|---|---|---|---|
| `cmd/drive` | Stream | `{ throttle, steer, seq, ts, session, name }` | Sent the moment the demand changes, then repeated every 150 ms for as long as the robot is moving. Lossy is fine because it repeats and is watchdog-backed. Obeyed only from the session holding the wheel. |
| `cmd/estop` | Param | `bool` | Latched. `true` holds the motors stopped. Re-syncs to a late-joining second operator. Never arbitrated: any operator may stop the robot. |
| `cmd/control` | Event | `ControlCommand` | Take the wheel from whoever holds it, or give it up. Not needed to start driving. |
| `cfg/max_speed` | Param | `number` 0..1 | Speed ceiling. Survives reconnect. |
| `status/protocol` | Param | `number` | Wire contract version, currently 2. Absent on robots predating multi-operator support. |
| `status/online` | Param | `bool` | Set true on connect, false on clean shutdown. |
| `status/mode` | Param | `string` | Current drive mode. |
| `status/estop` | Param | `bool` | Robot's mirror of the e-stop state, for the console. |
| `status/driver` | Param | `Driver` or null | Who holds the wheel. Null when it is free. |
| `status/viewers` | Param | `number` | How many operators are currently watching. |
| `status/battery` | Param | `number` 0..1 | Optional. Rendered when present. |
| `tel/motors` | Stream | `{ left, right }` | Applied wheel demand, ~5 Hz. |
| `video/hello` | Event | `{ session, role }` | A viewer's presence heartbeat, repeated every few seconds for as long as it wants video. `role` is read and ignored; see "One stream, no per-viewer state". |
| `video/signal/<session>` | Event | `SignalMessage` | SDP/ICE, keyed by recipient session. |
| `health` | Param | `string` | The robot's own liveness token, written and read back by itself. Consoles ignore it. See "Proving the link works". |

## DriveCommand

```json
{ "throttle": 0.5, "steer": -0.2, "seq": 1234, "ts": 1717000000000, "session": "...", "name": "Ada" }
```

`throttle` and `steer` are each normalized to -1..1. Positive `steer` turns the
robot to its right. `seq` is monotonic per operator; `ts` is the operator's send
time in milliseconds. `session` is the sender's CLASP session, which the robot
checks against the driving lease before the command reaches the motors. `name`
rides along so that taking a free wheel by simply driving still names the
driver for everyone else's console; sending it separately would race the drive
itself.

### Why this is a repeating stream and not an event per press

Sending only on press and release is the obvious design, and it is not safe on
this transport. `cmd/drive` is a Stream, best-effort by contract, so a dropped
release leaves the robot holding its last velocity. Worse, no release is ever
sent at all when an operator's network drops, their machine sleeps, or their
tab dies mid-drive. The watchdog coasting on silence is the only thing that
covers those cases, and it can only do that if silence means something.

So the console sends the demand the instant it changes, which is what makes the
controls feel immediate, and then repeats it every 150 ms for as long as the
robot is moving, purely so that going quiet is meaningful. A release sends one
zero frame and then nothing at all. Holding a key steady is a handful of
messages a second rather than a continuous sampling of the input.

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

- Driving while the wheel is free claims it implicitly, named from the drive
  command. Nobody ever asks for permission.
- The lease lapses 1.5 seconds after the holder's last drive command, which is
  about the length of a pause between deliberate movements. An operator who is
  driving renews it many times over; one who stops frees it almost at once. In
  practice the wheel belongs to whoever is currently driving, and taking turns
  needs no buttons at all.
- A claim still succeeds immediately, displacing the current holder. That is
  for grabbing the wheel from someone mid-drive. Taking over is a social
  problem, not a protocol one, and on a shared robot a lease nobody can break
  is worse than an occasional rude handoff.
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

**This path is vestigial.** The robot no longer grants anyone a WebRTC track:
`role` is ignored and every viewer is served the broadcast, for the reasons in
"One stream, no per-viewer state" above. The signalling addresses and the
`webrtcbin` code still exist but are unreachable, kept only because deleting
them is a large diff with no behavioural gain. Nothing sends an `offer` any
more.

Historically this was the video path, and it is the reason the section above
exists: the robot served up to four peers from branches of its own capture
pipeline, and admitting one rebuilt that pipeline and interrupted everybody
already watching.

`hello` is an Event rather than a persistent presence Param on purpose. A
persistent per-session Param would accumulate stale entries (every past console
session), and the robot would try to start a stream for each ghost on connect.
An Event is never re-sent to late joiners, so the robot only ever sees live
viewers. Because it repeats, it doubles as the liveness signal: a viewer the
robot has not heard from in 20 seconds is dropped, which is how a closed tab
frees its slot without waiting on an ICE timeout, and it is also what lets every
viewer recover on its own after a robot restart.

## One stream, no per-viewer state

Everyone watching gets the same thing: the robot encodes once, publishes one
copy on `video/broadcast`, and the relay fans it out. The robot pays the same
whether one person is watching or fifty, and the audience is bounded by the
relay rather than by the Pi.

The cost is not the point. **The publisher keeps no state per viewer**, and that
is what stops an arrival disturbing everyone already there. It is the property a
video conference has, and the reason joining one does not make the other
participants stutter.

This robot used to lack it. A console could ask for a WebRTC track of its own,
and granting one rebuilt the GStreamer pipeline that the camera, the encoder and
the broadcast tap all sit on. Measured on hardware, that froze every watcher for
1336 ms and then 1503 ms on two builds, and the robot reached its thirteenth
rebuild in ordinary use. After the change the same request costs 168 ms against
a 105 ms baseline, which is to say nothing at all.

So `role` in a `hello` is read off the wire and ignored. Ignoring it rather than
asking consoles to stop sending it is deliberate: the console is a static
deploy, so one stale browser tab would otherwise still be able to interrupt
everybody.

The measured case for the relay carrying this: five simultaneous subscribers for
ninety seconds delivered 11975 of 11975 frames, no loss, no duplicates, byte
identical counts on every subscriber, while the robot's uplink stayed at one
copy at 1.79 Mbps.

## Proving the link works

A CLASP `subscribe` is fire and forget. It registers the callback locally, puts
a frame on the wire, and returns success without waiting for the relay to
confirm anything. So there is a state in which the robot is connected, publishes
happily, and is never delivered a single command, because its subscriptions were
never registered. Every console shows it online and it obeys nothing. This has
happened in the field, and nothing in the protocol as described above can tell
you it is happening.

`health` closes that hole. The robot subscribes to it, writes itself a unique
token, and waits to see the token come back. A relay that echoes the robot's own
writes is a relay that is delivering to the robot's subscriptions, so one round
trip covers a closed socket, a forgotten session, a subscription that never
registered, and a reconnect that dropped them. The robot does this once before
it publishes `status/online`, which is what makes that flag mean "commands will
be obeyed" rather than "a socket opened", and then every 15 seconds afterwards.
Four consecutive failures and the agent exits so its supervisor can rebuild the
connection from scratch.

Consoles have no reason to subscribe here, which is why the address sits outside
`status/`, and no reason to write to it.

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
