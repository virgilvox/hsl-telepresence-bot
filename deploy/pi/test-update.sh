#!/usr/bin/env bash
# Exercise update.sh against a sandbox, because the alternative is testing it on
# the robot and finding out that the thing which installs fixes is itself
# broken.
#
# Builds a throwaway git repo, stubs runuser/systemctl/cargo, and points every
# destination at a temp dir. Run from anywhere:
#
#   deploy/pi/test-update.sh .
#
set -uo pipefail

SRC_REPO="${1:-$(cd "$(dirname "$0")/../.." && pwd)}"
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT

BIN="$ROOT/bin"; mkdir -p "$BIN"
UPSTREAM="$ROOT/upstream"; CLONE="$ROOT/clone"
DEST="$ROOT/dest"; UNITS="$ROOT/units"; mkdir -p "$DEST" "$UNITS"

# Stubs. runuser just runs the command in this shell; systemctl records calls.
# A sandbox HOME so the stubbed cargo is the one that gets found.
mkdir -p "$ROOT/home/.cargo/bin"
cat >"$ROOT/home/.cargo/bin/cargo" <<'EOS'
#!/usr/bin/env bash
echo "stub cargo $*"
EOS
chmod +x "$ROOT/home/.cargo/bin/cargo"

cat >"$BIN/runuser" <<EOS
#!/usr/bin/env bash
# runuser -l USER -c CMD
shift 2; shift; HOME="$ROOT/home" exec bash -c "\$1"
EOS
cat >"$BIN/systemctl" <<EOS
#!/usr/bin/env bash
echo "systemctl \$*" >>"$ROOT/systemctl.log"
EOS
chmod +x "$BIN/runuser" "$BIN/systemctl"
export PATH="$BIN:$PATH"

# A fake upstream with the deploy files and a stand-in "built binary".
mkdir -p "$UPSTREAM"
git -C "$UPSTREAM" init -q
mkdir -p "$UPSTREAM/deploy/pi" "$UPSTREAM/robot/target/release"
cp "$SRC_REPO/deploy/pi/update.sh" "$UPSTREAM/deploy/pi/"
cp "$SRC_REPO/deploy/pi/robot.service" "$UPSTREAM/deploy/pi/"
cp "$SRC_REPO/deploy/pi/hsl-robot-update.service" "$UPSTREAM/deploy/pi/"
cp "$SRC_REPO/deploy/pi/hsl-robot-update.timer" "$UPSTREAM/deploy/pi/"
echo "v1" >"$UPSTREAM/robot/target/release/robot"
git -C "$UPSTREAM" add -A >/dev/null
git -C "$UPSTREAM" -c user.email=t@t -c user.name=t commit -qm one
git -C "$UPSTREAM" branch -M main

git clone -q "$UPSTREAM" "$CLONE"
mkdir -p "$CLONE/robot/target/release"; echo "v1" >"$CLONE/robot/target/release/robot"

run() {
  HSL_REPO="$CLONE" HSL_BUILD_USER=nobody HSL_BRANCH=main \
  HSL_BINARY_DEST="$DEST/hsl-robot" HSL_UNIT_DIR="$UNITS" \
  HSL_SCRIPT_DEST="$DEST/hsl-robot-update" HSL_STAMP="$ROOT/stamp" \
    bash "$CLONE/deploy/pi/update.sh" 2>&1
}

fail=0
check() { if [ "$2" = "$3" ]; then echo "  ok: $1"; else echo "  FAIL: $1 (got '$2' want '$3')"; fail=1; fi; }

echo "1. first run, nothing pending -> adopts revision, installs nothing"
out=$(run); echo "$out" | sed 's/^/     /'
check "no binary installed" "$([ -f "$DEST/hsl-robot" ] && echo yes || echo no)" "no"
check "stamp recorded" "$([ -s "$ROOT/stamp" ] && echo yes || echo no)" "yes"

echo "2. upstream moves -> builds, syncs units + self, installs binary, restarts"
echo "# changed" >>"$UPSTREAM/deploy/pi/robot.service"
echo "v2" >"$UPSTREAM/robot/target/release/robot"
git -C "$UPSTREAM" add -A >/dev/null
git -C "$UPSTREAM" -c user.email=t@t -c user.name=t commit -qm two
out=$(run); echo "$out" | sed 's/^/     /'
check "binary installed" "$(cat "$DEST/hsl-robot" 2>/dev/null)" "v2"
check "unit installed" "$([ -f "$UNITS/hsl-robot.service" ] && echo yes || echo no)" "yes"
check "updater self-installed" "$([ -x "$DEST/hsl-robot-update" ] && echo yes || echo no)" "yes"
check "daemon-reload called" "$(grep -c 'daemon-reload' "$ROOT/systemctl.log" 2>/dev/null || echo 0)" "1"
check "service restarted" "$(grep -c 'restart hsl-robot' "$ROOT/systemctl.log" 2>/dev/null || echo 0)" "1"
check "no .new left behind" "$(ls "$UNITS"/*.new "$DEST"/*.new 2>/dev/null | wc -l | tr -d ' ')" "0"

echo "3. run again with nothing new -> no work, no extra daemon-reload"
out=$(run); echo "$out" | sed 's/^/     /'
check "reports up to date" "$(echo "$out" | grep -c 'already up to date')" "1"
check "still one daemon-reload" "$(grep -c 'daemon-reload' "$ROOT/systemctl.log")" "1"

echo "4. build fails -> binary and stamp untouched, so it retries next tick"
echo "v3" >"$UPSTREAM/robot/target/release/robot"
git -C "$UPSTREAM" add -A >/dev/null
git -C "$UPSTREAM" -c user.email=t@t -c user.name=t commit -qm three
cat >"$BIN/runuser" <<EOS
#!/usr/bin/env bash
shift 2; shift
case "\$1" in *"cargo build"*) echo "simulated build failure" >&2; exit 1;; esac
HOME="$ROOT/home" exec bash -c "\$1"
EOS
chmod +x "$BIN/runuser"
before_stamp=$(cat "$ROOT/stamp")
out=$(run); rc=$?
echo "$out" | sed 's/^/     /'
check "exits non-zero" "$rc" "1"
check "binary not replaced" "$(cat "$DEST/hsl-robot")" "v2"
check "stamp unchanged" "$(cat "$ROOT/stamp")" "$before_stamp"

echo
if [ "$fail" = 0 ]; then echo "ALL PASS"; else echo "FAILURES"; fi
exit "$fail"
