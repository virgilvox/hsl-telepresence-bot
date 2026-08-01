# Handoff

Last updated 2026-07-31.

Current state of the telepresence robot, what is verified, and what to do next.

## Read this first if you are picking up after 2026-07-31

The console gained multi-operator support and the robot side of it has **never
run on hardware**. The Pi was powered off when it was written, so it is verified
by unit tests, a full typecheck against real GStreamer headers, and an
end-to-end test against a stand-in robot, but not by a camera and two motors.
See "Multi-operator" below for exactly what is and is not proven, and for how to
back it out if the robot comes up unhappy.

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
  `cd web && npm run dev`. It publishes to GitHub Pages automatically on pushes
  to `main` that touch `web/` (`.github/workflows/pages.yml`), and can still be
  deployed to DigitalOcean App Platform with `deploy/digitalocean/app.yaml`.
- **Teleop drives the motors from the console** (confirmed on hardware). Video
  streams live to the console over WebRTC. Control and telemetry pass over the
  relay. The camera captures its side-by-side mode.

## Multi-operator (2026-07-31 session, not yet on hardware)

Several people can now watch one robot and take turns driving it.

**Video.** The Pi captures and encodes once and fans the encoded stream through a
`tee` to one `webrtcbin` per viewer, up to four. Encoding cost no longer scales
with the audience; only payloading and DTLS do. Joining rebuilds the pipeline,
which blacks out the existing viewers for a moment. That is deliberate, and the
reasoning is in the module docs at the top of `robot/src/video.rs`: splicing a
branch into a live pipeline can wedge the streaming thread and take the camera
down for everyone, while a rebuild cannot get stuck and makes the fresh encoder
emit a keyframe immediately. Leaving does not rebuild. Each branch queue is
`leaky=downstream`, so one wedged peer drops buffers instead of stalling the tee
and freezing everybody.

**Presence** is now a heartbeat. Consoles re-send `video/hello` every 3 s for as
long as they want video and send `bye` on their way out; the robot drops a viewer
it has not heard from in 20 s. That is what frees a slot when a tab closes,
without waiting on an ICE timeout, and it is why every viewer reattaches by
itself after a robot restart.

**Driving** is arbitrated by the robot in `robot/src/control.rs`, and taking
turns needs no buttons. Driving a free wheel claims it (named from the drive
command itself), and the lease lapses 1.5 s after the holder's last command,
which is about the length of a pause between deliberate movements. So the wheel
belongs to whoever is currently driving; the moment they stop, the next person
just starts. **Take over** is the only button, and it only appears when someone
else is actually driving. Commands from anyone else are dropped in `link.rs`
before they reach the motors or reset the drive watchdog. **The e-stop is not
arbitrated**: anyone watching can stop the robot, which is the point, because
the person who can see the collision coming is not always the one holding the
wheel.

**Drive commands** are sent on change and then repeated every 150 ms only while
the robot is moving, rather than sampled at a fixed rate. Pressing a key puts a
command on the wire in that event handler instead of up to a tick later, and
letting go sends one zero frame and then goes silent. The repeat exists so that
silence means something: it is what lets the 400 ms watchdog coast the robot
when an operator's network drops or their tab dies mid-drive, which a plain
press/release event pair cannot do on a best-effort Stream.

`status/protocol` (2) is how a console knows the robot arbitrates. An older robot
does not publish it, and the console then lets anyone drive rather than waiting
on a lease that will never be granted. This matters during a rollout: the console
auto-deploys on push while the Pi only updates when it is next powered on, so
new-console/old-robot is a state that really happens.

### What is proven, and what is not

Verified: 26 Rust unit tests including the lease rules; `cargo clippy
--all-targets` against real GStreamer headers; and the whole console flow
against the simulator in `web/sim.html`, including real WebRTC video, taking
turns at the wheel, a non-driver being refused, and a non-driver still being
able to e-stop.

Not verified: **anything involving the actual camera.** The multi-branch pipeline
has never had a v4l2 device attached. The first hardware run should watch
`journalctl -u hsl-robot -f` while a second viewer joins, and confirm the
existing viewer's picture comes back after the rebuild.

To back it out: `git revert` the multi-operator commit and let the self-updater
rebuild, or on the Pi
`sudo systemctl stop hsl-robot-update.timer && cd ~/hsl-telepresence-bot && git checkout 769e28e && (rebuild per "Rebuild and redeploy")`. The console tolerates
an older robot by design, so reverting only the Pi is safe.

## Audit, 2026-07-31

A pass over the whole system looking for defects rather than features. Seven
real ones, all fixed. Recorded here because most of them are the kind that stay
invisible until the day they matter.

**A reconnect killed video for good.** The relay issues a *new session on every
reconnect*, and the agent captured its session once at startup. From the first
reconnect onwards it filtered inbound signalling against a session that no
longer existed, so every answer and every ICE candidate was silently dropped:
video could never negotiate again, while the control plane carried on looking
perfectly healthy. The session is now read live at the moment it is used, and
outbound signalling is stamped as it leaves rather than when it is built.
(`link.rs`, `video.rs`, `protocol.rs`.) The simulator had inherited the same bug
and has the same fix.

