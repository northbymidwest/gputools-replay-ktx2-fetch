#!/usr/bin/env bash
# Runs the oracle suite with replayer hygiene. Refuses to start if a
# GPUToolsReplayService is already running (an orphaned session locks the
# replayer for two hours), serialises the tests, and warns if the service is
# still up afterwards. Do NOT Ctrl-C a running fetch: latency is 27 s to
# 20+ min, and interrupting orphans the session.
#
# Usage: tools/oracle.sh [extra cargo test args, e.g. --test oracle_depth]
set -euo pipefail
cd "$(dirname "$0")/.."

if pgrep -x GPUToolsReplayService >/dev/null; then
  echo "REFUSING: a GPUToolsReplayService is already running. Recover with:" >&2
  echo "  gpudebug --terminate all; pkill -9 -f GPUToolsReplayService" >&2
  exit 1
fi
command -v ktx >/dev/null || { echo "ktx (Khronos KTX-Software) must be on PATH" >&2; exit 1; }

export MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0
START=$(date +%s)
set +e
cargo test --features oracle "$@" -- --test-threads=1
CODE=$?
set -e
echo "== oracle suite exited $CODE after $(($(date +%s) - START))s =="
if pgrep -x GPUToolsReplayService >/dev/null; then
  echo "WARNING: a GPUToolsReplayService is still running. If the next run is refused:" >&2
  echo "  gpudebug --terminate all; pkill -9 -f GPUToolsReplayService" >&2
fi
exit $CODE
