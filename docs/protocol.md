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
| `cmd/drive` | Stream | `{ throttle, steer, seq, ts }` | 10 to 20 Hz teleop. Lossy is fine because it is continuous and watchdog-backed. |
| `cmd/estop` | Param | `bool` | Latched. `true` holds the motors stopped. Re-syncs to a late-joining second operator. |
| `cfg/max_speed` | Param | `number` 0..1 | Speed ceiling. Survives reconnect. |
| `status/online` | Param | `bool` | Set true on connect, false on clean shutdown. |
| `status/mode` | Param | `string` | Current drive mode. |
| `status/estop` | Param | `bool` | Robot's mirror of the e-stop state, for the console. |
| `status/battery` | Param | `number` 0..1 | Optional. Rendered when present. |
| `tel/motors` | Stream | `{ left, right }` | Applied wheel demand, ~5 Hz. |
| `video/presence/<session>` | Param | `{ session, role }` | A viewer announces itself here. |
| `video/signal/<session>` | Event | `SignalMessage` | SDP/ICE, keyed by recipient session. |

## DriveCommand

```json
{ "throttle": 0.5, "steer": -0.2, "seq": 1234, "ts": 1717000000000 }
```

`throttle` and `steer` are each normalized to -1..1. Positive `steer` turns the
robot to its right. `seq` is monotonic per operator; `ts` is the operator's send
time in milliseconds.

## SignalMessage

A tagged union keyed by `kind`, always carrying `from` (the sender's CLASP
session id) so a peer can reply and can ignore echoes of its own messages.

```json
{ "kind": "offer",  "from": "<session>", "sdp": "..." }
{ "kind": "answer", "from": "<session>", "sdp": "..." }
{ "kind": "ice",    "from": "<session>", "candidate": "...", "sdpMLineIndex": 0 }
{ "kind": "bye",    "from": "<session>" }
```

The robot is the offerer. On seeing a viewer's presence Param it sends an
`offer` to `video/signal/<viewerSession>`. The viewer replies with an `answer`
and both trickle `ice` candidates to each other's signaling address. Media flows
over the resulting native WebRTC track, never over CLASP.

## Safety model

Motion safety lives on the robot, not in the transport. Drive commands are
continuous Streams; if none arrives within the watchdog window (default 400 ms)
the motors coast. The e-stop is a latched Param, so losing the relay leaves the
robot stopped rather than holding its last command, and a reconnecting operator
sees the true state immediately.