**SIGTERM was never handled.** The shutdown path waited on `ctrl_c()` alone, and
systemd stops units with SIGTERM, so on every `systemctl restart` the robot was
simply killed: motors left exactly as they were, and a latched `status/online`
still telling every console the robot was there. Now both signals are handled.

**Nothing stopped the motors if the motion task died.** The PCA9685 holds its
registers until told otherwise, so a panic anywhere in the motion task would
have left the wheels turning with nothing left alive to stop them. The backend
now coasts on `Drop`, which covers the panic, the cancellation and the unwind,
not just the orderly path. The orchestrator also watches the motion task and
exits if it ever ends, so the supervisor restarts a healthy process.

**A viewer whose connection blipped could never get video again.** Dropping a
viewer removed it from the live set but left it in the running pipeline's served
list, so its next hello did not look like a new arrival, nothing rebuilt, and it
sat on "waiting for robot" forever while everyone else watched.
`Broadcast::forget` now takes it out of both.

**The pipeline blocked the async runtime.** Tearing down waits for the pipeline
to reach NULL, and opening a wedged USB camera can stall for a long time; both
ran on a runtime worker thread that the motion watchdog shares. Now on a
blocking thread, still ordered, because the old pipeline has to release the
camera before the new one opens it.

**The console called a dead robot online forever.** `status/online` is latched,
so a robot killed uncleanly never clears it and the relay serves `true` to
everyone afterwards. The staleness check meant to catch that compared against
`Date.now()` inside a computed, which is not reactive, so it only re-evaluated
when a message arrived: never, in the one case it existed for.

**Console state leaked between robots.** Point the console at an older robot
after a newer one and the previous robot's `status/protocol` convinced it the
new one arbitrated the wheel, while a stale `driver` named somebody else holding
it. The pad locked and the robot could not be driven at all. Mirrored state is
now cleared when the subscription is rebuilt.

Also: `robot.service` now orders after `time-sync.target`. The Pi has no clock
across reboots and could reach the network before it knew the date, which makes
the relay's certificate look not-yet-valid and kills the agent on boot; the
restart policy recovered it, but it filled the journal with a failure that looks
like a bug.

**The updater now updates more than the binary.** It used to install only
`hsl-robot`, which meant every fix to a unit file, or to the updater itself, could
reach the Pi only by hand: a fix that can only be applied by hand is one that
sits in git being forgotten, and two of them already had. It now also installs
the three systemd units (reloading only when one actually changed) and replaces
itself, writing beside each target and renaming so that swapping the script out
from under the running shell is atomic. `deploy/pi/test-update.sh` exercises the
whole thing against a sandbox with stubbed `runuser`, `systemctl` and `cargo`,
including the failed-build case, because the alternative is finding out on the
robot that the thing which installs fixes is itself broken.

That leaves exactly one manual step, once, to bootstrap the new updater onto a
Pi still running the old one:

```
cd ~/hsl-telepresence-bot && git pull
sudo install -m 755 deploy/pi/update.sh /usr/local/bin/hsl-robot-update
sudo cp deploy/pi/robot.service /etc/systemd/system/hsl-robot.service
sudo systemctl daemon-reload
```

After that it keeps itself current. The agent binary was already updating on its
own and still will, with or without this.

Not changed, by decision: the relay is unauthenticated and the console is
public, so anyone who loads the page can drive the robot and anyone can stand up
a simulator claiming to be it. Accepted for a hackerspace robot.

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

- Four viewers maximum, and a fifth is ignored with a warning in the journal
  rather than queued. The number is a guess at what a Pi 3B+ and a 100 Mbit NIC
  will carry; nobody has measured where it actually falls over.
- Somebody joining briefly interrupts the picture for everyone already watching,
  because the pipeline is rebuilt. See "Multi-operator" for why that trade was
  made.
- All viewers share one encoder, so the bitrate cannot adapt per viewer. One
  operator on a bad connection sees drops rather than a lower-quality stream.
- You can only drive from a visible tab. Browsers throttle timers in a hidden
  tab to roughly 1 Hz, which is slower than the robot's 400 ms drive watchdog,
  so commands from a background tab would arrive too sparsely to hold the
  motors. The console does not try to fight this: hiding the tab or losing
  focus releases every held key, the robot coasts, and the wheel frees on its
  own a few seconds later.
- Audio has no hardware; the audio task is a best-effort no-op.
- Public relay auth and rate limits are not documented; the robot connects
  anonymously. Self-hosting `clasp-relay` is the documented fallback. With the
  console hosted publicly on GitHub Pages, anyone who loads the page and knows
  the robot id (`hslbot`, the console's default) can drive the robot while it is
  online. The relay is the only gate, and today there is none. If that matters,
  put a token on the relay or rename the robot id to something unguessable.
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
