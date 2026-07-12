# hsl-telepresence-bot

A telepresence robot you drive and see through from a web browser. A Raspberry
Pi 3 carries the motors and camera; a static web console controls it. They talk
over CLASP, a low-latency relay, with live video on a peer-to-peer WebRTC track.

This is a modern rebuild of an earlier design (the two-part AWS Compute Blog
series "Building a Raspberry Pi telepresence robot using serverless" by Moheeb
Zara). The shape is the same, the stack is new: Rust on the Pi instead of
Python, CLASP instead of AWS IoT Core, a fresh Vue 3 console instead of the old
Amplify front end, and GStreamer WebRTC instead of the Kinesis Video Streams C
SDK.

## Status

Deployed and running on a Pi 3B+ (`hslbot`). Verified end to end: the agent runs
as a systemd service, connects to the relay, and its control and telemetry work
over the public relay; the camera captures and the robot produces a live H264
WebRTC offer. Operating and rebuild notes are in [docs/handoff.md](docs/handoff.md).

## Hardware

- Raspberry Pi 3 Model B+
- Adafruit DC and Stepper Motor HAT (PCA9685 at I2C `0x60`), two drive motors
- USB stereoscopic camera that outputs one wide frame with left and right side
  by side
- Microphone and speaker: planned, not required. The system runs fully without
  them.

## Architecture

```
  Browser (Vue console)                         Raspberry Pi (Rust agent)
  ---------------------                          -------------------------
  drive / estop / config  --.                .--  link  ---> motion (I2C, PCA9685)
  telemetry / status      <--\              /---  link  <--- telemetry
                              \            /
                          CLASP relay (relay.clasp.to)
                              /            \
  SDP / ICE signaling  <----'              '----> SDP / ICE signaling
        |                                                   |
        '========== WebRTC media track (peer to peer) ======'
                     (stereoscopic H264, never via the relay)
```

CLASP carries control, telemetry, and the signaling handshake. The video itself
rides a native WebRTC track directly between the Pi and the browser, because
CLASP is a control transport, not a media transport. If peer-to-peer cannot be
established, the control plane still works and the robot stays drivable.

The full address and message contract is in [docs/protocol.md](docs/protocol.md).

### Why not Conduyt for the motors

Conduyt is a host-to-device protocol whose "device" is a microcontroller
running Conduyt firmware. The Pi 3 is a Linux host and the Motor HAT sits on the
Pi's own I2C bus with no microcontroller in between, so Conduyt is not the right
tool for this path. The Pi drives the PCA9685 directly with a plain Rust I2C
driver (`pwm-pca9685` over `linux-embedded-hal`). Conduyt would earn a place
only if a separate microcontroller were added later, at which point the Pi could
speak Conduyt to it over serial for extra peripherals.

## Repository layout

```
robot/    Rust agent for the Pi (see robot/README.md)
web/      Vue 3 + Vite operator console (static site)
deploy/   DigitalOcean App Platform spec, Pi systemd unit and installer
docs/     Protocol contract
CLAUDE.md Project rules
```

## Quick start

### Web console (local)

```bash
cd web
npm install
npm run dev
```

Open the printed URL, set the robot id and relay (`wss://relay.clasp.to` by
default), and connect. The console works before a robot is online: it connects
to the relay, and the drive pad, e-stop, and telemetry panel are all live. Video
shows "Waiting for robot" until a robot answers.

### Robot agent (on the Pi)

See [robot/README.md](robot/README.md). In short:

```bash
deploy/pi/install.sh
ROBOT_ID=hsl-bot-1 robot/target/release/robot
```

Use the same `ROBOT_ID` in the console to pair them.

## Deploy the console

The console is a static site. On DigitalOcean App Platform:

```bash
doctl apps create --spec deploy/digitalocean/app.yaml
```

Edit the `github.repo` field in that spec first. Any static host works; the
build is `npm ci && npm run build` with output in `web/dist`.

## Versions

Pinned to the latest published libraries: CLASP Rust crates `4.5`, the
`@clasp-to/sdk` npm package `4.5.0`, `pwm-pca9685` `1.0`, `linux-embedded-hal`
`0.4`, and `gstreamer` (gstreamer-rs) `0.25`. The local CLASP and Conduyt
checkouts are references only and are not modified by this project.
