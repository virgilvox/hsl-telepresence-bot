# Handoff

Last updated 2026-07-15.

Current state of the telepresence robot, what is verified, and what to do next.

## Where things stand

- The robot is a Pi 3B+ named `hslbot` on the local network, running 64-bit
  Raspberry Pi OS (Debian 13, trixie). The Rust agent runs as the systemd
  service `hsl-robot`, starts on boot, and reconnects to the public CLASP relay
  on its own.
- The Pi now carries a full git clone of `origin/main` at
  `/home/pi/hsl-telepresence-bot`, and a timer-driven self-updater keeps it on
  the latest code (see "Self-update"). The installed binary
  (`/usr/local/bin/hsl-robot`) is built from the current `main`.
- The operator console is a Vue 3 static site in `web/`. It defaults to robot id
  `hslbot` and connects to `wss://relay.clasp.to`. Run it locally with
  `cd web && npm run dev`, or deploy it to DigitalOcean App Platform with
  `deploy/digitalocean/app.yaml`.
- **Teleop drives the motors from the console** (confirmed on hardware). Video
  streams live to the console over WebRTC. Control and telemetry pass over the
  relay. The camera captures its side-by-side mode.

## Action items, in order

1. **Confirm drive direction.** Forward now drives both wheels the same way
   (`INVERT_RIGHT=true`, see below). With the wheels clear, sanity-check that
   forward/reverse and left/right turns all go the intended way; adjust the
   `INVERT_*` env flags if any axis is reversed. No rebuild needed for that.
2. **Confirm the video picture renders** in the browser end to end (proven from
   the operator console this session; worth a second independent check).

## Making teleop actually drive (2026-07-15 session)

The wheels were wired to M1/M2 with motor power on, but the console could not
drive them. Three distinct things were in the way, now all resolved:

1. **Drive commands failed to deserialize (the real teleop bug).** Drive is a
   CLASP Stream and *was* reaching the robot, but `DriveCommand.seq`/`.ts` were
   `u64` while the browser sends `Date.now()` and the seq counter as JS numbers
   that CLASP can tag as `Float`. serde rejects a float into `u64`, so the whole
   command was dropped before the motor task saw it. `seq`/`ts` now deserialize
   leniently from any number encoding (`de_lenient_u64` in `protocol.rs`); they
   are informational only, the motor task reads just `throttle`/`steer`. This is
   why a direct I2C poke spun the wheels but teleop did nothing.
2. **Right motor ran reversed.** The chassis mounts the two motors
   mirror-imaged. Added `INVERT_LEFT`/`INVERT_RIGHT` env flags
   (`config.rs` -> `pca9685.rs`); `hslbot` runs `INVERT_RIGHT=true`.
3. **A latched e-stop was silently blocking motion.** `cmd/estop` had been left
   engaged; while engaged the robot ignores every drive by design. Cleared from
   the console (see Operating).

The motor **register logic was never broken.** It matches Adafruit's library and
the `pwm-pca9685` full-OFF-precedence rule, and a direct I2C drive spun both
wheels. See "Motor register logic" for the earlier audit that got it right.

## Motor register logic (audit, already deployed)

The PCA9685 drives each direction pin to full HIGH or full LOW via a per-channel
full-ON bit and full-OFF bit. Per the datasheet (and the crate's own docs),
**full-OFF takes precedence over full-ON**, and `pwm-pca9685`'s
`set_channel_full_on`/`set_channel_full_off` each write only their own side. So
driving a pin HIGH must set full-ON *and clear* full-OFF, and LOW the reverse.
Missing that would leave the direction pins stuck low after the startup coast.
`robot/src/motion/hat_driver.rs` does it correctly and is unit-tested with a
recording mock (`cargo test --no-default-features`, 10 tests pass), including a
regression test asserting the full-OFF bit is cleared when a pin goes high.

Other motor-control hardening in place:

- A failed drive write fails safe by attempting to coast.
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
drive timeout, max speed, `INVERT_LEFT`/`INVERT_RIGHT` motor direction, camera
device and resolution, log level). Editing it needs a `systemctl restart
hsl-robot` to take effect.

From the console: set the robot id to `hslbot`, connect, drive with the pad or
WASD (**hold** the key/drag; it only sends while held, and the watchdog coasts
~400 ms after input stops), and toggle Left/Both/Right to pick an eye of the
stereo feed.

**If the robot will not drive, check the e-stop first.** It is a latched Param.
When engaged, the console's big button reads "Release stop" (filled red) and the
robot ignores all drive; click it to release, or clear `/robot/hslbot/cmd/estop`
on the relay. If a long-running robot stops reacting to the e-stop button or
config changes (a live-Param delivery hiccup seen once this session), a
`systemctl restart hsl-robot` re-reads the latched state and restores it.

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
  `dmesg`, not as a driver problem (UVC needs no drivers). After such a glitch it
  can re-enumerate onto a different `/dev/videoN`, so the env points
  `CAMERA_DEVICE` at the stable `/dev/v4l/by-id/...-video-index0` symlink and the
  agent falls back to the first by-id capture node if the configured one is gone.
- The Pi's hardware H264 encoder maxes out at 1920 wide and needs explicit
  `video/x-h264,level=(string)4` output caps, so the robot captures the camera's
  **1280x480** mode, not its 2560-wide modes.

## Known limitations

- Video serves one viewer at a time. The established viewer holds the camera; a
  second operator sees "waiting" until the first disconnects. A session that
  never establishes within a grace window can be handed to a waiting viewer, so
  a stale viewer does not block the camera forever.
- A viewer that closes its tab is detected when its WebRTC connection fails, and
  the session is torn down so the next viewer can take over. There is no
  explicit `bye` on close yet, so recovery waits on the ICE failure timeout
  (seconds), not instant.
- Audio has no hardware; the audio task is a best-effort no-op.
- Public relay auth and rate limits are not documented; the robot connects
  anonymously. Self-hosting `clasp-relay` is the documented fallback.
- Seen once: a long-running robot stopped acting on live `cmd/estop` Param
  updates (drive Streams kept flowing) until a restart, which re-reads the
  latched value. Not yet root-caused; if it recurs, suspect the CLASP client's
  pattern-subscription liveness for Params. A restart is the workaround.

## Where to look

- `robot/src/motion/` motor control: `drive.rs` (kinematics + watchdog + e-stop),
  `hat_driver.rs` (PCA9685 register logic + tests), `pca9685.rs` (Linux backend).
- `robot/src/{link,video,telemetry}.rs` the relay, WebRTC, and telemetry planes.
- `web/src/composables/` the console's CLASP, control, video, and telemetry logic.
- `docs/protocol.md` the CLASP address and message contract.
