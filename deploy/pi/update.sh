#!/usr/bin/env bash
# Self-update the telepresence robot from the git remote.
#
# Run on boot and periodically by hsl-robot-update.timer. It fetches the
# tracked branch, and only when the remote has moved does it rebuild the agent,
# install the new binary, and restart the service. When already up to date it
# does nothing, so running it often is cheap.
#
# The service unit runs this as root. Git and the build run as the owning user
# (pi) so the repo and the cargo cache stay under that user; only the install
# and the service restart need root.
set -euo pipefail

REPO="${HSL_REPO:-/home/pi/hsl-telepresence-bot}"
BUILD_USER="${HSL_BUILD_USER:-pi}"
BRANCH="${HSL_BRANCH:-main}"
# Destinations, overridable so the whole script can be exercised against a
# sandbox rather than the real system paths.
BINARY_DEST="${HSL_BINARY_DEST:-/usr/local/bin/hsl-robot}"
UNIT_DIR="${HSL_UNIT_DIR:-/etc/systemd/system}"
SCRIPT_DEST="${HSL_SCRIPT_DEST:-/usr/local/bin/hsl-robot-update}"
# The revision the installed binary was actually built from. Compared against
# the remote instead of the checkout's HEAD, because the checkout moves forward
# before the build runs: if a build fails, HEAD already matches the remote and
# comparing the two would report "up to date" forever while the robot quietly
# kept running the old binary. Comparing what was *installed* makes a failed
# build retry on the next timer tick instead.
STAMP="${HSL_STAMP:-/var/lib/hsl-telepresence/built-rev}"

# Build with LTO off and a bounded job count. The Pi 3B+ has under 1 GB of RAM,
# and a full-LTO release link is reliably OOM-killed even with swap.
BUILD_ENV="CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=3"

log() { echo "[hsl-robot-update] $*"; }

if [ ! -d "$REPO/.git" ]; then
  log "no git repo at $REPO; nothing to update"
  exit 0
fi

# All git/build work happens as the repo owner via a login shell so ~/.cargo is
# on PATH.
as_user() { runuser -l "$BUILD_USER" -c "$*"; }

if ! as_user "cd '$REPO' && git fetch --quiet origin '$BRANCH'"; then
  log "fetch failed (offline?); leaving current build in place"
  exit 0
fi

# A login shell can print a banner (e.g. the Pi default-password warning) ahead
# of the command output, so keep only the last line: the revision itself.
local_rev=$(as_user "cd '$REPO' && git rev-parse HEAD" | tail -n1)
remote_rev=$(as_user "cd '$REPO' && git rev-parse 'origin/$BRANCH'" | tail -n1)

mkdir -p "$(dirname "$STAMP")"
built_rev=$(cat "$STAMP" 2>/dev/null || true)

# First run under the stamp scheme with nothing pending: adopt the current
# revision rather than forcing a rebuild of code that is already installed.
if [ -z "$built_rev" ] && [ "$local_rev" = "$remote_rev" ]; then
  echo "$remote_rev" >"$STAMP"
  log "already up to date at ${local_rev:0:12} (recorded)"
  exit 0
fi

if [ "$built_rev" = "$remote_rev" ]; then
  log "already up to date at ${remote_rev:0:12}"
  exit 0
fi

log "updating ${built_rev:-unknown} -> ${remote_rev:0:12}"
as_user "cd '$REPO' && git reset --hard 'origin/$BRANCH'"

log "building release agent (this takes several minutes on the Pi)"
as_user "cd '$REPO/robot' && $BUILD_ENV \$HOME/.cargo/bin/cargo build --release"

# Bring the units and this script itself into step with the repo, not just the
# binary. Installing only the binary means every fix to the unit that runs the
# agent, or to this updater, can reach the Pi only by hand, and a fix that can
# only be applied by hand is one that sits in git being forgotten. Done before
# the restart so a unit change shipped alongside a code change takes effect in
# the same pass.
sync_file() {
  local src="$1" dest="$2" mode="$3"
  if [ ! -f "$src" ] || cmp -s "$src" "$dest"; then
    return 1
  fi
  # Write beside the target and rename, rather than writing through it. The
  # rename is atomic, which matters most for this very script: bash reads it as
  # it goes, and truncating it mid-run would feed the shell garbage.
  install -m "$mode" "$src" "$dest.new"
  mv -f "$dest.new" "$dest"
  log "installed $(basename "$dest")"
  return 0
}

units_changed=0
for unit in robot.service hsl-robot-update.service hsl-robot-update.timer; do
  dest="$UNIT_DIR/$unit"
  if [ "$unit" = "robot.service" ]; then dest="$UNIT_DIR/hsl-robot.service"; fi
  if sync_file "$REPO/deploy/pi/$unit" "$dest" 644; then
    units_changed=1
  fi
done
if [ "$units_changed" = 1 ]; then
  systemctl daemon-reload
fi

# Replacing the running script is safe because of the rename above: this
# process keeps reading the old inode until it exits, and the next timer tick
# runs the new one.
sync_file "$REPO/deploy/pi/update.sh" "$SCRIPT_DEST" 755 || true

log "installing binary and restarting service"
install -m 755 "$REPO/robot/target/release/robot" "$BINARY_DEST"
# Recorded only after a successful build and install, so the stamp always names
# a revision that is really running.
echo "$remote_rev" >"$STAMP"
systemctl restart hsl-robot

log "updated to ${remote_rev:0:12}"
