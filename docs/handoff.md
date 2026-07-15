# Handoff

Last updated 2026-07-14.

Current state of the telepresence robot, what is verified, and what to do next.

## Where things stand

- The robot is a Pi 3B+ named `hslbot` on the local network, running 64-bit
  Raspberry Pi OS (Debian 13, trixie). The Rust agent runs as the systemd
  service `hsl-robot`, starts on boot, and reconnects to the public CLASP relay
  on its own.
- The Pi now carries a full git clone of `origin/main` at
  `/home/pi/hsl-telepresence-bot`, and a timer-driven self-updater keeps it on
  the latest code (see "Self-update"). The installed binary
  (`/usr/local/bin/hsl-robot`) is built from the current `main` and includes the
  motor-control fix below.
- The operator console is a Vue 3 static site in `web/`. It defaults to robot id
  `hslbot` and connects to `wss://relay.clasp.to`. Run it locally with
  `cd web && npm run dev`, or deploy it to DigitalOcean App Platform with
  `deploy/digitalocean/app.yaml`.
- Verified working end to end: service comes online, control and telemetry pass
  over the relay (confirmed from an independent client), the USB camera captures,
  and the robot emits a live H264 WebRTC offer when a viewer says hello.

## Action items, in order

1. **Confirm driving on hardware.** With the wheels clear, drive from the
   console and confirm forward, reverse, and turning. This is the one thing the
   automated tests cannot prove. The agent now runs the fixed binary, so this is
   ready to try.
2. **Confirm the video picture renders** in the browser (the robot side of the
   WebRTC handshake is proven; the browser answer/ICE path needs a real browser
   to confirm the frames paint).

The earlier "redeploy to pick up the motor fix" item is done: the Pi was
converted to a git clone, rebuilt from `main`, and is running the fixed binary.

## Motor control audit finding (fixed in code, needs redeploy)

The PCA9685 drives each motor's direction pins to full HIGH or full LOW using a
per-channel full-ON bit and full-OFF bit. Per the datasheet, **full-OFF takes
precedence over full-ON**, and the `pwm-pca9685` crate's `set_channel_full_on`
and `set_channel_full_off` each write only their own side and never clear the
other. The startup coast set the full-OFF bit on both direction pins, so the
later `set_channel_full_on` could not drive them high: after the first coast the
direction pins were stuck low and **the motors would never turn**.

The fix (in `robot/src/motion/hat_driver.rs`) sets a pin HIGH by setting full-ON
and clearing the full-OFF bit, and sets it LOW by setting full-OFF and clearing
full-ON. The register logic is now generic over the I2C bus and unit-tested with
a recording mock (`cargo test --no-default-features`), including a regression
test that asserts the full-OFF bit is cleared when a direction pin goes high.
Those tests run on any machine, no Pi required.

Other motor-control hardening from the audit:

- A failed drive write now fails safe by attempting to coast.
- Non-finite speed inputs map to a stop rather than an undefined cast.
- A deterministic test covers the e-stop invariant (drives ignored while
  stopped, resumed when cleared).

## Operating the robot

On the Pi:

```
systemctl status hsl-robot          # is it running
journalctl -u hsl-robot -f          # live logs
systemctl restart hsl-robot         # restart
```

Config is in `/etc/hsl-telepresence/robot.env` (id, relay URL, I2C bus/address,
drive timeout, max speed, camera device and resolution, log level).

From the console: set the robot id to `hslbot`, connect, drive with the pad or
WASD, and toggle Left/Both/Right to pick an eye of the stereo feed. The e-stop is
a latched Param; if the robot ever comes up stopped, release it from the console
or clear `/robot/hslbot/cmd/estop` on the relay.

## Rebuild and redeploy

Build natively on the Pi (cross-compiling is impractical because of GStreamer).
The Pi has 905 MB RAM, so a 2 GB swapfile and `CARGO_PROFILE_RELEASE_LTO=false`
are needed to avoid an out-of-memory kill.

```
cd ~/hsl-telepresence-bot/robot
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  CARGO_BUILD_JOBS=3 cargo build --release
sudo install -m 755 target/release/robot /usr/local/bin/hsl-robot
sudo systemctl restart hsl-robot
```

Sync source to the Pi with rsync (exclude `target`, `node_modules`, `.git`), or
`git pull` on the Pi. Normally you do not need to do this by hand: the
self-updater below pulls and rebuilds on its own.

## Self-update

The Pi keeps itself current from the git remote. Three pieces, all in
`deploy/pi/`:

- `update.sh`, installed as `/usr/local/bin/hsl-robot-update`. It fetches
  `origin/main`, and only when the remote has moved does it `git reset --hard`,
  rebuild the agent (LTO off, bounded jobs), install the binary, and restart
  `hsl-robot`. Up to date means it does nothing, so running it often is cheap.
  Git and the build run as the `pi` user; only the install and restart use root.
- `hsl-robot-update.service`, a oneshot that runs the script after
  `network-online.target`.
- `hsl-robot-update.timer`, which triggers the service 2 minutes after boot and
  hourly after that (`Persistent=true` catches up a missed run).

Enable the timer, not the service:

```
sudo install -m 755 deploy/pi/update.sh /usr/local/bin/hsl-robot-update
sudo cp deploy/pi/hsl-robot-update.service /etc/systemd/system/
sudo cp deploy/pi/hsl-robot-update.timer /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now hsl-robot-update.timer
```

Watch it: `journalctl -u hsl-robot-update -f`. Force a check now:
`sudo systemctl start hsl-robot-update.service`. Pause it:
`sudo systemctl disable --now hsl-robot-update.timer`.

System dependencies (already installed on `hslbot`): `build-essential`,
`pkg-config`, `libssl-dev`, `i2c-tools`, `v4l-utils`, and the GStreamer stack
including `libgstreamer-plugins-bad1.0-dev` (needed for `gstreamer-webrtc-1.0.pc`).

Features: `motor-hat` and `video` are on by default. Off the Pi, build or test
with `--no-default-features` to use the mock motor backend and skip GStreamer.

## Hardware notes

- Motor HAT (PCA9685) confirmed at I2C `0x60` on `/dev/i2c-1`. Header I2C had to
  be enabled with `raspi-config` and a reboot.
- Camera is a UVC "3D USB Camera" (`05a3:9750`), MJPEG side-by-side. It draws
  enough current that it only enumerates cleanly on a good port or powered hub;
  a bad cable or weak port shows up as `error -32 / unable to enumerate` in
  `dmesg`, not as a driver problem (UVC needs no drivers).
- The Pi's hardware H264 encoder maxes out at 1920 wide and needs explicit
  `video/x-h264,level=(string)4` output caps, so the robot captures the camera's
  **1280x480** mode, not its 2560-wide modes.

## Known limitations

- Video serves one viewer at a time; a new viewer replaces the current session.
- A viewer that leaves without a `bye` leaves the robot streaming until another
  viewer connects (no idle-session timeout yet). CPU waste, not a correctness
  problem.
- Audio has no hardware; the audio task is a best-effort no-op.
- Public relay auth and rate limits are not documented; the robot connects
  anonymously. Self-hosting `clasp-relay` is the documented fallback.

## Where to look

- `robot/src/motion/` motor control: `drive.rs` (kinematics + watchdog + e-stop),
  `hat_driver.rs` (PCA9685 register logic + tests), `pca9685.rs` (Linux backend).
- `robot/src/{link,video,telemetry}.rs` the relay, WebRTC, and telemetry planes.
- `web/src/composables/` the console's CLASP, control, video, and telemetry logic.
- `docs/protocol.md` the CLASP address and message contract.
