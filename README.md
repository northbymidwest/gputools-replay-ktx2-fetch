# gputools-replay-ktx2-fetch

[![github](https://img.shields.io/badge/github-northbymidwest%2Fgputools--replay--ktx2--fetch-blue?logo=github)](https://github.com/northbymidwest/gputools-replay-ktx2-fetch)
[![crates.io](https://img.shields.io/crates/v/gputools-replay-ktx2-fetch.svg)](https://crates.io/crates/gputools-replay-ktx2-fetch)
[![docs.rs](https://docs.rs/gputools-replay-ktx2-fetch/badge.svg)](https://docs.rs/gputools-replay-ktx2-fetch)
[![CI](https://github.com/northbymidwest/gputools-replay-ktx2-fetch/actions/workflows/ci.yml/badge.svg)](https://github.com/northbymidwest/gputools-replay-ktx2-fetch/actions/workflows/ci.yml)

Exports every texture of an Xcode `.gputrace` capture as a lossless KTX2
file, in its native pixel format, byte for byte, with the capture's own
metadata attached. It exists because `gpudebug fetch`, the built-in export,
only writes PNG, which drops alpha and destroys float range.

It reads captures through the `gputools-replay-hl` crate and writes every
format that crate describes and Khronos' `ktx validate` accepts:
byte-aligned colour (8/16/32-bit, all numeric kinds, sRGB variants),
single-aspect depth and stencil, and BC, ETC2, EAC, and ASTC block formats
written as raw blocks. Design:
`docs/superpowers/specs/2026-09-02-gputools-replay-ktx2-fetch-hl-design.md`.

## Requirements

- macOS 27 with Xcode Command Line Tools (the engine links the private
  `GPUToolsReplay` framework they ship; no entitlement is needed).
- The `gputools-replay-hl` crate from crates.io (0.1.0), the only
  dependency, pulled in by `cargo build`; no sibling checkout is needed.
- Khronos `ktx` on `PATH` for the oracle suite (installed via Nix here).
- `clang` and `gpucapture` to regenerate the fixture captures.

## Usage

Install from crates.io:

```
cargo install gputools-replay-ktx2-fetch
```

```
gputools-replay-ktx2-fetch <bundle>.gputrace --out <dir> [--max-stream-ref N] [--force-load-unused] [--timeout SECS]
```

Writes one `.ktx2` per fetched texture (level 0, slice 0; a combined
depth-stencil resource becomes a depth file and a `_stencil` sibling) plus
`<dir>/manifest.json`. Exit 0 when nothing failed, 1 when any texture or
the sweep failed (the manifest says which), 2 when the run could not start.

- `--max-stream-ref`: highest streamRef to sweep. streamRefs are assigned
  by the replayer at load time and are not stored in the bundle, so the tool
  sweeps a range and keeps what answers. By default the bound is the
  bundle's index record count plus a margin, which the refs cannot exceed
  (the replayer creates at most one resource per record), and 20000 when
  the bundle cannot be read; the manifest records which. The sweep runs in
  chunks of 2000 refs, so a timeout or replayer error costs one chunk, not
  the run.
- `--force-load-unused`: textures no captured command reads answer only
  with this. On the known-textures fixture, a run without it answers the 3
  textures a captured command uses and reports the other 4 as
  `coverage.listed_not_answered`. When that flag is off, the tool also
  sets `MTLREPLAYER_IGNORE_UNUSED_RESOURCE=1`, so a texture the replayer
  cannot create because nothing uses it is skipped instead of failing the
  whole sweep. The two are never set together: measured on this machine,
  the ignore flag overrides force-load.
- `--timeout` (default 600 s): per fetch. Slow is not hung: fetches take
  from 27 seconds to over 20 minutes on large captures.
- The binary sets `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0` itself,
  before anything else; without it the replayer cannot create its command
  queue in an unentitled process. It also clears any ambient
  `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE` / `MTLREPLAYER_IGNORE_UNUSED_RESOURCE`
  so the run matches what its manifest records.

## What each file carries

The KTX2 header describes the image in the file: one 2D image, level 0,
slice 0, whatever the resource was. The Data Format Descriptor is derived
from the format (channel layout, numeric kind, sRGB transfer (with the
alpha sample of an sRGB format marked linear, as Khronos' own writer
does)) and checked byte for byte against `ktx create`'s own output;
primaries stay UNSPECIFIED because the capture records no colour space.
Key/value data under `gputrace.` records the streamRef, aspect, fetched
stride, whether padded rows were tightened, and, when the bundle's
descriptor was attributed, the resource's mip count, array length, depth,
texture type, usage, and whether that attribution is `certain` or
`ambiguous`.

## Coverage and attribution

The bundle lists what the capture holds; the sweep learns what answers;
the two are joined by creation-order rank (measured in the hl campaign).
`manifest.json` reports `bundle_manifest` (listed count, or that the
bundle has no descriptors, or could not be parsed) and `coverage`
(answered, attributed, unattributed, listed but not answered). An
attribution is `certain` only when the fetched and listed counts for that
exact width, height, and format agree; otherwise it is `ambiguous`, and
its descriptor fields are recorded but trusted for nothing.

## Known gaps

1. Packed colour formats (RGB10A2, RG11B10, RGB9E5, 16-bit packed) are not
   written: the Vulkan bit order needs a fixture-level check first.
2. Mip levels and array slices are not written. The fetch clamps past the
   real range (measured), so the next version walks the descriptor's
   counts, verifies each level's dimensions, and walks slices only for
   `certain` attributions.
3. Volumes: the fetch serves one unidentified z-plane and reports depth 1.
   A certain-attributed volume deeper than 1 is refused by name; an
   ambiguous one ships as one unlabelled plane.
4. Combined depth-stencil aspects carry no descriptor.
5. PVRTC has no KTX2 representation.
6. Alpha is assumed straight; Metal does not record premultiplication.

## Testing

```
cargo test                 # no hardware: DFDs vs ktx create fixtures, writer, emitter, sweep
tools/oracle.sh            # drives the real replayer against captures/ and runs ktx validate
```

The oracle suite needs the fixture captures: `fixtures/build-all.sh`
builds and captures all nine (known-textures, known-depth,
known-depth-stencil, known-stencil, known-astc, known-ycbcr,
known-ambiguous, known-3d, known-mips; see `fixtures/README.md` for what
each proves). The sibling campaign's `known-ds-pair` fixture is
deliberately not used: it recreates its textures inside the capture
window, so its captures hold no stored content (measured against the
sibling's own capture as well). `tools/capture-dfd-fixtures.py`
regenerates the reference DFDs when a format row is added.

**The replayer is a shared, crash-prone resource.** One session per
process, one process per machine. Check `pgrep -x GPUToolsReplayService`
prints nothing before and after a run. An interrupted fetch orphans a
session that locks the replayer for two hours; recover with:

```
gpudebug --terminate all
pkill -9 -f GPUToolsReplayService
```

Pre-commit hook (rustfmt check): `git config core.hooksPath .githooks`.

## License

[BSD Zero Clause License](LICENSE)

### Why 0BSD?

The majority of this codebase was generated by AI coding agents (primarily
Claude). AI-generated code is not copyrightable and is effectively public
domain, making 0BSD, which imposes no restrictions on use, the most
appropriate license.

### Disclaimer

While AI-generated code itself is public domain, AI agents may have reproduced
or closely derived code from copyrighted sources (training data, reference
implementations, open-source projects, etc.). No audit has been conducted to
identify such instances, as this is a personal side project. Any such code
fragments remain subject to the licenses of their original creators. Use at
your own discretion.
