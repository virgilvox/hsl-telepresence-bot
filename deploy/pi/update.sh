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
BINARY_DEST="/usr/local/bin/hsl-robot"

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

if [ "$local_rev" = "$remote_rev" ]; then
  log "already up to date at ${local_rev:0:12}"
  exit 0
fi

log "updating ${local_rev:0:12} -> ${remote_rev:0:12}"
as_user "cd '$REPO' && git reset --hard 'origin/$BRANCH'"

log "building release agent (this takes several minutes on the Pi)"
as_user "cd '$REPO/robot' && $BUILD_ENV \$HOME/.cargo/bin/cargo build --release"

log "installing binary and restarting service"
install -m 755 "$REPO/robot/target/release/robot" "$BINARY_DEST"
systemctl restart hsl-robot

log "updated to ${remote_rev:0:12}"
