#!/usr/bin/env bash
# Builds every fixture app with clang and captures it into captures/ with the
# late-boundary script, so one command regenerates the oracle set on a fresh
# clone. Existing captures are kept; delete one to regenerate it.
#
# Needs: Xcode Command Line Tools (clang, gpucapture). Captures use
# gpucapture, not the replayer, so no replayer hygiene is needed here.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p captures
BIN="${TMPDIR:-/tmp}/ktx2-fetch-fixtures"
mkdir -p "$BIN"

build() {
  local name="$1"; shift
  clang -fobjc-arc -fmodules -O0 -o "$BIN/$name" "fixtures/$name.m" \
        -framework Metal -framework Foundation "$@"
}
capture() {
  local name="$1" out="captures/$2.gputrace"
  if [ -d "$out" ]; then echo "$out exists, keeping it"; return; fi
  fixtures/capture-late.sh "$BIN/$name" "$out"
}

build known-textures;  capture known-textures  known-textures-late
build known-depth;     capture known-depth     known-depth
build known-depth-stencil; capture known-depth-stencil known-depth-stencil
build known-stencil;   capture known-stencil   known-stencil
build known-astc;      capture known-astc      known-astc
build known-ycbcr -framework CoreVideo; capture known-ycbcr known-ycbcr
build known-ambiguous; capture known-ambiguous known-ambiguous
build known-3d;        capture known-3d        known-3d
build known-mips;      capture known-mips      known-mips
echo "all captures present under captures/"
