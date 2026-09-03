#!/usr/bin/env bash
# Captures a fixture app with the capture boundary AFTER the app has created
# its resources, so the trace holds resources that pre-existed the capture.
# Companion to capture.sh, which (via MTLCAPTURE_WAIT_FOR_SIGNAL) puts the
# boundary at device creation so that everything is created inside it.
#
# Usage: fixtures/capture-late.sh <app-binary> <output.gputrace>
#
# The app must honour FIXTURE_GO_FILE (or KNOWN_TEXTURES_GO_FILE, a legacy
# alias; both are exported to the same path): do its phase-1 work, then block
# until that file exists, then do phase-2 work and exit. This script starts the
# capture during the block and only then creates the file.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <app-binary> <output.gputrace>" >&2
  exit 2
fi
APP="$1"; OUT="$2"
GO=$(mktemp "${TMPDIR:-/tmp}/known-textures-go.XXXXXX")
rm -f "$GO"

# No WAIT_FOR_SIGNAL: the app runs phase 1 immediately.
MTL_CAPTURE_ENABLED=1 KNOWN_TEXTURES_GO_FILE="$GO" FIXTURE_GO_FILE="$GO" "$APP" &
APP_PID=$!
cleanup() { kill -9 "$APP_PID" 2>/dev/null || true; rm -f "$GO"; }
trap cleanup EXIT
echo "launched $APP as pid $APP_PID (go-file $GO)"

# Wait for phase 1 to finish and the app to be capturable. Phase 1 takes
# milliseconds; being listed by gpucapture is what actually takes time.
for _ in $(seq 1 100); do
  kill -0 "$APP_PID" 2>/dev/null || { echo "app exited early" >&2; exit 1; }
  if gpucapture list 2>/dev/null | awk '{print $1}' | grep -qx "$APP_PID"; then break; fi
  sleep 0.1
done
sleep 0.5  # phase 1 is long finished by now; this is belt and braces

echo "starting capture with the app blocked between phases..."
gpucapture start --pid "$APP_PID" --until-exit --output "$OUT" &
CAP_PID=$!
sleep 1.5  # let gpucapture attach and begin before releasing phase 2
touch "$GO"
echo "released phase 2"

wait "$CAP_PID"
trap - EXIT
wait "$APP_PID" || true
rm -f "$GO"
echo "capture written to $OUT"
