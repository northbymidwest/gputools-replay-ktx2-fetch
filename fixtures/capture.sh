#!/usr/bin/env bash
# Captures a .gputrace from a fixture app using gpucapture(1).
# Usage: fixtures/capture.sh <app-binary> <output.gputrace>
#
# The app must create an MTLDevice. It is launched with capture enabled and
# halted until the capture signal, so the trace is deterministic.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <app-binary> <output.gputrace>" >&2
  exit 2
fi
APP="$1"; OUT="$2"

# MTL_CAPTURE_ENABLED loads GPUToolsCapture into the target; WAIT_FOR_SIGNAL
# halts it on device creation so gpucapture start controls the boundary.
MTL_CAPTURE_ENABLED=1 MTLCAPTURE_WAIT_FOR_SIGNAL=1 "$APP" &
APP_PID=$!
echo "launched $APP as pid $APP_PID"

# The app does not become capturable the instant it is spawned: it has to load
# GPUToolsCapture and create its MTLDevice first. Calling `gpucapture start`
# before that fails with "invalid PID" even though the process exists, so wait
# for the PID to appear in `gpucapture list` rather than racing it.
#
# On any failure past this point the app must be killed: MTLCAPTURE_WAIT_FOR_-
# SIGNAL leaves it halted forever, and a leaked halted process keeps showing up
# as capturable and confuses the next run.
cleanup() { kill -9 "$APP_PID" 2>/dev/null || true; }
trap cleanup EXIT

READY=0
for _ in $(seq 1 100); do
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "app exited before it became capturable; is MTL_CAPTURE_ENABLED honoured?" >&2
    exit 1
  fi
  if gpucapture list 2>/dev/null | awk '{print $1}' | grep -qx "$APP_PID"; then
    READY=1
    break
  fi
  sleep 0.1
done
if [ "$READY" -ne 1 ]; then
  echo "app never appeared in \`gpucapture list\`; nothing to capture" >&2
  exit 1
fi
echo "pid $APP_PID is capturable; starting capture..."

# --until-exit captures until the process exits (good for CLI workloads).
gpucapture start --pid "$APP_PID" --until-exit --output "$OUT"

trap - EXIT
wait "$APP_PID" || true
echo "capture written to $OUT"
