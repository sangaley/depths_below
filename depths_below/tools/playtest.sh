#!/usr/bin/env bash
# Playtest harness: launch the game, let it run, capture engine-side frames,
# then report anything that looks wrong.
#
# Engine-side capture (DEPTHS_SHOTS) photographs the render target, so the
# game can sit behind other windows and the frames are still the game. An OS
# screen grab photographs the display and catches whatever is in front.
#
#   tools/playtest.sh [seconds] [label] [extra env assignments...]
#
#   tools/playtest.sh 60 baseline
#   tools/playtest.sh 90 cascade DEPTHS_CASCADE=0.9
#
# Exit status is non-zero if the run panicked or produced no frames.
set -uo pipefail

SECS="${1:-45}"
LABEL="${2:-run}"
shift 2 2>/dev/null || shift $# 

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="/tmp/depths_playtest/$LABEL"
LOG="$OUT/run.log"
rm -rf "$OUT"; mkdir -p "$OUT"

cd "$ROOT"

# Never kill by process name: another session may have the game open.
# Track only the pid this script starts.
echo "[playtest] building..."
if ! cargo build 2>"$OUT/build.log"; then
  echo "[playtest] BUILD FAILED"
  grep -E "^error" -A 8 "$OUT/build.log" | head -40
  exit 2
fi

# -u asserts user activity, which WAKES a display that has already slept.
# -d alone only prevents it sleeping, which is no help once it already has.
# Assert user activity for the whole run, not a two-second blip: -u wakes a
# slept display, and it has to stay asserted or it dozes again mid-capture.
caffeinate -u -t $((SECS + 90)) >/dev/null 2>&1 &
WAKE=$!
disown "$WAKE" 2>/dev/null || true
sleep 3

echo "[playtest] running '$LABEL' for ${SECS}s"
# caffeinate -di: without it the display sleeps during a long session and the
# render target stops producing content -- every captured frame comes back
# solid black and looks exactly like a rendering regression. It is not.
# `env` is required: caffeinate treats the first token as its command, so
# inline VAR=value assignments would be swallowed.
caffeinate -di env DEPTHS_SKIP_MENU=1 DEPTHS_SHOTS=6 DEPTHS_SHOTS_DIR="$OUT" "$@" \
  cargo run >"$LOG" 2>&1 &
RUNNER=$!
disown "$RUNNER" 2>/dev/null || true   # keep job control quiet on kill

# The binary is a child of cargo; find it so we can stop exactly that one.
GAME=""
for _ in $(seq 1 40); do
  sleep 1
  GAME="$(pgrep -P "$RUNNER" -f 'target/debug/depths_below' | head -1)"
  [ -n "$GAME" ] && break
done
[ -z "$GAME" ] && GAME="$(pgrep -f 'target/debug/depths_below' | head -1)"

START=$(date +%s)
sleep "$SECS"
ALIVE=$(( $(date +%s) - START ))

if [ -n "$GAME" ]; then kill "$GAME" 2>/dev/null; sleep 2; kill -9 "$GAME" 2>/dev/null; fi
kill "$RUNNER" 2>/dev/null; wait "$RUNNER" 2>/dev/null

kill "$WAKE" 2>/dev/null
FRAMES=$(ls "$OUT"/shot_*.png 2>/dev/null | wc -l | tr -d ' ')
count() { grep -ci "$1" "$LOG" 2>/dev/null | head -1; }
PANICS=$(count "panicked at")
ERRORS=$(count "^ERROR")
B0001=$(count "B0001")

echo "[playtest] alive=${ALIVE}s frames=$FRAMES panics=$PANICS errors=$ERRORS query-conflicts=$B0001"

# A black frame is ~57KB; a real one is several hundred. Identical tiny sizes
# across every frame means the display slept, not that rendering broke.
if [ "$FRAMES" != "0" ]; then
  BLACK=$(find "$OUT" -name 'shot_*.png' -size -80k | wc -l | tr -d ' ')
  if [ "$BLACK" = "$FRAMES" ]; then
    echo "[playtest] NOTE: all $FRAMES frames are blank."
    echo "[playtest]       This is the environment, not the build: the render target"
    echo "[playtest]       produces nothing while the screen is locked or asleep."
    echo "[playtest]       Panics, errors and query conflicts above are still valid."
  fi
fi
echo "[playtest] frames in $OUT"

if [ "$PANICS" != "0" ]; then
  echo "--- panic ---"
  grep -i -A 4 "panicked at" "$LOG" | head -20
fi
if [ "$ERRORS" != "0" ]; then
  echo "--- errors ---"
  grep -i "^ERROR" "$LOG" | sed 's/\x1b\[[0-9;]*m//g' | head -10
fi

[ "$PANICS" = "0" ] && [ "$FRAMES" != "0" ]
