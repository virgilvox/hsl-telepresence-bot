# Robot agent

The Rust program that runs on the Raspberry Pi. It connects to the CLASP relay,
drives the Adafruit Motor HAT over I2C, streams the USB camera over WebRTC, and
publishes telemetry.

## Modules

| Module | Responsibility | Touches |
|---|---|---|
| `config` | Read configuration from the environment | nothing |
| `protocol` | CLASP address and message contract | nothing |
| `link` | Connect to the relay, route inbound commands, publish status | CLASP only |
| `motion` | Differential drive, safety watchdog, motor backend | I2C only |
| `motion::hat_driver` | PCA9685 register logic, generic over the I2C bus, unit-tested | pure logic |
| `video` | Camera capture and WebRTC media, signaling over CLASP | GStreamer only |
| `telemetry` | Publish motor duty and e-stop mirror | via `link` |
| `audio` | Optional mic/speaker, best-effort | nothing yet |

Modules never share hardware handles. They communicate over `tokio` channels, so
one plane can fail or restart without disturbing the others.

## Build on the Pi

Use 64-bit Raspberry Pi OS (Bookworm). Building natively avoids the OpenSSL
cross-compile problem.

```bash
deploy/pi/install.sh          # system packages, Rust, release build
```

Or by hand:

```bash
sudo apt-get install -y build-essential pkg-config libssl-dev libudev-dev \
  i2c-tools gstreamer1.0-tools libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-nice
cargo build --release
```

Confirm the HAT is present: `i2cdetect -y 1` should show `0x60`.

## Run

Configuration is entirely environment driven. See
`../deploy/pi/robot.env.example` for every variable.

```bash
ROBOT_ID=hsl-bot-1 CLASP_URL=wss://relay.clasp.to ./target/release/robot
```

For a persistent install, use the systemd unit in `../deploy/pi/`.

## Features

- `motor-hat` (default): drive the PCA9685 over `/dev/i2c-1`.
- `video` (default): capture the camera and stream over WebRTC via GStreamer.

Off the Pi, build without hardware so the control and video logic still compile
and the unit tests run:

```bash
cargo test --no-default-features    # mock motor backend, video compiled out
```

## Notes

- Motion safety is local. If drive commands stop arriving within
  `DRIVE_TIMEOUT_MS`, the motors coast; the e-stop is a latched Param that
  survives reconnects. Neither depends on the relay behaving.
- Audio has no hardware yet. The audio task is spawned best-effort and its
  absence never blocks startup.
- The GStreamer `webrtcbin` signaling wiring should be validated on-device the
  first time you flash a Pi, since element behavior can vary by plugin version.
