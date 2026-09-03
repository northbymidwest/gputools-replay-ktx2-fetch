# ktx2-fetch 0.2: lossless texture export via gputools-replay-hl

Design spec. 2026-09-02, revised the same day after the hl surface review
(section 13). Successor to `../gputrace-tool-2`'s `ktx2-fetch` (spec
`2026-08-31-gputrace-tool-2-design.md` in that repo). Same purpose and
same output contract; the private-framework engine tool-2 carried is
replaced by the `gputools-replay-hl` crate, and the output is widened to
every format that crate describes and Khronos' `ktx validate` accepts.

Conventions: a claim marked MEASURED names how it was measured, and
"dossier 00" means `docs/findings/00-texture-fetch.md` in the
`gputools-replay` repo. Anything not marked is a design decision, not a
fact about the replayer.

## 1. Purpose

Xcode's `gpudebug fetch` exports textures only as PNG, and PNG loses data
that matters: alpha is dropped and float range is destroyed (MEASURED by
tool-2, whose README carries the figures). `ktx2-fetch` writes each
texture of a `.gputrace` capture as a KTX2 file in its native pixel
format, byte for byte, with no colour management, no channel reordering,
and no alpha loss.

The 0.2 rewrite exists for two reasons:

1. **Engine.** tool-2 carried its own FFI bootstrap, reply parser, and a
   four-entry format table. `gputools-replay-hl` now provides all of that
   as a library (`Capture`, `Texture`, `FormatKind`), plus a session-free
   reader of the bundle's own texture list, with a regression test
   against the live runtime that this tool inherits for free.
2. **Metadata.** tool-2 wrote a linear transfer and a four-format table.
   hl's format table exposes sRGB, numeric kind, channel layout, and block
   geometry for every `MTLPixelFormat`, and the bundle reader exposes mip
   count, array length, and texture type. This tool encodes every one of
   those into the KTX2 container wherever the source determines it, and
   leaves a field unspecified only when the capture genuinely does not
   establish it. Colour primaries are the one such field.

## 2. Non-goals

- Not a replacement for `gpudebug`.
- No PNG, no transcoding, no supercompression, no decompression of block
  formats: compressed textures are written as their raw blocks.
- No mip levels or array slices in 0.2. hl selects them correctly
  (MEASURED, `known-mips`) and the bundle reader supplies the counts, so
  this is the first follow-up (section 10, gap 2). 0.2 already records
  the one fact that follow-up depends on: whether each texture's
  attribution is certain (section 5, step 5).
- No buffers, heaps, pipelines, acceleration structures, or wireframes.
- No dependency on `gpudebug`, on `../gputrace-tool-2`, or on the
  `gputools-replay` workspace beyond the one crate dependency (section 4).

## 3. Naming and CLI

```
ktx2-fetch <bundle>.gputrace --out <dir> [--max-stream-ref N] [--force-load-unused] [--timeout SECS]
```

- `--max-stream-ref` (default 2000): highest streamRef swept. streamRef
  values are assigned by the replayer's load path and are not stored in
  the bundle (MEASURED, dossier 00), so a sweep is the only way to learn
  them. Refs are sparse; the tool keeps whatever answers.
- `--force-load-unused`: sets `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1`
  via `ReplayerConfig`. Needed for captures whose textures are never read
  by a captured command (MEASURED on `known-textures-late`: 3 of 7 answer
  without it, the ones a captured command uses; 7 with it). When the flag
  is off the tool also sets `MTLREPLAYER_IGNORE_UNUSED_RESOURCE=1` via
  `ReplayerConfig::ignore_unused_resources`; without that, one unused
  texture fails the whole batched fetch (MEASURED: MTLReplayer error 150,
  "Metal object creation failed", `GTErrorKeyResourceUnused=true`). The
  two variables are never set together: with both set the ignore flag
  overrides force-load (MEASURED).
- `--timeout` (default 600): per-fetch timeout passed to
  `Capture::set_timeout`. Fetch latency legitimately ranges from about 27
  seconds to over 20 minutes on large captures.

The crate is `ktx2-fetch` version `0.1.0`, edition 2024. ("0.2" in this document names the rewrite's generation relative to tool-2, not the crate version: the tool-2 crate was a throwaway and this crate starts at 0.1.0.) One binary, one
library (`ktx2_fetch`) so the modules are unit-testable.

## 4. Architecture

### 4.1 Dependency

`gputools-replay-hl` is the only external tie, as a published crate:

```toml
gputools-replay-hl = "0.1.0"
```

(0.2 was built against a path dependency on the sibling checkout until
0.1.0 was published on 2026-09-03; the switch changed nothing but the
dependency line and the manifest's `engine` source.) The tool uses only hl's public surface: `Capture`, `Texture`,
`Aspect`, `ReplayerConfig`, `Error`, `MTLPixelFormat`, `MTLTextureType`,
`TextureDescriptor`, and the `format` and `describe` modules. hl
re-exports the bundle reader's types, so the tool never names
`gputools-replay-bundle` or `gputools-replay` directly.

The engine version recorded in the manifest is read at build time by
`build.rs`, which reads the `gputools-replay-hl` entry of `Cargo.lock`
(version and source) with the `toml` crate, the crate's one
build-dependency; it comes from no hl API.

Other dependencies: `clap` (derive), `serde` + `serde_json` (manifest),
`thiserror`. No `objc2`, no `plist`, no `block2`: those were the engine.
Build-dependency: `toml`.

### 4.2 Modules

Each module has one job and is testable without the others.

| module | job | depends on |
| --- | --- | --- |
| `main.rs` | env setup, arg parsing, orchestration, exit code | everything below |
| `sweep.rs` | the two-pass fetch, dedupe, and descriptor attribution; returns `Sweep` | hl |
| `vkformat.rs` | `MTLPixelFormat` -> `VkFormat` table | hl `MTLPixelFormat` |
| `dfd.rs` | KTX2 Data Format Descriptor derived from hl `FormatKind` | hl `format` |
| `ktx.rs` | KTX2 container writer | `dfd` (as bytes only) |
| `emit.rs` | one `Fetched` -> one file, or one recorded failure | `vkformat`, `dfd`, `ktx`, `manifest` |
| `manifest.rs` | `manifest.json` types, exit-code policy | serde |

`Sweep` is `{ fetched: Vec<Fetched>, probes: Vec<StencilProbe>,
duplicates: Vec<Duplicate>, coverage: Coverage }`. `Fetched` is
`{ texture: Texture, aspect: Aspect, descriptor: Option<TextureDescriptor> }`
where `Aspect` is the tool's own three-valued enum (`Color`, `Depth`,
`Stencil`), distinct from hl's two-valued fetch selector.

### 4.3 Unsafe policy

`#![deny(unsafe_code)]` on the library. The binary allows exactly one
`unsafe` block: the two environment writes at the top of `main`
(section 9). `deny(clippy::unwrap_used, clippy::expect_used,
clippy::indexing_slicing)` outside tests, inherited from tool-2.

## 5. Data flow

1. `Capture::open(bundle)`. The substrate checks the bundle's shape and
   the unlock env var before touching anything global, so the tool does
   no pre-validation of its own.
2. **Manifest status.** `cap.manifest_status()` is recorded verbatim:
   `Ok(n)`, the number of textures the bundle lists; `NoDescriptors`
   (parsed, zero descriptors: `sample.gputrace`'s older schema); or
   `Unparseable` (hl carries no reason string). This is the coverage
   denominator and is independent of every later step.
3. **Pass 1 (colour, compressed, depth, plain stencil).**
   `cap.textures(0..=max_stream_ref)`. Each `Texture` is classified by
   `format_kind()`:
   - `is_depth_only()` -> `Aspect::Depth`
   - `is_stencil_only()` -> `Aspect::Stencil` (a base `Stencil8`, or an
     app-created `X24/X32_Stencil8` view; MEASURED, `known-stencil`)
   - everything else -> `Aspect::Color`
4. **Dedupe.** The fetch can emit one streamRef twice (MEASURED, dossier
   00: retroarch refs 1121 and 1122). Records sharing `(stream_ref,
   aspect)` are compared byte for byte: identical ones collapse to one
   `Fetched` and a `Duplicate { stream_ref, identical: true }` entry;
   differing ones are both dropped and recorded as a per-texture failure,
   since the tool cannot say which is the texture. This is the check
   tool-2 lacked when it overwrote one file with another's.
5. **Describe.** `cap.describe(&textures)` joins pass-1 textures to the
   bundle's descriptors by creation-order rank (MEASURED, dossier 00: 208
   matches, zero ordering violations across the fixture corpus). It never
   fetches and never fails on a gap: each texture gets
   `Some(descriptor)` or `None`, and descriptors nothing claimed come
   back as `unplaced`. The tool records `Coverage { listed, answered,
   attributed, unattributed, listed_not_answered }` from those lists.
   Attribution is positional, so an undescribed texture that shares
   exact dimensions and format with a described one and sorts earlier
   can shift attribution by one silently (documented hl limitation).
   The tool grades every attribution: for each exact `(width, height,
   format)` key, if the number of fetched pass-1 textures with that key
   equals the number of descriptors with that key, every attribution in
   that group is `certain`; otherwise (more fetched than listed, or more
   listed than answered) every attribution in the group is `ambiguous`,
   because a rank zip over an unequal group can pair a texture with a
   neighbour's descriptor. In 0.2 descriptor fields are metadata only
   and never change which bytes are written, so the grade is recorded
   and nothing else depends on it; the mip/slice follow-up (gap 2) will
   walk only `certain` attributions.
6. **3D check.** The fetch cannot reveal a volume: `Texture::depth()`
   reads 1 even for a 16x16x4 volume, because the fetch serves exactly
   one fixed z-plane and reports that plane (MEASURED, hl
   `live_hl_provenance_3d`, `known-3d`). The only 3D signal is the
   descriptor's `texture_type`. So: a texture whose attribution is
   `certain`, whose descriptor type is `Type3D`, and whose descriptor
   depth exceeds 1 is a per-texture failure ("3D texture, depth N: the
   fetch serves one unidentified z-plane"), since the tool cannot say
   which plane it holds. A `Type3D` descriptor with depth 1 is written:
   its one plane is the whole resource, and its type is recorded in the
   KV data. An `ambiguous` attribution is always written, whatever the
   descriptor says, because a shifted descriptor must never withhold
   real bytes; the ambiguity is recorded. This is the one place a
   descriptor field decides whether a file is written, and it does so
   only under a `certain` grade, where the rank invariant has no room
   to shift.
7. **Pass 2 (stencil aspect of combined textures).** For every pass-1
   texture classified `Depth`, request `cap.texture_aspects(refs,
   Aspect::Stencil)` in one batch. MEASURED (dossier 00, `known-ds-pair`):
   a `Depth32Float_Stencil8` resource is one streamRef; plane 0 serves
   its depth aspect as `Depth32Float` and plane 1 its stencil aspect as
   `X24_Stencil8` at one byte per pixel. Plane is inert on ordinary
   textures, so a plain depth texture answers plane 1 with its depth
   again. A pass-2 reply is kept only if `is_stencil_only()`; anything
   else is dropped as an echo. Each probed ref is recorded as a
   `StencilProbe { stream_ref, outcome: Written | Absent }`. Stencil
   aspects carry no descriptor (combined descriptors are transparent to
   the join by design).
8. A fetch error in either pass is a run-level `sweep_error`. Pass-1
   records already in hand are still emitted.
9. Every `Fetched` goes through `emit`. Per-texture failures are recorded
   and never abort the run.
10. The manifest is written last. If that write fails, the manifest is
    printed to stderr so the run record is not lost (tool-2 policy).

## 6. Format mapping

### 6.1 The VkFormat table

`vkformat.rs` maps each supported `MTLPixelFormat` to a `VkFormat` value.
Metal format names come from hl's `format::name()`; the tool keeps no
name table of its own. A format with no row is a per-texture failure that
names the raw value and the hl name if there is one; the table is never
guessed at.

**Every row is confirmed, not assumed:** the implementation plan requires
that each `VkFormat` value be read back from a file written by `ktx create
--raw --format <NAME>` (Khronos' reference writer), and that an 8x8 file
of ours in that format pass `ktx validate`. That is how tool-2 confirmed
its four rows, and it is the only admission rule for a new row.

Coverage in 0.2:

**Byte-aligned colour** (hl `ColorFormat` with `byte_aligned`):

| Metal | Vulkan |
| --- | --- |
| A8Unorm | A8_UNORM_KHR |
| R8Unorm / _sRGB / Snorm / Uint / Sint | R8_{UNORM,SRGB,SNORM,UINT,SINT} |
| RG8Unorm / _sRGB / Snorm / Uint / Sint | R8G8_{...} |
| RGBA8Unorm / _sRGB / Snorm / Uint / Sint | R8G8B8A8_{...} |
| BGRA8Unorm / _sRGB | B8G8R8A8_{UNORM,SRGB} |
| R16Unorm / Snorm / Uint / Sint / Float | R16_{UNORM,SNORM,UINT,SINT,SFLOAT} |
| RG16 (same five) | R16G16_{...} |
| RGBA16 (same five) | R16G16B16A16_{...} |
| R32Uint / Sint / Float | R32_{UINT,SINT,SFLOAT} |
| RG32 (same three) | R32G32_{...} |
| RGBA32 (same three) | R32G32B32A32_{...} |

**Single-aspect depth and stencil:**

| Metal | Vulkan | bytes/px as served |
| --- | --- | --- |
| Depth16Unorm | D16_UNORM | 2 |
| Depth32Float | D32_SFLOAT | 4 |
| Stencil8 | S8_UINT | 1 |
| X24_Stencil8, X32_Stencil8 | S8_UINT | 1 |

The X24/X32 rows are written at 1 byte per pixel because that is the
stride the replayer serves (MEASURED, encoded in hl's `checked_bpp`), not
the 4 or 8 bytes the nominal Metal format implies.

**Block-compressed:**

| Metal | Vulkan |
| --- | --- |
| BC1_RGBA / _sRGB | BC1_RGBA_{UNORM,SRGB}_BLOCK |
| BC2_RGBA / _sRGB | BC2_{UNORM,SRGB}_BLOCK |
| BC3_RGBA / _sRGB | BC3_{UNORM,SRGB}_BLOCK |
| BC4_RUnorm / RSnorm | BC4_{UNORM,SNORM}_BLOCK |
| BC5_RGUnorm / RGSnorm | BC5_{UNORM,SNORM}_BLOCK |
| BC6H_RGBFloat / RGBUfloat | BC6H_{SFLOAT,UFLOAT}_BLOCK |
| BC7_RGBAUnorm / _sRGB | BC7_{UNORM,SRGB}_BLOCK |
| ETC2_RGB8 / _sRGB | ETC2_R8G8B8_{UNORM,SRGB}_BLOCK |
| ETC2_RGB8A1 / _sRGB | ETC2_R8G8B8A1_{UNORM,SRGB}_BLOCK |
| EAC_RGBA8 / _sRGB | ETC2_R8G8B8A8_{UNORM,SRGB}_BLOCK |
| EAC_R11Unorm / Snorm | EAC_R11_{UNORM,SNORM}_BLOCK |
| EAC_RG11Unorm / Snorm | EAC_R11G11_{UNORM,SNORM}_BLOCK |
| ASTC_WxH_LDR / _sRGB | ASTC_WxH_{UNORM,SRGB}_BLOCK |
| ASTC_WxH_HDR | ASTC_WxH_SFLOAT_BLOCK |

### 6.2 Deliberately unmapped (section 10)

- **Packed colour**: RGB10A2, BGR10A2, RG11B10, RGB9E5, B5G6R5, A1BGR5,
  ABGR4, BGR5A1. Vulkan's `_PACK32`/`_PACK16` names describe bit
  positions from the opposite end to Metal's channel order, and
  `ktx validate` checks structure, not channel semantics, so a wrong row
  would validate and lie. Admitted only with a bit-level fixture check.
- **PVRTC**: no `VkFormat` exists for it.
- **`Depth32Float_Stencil8` (260)**: never served as a combined format
  (section 5, step 7). Its aspects are covered.
- **`FormatKind::Unknown`**: hl's table does not describe it.

### 6.3 The Data Format Descriptor

`dfd.rs` derives the DFD from hl's `FormatKind`; it has no per-format
table of its own.

- **Colour.** One sample per `Channel` in memory order, `bitOffset`
  accumulated from the preceding channels' widths, `bitLength = bits - 1`,
  `channelType` R=0, G=1, B=2, A=15. Flags from `NumericKind`: `FLOAT |
  SIGNED` for Float, `SIGNED` for Snorm and Sint, none for Unorm and
  Uint. The alpha sample of an sRGB colour format additionally carries the
  `LINEAR` qualifier (0x10): alpha bypasses the block's sRGB transfer, and
  `ktx create` writes it so (MEASURED by the fixture test).
  `sampleLower`/`sampleUpper` per the Khronos Data Format spec for
  each kind and width. `typeSize` in the header is the channel width in
  bytes.
- **Transfer.** `KHR_DF_TRANSFER_SRGB` when `FormatKind::is_srgb()`,
  otherwise `KHR_DF_TRANSFER_LINEAR`. The format itself declares this,
  so writing it is accurate.
- **Primaries.** `KHR_DF_PRIMARIES_UNSPECIFIED`, always. The capture does
  not record a colour space, `gpudebug` does not report one, and float
  render targets are not colour at all. This is the one field left open.
- **Alpha.** `KHR_DF_FLAG_ALPHA_STRAIGHT`, disclosed as an assumption in
  the KV data as tool-2 did: Metal does not record premultiplication.
- **Depth / stencil.** `channelType` DEPTH (14) or STENCIL (13), colour
  model RGBSDA, flags as for the numeric kind.
- **Compressed.** The reference writer's own DFD for that `VkFormat`,
  captured as a fixture (below) and embedded, with `primaries` set to
  `UNSPECIFIED`. The KDF sample layouts for block models are
  model-specific conventions rather than derivable structure (MEASURED
  from `ktx create` output: BC1 encodes "alpha present" as channel id 1,
  ETC2 uses a COLOR channel id 2, BC6H UFLOAT is FLOAT without SIGNED),
  so Khronos' writer is the honest source and hl's `CompressedFormat`
  is used only to select the row and to size the payload.

**The generator is checked against the reference writer, not against
itself.** For every supported `VkFormat`, a script captures the DFD bytes
`ktx create --raw` emits into `tests/fixtures/dfd/<VkFormat>.dfd`, and a
unit test asserts our output equals those bytes once the fixture's
`primaries` byte is set to UNSPECIFIED (Khronos writes BT709 for colour
and already writes UNSPECIFIED for depth and stencil). MEASURED for this
spec: all 111 VkFormats in 6.1 were created with `ktx create --raw`
(Khronos `ktx` v5.0.0-rc1), pass `ktx validate`, and still pass with
`primaries` patched to UNSPECIFIED. This replaces tool-2's hand-typed
sample tables with an external oracle.

## 7. Output

### 7.1 Payload

KTX2 requires tightly packed rows. `emit` picks the payload path from
`format_kind()`:

- **Colour / single-aspect depth-stencil**: `texture.packed_bytes()`,
  hl's tight-row view: borrowed when `bytes_per_row == width * bpp`,
  otherwise a copy with the trailing padding of each row dropped and the
  pixel bytes untouched. It performs hl's own `Truncated` check.
  `rowsRepacked` records whether a copy was made. tool-2 refused padded
  textures; 0.2 writes them.
- **Compressed**: `blocks().bytes`, with `Blocks::expected_len()` (hl's
  count based on `ceil(width / block.0)`, not the padding-inclusive
  `blocks_per_row`) as the required payload length. A length mismatch is
  a per-texture failure.

### 7.2 Naming

`ref{stream_ref}_{W}x{H}_{MetalName}.ktx2`, with `_stencil` appended for a
pass-2 stencil aspect. After the dedupe in section 5 step 4, the identity
of a fetched record is `(stream_ref, aspect)`, which is exactly what one
request asks for, so two files in one run can never collide. tool-2's
`tex{ordinal:04}` prefix is gone: that field is a per-session request
ordinal, not a resource identity.

### 7.3 Per-file provenance

KTX2 key/value data, keys sorted, under the `gputrace.` prefix (MEASURED
by tool-2: `ktx validate` rejects unknown keys starting with `ktx`/`KTX`,
and accepts `gputrace.`). Keys marked (desc) are written only when the
texture was attributed a descriptor.

```
KTXwriter                 ktx2-fetch 0.1.0
gputrace.arrayLength      <n>                        (desc)
gputrace.aspect           color | depth | stencil
gputrace.assumptions      MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0; MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=<0|1>; MTLREPLAYER_IGNORE_UNUSED_RESOURCE=<1|0>; alpha assumed straight (Metal does not record premultiplication)
gputrace.bundle           <bundle path as given on the command line>
gputrace.bytesPerImage    <as fetched>
gputrace.bytesPerRow      <stride as fetched>
gputrace.descriptorAttribution  certain | ambiguous   (desc)
gputrace.depth            <descriptor depth: 1 for 2D, N for a volume> (desc)
gputrace.mipLevelCount    <n>                        (desc)
gputrace.mtlPixelFormat   <MetalName> (<raw value>)
gputrace.rowsRepacked     true | false
gputrace.streamRef        <n>
gputrace.textureType      <MTLTextureType name>      (desc)
gputrace.textureUsage     <MTLTextureUsage bits>     (desc)
```

The descriptor keys describe the resource the bundle lists. **The KTX2
header describes the image in the file, which is always one 2D image:
level 0, slice 0, plane 0 of the resource**, whether that resource is a
2D texture, one slice of an array, one face of a cube, or the single
plane of a depth-1 volume. `pixelDepth`, `layerCount`, and `faceCount`
in the header are therefore 0, 0, and 1 for every file 0.2 writes, and
the resource's real type lives in `gputrace.textureType`. Nothing in the
tool infers a resource type from the fetch: the fetched record's own
depth field reads 1 for every served image, volume or not. The unmapped
reply fields tool-2 preserved are not written: MEASURED (dossier 00) they
are constant across every texture and carry no per-resource
information.

### 7.4 Run manifest

```json
{
  "bundle": "...",
  "tool_version": "0.1.0",
  "engine": "gputools-replay-hl <version from cargo metadata>",
  "max_stream_ref": 2000,
  "force_load_unused": false,
  "timeout_secs": 600,
  "assumptions": ["..."],
  "bundle_manifest": {"status": "ok", "textures_listed": 180}
                  | {"status": "no_descriptors"}
                  | {"status": "unparseable"},
  "coverage": {"answered": 182, "attributed": 180, "unattributed": 2, "listed_not_answered": 0},
  "textures": [
    {"stream_ref": 25, "aspect": "color", "file": "ref25_2880x2592_BGRA8Unorm.ktx2",
     "mtl_pixel_format": "BGRA8Unorm", "mtl_pixel_format_raw": 80,
     "vk_format": "B8G8R8A8_UNORM", "width": 2880, "height": 2592,
     "bytes_per_row": 11520, "rows_repacked": false,
     "descriptor": {"mip_levels": 1, "array_length": 1, "depth": 1, "texture_type": "2D",
                    "usage": 5, "attribution": "certain"} | null}
  ],
  "duplicates": [{"stream_ref": 1121, "identical": true}],
  "stencil_probes": [{"stream_ref": 4, "outcome": "written"}],
  "failures": [{"stream_ref": 7, "aspect": "color",
                "reason": "unsupported MTLPixelFormat RGB10A2Unorm (90): packed formats are not mapped"}],
  "sweep_error": null
}
```

`coverage` is present only when `bundle_manifest.status` is `ok`.
`answered` counts distinct pass-1 streamRefs after dedupe.
`textures_listed` is the bundle's full descriptor count, including
combined depth-stencil descriptors; `listed_not_answered` counts only
descriptors the join could have placed, so the two need not sum with
`attributed`.

**Exit code:** 0 if `failures` is empty and `sweep_error` is null; 1
otherwise; 2 for a failure before the sweep began (bad arguments, bundle
cannot be opened), in which case no manifest is written. An empty
`textures` list warns on stderr (wrong bundle, or `--max-stream-ref` too
low) but is not a failure. `listed_not_answered > 0` is reported, not a
failure: it is the measured coverage gap, and the usual fix is
`--force-load-unused`, which the warning names.

## 8. Error handling

| tier | examples | where it lands | exit |
| --- | --- | --- | --- |
| before the sweep | bad bundle, unlock var unset, session already open, `--out` not creatable | stderr | 2 |
| run-level | `Error::Session` / `Error::Fetch` from either pass | `sweep_error` | 1 |
| per-texture | unmapped format, certain-attributed volume with depth > 1, conflicting duplicate, `Truncated`, `FormatMismatch`, payload length mismatch, file write error | `failures[]` | 1 |
| informational | manifest unparseable, unattributed textures, unanswered descriptors, identical duplicates | `bundle_manifest`, `coverage`, `duplicates` | 0 |

hl's `Error` is matched by variant and only turned into a string when it
is written to the manifest, so the reason text always says which kind of
failure it was.

## 9. Operational constraints

Inherited from the substrate; restated because they bind this tool's
`main` and its test harness.

- **`MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0` is mandatory.** The
  binary sets it as the first statement of `main`, before any thread
  exists; `Capture::open` (via the substrate) verifies it and refuses
  with a named error otherwise. `Capture::configure_env` is called right
  after it, under the same single-threaded precondition, to apply
  `--force-load-unused`. When that flag is off, `ignore_unused_resources`
  is set instead (section 3).
- **One session per process, one process per machine.** The CLI opens
  exactly one `Capture`. Every oracle test is its own integration-test
  binary, and `tools/oracle.sh` runs them with `--test-threads=1`.
- **An interrupted run orphans the replayer for two hours.** Recovery:
  `gpudebug --terminate all; pkill -9 -f GPUToolsReplayService`.
  `tools/oracle.sh` refuses to start if `GPUToolsReplayService` is
  already running, and warns if it is still running afterwards.

## 10. Known gaps carried forward

1. **Packed colour formats are not written** (6.2). Needs a fixture with
   a known bit pattern per channel and a decoder-side check of the
   Vulkan bit order before a row is admitted.
2. **Mip levels and array slices are not written.** hl selects them
   (MEASURED, `known-mips`) and the descriptor join supplies
   `mip_levels`, `array_length`, and `texture_type`, so the next version
   can write multi-level, multi-layer (or six-face, for cube types)
   KTX2 files. The rules that version must follow are fixed now, by a
   measurement made for this spec (dossier 00, `known-mips`: a 2-slice
   array with a 7-level chain, 64 down to 1):
   - **The fetch clamps on an out-of-range level or slice.** It never
     errors and never returns nothing. A level past the chain returns
     the last real level's bytes again; a slice past the array returns a
     full-size image whose content is neither valid slice (observed
     constant `ff00ffff`, identical for every out-of-range slice). So a
     "probe until it stops answering" walk is unsafe, and the
     descriptor's counts are the only bound. Never fetch past them.
   - **Mip walks are geometrically checkable.** Levels 0..6 halve
     correctly. Each fetched level must have dimensions
     `max(1, base >> level)` and the count must not exceed the full-chain
     length `floor(log2(max(w, h))) + 1`; together these catch any
     over-long count, including a clamped duplicate at a chain that stops
     short of 1x1. A full chain can only be over-walked by a count no
     real descriptor can carry, which the chain-length bound rejects.
   - **Slice walks are not checkable.** An out-of-range slice has valid
     dimensions and plausible bytes. The only defence is correct
     attribution, which is why step 5 grades it: a slice walk is
     permitted only for a `certain` attribution. An `ambiguous` texture
     is written as level 0, slice 0 with the reason recorded, never as a
     multi-layer file that might carry a neighbour's array length.
3. **Volumes with depth > 1 are refused when identified, and cannot be
   identified from the fetch.** The fetch serves one fixed z-plane with
   no parameter to select another, and reports depth 1 for it (MEASURED,
   `known-3d`). Only a `certain` descriptor attribution names a volume;
   an `ambiguous` one is written as the 2D image it is, with the
   ambiguity recorded, so a volume in an ambiguous geometry group ships
   as one unlabelled plane. A depth-1 volume is written and typed in the
   KV data.
4. **Coverage is measured but the join is positional.** The bundle lists
   what the capture holds; the sweep learns what answers; the join
   attributes by rank. `listed_not_answered` is exact. Attribution can
   shift by one for same-geometry neighbours when an undescribed texture
   intervenes (hl's documented limitation), which is why every
   attribution carries a `certain`/`ambiguous` grade (section 5, step 5)
   and descriptor fields never decide which bytes are written in 0.2.
5. **Combined depth-stencil resources are two files with no descriptor.**
   The join leaves combined descriptors transparent, so their aspects
   carry `descriptor: null` and a `Depth32Float` file cannot say whether
   it was a base depth texture or the depth aspect of a combined one. The
   `_stencil` sibling and the `stencil_probes` list are the only
   indication. Attributing the aspects to the combined descriptor is
   deferred on the hl side.
6. **PVRTC has no KTX2 representation.**
7. **Alpha premultiplication is assumed straight**, disclosed in KV data.

## 11. Testing

### 11.1 Default suite (`cargo test`, no hardware)

- `dfd`: byte-equality against `ktx create --raw` reference DFDs for
  every supported `VkFormat` (6.3), which include the four formats tool-2
  wrote; the reference writer is the only oracle, so nothing in this repo
  points at tool-2.
- `vkformat`: every table row has an hl name and a raw value; every row's
  `FormatKind` is one the DFD generator handles; no row is packed,
  PVRTC, or 260.
- `ktx`: header fields, level alignment (lcm of 4 and the texel block
  size), KV sorting and 4-byte padding, payload-length check, and a
  `ktx validate` pass on a synthetic file when `ktx` is on `PATH`
  (skipped with a message otherwise).
- `emit`: padded rows repacked to exactly the right bytes; unpadded and
  compressed payloads pass through untouched; unmapped format, a certain
  `Type3D` descriptor with depth > 1, and truncated payload become
  recorded failures with the right reason; a certain `Type3D` with depth
  1 and an ambiguous `Type3D` with depth 4 are both written; descriptor
  keys present only when attributed.
- `sweep`: over a fake fetcher and a fake describer: classification,
  identical-duplicate collapse and conflicting-duplicate failure,
  coverage arithmetic, attribution grading (equal group counts are
  `certain`; one extra fetched or one unanswered descriptor in a group
  makes the whole group `ambiguous`), pass-2 selection (depth records probed,
  stencil-only replies kept, echoed depth replies dropped, probe
  outcomes recorded).
- `manifest`: exit-code policy, serialisation shape, `coverage` omitted
  unless the bundle manifest parsed.

### 11.2 Oracle suite (`tools/oracle.sh`, drives the real replayer)

Gated by the `oracle` cargo feature. Each test is its own binary under
`tests/`, runs the CLI end to end on one capture from `captures/`, passes
every written file through `ktx validate`, then reads each file's
payload back and checks it against the fixture's ground truth. A missing
capture skips with a message naming `fixtures/build-all.sh`.

| test | capture | flags | ground truth checked |
| --- | --- | --- | --- |
| `oracle_textures` | `known-textures-late` | `--force-load-unused` | cyan BGRA in blit source and destination; coverage listed 7, answered 7, attributed 7 |
| `oracle_coverage_gap` | `known-textures-late` | (none) | 3 answered (the used textures), `listed_not_answered` 4, exit 0 with the force-load warning |
| `oracle_depth` | `known-depth` | | a `Depth32Float` file reading 0.5 everywhere |
| `oracle_depth_stencil` | `known-depth-stencil` | | one ref yields a depth file (0.5) and a `_stencil` file (42); probe `written`; both `descriptor: null` |
| `oracle_stencil` | `known-stencil` | | a `Stencil8` file reading 42; the combined resource's depth ref probes `written` |
| `oracle_astc` | `known-astc` | | raw block bytes 0x00..0x0F repeated, 4096 bytes |
| `oracle_ycbcr` | `known-ycbcr` | | an R8 file of 128s and an RG8 file of (100, 150) |
| `oracle_3d` | `known-3d` | `--force-load-unused` | the volume is attributed `certain` (it is the only 16x16 BGRA texture), typed `Type3D` depth 4, and is a recorded failure naming depth 4; nothing written for it |
| `oracle_ambiguous` | `known-ambiguous` | | three same-dims BGRA files; red/green/blue pixels carry `mipLevelCount` 1/3/7 respectively, all graded `certain` |

The last row is the attribution check: pixel colour identifies the
physical texture and construction pins colour to mip count (MEASURED,
dossier 00), so a shifted join would put the wrong count on a file.

`sample.gputrace` and `retroarch-trace.gputrace` are not required by any
test. The README records the regression figures tool-2 measured on them
(4 files on sample; 182 records with 10 `RGBA32Float` on retroarch, min
-1, max 46250) for manual runs.

### 11.3 Fixtures and captures are this repo's own

- `fixtures/` holds the fixture apps this tool needs, copied from the
  sibling campaign and maintained here: `known-textures.m`,
  `known-depth.m`, `known-depth-stencil.m`, `known-stencil.m`,
  `known-astc.m`, `known-ycbcr.m`, `known-ambiguous.m`, `known-3d.m`, and
  `known-mips.m` (for the gap-2 follow-up). `fixtures/README.md` states
  each app's ground truth and build line. `known-ds-pair.m` from the
  sibling campaign is not used: it recreates its textures inside the
  capture window, so its captures hold no stored content (MEASURED,
  including against the sibling's own capture).
- `fixtures/capture.sh` and `fixtures/capture-late.sh` are copied as
  they are. `fixtures/build-all.sh` is new: it compiles every fixture
  with `clang` and captures it into `captures/` with the appropriate
  script, so one command regenerates the oracle set on a fresh clone.
- `captures/` is gitignored except `captures/README.md`.

## 12. Repository conventions

- rustfmt pre-commit hook under `.githooks`, enabled with
  `git config core.hooksPath .githooks`.
- `.gitignore`: `/target`, `/captures/*` with `!/captures/README.md`,
  `*.gputrace`, `out/`, `.superpowers/`.
- README covers: purpose, the sibling-checkout requirement, usage, the
  mandatory env var, format coverage and gaps, fixtures and captures,
  the oracle workflow, and the replayer serialisation and recovery rules.

## 13. hl surface this spec depends on

Agreed with the hl implementer on 2026-09-02, after a review in which
the guiding rule was that hl stays a faithful, general library and does
not mould itself to this tool. All of it shipped in `gputools-replay`
commits `0d25143..fd706b2`, verified against the crate on 2026-09-02:

- `Capture::describe(&[Texture]) -> Descriptions { per_texture, unplaced }`,
  a pure join with no fetch and no hard error on gaps.
- `Capture::manifest_status()` distinguishing listed-N, no-descriptors,
  and unparseable.
- `Texture::depth()`, `bytes_per_image()`, and the plane/slice/level of
  the request each returned texture answered.
- `format::name()`, `FormatKind::is_depth_only()` / `is_stencil_only()`
  / `is_combined_depth_stencil()`.
- `Texture::packed_bytes()`, `Blocks::expected_len()`.
- `MTLTextureType` and `MTLTextureUsage` re-exported; `TextureDescriptor`
  keeps raw fields and `DescribedTexture` offers the typed accessors. The
  tool names texture types for output through its own small match over
  `MTLTextureType`, since objc2-metal's newtype has no name method.
- `textures()` documents that it can emit a streamRef twice; it does not
  dedupe, since dropping real fetch records would be a semantic change.
  By the same rule, `texture_aspects(_, Stencil)` returns every record
  plane 1 answers, unfiltered; the tool filters with `is_stencil_only()`.
- `Capture::describe` returns `Descriptions` directly (no `Result`), and
  `Texture` carries `plane()`, `slice()`, `level()` provenance.
- `Texture::depth()` is NOT a volume signal (reads 1 for a 16x16x4
  volume's fetch; hl `live_hl_provenance_3d`). Dossier 00's line saying
  the record's depth field "carries" 3D detection predates that test and
  is stale; the tool follows the test.

Measured by hl for this spec: out-of-range `level`/`slice` clamps
(gap 2, dossier 00). Declined on the hl side, and accepted here: a `VERSION` constant (cargo
metadata suffices), an accessor for the constant unmapped reply fields,
and attribution of combined depth-stencil descriptors to their aspects.

- Measured during implementation: `ignore_unused_resources` overrides
  `force_load_unused_resources` when both are set, and without either an
  unused texture fails the whole batched fetch; the tool therefore sets
  exactly one of them.
