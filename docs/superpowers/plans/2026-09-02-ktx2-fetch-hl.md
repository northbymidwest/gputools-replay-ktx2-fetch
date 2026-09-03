# ktx2-fetch 0.2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `ktx2-fetch` 0.2, a CLI that writes every texture of an Xcode `.gputrace` capture as a lossless KTX2 file with full provenance, using `gputools-replay-hl` as its only engine.

**Architecture:** One Rust crate with a library of small modules (VkFormat table, DFD builder, KTX2 writer, a `Tex` seam over hl's `Texture`, the two-pass sweep, the emitter, the manifest) and a thin binary. Everything that touches the replayer is behind two traits (`Tex`, `Fetcher`) so the sweep and emitter are unit-tested with fakes, and a feature-gated oracle suite drives the real replayer against this repo's own fixture captures and checks every file with Khronos' `ktx validate`.

**Tech Stack:** Rust 2024 (rustc 1.98), `gputools-replay-hl` by path, `clap`, `serde`/`serde_json`, `thiserror`. Khronos `ktx` CLI (v5.0.0-rc1) as an external oracle. `clang` + `gpucapture` to build and capture fixtures. Python 3 for the one fixture-generation script.

**Spec:** `docs/superpowers/specs/2026-09-02-ktx2-fetch-hl-design.md` (commit `98057a4`). Section numbers below refer to it.

## Global Constraints

- Crate `ktx2-fetch`, version `0.2.0`, edition `2024`, `rust-version = "1.98"`. Library name `ktx2_fetch`, binary `ktx2-fetch`.
- The only external tie is `gputools-replay-hl = { path = "../gputools-replay/crates/gputools-replay-hl" }`. Never name `gputools_replay` or `gputools_replay_bundle` in this crate; everything comes through hl's re-exports (spec 4.1).
- Other dependencies: `clap` (derive), `serde` (derive), `serde_json`, `thiserror`. Nothing else (spec 4.1).
- `#![deny(unsafe_code)]` on the library. The binary has exactly one `unsafe` block, the two env writes at the top of `main`. `deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)` outside tests, relaxed under `cfg(test)` (spec 4.3).
- Never reference `../gputrace-tool-2` or `../gputools-replay/captures` from code or tests. Fixtures and captures live in this repo (spec 11.3).
- Every `VkFormat` row is confirmed by `ktx create --raw` + `ktx validate` (spec 6.1); the DFD builder is tested byte-for-byte against `ktx create`'s output (spec 6.3).
- Descriptor fields are metadata. The one place they decide whether a file is written is the certain-attributed `Type3D` with depth > 1 refusal (spec 5 step 6). They never change which bytes are written.
- Commit after every task with the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. No em dashes anywhere in code, docs, or messages.
- Replayer hygiene for anything oracle: one process at a time, check `pgrep -x GPUToolsReplayService` prints nothing before and after, never interrupt a running fetch (spec 9).

## Verified facts the plan relies on

Measured on 2026-09-02 while writing this plan, on this machine:

- `ktx create --raw --format <NAME> --width 8 --height 8 <raw> <out>` creates and `ktx validate` accepts all 111 VkFormats in spec 6.1 (113 Metal formats: the three stencil formats share S8_UINT), and still accepts them with the DFD `primaries` byte (offset 13 within the DFD) patched from 1 (BT709) to 0 (UNSPECIFIED).
- Khronos writes `primaries = 0` already for `D16_UNORM`, `D32_SFLOAT`, `S8_UINT`.
- A path dependency on hl from a crate outside its workspace builds and runs (`format::name(BGRA8Unorm)` returned `Some("BGRA8Unorm")`).
- hl's shipped surface: `Capture::{open, set_timeout, textures, texture_aspects, describe, manifest_status}`, `Descriptions { per_texture: Vec<Option<TextureDescriptor>>, unplaced: Vec<TextureDescriptor> }`, `ManifestStatus::{Ok(usize), NoDescriptors, Unparseable}`, `Texture::{stream_ref, width, height, depth, bytes_per_row, bytes_per_image, plane, slice, level, format, format_kind, raw_bytes, packed_bytes, blocks}`, `Blocks { bytes, block, block_bytes, blocks_per_row }` + `expected_len()`, `format::{format_kind, name, FormatKind, ColorFormat, Channel, Component, NumericKind, DepthStencilFormat, DepthKind, StencilKind, CompressedFormat, CompressionScheme}`, `FormatKind::{is_srgb, is_depth_only, is_stencil_only, is_combined_depth_stencil, bytes_per_pixel, byte_aligned}`, re-exports `MTLPixelFormat`, `MTLTextureType`, `MTLTextureUsage`, `TextureDescriptor` (all-pub fields: `store0_offset, format, texture_type, width, height, depth, mip_levels, array_length, sample_count, usage, texture_id`; `Copy`).
- `MTLTextureType` values: 1D=0, 1DArray=1, 2D=2, 2DArray=3, 2DMultisample=4, Cube=5, CubeArray=6, 3D=7, 2DMultisampleArray=8, TextureBuffer=9.

## File structure

| path | responsibility |
| --- | --- |
| `Cargo.toml`, `build.rs` | crate manifest; `build.rs` reads the hl version out of `Cargo.lock` into `KTX2_FETCH_ENGINE` |
| `src/lib.rs` | lint posture, module list, `TOOL_VERSION`, `engine()` |
| `src/vkformat.rs` | `MTLPixelFormat` -> `VkFormat { name, value, type_size }` table, plus `metal_name` |
| `src/dfd_ref.rs` | generated: `reference(name) -> Option<&'static [u8]>` over `tests/fixtures/dfd/*.dfd` |
| `src/dfd.rs` | DFD builder: generated for colour and depth/stencil, reference bytes for compressed |
| `src/ktx.rs` | KTX2 writer, plus a small reader used by tests |
| `src/tex.rs` | `Tex` trait, `Payload`, `Aspect`, `aspect_bpp`, `impl Tex for Texture`, `FakeTex` under `cfg(test)` |
| `src/manifest.rs` | serde types for `manifest.json`, exit-code policy |
| `src/emit.rs` | one `Fetched` -> one file or one failure |
| `src/sweep.rs` | `Fetcher` trait, two-pass sweep, dedupe, attribution grading, coverage |
| `src/main.rs` | env, args, `impl Fetcher for Capture`, orchestration |
| `tools/capture-dfd-fixtures.py` | generates `tests/fixtures/dfd/*.dfd`, `index.tsv`, `src/dfd_ref.rs` |
| `tools/oracle.sh` | replayer hygiene wrapper around the oracle suite |
| `fixtures/*.m`, `fixtures/capture*.sh`, `fixtures/build-all.sh`, `fixtures/README.md` | this repo's fixture apps and capture tooling |
| `captures/README.md` | what each capture holds (captures themselves gitignored) |
| `tests/cli.rs` | binary smoke tests, no hardware |
| `tests/common/mod.rs`, `tests/oracle_*.rs` | oracle suite |
| `README.md` | usage, constraints, coverage, gaps |

---

### Task 1: Crate scaffold, engine version, hooks

**Files:**
- Create: `Cargo.toml`, `build.rs`, `src/lib.rs`, `src/main.rs`, `.gitignore`, `.githooks/pre-commit`
- Test: `src/lib.rs` (unit)

**Interfaces:**
- Produces: `ktx2_fetch::TOOL_VERSION: &str`, `ktx2_fetch::engine() -> &'static str` (e.g. `gputools-replay-hl 0.0.0 (path)`).

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "ktx2-fetch"
version = "0.2.0"
edition = "2024"
rust-version = "1.98"
description = "Lossless KTX2 export of every texture in an Xcode .gputrace capture, via gputools-replay-hl"
publish = false

[lib]
name = "ktx2_fetch"
path = "src/lib.rs"

[[bin]]
name = "ktx2-fetch"
path = "src/main.rs"

[dependencies]
gputools-replay-hl = { path = "../gputools-replay/crates/gputools-replay-hl" }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[features]
# Gates every test that drives the real replayer or needs a capture.
oracle = []
```

- [ ] **Step 2: Write `build.rs`**

It parses `Cargo.lock` by hand (no build-dependencies) and exports the hl version and source as an env var the library embeds.

```rust
use std::{env, fs, path::Path};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let lock = Path::new(&manifest_dir).join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let text = fs::read_to_string(&lock).unwrap_or_default();
    let engine = engine_from_lock(&text)
        .unwrap_or_else(|| "gputools-replay-hl (version unknown: not found in Cargo.lock)".to_string());
    println!("cargo:rustc-env=KTX2_FETCH_ENGINE={engine}");
}

/// `gputools-replay-hl <version> (<source>)`, where source is the lock's
/// `source` field (a registry or git URL once published) or `path` today.
fn engine_from_lock(text: &str) -> Option<String> {
    for block in text.split("[[package]]") {
        let mut name = None;
        let mut version = None;
        let mut source = None;
        for line in block.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name = ") {
                name = Some(v.trim_matches('"'));
            } else if let Some(v) = line.strip_prefix("version = ") {
                version = Some(v.trim_matches('"'));
            } else if let Some(v) = line.strip_prefix("source = ") {
                source = Some(v.trim_matches('"'));
            }
        }
        if name == Some("gputools-replay-hl") {
            return Some(format!(
                "gputools-replay-hl {} ({})",
                version?,
                source.unwrap_or("path")
            ));
        }
    }
    None
}
```

- [ ] **Step 3: Write `src/lib.rs` with the failing test**

```rust
//! ktx2-fetch: lossless KTX2 export of a `.gputrace` capture's textures,
//! on the `gputools-replay-hl` engine. See the spec in
//! `docs/superpowers/specs/2026-09-02-ktx2-fetch-hl-design.md`.
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

/// This crate's version, written into every file's `KTXwriter` key.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The engine this binary was built against, resolved from `Cargo.lock` by
/// `build.rs`: `gputools-replay-hl <version> (<source>)`.
pub fn engine() -> &'static str {
    env!("KTX2_FETCH_ENGINE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_names_hl_and_a_version() {
        let e = engine();
        assert!(e.starts_with("gputools-replay-hl "), "{e}");
        assert!(e.contains('('), "source missing: {e}");
        assert!(!e.contains("unknown"), "{e}");
    }

    #[test]
    fn tool_version_is_0_2() {
        assert_eq!(TOOL_VERSION, "0.2.0");
    }
}
```

- [ ] **Step 4: Write a placeholder `src/main.rs`** (replaced in Task 9)

```rust
fn main() {
    println!("ktx2-fetch {} on {}", ktx2_fetch::TOOL_VERSION, ktx2_fetch::engine());
}
```

- [ ] **Step 5: Write `.gitignore` and the pre-commit hook**

`.gitignore`:
```
/target
/captures/*
!/captures/README.md
*.gputrace
out/
.superpowers/
.DS_Store
```

`.githooks/pre-commit` (then `chmod +x .githooks/pre-commit`):
```bash
#!/usr/bin/env bash
# Refuses a commit that is not rustfmt-clean. Deliberately does NOT reformat:
# a hook that edits your tree makes you commit code you did not read.
# Bypass for a genuine exception with `git commit --no-verify`.
set -euo pipefail

if ! command -v cargo >/dev/null; then
  echo "pre-commit: cargo not on PATH; skipping the rustfmt check" >&2
  exit 0
fi

if ! cargo fmt --check --quiet; then
  echo >&2
  echo "pre-commit: this commit is not rustfmt-clean." >&2
  echo "  Run 'cargo fmt', review the diff, and stage it." >&2
  exit 1
fi
```

- [ ] **Step 6: Build and run the tests**

Run: `git config core.hooksPath .githooks && cargo test`
Expected: both lib tests PASS. The first build compiles hl and its substrate; that takes a minute.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock build.rs src .gitignore .githooks
git commit -m "feat: scaffold ktx2-fetch 0.2 on gputools-replay-hl

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Reference DFD fixtures and the VkFormat table

**Files:**
- Create: `tools/capture-dfd-fixtures.py`, `tests/fixtures/dfd/*.dfd` (generated), `tests/fixtures/dfd/index.tsv` (generated), `src/dfd_ref.rs` (generated), `src/vkformat.rs`
- Modify: `src/lib.rs` (add `pub mod dfd_ref; pub mod vkformat;`)

**Interfaces:**
- Produces: `vkformat::VkFormat { name: &'static str, value: u32, type_size: u32 }`, `vkformat::lookup(fmt: MTLPixelFormat) -> Option<VkFormat>`, `vkformat::ROWS: &[(u32, VkFormat)]` (Metal raw value, row), `vkformat::metal_name(fmt: MTLPixelFormat) -> String`, `dfd_ref::reference(name: &str) -> Option<&'static [u8]>`.

- [ ] **Step 1: Write `tools/capture-dfd-fixtures.py`**

```python
#!/usr/bin/env python3
"""Capture Khronos' own DFD for every VkFormat ktx2-fetch writes.

For each format: `ktx create --raw` an 8x8 file, `ktx validate` it, and
record (a) the DFD bytes to tests/fixtures/dfd/<NAME>.dfd, (b) the header's
vkFormat and typeSize to tests/fixtures/dfd/index.tsv, (c) an include_bytes!
table to src/dfd_ref.rs. Re-run whenever a row is added to vkformat.rs.
Requires `ktx` (v5.0.0-rc1 or later) on PATH.
"""
import math, pathlib, struct, subprocess, sys, tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIX = ROOT / "tests" / "fixtures" / "dfd"
FIX.mkdir(parents=True, exist_ok=True)

def U(n, bpp): return (n, bpp, None)
def C(n, bw, bh, bb): return (n, None, (bw, bh, bb))

FORMATS = [
    U("A8_UNORM_KHR", 1),
    U("R8_UNORM", 1), U("R8_SRGB", 1), U("R8_SNORM", 1), U("R8_UINT", 1), U("R8_SINT", 1),
    U("R8G8_UNORM", 2), U("R8G8_SRGB", 2), U("R8G8_SNORM", 2), U("R8G8_UINT", 2), U("R8G8_SINT", 2),
    U("R8G8B8A8_UNORM", 4), U("R8G8B8A8_SRGB", 4), U("R8G8B8A8_SNORM", 4), U("R8G8B8A8_UINT", 4), U("R8G8B8A8_SINT", 4),
    U("B8G8R8A8_UNORM", 4), U("B8G8R8A8_SRGB", 4),
    U("R16_UNORM", 2), U("R16_SNORM", 2), U("R16_UINT", 2), U("R16_SINT", 2), U("R16_SFLOAT", 2),
    U("R16G16_UNORM", 4), U("R16G16_SNORM", 4), U("R16G16_UINT", 4), U("R16G16_SINT", 4), U("R16G16_SFLOAT", 4),
    U("R16G16B16A16_UNORM", 8), U("R16G16B16A16_SNORM", 8), U("R16G16B16A16_UINT", 8), U("R16G16B16A16_SINT", 8), U("R16G16B16A16_SFLOAT", 8),
    U("R32_UINT", 4), U("R32_SINT", 4), U("R32_SFLOAT", 4),
    U("R32G32_UINT", 8), U("R32G32_SINT", 8), U("R32G32_SFLOAT", 8),
    U("R32G32B32A32_UINT", 16), U("R32G32B32A32_SINT", 16), U("R32G32B32A32_SFLOAT", 16),
    U("D16_UNORM", 2), U("D32_SFLOAT", 4), U("S8_UINT", 1),
    C("BC1_RGBA_UNORM_BLOCK", 4, 4, 8), C("BC1_RGBA_SRGB_BLOCK", 4, 4, 8),
    C("BC2_UNORM_BLOCK", 4, 4, 16), C("BC2_SRGB_BLOCK", 4, 4, 16),
    C("BC3_UNORM_BLOCK", 4, 4, 16), C("BC3_SRGB_BLOCK", 4, 4, 16),
    C("BC4_UNORM_BLOCK", 4, 4, 8), C("BC4_SNORM_BLOCK", 4, 4, 8),
    C("BC5_UNORM_BLOCK", 4, 4, 16), C("BC5_SNORM_BLOCK", 4, 4, 16),
    C("BC6H_SFLOAT_BLOCK", 4, 4, 16), C("BC6H_UFLOAT_BLOCK", 4, 4, 16),
    C("BC7_UNORM_BLOCK", 4, 4, 16), C("BC7_SRGB_BLOCK", 4, 4, 16),
    C("ETC2_R8G8B8_UNORM_BLOCK", 4, 4, 8), C("ETC2_R8G8B8_SRGB_BLOCK", 4, 4, 8),
    C("ETC2_R8G8B8A1_UNORM_BLOCK", 4, 4, 8), C("ETC2_R8G8B8A1_SRGB_BLOCK", 4, 4, 8),
    C("ETC2_R8G8B8A8_UNORM_BLOCK", 4, 4, 16), C("ETC2_R8G8B8A8_SRGB_BLOCK", 4, 4, 16),
    C("EAC_R11_UNORM_BLOCK", 4, 4, 8), C("EAC_R11_SNORM_BLOCK", 4, 4, 8),
    C("EAC_R11G11_UNORM_BLOCK", 4, 4, 16), C("EAC_R11G11_SNORM_BLOCK", 4, 4, 16),
]
for bw, bh in [(4, 4), (5, 4), (5, 5), (6, 5), (6, 6), (8, 5), (8, 6), (8, 8),
               (10, 5), (10, 6), (10, 8), (10, 10), (12, 10), (12, 12)]:
    for suffix in ["UNORM", "SRGB", "SFLOAT"]:
        FORMATS.append(C(f"ASTC_{bw}x{bh}_{suffix}_BLOCK", bw, bh, 16))

W = H = 8
index_lines = []
ref_arms = []
with tempfile.TemporaryDirectory() as tmp:
    tmp = pathlib.Path(tmp)
    for name, bpp, blk in FORMATS:
        if bpp:
            n = W * H * bpp
        else:
            bw, bh, bb = blk
            n = math.ceil(W / bw) * math.ceil(H / bh) * bb
        raw = tmp / f"{name}.raw"
        raw.write_bytes((bytes(range(256)) * (n // 256 + 1))[:n])
        out = tmp / f"{name}.ktx2"
        subprocess.run(["ktx", "create", "--raw", "--format", name, "--width", str(W),
                        "--height", str(H), str(raw), str(out)], check=True)
        subprocess.run(["ktx", "validate", str(out)], check=True)
        b = out.read_bytes()
        vk_format, type_size = struct.unpack_from("<II", b, 12)
        dfd_off, dfd_len = struct.unpack_from("<II", b, 48)
        (FIX / f"{name}.dfd").write_bytes(b[dfd_off:dfd_off + dfd_len])
        index_lines.append(f"{name}\t{vk_format}\t{type_size}")
        ref_arms.append(f'        "{name}" => Some(include_bytes!("../tests/fixtures/dfd/{name}.dfd")),')

(FIX / "index.tsv").write_text("\n".join(index_lines) + "\n")
(ROOT / "src" / "dfd_ref.rs").write_text(
    "//! GENERATED by tools/capture-dfd-fixtures.py. Do not edit.\n"
    "//!\n"
    "//! Khronos' own Data Format Descriptor for each VkFormat this tool writes,\n"
    "//! as emitted by `ktx create --raw` (v5.0.0-rc1) for an 8x8 image. The DFD\n"
    "//! does not depend on image size. `primaries` is left as Khronos wrote it\n"
    "//! (BT709 for colour); `dfd::build` overrides it to UNSPECIFIED.\n\n"
    "/// The reference DFD bytes for `name` (a VkFormat name without the\n"
    "/// `VK_FORMAT_` prefix), or `None` if no fixture was captured for it.\n"
    "pub fn reference(name: &str) -> Option<&'static [u8]> {\n"
    "    match name {\n" + "\n".join(ref_arms) + "\n        _ => None,\n    }\n}\n")
print(f"captured {len(FORMATS)} formats", file=sys.stderr)
```

- [ ] **Step 2: Run it**

Run: `python3 tools/capture-dfd-fixtures.py && wc -l tests/fixtures/dfd/index.tsv && head -3 tests/fixtures/dfd/index.tsv`
Expected: `captured 111 formats`; `index.tsv` has 111 lines; first line `A8_UNORM_KHR	1000470001	1`.

- [ ] **Step 3: Write the failing tests in `src/vkformat.rs`**

```rust
//! The `MTLPixelFormat` -> `VkFormat` table (spec 6.1). Every row is
//! confirmed by `ktx create --raw` + `ktx validate` (the fixtures under
//! `tests/fixtures/dfd/`); a format with no row is a per-texture failure,
//! never a guess. Metal names come from hl's `format::name`.

use gputools_replay_hl::MTLPixelFormat;
use gputools_replay_hl::format::name;

/// One KTX2 format identity: the VkFormat name (as `ktx create` spells it,
/// without the `VK_FORMAT_` prefix), its numeric value, and the KTX2 header
/// `typeSize` (bytes per channel element, 1 for block-compressed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VkFormat {
    pub name: &'static str,
    pub value: u32,
    pub type_size: u32,
}

const fn vk(name: &'static str, value: u32, type_size: u32) -> VkFormat {
    VkFormat { name, value, type_size }
}

/// Every supported row: (raw `MTLPixelFormat` value, KTX2 identity).
pub const ROWS: &[(u32, VkFormat)] = &[
    // -- byte-aligned colour --
    (1, vk("A8_UNORM_KHR", 1000470001, 1)),
    (10, vk("R8_UNORM", 9, 1)),
    (11, vk("R8_SRGB", 15, 1)),
    (12, vk("R8_SNORM", 10, 1)),
    (13, vk("R8_UINT", 13, 1)),
    (14, vk("R8_SINT", 14, 1)),
    (20, vk("R16_UNORM", 70, 2)),
    (22, vk("R16_SNORM", 71, 2)),
    (23, vk("R16_UINT", 74, 2)),
    (24, vk("R16_SINT", 75, 2)),
    (25, vk("R16_SFLOAT", 76, 2)),
    (53, vk("R32_UINT", 98, 4)),
    (54, vk("R32_SINT", 99, 4)),
    (55, vk("R32_SFLOAT", 100, 4)),
    (30, vk("R8G8_UNORM", 16, 1)),
    (31, vk("R8G8_SRGB", 22, 1)),
    (32, vk("R8G8_SNORM", 17, 1)),
    (33, vk("R8G8_UINT", 20, 1)),
    (34, vk("R8G8_SINT", 21, 1)),
    (60, vk("R16G16_UNORM", 77, 2)),
    (62, vk("R16G16_SNORM", 78, 2)),
    (63, vk("R16G16_UINT", 81, 2)),
    (64, vk("R16G16_SINT", 82, 2)),
    (65, vk("R16G16_SFLOAT", 83, 2)),
    (103, vk("R32G32_UINT", 101, 4)),
    (104, vk("R32G32_SINT", 102, 4)),
    (105, vk("R32G32_SFLOAT", 103, 4)),
    (70, vk("R8G8B8A8_UNORM", 37, 1)),
    (71, vk("R8G8B8A8_SRGB", 43, 1)),
    (72, vk("R8G8B8A8_SNORM", 38, 1)),
    (73, vk("R8G8B8A8_UINT", 41, 1)),
    (74, vk("R8G8B8A8_SINT", 42, 1)),
    (80, vk("B8G8R8A8_UNORM", 44, 1)),
    (81, vk("B8G8R8A8_SRGB", 50, 1)),
    (110, vk("R16G16B16A16_UNORM", 91, 2)),
    (112, vk("R16G16B16A16_SNORM", 92, 2)),
    (113, vk("R16G16B16A16_UINT", 95, 2)),
    (114, vk("R16G16B16A16_SINT", 96, 2)),
    (115, vk("R16G16B16A16_SFLOAT", 97, 2)),
    (123, vk("R32G32B32A32_UINT", 107, 4)),
    (124, vk("R32G32B32A32_SINT", 108, 4)),
    (125, vk("R32G32B32A32_SFLOAT", 109, 4)),
    // -- single-aspect depth / stencil (X24/X32 stencil aspects are served
    //    at 1 byte per pixel, hence S8_UINT) --
    (250, vk("D16_UNORM", 124, 2)),
    (252, vk("D32_SFLOAT", 126, 4)),
    (253, vk("S8_UINT", 127, 1)),
    (261, vk("S8_UINT", 127, 1)),
    (262, vk("S8_UINT", 127, 1)),
    // -- BC --
    (130, vk("BC1_RGBA_UNORM_BLOCK", 133, 1)),
    (131, vk("BC1_RGBA_SRGB_BLOCK", 134, 1)),
    (132, vk("BC2_UNORM_BLOCK", 135, 1)),
    (133, vk("BC2_SRGB_BLOCK", 136, 1)),
    (134, vk("BC3_UNORM_BLOCK", 137, 1)),
    (135, vk("BC3_SRGB_BLOCK", 138, 1)),
    (140, vk("BC4_UNORM_BLOCK", 139, 1)),
    (141, vk("BC4_SNORM_BLOCK", 140, 1)),
    (142, vk("BC5_UNORM_BLOCK", 141, 1)),
    (143, vk("BC5_SNORM_BLOCK", 142, 1)),
    (150, vk("BC6H_SFLOAT_BLOCK", 144, 1)),
    (151, vk("BC6H_UFLOAT_BLOCK", 143, 1)),
    (152, vk("BC7_UNORM_BLOCK", 145, 1)),
    (153, vk("BC7_SRGB_BLOCK", 146, 1)),
    // -- EAC / ETC2 --
    (170, vk("EAC_R11_UNORM_BLOCK", 153, 1)),
    (172, vk("EAC_R11_SNORM_BLOCK", 154, 1)),
    (174, vk("EAC_R11G11_UNORM_BLOCK", 155, 1)),
    (176, vk("EAC_R11G11_SNORM_BLOCK", 156, 1)),
    (178, vk("ETC2_R8G8B8A8_UNORM_BLOCK", 151, 1)),
    (179, vk("ETC2_R8G8B8A8_SRGB_BLOCK", 152, 1)),
    (180, vk("ETC2_R8G8B8_UNORM_BLOCK", 147, 1)),
    (181, vk("ETC2_R8G8B8_SRGB_BLOCK", 148, 1)),
    (182, vk("ETC2_R8G8B8A1_UNORM_BLOCK", 149, 1)),
    (183, vk("ETC2_R8G8B8A1_SRGB_BLOCK", 150, 1)),
    // -- ASTC sRGB --
    (186, vk("ASTC_4x4_SRGB_BLOCK", 158, 1)),
    (187, vk("ASTC_5x4_SRGB_BLOCK", 160, 1)),
    (188, vk("ASTC_5x5_SRGB_BLOCK", 162, 1)),
    (189, vk("ASTC_6x5_SRGB_BLOCK", 164, 1)),
    (190, vk("ASTC_6x6_SRGB_BLOCK", 166, 1)),
    (192, vk("ASTC_8x5_SRGB_BLOCK", 168, 1)),
    (193, vk("ASTC_8x6_SRGB_BLOCK", 170, 1)),
    (194, vk("ASTC_8x8_SRGB_BLOCK", 172, 1)),
    (195, vk("ASTC_10x5_SRGB_BLOCK", 174, 1)),
    (196, vk("ASTC_10x6_SRGB_BLOCK", 176, 1)),
    (197, vk("ASTC_10x8_SRGB_BLOCK", 178, 1)),
    (198, vk("ASTC_10x10_SRGB_BLOCK", 180, 1)),
    (199, vk("ASTC_12x10_SRGB_BLOCK", 182, 1)),
    (200, vk("ASTC_12x12_SRGB_BLOCK", 184, 1)),
    // -- ASTC LDR --
    (204, vk("ASTC_4x4_UNORM_BLOCK", 157, 1)),
    (205, vk("ASTC_5x4_UNORM_BLOCK", 159, 1)),
    (206, vk("ASTC_5x5_UNORM_BLOCK", 161, 1)),
    (207, vk("ASTC_6x5_UNORM_BLOCK", 163, 1)),
    (208, vk("ASTC_6x6_UNORM_BLOCK", 165, 1)),
    (210, vk("ASTC_8x5_UNORM_BLOCK", 167, 1)),
    (211, vk("ASTC_8x6_UNORM_BLOCK", 169, 1)),
    (212, vk("ASTC_8x8_UNORM_BLOCK", 171, 1)),
    (213, vk("ASTC_10x5_UNORM_BLOCK", 173, 1)),
    (214, vk("ASTC_10x6_UNORM_BLOCK", 175, 1)),
    (215, vk("ASTC_10x8_UNORM_BLOCK", 177, 1)),
    (216, vk("ASTC_10x10_UNORM_BLOCK", 179, 1)),
    (217, vk("ASTC_12x10_UNORM_BLOCK", 181, 1)),
    (218, vk("ASTC_12x12_UNORM_BLOCK", 183, 1)),
    // -- ASTC HDR --
    (222, vk("ASTC_4x4_SFLOAT_BLOCK", 1000066000, 1)),
    (223, vk("ASTC_5x4_SFLOAT_BLOCK", 1000066001, 1)),
    (224, vk("ASTC_5x5_SFLOAT_BLOCK", 1000066002, 1)),
    (225, vk("ASTC_6x5_SFLOAT_BLOCK", 1000066003, 1)),
    (226, vk("ASTC_6x6_SFLOAT_BLOCK", 1000066004, 1)),
    (228, vk("ASTC_8x5_SFLOAT_BLOCK", 1000066005, 1)),
    (229, vk("ASTC_8x6_SFLOAT_BLOCK", 1000066006, 1)),
    (230, vk("ASTC_8x8_SFLOAT_BLOCK", 1000066007, 1)),
    (231, vk("ASTC_10x5_SFLOAT_BLOCK", 1000066008, 1)),
    (232, vk("ASTC_10x6_SFLOAT_BLOCK", 1000066009, 1)),
    (233, vk("ASTC_10x8_SFLOAT_BLOCK", 1000066010, 1)),
    (234, vk("ASTC_10x10_SFLOAT_BLOCK", 1000066011, 1)),
    (235, vk("ASTC_12x10_SFLOAT_BLOCK", 1000066012, 1)),
    (236, vk("ASTC_12x12_SFLOAT_BLOCK", 1000066013, 1)),
];

/// The KTX2 identity for a Metal format, or `None` when the tool does not
/// write it (packed colour, PVRTC, combined depth-stencil, unknown).
pub fn lookup(fmt: MTLPixelFormat) -> Option<VkFormat> {
    let raw = fmt.0 as u32;
    ROWS.iter().find(|(m, _)| *m == raw).map(|(_, v)| *v)
}

/// hl's canonical Metal name, or `MTLPixelFormat(<raw>)` when hl's table
/// does not describe the value. Used in filenames, KV data, and reasons.
pub fn metal_name(fmt: MTLPixelFormat) -> String {
    match name(fmt) {
        Some(n) => n.to_string(),
        None => format!("MTLPixelFormat({})", fmt.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gputools_replay_hl::format::{CompressionScheme, FormatKind, format_kind};
    use std::collections::HashMap;

    fn index() -> HashMap<String, (u32, u32)> {
        let text = include_str!("../tests/fixtures/dfd/index.tsv");
        text.lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f[0].to_string(), (f[1].parse().unwrap(), f[2].parse().unwrap()))
            })
            .collect()
    }

    #[test]
    fn every_row_matches_the_reference_writer_header() {
        let idx = index();
        for (mtl, row) in ROWS {
            let (value, type_size) = idx
                .get(row.name)
                .unwrap_or_else(|| panic!("no fixture for {} (mtl {mtl})", row.name));
            assert_eq!(row.value, *value, "{}: vkFormat value", row.name);
            assert_eq!(row.type_size, *type_size, "{}: typeSize", row.name);
        }
    }

    #[test]
    fn every_row_has_an_hl_name_and_a_supported_kind() {
        for (mtl, row) in ROWS {
            assert!(name(MTLPixelFormat(*mtl as _)).is_some(), "mtl {mtl} ({})", row.name);
            match format_kind(*mtl) {
                FormatKind::Color(c) => assert!(c.byte_aligned, "mtl {mtl} is packed"),
                FormatKind::DepthStencil(d) => {
                    assert!(d.depth.is_some() != d.stencil.is_some(), "mtl {mtl} is combined")
                }
                FormatKind::Compressed(c) => {
                    assert_ne!(c.scheme, CompressionScheme::Pvrtc, "mtl {mtl} is PVRTC")
                }
                FormatKind::Unknown => panic!("mtl {mtl} is unknown to hl"),
            }
        }
    }

    #[test]
    fn rows_are_unique_per_metal_format() {
        let mut seen = std::collections::HashSet::new();
        for (mtl, _) in ROWS {
            assert!(seen.insert(*mtl), "duplicate row for mtl {mtl}");
        }
        assert_eq!(ROWS.len(), 113);
    }

    #[test]
    fn excluded_formats_have_no_row() {
        for raw in [90u32, 91, 92, 93, 94, 40, 41, 42, 43, 160, 167, 260, 0xffff_ff00] {
            assert!(lookup(MTLPixelFormat(raw as _)).is_none(), "raw {raw} must not map");
        }
    }

    #[test]
    fn metal_name_falls_back_to_the_raw_value() {
        assert_eq!(metal_name(MTLPixelFormat::BGRA8Unorm), "BGRA8Unorm");
        assert_eq!(metal_name(MTLPixelFormat(0xffff_ff00 as _)), "MTLPixelFormat(4294967040)");
    }
}
```

- [ ] **Step 4: Register the modules in `src/lib.rs`**

Add after the lint attributes:
```rust
pub mod dfd_ref;
pub mod vkformat;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test vkformat`
Expected: 5 tests PASS. If `every_row_matches_the_reference_writer_header` fails on a value, the table is wrong, not the fixture: fix the table.

- [ ] **Step 6: Commit**

```bash
git add tools/capture-dfd-fixtures.py tests/fixtures/dfd src/dfd_ref.rs src/vkformat.rs src/lib.rs
git commit -m "feat: VkFormat table confirmed against ktx create reference files

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 3: The Data Format Descriptor builder

**Files:**
- Create: `src/dfd.rs`
- Modify: `src/lib.rs` (add `pub mod dfd;`)

**Interfaces:**
- Consumes: `vkformat::VkFormat`, `dfd_ref::reference`.
- Produces: `dfd::build(mtl_raw: u32, vk: &VkFormat) -> Result<Vec<u8>, DfdError>`, `dfd::DfdError`, `dfd::PRIMARIES_OFFSET: usize = 13`.

- [ ] **Step 1: Write the failing tests and the builder in `src/dfd.rs`**

```rust
//! The KTX2 Data Format Descriptor (spec 6.3), derived from hl's
//! `FormatKind` for colour and single-aspect depth/stencil, and taken from
//! Khronos' reference writer (`dfd_ref`) for block-compressed formats.
//! Primaries are always UNSPECIFIED: the capture records no colour space.

use crate::dfd_ref::reference;
use crate::vkformat::VkFormat;
use gputools_replay_hl::format::{
    Component, CompressionScheme, DepthKind, FormatKind, NumericKind, StencilKind, format_kind,
};

/// Byte offset of `primaries` within a DFD (after the 4-byte total size,
/// 4-byte vendor/descriptor ids, 4-byte version/size, and `colorModel`).
pub const PRIMARIES_OFFSET: usize = 13;

const MODEL_RGBSDA: u8 = 1;
const PRIMARIES_UNSPECIFIED: u8 = 0;
const TRANSFER_LINEAR: u8 = 1;
const TRANSFER_SRGB: u8 = 2;
const FLAG_ALPHA_STRAIGHT: u8 = 0;
const CH_R: u8 = 0;
const CH_G: u8 = 1;
const CH_B: u8 = 2;
const CH_STENCIL: u8 = 13;
const CH_DEPTH: u8 = 14;
const CH_A: u8 = 15;
const QUAL_SIGNED: u8 = 0x40;
const QUAL_FLOAT: u8 = 0x80;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DfdError {
    #[error("packed colour format: channel widths are not byte-aligned")]
    Packed,
    #[error("combined depth-stencil is never served as one image")]
    CombinedDepthStencil,
    #[error("PVRTC has no VkFormat and no KTX2 data format descriptor")]
    Pvrtc,
    #[error("no reference data format descriptor captured for {0}")]
    NoReference(&'static str),
    #[error("format not described by gputools-replay-hl's format table")]
    Unknown,
}

struct Sample {
    bit_offset: u16,
    bits: u8,
    channel: u8,
    qualifiers: u8,
    lower: u32,
    upper: u32,
}

/// `(lower, upper, qualifiers)` per the Khronos Data Format spec, as
/// `ktx create` writes them (MEASURED, tests/fixtures/dfd).
fn range(numeric: NumericKind, bits: u8) -> Result<(u32, u32, u8), DfdError> {
    let max_unsigned = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
    let max_signed = if bits >= 32 { i32::MAX } else { (1i32 << (bits - 1)) - 1 };
    Ok(match numeric {
        NumericKind::Unorm => (0, max_unsigned, 0),
        NumericKind::Snorm => ((-max_signed) as u32, max_signed as u32, QUAL_SIGNED),
        NumericKind::Uint => (0, 1, 0),
        NumericKind::Sint => (u32::MAX, 1, QUAL_SIGNED),
        NumericKind::Float => (0xbf80_0000, 0x3f80_0000, QUAL_SIGNED | QUAL_FLOAT),
        NumericKind::SharedExponent => return Err(DfdError::Packed),
    })
}

fn assemble(samples: &[Sample], bytes_plane0: u8, transfer: u8) -> Vec<u8> {
    let block_size = 24 + 16 * samples.len() as u32;
    let mut d = Vec::with_capacity(4 + block_size as usize);
    d.extend_from_slice(&(block_size + 4).to_le_bytes()); // dfdTotalSize
    d.extend_from_slice(&0u32.to_le_bytes()); // vendorId 0, descriptorType 0
    d.extend_from_slice(&2u16.to_le_bytes()); // versionNumber
    d.extend_from_slice(&(block_size as u16).to_le_bytes()); // descriptorBlockSize
    d.extend_from_slice(&[MODEL_RGBSDA, PRIMARIES_UNSPECIFIED, transfer, FLAG_ALPHA_STRAIGHT]);
    d.extend_from_slice(&[0, 0, 0, 0]); // texelBlockDimension0..3 => 1x1x1x1
    let mut planes = [0u8; 8];
    planes[0] = bytes_plane0;
    d.extend_from_slice(&planes);
    for s in samples {
        d.extend_from_slice(&s.bit_offset.to_le_bytes());
        d.push(s.bits - 1);
        d.push(s.channel | s.qualifiers);
        d.extend_from_slice(&[0, 0, 0, 0]); // samplePosition0..3
        d.extend_from_slice(&s.lower.to_le_bytes());
        d.extend_from_slice(&s.upper.to_le_bytes());
    }
    d
}

/// Build the DFD for Metal format `mtl_raw`, whose KTX2 identity is `vk`.
pub fn build(mtl_raw: u32, vk: &VkFormat) -> Result<Vec<u8>, DfdError> {
    match format_kind(mtl_raw) {
        FormatKind::Color(c) => {
            if !c.byte_aligned {
                return Err(DfdError::Packed);
            }
            let mut samples = Vec::with_capacity(c.channels.len());
            let mut offset: u16 = 0;
            for ch in c.channels {
                let (lower, upper, qualifiers) = range(c.numeric, ch.bits)?;
                let channel = match ch.component {
                    Component::R => CH_R,
                    Component::G => CH_G,
                    Component::B => CH_B,
                    Component::A => CH_A,
                };
                samples.push(Sample { bit_offset: offset, bits: ch.bits, channel, qualifiers, lower, upper });
                offset += u16::from(ch.bits);
            }
            let transfer = if c.srgb { TRANSFER_SRGB } else { TRANSFER_LINEAR };
            Ok(assemble(&samples, c.bytes_per_pixel as u8, transfer))
        }
        FormatKind::DepthStencil(d) => match (d.depth, d.stencil) {
            (Some(depth), None) => {
                let (bits, numeric, bpp) = match depth {
                    DepthKind::Unorm16 => (16u8, NumericKind::Unorm, 2u8),
                    DepthKind::Float32 => (32, NumericKind::Float, 4),
                };
                let (lower, upper, qualifiers) = range(numeric, bits)?;
                let s = Sample { bit_offset: 0, bits, channel: CH_DEPTH, qualifiers, lower, upper };
                Ok(assemble(&[s], bpp, TRANSFER_LINEAR))
            }
            (None, Some(StencilKind::Uint8)) => {
                let (lower, upper, qualifiers) = range(NumericKind::Uint, 8)?;
                let s = Sample { bit_offset: 0, bits: 8, channel: CH_STENCIL, qualifiers, lower, upper };
                Ok(assemble(&[s], 1, TRANSFER_LINEAR))
            }
            _ => Err(DfdError::CombinedDepthStencil),
        },
        FormatKind::Compressed(c) => {
            if c.scheme == CompressionScheme::Pvrtc {
                return Err(DfdError::Pvrtc);
            }
            let mut d = reference(vk.name).ok_or(DfdError::NoReference(vk.name))?.to_vec();
            if let Some(p) = d.get_mut(PRIMARIES_OFFSET) {
                *p = PRIMARIES_UNSPECIFIED;
            }
            Ok(d)
        }
        FormatKind::Unknown => Err(DfdError::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vkformat::{ROWS, lookup};
    use gputools_replay_hl::MTLPixelFormat;

    fn expected(name: &str) -> Vec<u8> {
        let mut d = reference(name).unwrap().to_vec();
        d[PRIMARIES_OFFSET] = PRIMARIES_UNSPECIFIED;
        d
    }

    #[test]
    fn every_supported_format_reproduces_the_reference_writer_dfd() {
        for (mtl, vk) in ROWS {
            let ours = build(*mtl, vk).unwrap_or_else(|e| panic!("{}: {e}", vk.name));
            assert_eq!(ours, expected(vk.name), "{} (mtl {mtl}) differs from ktx create", vk.name);
        }
    }

    #[test]
    fn primaries_are_unspecified_and_srgb_sets_the_transfer() {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm_sRGB).unwrap();
        let d = build(81, &vk).unwrap();
        assert_eq!(d[PRIMARIES_OFFSET], 0);
        assert_eq!(d[14], TRANSFER_SRGB);
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        assert_eq!(build(80, &vk).unwrap()[14], TRANSFER_LINEAR);
    }

    #[test]
    fn bgra_samples_are_in_memory_order() {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        let d = build(80, &vk).unwrap();
        // sample i starts at 28 + 16*i; byte 3 of a sample is channelType.
        let chan = |i: usize| d[28 + 16 * i + 3] & 0x0f;
        assert_eq!([chan(0), chan(1), chan(2), chan(3)], [CH_B, CH_G, CH_R, CH_A]);
    }

    #[test]
    fn excluded_kinds_are_named_errors() {
        let fake = VkFormat { name: "NONE", value: 0, type_size: 1 };
        assert_eq!(build(90, &fake), Err(DfdError::Packed)); // RGB10A2Unorm
        assert_eq!(build(93, &fake), Err(DfdError::Packed)); // RGB9E5Float
        assert_eq!(build(160, &fake), Err(DfdError::Pvrtc));
        assert_eq!(build(260, &fake), Err(DfdError::CombinedDepthStencil));
        assert_eq!(build(0xffff_ff00, &fake), Err(DfdError::Unknown));
        assert_eq!(build(204, &fake), Err(DfdError::NoReference("NONE")));
    }
}
```

- [ ] **Step 2: Register the module and run the tests**

Add `pub mod dfd;` to `src/lib.rs`.
Run: `cargo test dfd`
Expected: 4 tests PASS. If `every_supported_format_reproduces_the_reference_writer_dfd` fails for a colour or depth format, print both byte vectors and compare against the fixture: the fixture is the oracle.

- [ ] **Step 3: Commit**

```bash
git add src/dfd.rs src/lib.rs
git commit -m "feat: DFD builder byte-equal to ktx create for every supported format

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: The KTX2 writer (and the small reader the tests use)

**Files:**
- Create: `src/ktx.rs`
- Modify: `src/lib.rs` (add `pub mod ktx;`)

**Interfaces:**
- Produces: `ktx::Ktx2Params<'a> { vk_format: u32, type_size: u32, width: u32, height: u32, texel_block_bytes: u32, dfd: &'a [u8], kv: &'a [(String, String)] }`, `ktx::write_ktx2(&Ktx2Params, payload: &[u8]) -> Result<Vec<u8>, KtxError>`, `ktx::Header { vk_format, type_size, width, height, depth, layer_count, face_count, level_count, dfd_offset, dfd_len, kvd_offset, kvd_len }`, `ktx::parse_header(&[u8]) -> Option<Header>`, `ktx::level0(&[u8]) -> Option<&[u8]>`, `ktx::kv_pairs(&[u8]) -> Vec<(String, String)>`.

- [ ] **Step 1: Write `src/ktx.rs`**

```rust
//! A minimal KTX2 writer (spec 7): one level, one layer, one face, no
//! supercompression. Header, index, level index, DFD, key/value data,
//! aligned level payload. Plus a reader used by tests to check what was
//! written without trusting the writer.

const IDENTIFIER: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];
const HEADER_LEN: u64 = 48;
const INDEX_LEN: u64 = 32;
const LEVEL_INDEX_LEN: u64 = 24; // one level
/// Where the DFD starts: header, index, one level-index entry.
pub const DFD_OFFSET: u64 = HEADER_LEN + INDEX_LEN + LEVEL_INDEX_LEN;

#[derive(Debug, Clone, Copy)]
pub struct Ktx2Params<'a> {
    pub vk_format: u32,
    pub type_size: u32,
    pub width: u32,
    pub height: u32,
    /// Bytes per texel block: bytes per pixel for uncompressed formats,
    /// `block_bytes` for compressed. Sets the level data alignment.
    pub texel_block_bytes: u32,
    pub dfd: &'a [u8],
    pub kv: &'a [(String, String)],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KtxError {
    #[error("dimension must be non-zero (got {width}x{height})")]
    ZeroDimension { width: u32, height: u32 },
    #[error("payload is empty")]
    EmptyPayload,
    #[error("texel block size must be non-zero")]
    ZeroBlock,
}

fn build_kvd(kv: &[(String, String)]) -> Vec<u8> {
    let mut entries: Vec<&(String, String)> = kv.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut out = Vec::new();
    for (k, v) in entries {
        let len = k.len() + 1 + v.len() + 1;
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        out.extend_from_slice(v.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

fn lcm4(n: u64) -> u64 {
    let (mut a, mut b) = (n, 4u64);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    n / a * 4
}

pub fn write_ktx2(p: &Ktx2Params, payload: &[u8]) -> Result<Vec<u8>, KtxError> {
    if p.width == 0 || p.height == 0 {
        return Err(KtxError::ZeroDimension { width: p.width, height: p.height });
    }
    if payload.is_empty() {
        return Err(KtxError::EmptyPayload);
    }
    if p.texel_block_bytes == 0 {
        return Err(KtxError::ZeroBlock);
    }
    let kvd = build_kvd(p.kv);
    let kvd_off = DFD_OFFSET + p.dfd.len() as u64;
    let align = lcm4(u64::from(p.texel_block_bytes));
    let mut level_off = kvd_off + kvd.len() as u64;
    if !level_off.is_multiple_of(align) {
        level_off += align - (level_off % align);
    }

    let mut out = Vec::with_capacity(level_off as usize + payload.len());
    out.extend_from_slice(&IDENTIFIER);
    for v in [p.vk_format, p.type_size, p.width, p.height, 0, 0, 1, 1, 0] {
        // pixelDepth 0, layerCount 0, faceCount 1, levelCount 1, scheme 0
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(DFD_OFFSET as u32).to_le_bytes());
    out.extend_from_slice(&(p.dfd.len() as u32).to_le_bytes());
    out.extend_from_slice(&(if kvd.is_empty() { 0 } else { kvd_off as u32 }).to_le_bytes());
    out.extend_from_slice(&(kvd.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteOffset
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteLength
    out.extend_from_slice(&level_off.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(p.dfd);
    out.extend_from_slice(&kvd);
    out.resize(level_off as usize, 0);
    out.extend_from_slice(payload);
    Ok(out)
}

/// The fields of a KTX2 header and index this tool cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub vk_format: u32,
    pub type_size: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub layer_count: u32,
    pub face_count: u32,
    pub level_count: u32,
    pub dfd_offset: u32,
    pub dfd_len: u32,
    pub kvd_offset: u32,
    pub kvd_len: u32,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

pub fn parse_header(b: &[u8]) -> Option<Header> {
    if b.get(..12)? != IDENTIFIER {
        return None;
    }
    Some(Header {
        vk_format: u32_at(b, 12)?,
        type_size: u32_at(b, 16)?,
        width: u32_at(b, 20)?,
        height: u32_at(b, 24)?,
        depth: u32_at(b, 28)?,
        layer_count: u32_at(b, 32)?,
        face_count: u32_at(b, 36)?,
        level_count: u32_at(b, 40)?,
        dfd_offset: u32_at(b, 48)?,
        dfd_len: u32_at(b, 52)?,
        kvd_offset: u32_at(b, 56)?,
        kvd_len: u32_at(b, 60)?,
    })
}

/// The level-0 payload bytes.
pub fn level0(b: &[u8]) -> Option<&[u8]> {
    let off = u64_at(b, 80)? as usize;
    let len = u64_at(b, 88)? as usize;
    b.get(off..off.checked_add(len)?)
}

/// The key/value pairs, in file order.
pub fn kv_pairs(b: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(h) = parse_header(b) else { return out };
    let (start, end) = (h.kvd_offset as usize, (h.kvd_offset + h.kvd_len) as usize);
    let mut at = start;
    while at + 4 <= end {
        let Some(len) = u32_at(b, at) else { break };
        let Some(pair) = b.get(at + 4..at + 4 + len as usize) else { break };
        let mut parts = pair.split(|&c| c == 0);
        let k = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
        let v = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
        out.push((k, v));
        at += 4 + len as usize;
        while at % 4 != 0 {
            at += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dfd;
    use crate::vkformat::lookup;
    use gputools_replay_hl::MTLPixelFormat;

    fn bgra_params<'a>(dfd: &'a [u8], kv: &'a [(String, String)]) -> Ktx2Params<'a> {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        Ktx2Params { vk_format: vk.value, type_size: vk.type_size, width: 4, height: 4, texel_block_bytes: 4, dfd, kv }
    }

    #[test]
    fn header_fields_round_trip() {
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let out = write_ktx2(&bgra_params(&d, &[]), &[7u8; 64]).unwrap();
        let h = parse_header(&out).unwrap();
        assert_eq!(h.vk_format, 44);
        assert_eq!(h.type_size, 1);
        assert_eq!((h.width, h.height, h.depth), (4, 4, 0));
        assert_eq!((h.layer_count, h.face_count, h.level_count), (0, 1, 1));
        assert_eq!(h.dfd_offset as u64, DFD_OFFSET);
        assert_eq!(h.dfd_len as usize, d.len());
        assert_eq!((h.kvd_offset, h.kvd_len), (0, 0));
        assert_eq!(level0(&out).unwrap(), &[7u8; 64]);
    }

    #[test]
    fn kv_pairs_are_sorted_padded_and_read_back() {
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let kv = vec![
            ("gputrace.streamRef".to_string(), "25".to_string()),
            ("KTXwriter".to_string(), "ktx2-fetch 0.2.0".to_string()),
        ];
        let out = write_ktx2(&bgra_params(&d, &kv), &[0u8; 64]).unwrap();
        let read = kv_pairs(&out);
        assert_eq!(read[0].0, "KTXwriter"); // 'K' (0x4b) sorts before 'g'
        assert_eq!(read[1], ("gputrace.streamRef".to_string(), "25".to_string()));
        let h = parse_header(&out).unwrap();
        assert_eq!(h.kvd_offset % 4, 0);
        assert_eq!(h.kvd_len % 4, 0);
    }

    #[test]
    fn level_data_is_aligned_to_lcm_of_4_and_the_block_size() {
        for (block, want) in [(1u32, 4u64), (2, 4), (4, 4), (8, 8), (16, 16)] {
            let d = vec![0u8; 44];
            let kv = vec![("a".to_string(), "b".to_string())]; // 4+4 = 8 bytes: kvd length 8
            let p = Ktx2Params { vk_format: 1, type_size: 1, width: 1, height: 1, texel_block_bytes: block, dfd: &d, kv: &kv };
            let out = write_ktx2(&p, &[1u8; 16]).unwrap();
            let off = u64_at(&out, 80).unwrap();
            assert_eq!(off % want, 0, "block {block}");
            assert_eq!(level0(&out).unwrap(), &[1u8; 16]);
        }
    }

    #[test]
    fn rejects_zero_dimensions_and_empty_payloads() {
        let d = vec![0u8; 44];
        let p = Ktx2Params { vk_format: 1, type_size: 1, width: 0, height: 4, texel_block_bytes: 1, dfd: &d, kv: &[] };
        assert_eq!(write_ktx2(&p, &[1]), Err(KtxError::ZeroDimension { width: 0, height: 4 }));
        let p = Ktx2Params { width: 1, ..p };
        assert_eq!(write_ktx2(&p, &[]), Err(KtxError::EmptyPayload));
    }

    /// External oracle, only when Khronos' `ktx` is on PATH.
    #[test]
    fn a_written_file_passes_ktx_validate_when_available() {
        if std::process::Command::new("ktx").arg("--version").output().is_err() {
            eprintln!("SKIP: ktx not on PATH");
            return;
        }
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let kv = vec![("KTXwriter".to_string(), "ktx2-fetch test".to_string()), ("gputrace.streamRef".to_string(), "1".to_string())];
        let out = write_ktx2(&bgra_params(&d, &kv), &[9u8; 64]).unwrap();
        let path = std::env::temp_dir().join(format!("ktx2_fetch_unit_{}.ktx2", std::process::id()));
        std::fs::write(&path, &out).unwrap();
        let st = std::process::Command::new("ktx").arg("validate").arg(&path).output().unwrap();
        assert!(st.status.success(), "{}{}", String::from_utf8_lossy(&st.stdout), String::from_utf8_lossy(&st.stderr));
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod ktx;` to `src/lib.rs`.
Run: `cargo test ktx::`
Expected: 5 tests PASS (the validate test runs, since `ktx` is installed here).

- [ ] **Step 3: Commit**

```bash
git add src/ktx.rs src/lib.rs
git commit -m "feat: KTX2 writer with reader helpers, validated by ktx

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: The `Tex` seam over hl's `Texture`

**Files:**
- Create: `src/tex.rs`
- Modify: `src/lib.rs` (add `pub mod tex;`)

**Interfaces:**
- Produces: `tex::Aspect { Color, Depth, Stencil }` (serde lowercase), `tex::classify(&FormatKind) -> Aspect`, `tex::aspect_bpp(&FormatKind) -> Option<usize>`, `tex::Payload<'a> { Pixels(Cow<'a, [u8]>), Blocks { bytes: &'a [u8], expected_len: usize } }`, `trait tex::Tex { stream_ref, width, height, bytes_per_row, bytes_per_image, format, format_kind, raw_bytes, payload }`, `impl Tex for gputools_replay_hl::Texture`, and under `cfg(test)`: `tex::fake::FakeTex` with `FakeTex::new(stream_ref, width, height, bytes_per_row, mtl_raw, bytes)` and `FakeTex::solid(stream_ref, width, height, mtl_raw, pixel: &[u8])`.

- [ ] **Step 1: Write `src/tex.rs`**

```rust
//! The seam between hl's `Texture` and this tool: a trait exposing exactly
//! what the sweep and emitter read, so both are unit-tested with a fake
//! (hl's `Texture` cannot be constructed outside hl).

use gputools_replay_hl::format::{DepthKind, FormatKind, StencilKind, format_kind};
use gputools_replay_hl::{Error, MTLPixelFormat, Texture};
use serde::Serialize;
use std::borrow::Cow;

/// Which aspect a fetched image is (spec 5 step 3). `Color` covers every
/// colour and compressed format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Aspect {
    Color,
    Depth,
    Stencil,
}

pub fn classify(kind: &FormatKind) -> Aspect {
    if kind.is_depth_only() {
        Aspect::Depth
    } else if kind.is_stencil_only() {
        Aspect::Stencil
    } else {
        Aspect::Color
    }
}

/// Bytes per pixel as the replayer serves them: a colour format's stride,
/// or a single aspect's own element size (a stencil aspect is 1 byte per
/// pixel whatever the nominal X-padded Metal stride says). `None` for
/// compressed, combined depth-stencil, and unknown formats.
pub fn aspect_bpp(kind: &FormatKind) -> Option<usize> {
    match kind {
        FormatKind::Color(c) => Some(c.bytes_per_pixel),
        FormatKind::DepthStencil(d) => match (d.depth, d.stencil) {
            (Some(DepthKind::Unorm16), None) => Some(2),
            (Some(DepthKind::Float32), None) => Some(4),
            (None, Some(StencilKind::Uint8)) => Some(1),
            _ => None,
        },
        FormatKind::Compressed(_) | FormatKind::Unknown => None,
    }
}

/// The bytes a KTX2 level is written from.
pub enum Payload<'a> {
    /// Tightly packed rows. `Cow::Owned` when padding had to be dropped.
    Pixels(Cow<'a, [u8]>),
    /// Raw compressed blocks at the fetched row stride, and the tight
    /// length hl computes from the geometry.
    Blocks { bytes: &'a [u8], expected_len: usize },
}

pub trait Tex {
    fn stream_ref(&self) -> u64;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn bytes_per_row(&self) -> u32;
    fn bytes_per_image(&self) -> u32;
    fn format(&self) -> MTLPixelFormat;
    fn format_kind(&self) -> FormatKind {
        format_kind(self.format().0 as u32)
    }
    fn raw_bytes(&self) -> &[u8];
    fn payload(&self) -> Result<Payload<'_>, Error>;
}

impl Tex for Texture {
    fn stream_ref(&self) -> u64 {
        Texture::stream_ref(self)
    }
    fn width(&self) -> u32 {
        Texture::width(self)
    }
    fn height(&self) -> u32 {
        Texture::height(self)
    }
    fn bytes_per_row(&self) -> u32 {
        Texture::bytes_per_row(self)
    }
    fn bytes_per_image(&self) -> u32 {
        Texture::bytes_per_image(self)
    }
    fn format(&self) -> MTLPixelFormat {
        Texture::format(self)
    }
    fn raw_bytes(&self) -> &[u8] {
        Texture::raw_bytes(self)
    }
    fn payload(&self) -> Result<Payload<'_>, Error> {
        match Texture::format_kind(self) {
            FormatKind::Compressed(_) => {
                let b = self.blocks()?;
                Ok(Payload::Blocks { bytes: b.bytes, expected_len: b.expected_len() })
            }
            _ => Ok(Payload::Pixels(self.packed_bytes()?)),
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;

    /// A texture built from parts, mimicking hl's `packed_bytes`/`blocks`
    /// contract closely enough for the emitter and sweep tests.
    #[derive(Debug, Clone)]
    pub struct FakeTex {
        pub stream_ref: u64,
        pub width: u32,
        pub height: u32,
        pub bytes_per_row: u32,
        pub mtl_raw: u32,
        pub bytes: Vec<u8>,
    }

    impl FakeTex {
        pub fn new(stream_ref: u64, width: u32, height: u32, bytes_per_row: u32, mtl_raw: u32, bytes: Vec<u8>) -> Self {
            Self { stream_ref, width, height, bytes_per_row, mtl_raw, bytes }
        }

        /// An unpadded texture filled with one pixel value.
        pub fn solid(stream_ref: u64, width: u32, height: u32, mtl_raw: u32, pixel: &[u8]) -> Self {
            let bpr = width * pixel.len() as u32;
            let bytes = pixel.repeat((width * height) as usize);
            Self::new(stream_ref, width, height, bpr, mtl_raw, bytes)
        }
    }

    impl Tex for FakeTex {
        fn stream_ref(&self) -> u64 {
            self.stream_ref
        }
        fn width(&self) -> u32 {
            self.width
        }
        fn height(&self) -> u32 {
            self.height
        }
        fn bytes_per_row(&self) -> u32 {
            self.bytes_per_row
        }
        fn bytes_per_image(&self) -> u32 {
            self.bytes_per_row * self.height
        }
        fn format(&self) -> MTLPixelFormat {
            MTLPixelFormat(self.mtl_raw as _)
        }
        fn raw_bytes(&self) -> &[u8] {
            &self.bytes
        }
        fn payload(&self) -> Result<Payload<'_>, Error> {
            let kind = self.format_kind();
            if let FormatKind::Compressed(c) = &kind {
                let cols = (self.width as usize).div_ceil(c.block.0 as usize);
                let rows = (self.height as usize).div_ceil(c.block.1 as usize);
                return Ok(Payload::Blocks { bytes: &self.bytes, expected_len: cols * rows * c.block_bytes as usize });
            }
            let bpp = aspect_bpp(&kind).ok_or(Error::WrongCategory("fake: no per-pixel size"))?;
            let (w, h, bpr) = (self.width as usize, self.height as usize, self.bytes_per_row as usize);
            let row = w * bpp;
            if self.bytes.len() < h * bpr {
                return Err(Error::Truncated);
            }
            if bpr == row {
                return Ok(Payload::Pixels(Cow::Borrowed(&self.bytes[..h * bpr])));
            }
            let mut v = Vec::with_capacity(row * h);
            for y in 0..h {
                v.extend_from_slice(&self.bytes[y * bpr..y * bpr + row]);
            }
            Ok(Payload::Pixels(Cow::Owned(v)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeTex;
    use super::*;

    #[test]
    fn classify_by_aspect() {
        assert_eq!(classify(&format_kind(80)), Aspect::Color); // BGRA8Unorm
        assert_eq!(classify(&format_kind(204)), Aspect::Color); // ASTC_4x4_LDR
        assert_eq!(classify(&format_kind(252)), Aspect::Depth); // Depth32Float
        assert_eq!(classify(&format_kind(253)), Aspect::Stencil); // Stencil8
        assert_eq!(classify(&format_kind(261)), Aspect::Stencil); // X32_Stencil8
    }

    #[test]
    fn aspect_bpp_uses_the_served_size_not_the_nominal_stride() {
        assert_eq!(aspect_bpp(&format_kind(261)), Some(1)); // X32_Stencil8 served at 1 B/px
        assert_eq!(aspect_bpp(&format_kind(252)), Some(4));
        assert_eq!(aspect_bpp(&format_kind(250)), Some(2));
        assert_eq!(aspect_bpp(&format_kind(125)), Some(16));
        assert_eq!(aspect_bpp(&format_kind(260)), None);
        assert_eq!(aspect_bpp(&format_kind(204)), None);
    }

    #[test]
    fn fake_payload_borrows_when_tight_and_repacks_when_padded() {
        let tight = FakeTex::solid(1, 2, 2, 80, &[1, 2, 3, 4]);
        assert!(matches!(tight.payload().unwrap(), Payload::Pixels(Cow::Borrowed(_))));
        let mut bytes = vec![0xEE; 12 * 2];
        for y in 0..2 {
            for x in 0..2 {
                bytes[y * 12 + x * 4..y * 12 + x * 4 + 4].copy_from_slice(&[x as u8, y as u8, 9, 9]);
            }
        }
        let padded = FakeTex::new(1, 2, 2, 12, 80, bytes);
        match padded.payload().unwrap() {
            Payload::Pixels(Cow::Owned(v)) => assert_eq!(v, vec![0, 0, 9, 9, 1, 0, 9, 9, 0, 1, 9, 9, 1, 1, 9, 9]),
            _ => panic!("expected an owned repack"),
        }
        let short = FakeTex::new(1, 4, 4, 16, 70, vec![0; 8]);
        assert!(matches!(short.payload(), Err(Error::Truncated)));
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod tex;` to `src/lib.rs`.
Run: `cargo test tex::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/tex.rs src/lib.rs
git commit -m "feat: Tex seam over hl Texture with a test fake

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 6: The manifest types and exit-code policy

**Files:**
- Create: `src/manifest.rs`
- Modify: `src/lib.rs` (add `pub mod manifest;`)

**Interfaces:**
- Consumes: `tex::Aspect`.
- Produces (all `Serialize`, all fields `pub`): `manifest::Manifest`, `BundleManifest { Ok { textures_listed }, NoDescriptors, Unparseable }`, `Coverage { answered, attributed, unattributed, listed_not_answered }`, `TextureEntry`, `DescriptorEntry`, `Attribution { Certain, Ambiguous }`, `Duplicate { stream_ref, identical }`, `StencilProbe { stream_ref, outcome }`, `ProbeOutcome { Written, Absent }`, `Failure { stream_ref, aspect, reason }`, `Manifest::new(bundle, max_stream_ref, force_load_unused, timeout_secs) -> Manifest`, `Manifest::exit_code(&self) -> u8`, `Manifest::write(&self, path) -> io::Result<()>`, `Manifest::assumptions_line(force_load_unused: bool) -> String`.

- [ ] **Step 1: Write `src/manifest.rs`**

```rust
//! `manifest.json` (spec 7.4) and the exit-code policy (spec 8).

use crate::tex::Aspect;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BundleManifest {
    Ok { textures_listed: usize },
    NoDescriptors,
    Unparseable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Coverage {
    /// Distinct pass-1 streamRefs after dedupe.
    pub answered: usize,
    pub attributed: usize,
    pub unattributed: usize,
    /// Descriptors the bundle lists that no fetched texture claimed.
    pub listed_not_answered: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Attribution {
    Certain,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DescriptorEntry {
    pub mip_levels: u32,
    pub array_length: u32,
    pub depth: u32,
    pub texture_type: String,
    pub usage: u64,
    pub attribution: Attribution,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextureEntry {
    pub stream_ref: u64,
    pub aspect: Aspect,
    pub file: String,
    pub mtl_pixel_format: String,
    pub mtl_pixel_format_raw: u32,
    pub vk_format: String,
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub rows_repacked: bool,
    pub descriptor: Option<DescriptorEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Duplicate {
    pub stream_ref: u64,
    pub identical: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProbeOutcome {
    Written,
    Absent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StencilProbe {
    pub stream_ref: u64,
    pub outcome: ProbeOutcome,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub stream_ref: u64,
    pub aspect: Aspect,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub bundle: String,
    pub tool_version: String,
    pub engine: String,
    pub max_stream_ref: u64,
    pub force_load_unused: bool,
    pub timeout_secs: u64,
    pub assumptions: Vec<String>,
    pub bundle_manifest: BundleManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
    pub textures: Vec<TextureEntry>,
    pub duplicates: Vec<Duplicate>,
    pub stencil_probes: Vec<StencilProbe>,
    pub failures: Vec<Failure>,
    pub sweep_error: Option<String>,
}

impl Manifest {
    pub fn new(bundle: String, max_stream_ref: u64, force_load_unused: bool, timeout_secs: u64) -> Self {
        Self {
            bundle,
            tool_version: crate::TOOL_VERSION.to_string(),
            engine: crate::engine().to_string(),
            max_stream_ref,
            force_load_unused,
            timeout_secs,
            assumptions: vec![
                "MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 was set; without it the replayer cannot create its command queue in an unentitled process".to_string(),
                format!("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE={}; textures no captured command reads answer only when it is 1", u8::from(force_load_unused)),
                "streamRefs are swept 0..=max_stream_ref; they are assigned by the replayer's load path and are not stored in the bundle".to_string(),
                "alpha is assumed straight (Metal does not record premultiplication)".to_string(),
                "descriptor attribution is by creation-order rank; 'ambiguous' marks geometry groups where fetched and listed counts differ".to_string(),
            ],
            bundle_manifest: BundleManifest::Unparseable,
            coverage: None,
            textures: Vec::new(),
            duplicates: Vec::new(),
            stencil_probes: Vec::new(),
            failures: Vec::new(),
            sweep_error: None,
        }
    }

    /// The one-line assumptions string written into every file's KV data.
    pub fn assumptions_line(force_load_unused: bool) -> String {
        format!(
            "MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0; MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE={}; alpha assumed straight (Metal does not record premultiplication)",
            u8::from(force_load_unused)
        )
    }

    /// 0 when nothing failed; 1 when any per-texture failure or a sweep
    /// error was recorded. (2, a failure before the sweep, is the binary's.)
    pub fn exit_code(&self) -> u8 {
        if self.failures.is_empty() && self.sweep_error.is_none() { 0 } else { 1 }
    }

    pub fn write(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_reflects_failures_and_sweep_errors_only() {
        let mut m = Manifest::new("b".into(), 10, false, 60);
        assert_eq!(m.exit_code(), 0);
        m.duplicates.push(Duplicate { stream_ref: 1, identical: true });
        m.stencil_probes.push(StencilProbe { stream_ref: 2, outcome: ProbeOutcome::Absent });
        assert_eq!(m.exit_code(), 0, "informational entries are not failures");
        m.failures.push(Failure { stream_ref: 3, aspect: Aspect::Color, reason: "x".into() });
        assert_eq!(m.exit_code(), 1);
        let mut m = Manifest::new("b".into(), 10, false, 60);
        m.sweep_error = Some("fetch timed out".into());
        assert_eq!(m.exit_code(), 1);
    }

    #[test]
    fn serialises_the_spec_shape() {
        let mut m = Manifest::new("cap.gputrace".into(), 2000, true, 600);
        m.bundle_manifest = BundleManifest::Ok { textures_listed: 7 };
        m.coverage = Some(Coverage { answered: 7, attributed: 7, unattributed: 0, listed_not_answered: 0 });
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(v["bundle_manifest"]["status"], "ok");
        assert_eq!(v["bundle_manifest"]["textures_listed"], 7);
        assert_eq!(v["coverage"]["answered"], 7);
        assert_eq!(v["force_load_unused"], true);
        assert!(v["engine"].as_str().unwrap().starts_with("gputools-replay-hl"));
        let m = Manifest::new("cap".into(), 1, false, 1);
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(v["bundle_manifest"]["status"], "unparseable");
        assert!(v.get("coverage").is_none(), "coverage is omitted without a parsed manifest");
        assert_eq!(v["sweep_error"], serde_json::Value::Null);
    }

    #[test]
    fn enums_serialise_lowercase() {
        assert_eq!(serde_json::to_string(&Attribution::Certain).unwrap(), "\"certain\"");
        assert_eq!(serde_json::to_string(&ProbeOutcome::Written).unwrap(), "\"written\"");
        assert_eq!(serde_json::to_string(&Aspect::Stencil).unwrap(), "\"stencil\"");
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod manifest;` to `src/lib.rs`.
Run: `cargo test manifest::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/manifest.rs src/lib.rs
git commit -m "feat: manifest types and exit-code policy

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: The emitter: one fetched texture to one file

**Files:**
- Create: `src/emit.rs`
- Modify: `src/lib.rs` (add `pub mod emit;`)

**Interfaces:**
- Consumes: `tex::{Tex, Payload, Aspect, aspect_bpp}`, `vkformat::{lookup, metal_name}`, `dfd::build`, `ktx::{Ktx2Params, write_ktx2}`, `manifest::*`.
- Produces: `emit::Attributed { descriptor: TextureDescriptor, attribution: Attribution }`, `emit::Fetched<T> { texture: T, aspect: Aspect, probed: bool, descriptor: Option<Attributed> }`, `emit::Context<'a> { out: &'a Path, bundle: &'a str, force_load_unused: bool }`, `emit::emit_one<T: Tex>(ctx: &Context, f: &Fetched<T>, man: &mut Manifest)`, `emit::file_name<T: Tex>(f: &Fetched<T>) -> String`, `emit::texture_type_name(MTLTextureType) -> &'static str`.

- [ ] **Step 1: Write `src/emit.rs`**

```rust
//! One fetched texture to one `.ktx2` file, or one recorded failure
//! (spec 7). Nothing here aborts a run.

use crate::dfd;
use crate::ktx::{Ktx2Params, write_ktx2};
use crate::manifest::{Attribution, DescriptorEntry, Failure, Manifest, TextureEntry};
use crate::tex::{Aspect, Payload, Tex, aspect_bpp};
use crate::vkformat::{lookup, metal_name};
use gputools_replay_hl::format::FormatKind;
use gputools_replay_hl::{MTLTextureType, TextureDescriptor};
use std::borrow::Cow;
use std::path::Path;

/// A descriptor the join attributed, with the grade from spec 5 step 5.
#[derive(Debug, Clone, Copy)]
pub struct Attributed {
    pub descriptor: TextureDescriptor,
    pub attribution: Attribution,
}

/// One record the sweep decided to write.
pub struct Fetched<T> {
    pub texture: T,
    pub aspect: Aspect,
    /// True for a stencil aspect obtained by the pass-2 probe (plane 1);
    /// such files get the `_stencil` suffix.
    pub probed: bool,
    pub descriptor: Option<Attributed>,
}

pub struct Context<'a> {
    pub out: &'a Path,
    pub bundle: &'a str,
    pub force_load_unused: bool,
}

pub fn texture_type_name(t: MTLTextureType) -> &'static str {
    match t {
        MTLTextureType::Type1D => "1D",
        MTLTextureType::Type1DArray => "1DArray",
        MTLTextureType::Type2D => "2D",
        MTLTextureType::Type2DArray => "2DArray",
        MTLTextureType::Type2DMultisample => "2DMultisample",
        MTLTextureType::TypeCube => "Cube",
        MTLTextureType::TypeCubeArray => "CubeArray",
        MTLTextureType::Type3D => "3D",
        MTLTextureType::Type2DMultisampleArray => "2DMultisampleArray",
        MTLTextureType::TypeTextureBuffer => "TextureBuffer",
        _ => "unknown",
    }
}

fn texture_type(d: &TextureDescriptor) -> MTLTextureType {
    MTLTextureType(d.texture_type as _)
}

pub fn file_name<T: Tex>(f: &Fetched<T>) -> String {
    let mut name = format!(
        "ref{}_{}x{}_{}",
        f.texture.stream_ref(),
        f.texture.width(),
        f.texture.height(),
        metal_name(f.texture.format())
    );
    if f.probed {
        name.push_str("_stencil");
    }
    name.push_str(".ktx2");
    name
}

fn unsupported_reason(kind: &FormatKind) -> &'static str {
    match kind {
        FormatKind::Color(_) => "packed formats are not mapped",
        FormatKind::DepthStencil(_) => "combined depth-stencil is never served as one image",
        FormatKind::Compressed(_) => "PVRTC has no VkFormat",
        FormatKind::Unknown => "not described by gputools-replay-hl's format table",
    }
}

fn provenance_kv<T: Tex>(ctx: &Context, f: &Fetched<T>, rows_repacked: bool) -> Vec<(String, String)> {
    let t = &f.texture;
    let aspect = match f.aspect {
        Aspect::Color => "color",
        Aspect::Depth => "depth",
        Aspect::Stencil => "stencil",
    };
    let mut kv = vec![
        ("KTXwriter".to_string(), format!("ktx2-fetch {}", crate::TOOL_VERSION)),
        ("gputrace.aspect".to_string(), aspect.to_string()),
        ("gputrace.assumptions".to_string(), Manifest::assumptions_line(ctx.force_load_unused)),
        ("gputrace.bundle".to_string(), ctx.bundle.to_string()),
        ("gputrace.bytesPerImage".to_string(), t.bytes_per_image().to_string()),
        ("gputrace.bytesPerRow".to_string(), t.bytes_per_row().to_string()),
        ("gputrace.mtlPixelFormat".to_string(), format!("{} ({})", metal_name(t.format()), t.format().0)),
        ("gputrace.rowsRepacked".to_string(), rows_repacked.to_string()),
        ("gputrace.streamRef".to_string(), t.stream_ref().to_string()),
    ];
    if let Some(a) = &f.descriptor {
        let d = &a.descriptor;
        let grade = match a.attribution {
            Attribution::Certain => "certain",
            Attribution::Ambiguous => "ambiguous",
        };
        kv.push(("gputrace.arrayLength".to_string(), d.array_length.to_string()));
        kv.push(("gputrace.depth".to_string(), d.depth.to_string()));
        kv.push(("gputrace.descriptorAttribution".to_string(), grade.to_string()));
        kv.push(("gputrace.mipLevelCount".to_string(), d.mip_levels.to_string()));
        kv.push(("gputrace.textureType".to_string(), texture_type_name(texture_type(d)).to_string()));
        kv.push(("gputrace.textureUsage".to_string(), d.usage.to_string()));
    }
    kv
}

/// Tight compressed block rows: hl's `blocks().bytes` is at the fetched
/// row stride, which may hold padding blocks past `ceil(width / bw)`.
fn packed_blocks<'a>(bytes: &'a [u8], width: u32, height: u32, bytes_per_row: u32, block: (u8, u8), block_bytes: u8, expected_len: usize) -> Result<Cow<'a, [u8]>, String> {
    let cols = (width as usize).div_ceil(block.0 as usize);
    let rows = (height as usize).div_ceil(block.1 as usize);
    let tight_row = cols * block_bytes as usize;
    let bpr = bytes_per_row as usize;
    if bpr < tight_row {
        return Err(format!("bytesPerRow {bpr} is smaller than the {tight_row} bytes {cols} blocks need"));
    }
    if bytes.len() < rows * bpr {
        return Err(format!("payload is {} bytes but {} block rows of {bpr} bytes need {}", bytes.len(), rows, rows * bpr));
    }
    if bpr == tight_row {
        let got = bytes.get(..expected_len).ok_or_else(|| format!("payload shorter than expected {expected_len}"))?;
        return Ok(Cow::Borrowed(got));
    }
    let mut v = Vec::with_capacity(expected_len);
    for r in 0..rows {
        let row = bytes.get(r * bpr..r * bpr + tight_row).ok_or_else(|| "block row out of range".to_string())?;
        v.extend_from_slice(row);
    }
    Ok(Cow::Owned(v))
}

/// Write one texture. Every failure is recorded against `(stream_ref,
/// aspect)` in `man.failures`; every success in `man.textures`.
pub fn emit_one<T: Tex>(ctx: &Context, f: &Fetched<T>, man: &mut Manifest) {
    let t = &f.texture;
    let fail = |man: &mut Manifest, reason: String| {
        man.failures.push(Failure { stream_ref: t.stream_ref(), aspect: f.aspect, reason });
    };

    // Spec 5 step 6: a certain-attributed volume with depth > 1 cannot say
    // which z-plane it holds. Depth-1 volumes and ambiguous ones are written.
    if let Some(a) = &f.descriptor
        && a.attribution == Attribution::Certain
        && texture_type(&a.descriptor) == MTLTextureType::Type3D
        && a.descriptor.depth > 1
    {
        return fail(man, format!("3D texture, depth {}: the fetch serves one unidentified z-plane", a.descriptor.depth));
    }

    let kind = t.format_kind();
    let Some(vk) = lookup(t.format()) else {
        return fail(man, format!("unsupported MTLPixelFormat {} ({}): {}", metal_name(t.format()), t.format().0, unsupported_reason(&kind)));
    };
    let dfd = match dfd::build(t.format().0 as u32, &vk) {
        Ok(d) => d,
        Err(e) => return fail(man, format!("data format descriptor: {e}")),
    };

    let payload = match t.payload() {
        Ok(p) => p,
        Err(e) => return fail(man, format!("payload: {e}")),
    };
    let (bytes, rows_repacked, texel_block_bytes): (Cow<[u8]>, bool, u32) = match (payload, &kind) {
        (Payload::Pixels(p), kind) => {
            let Some(bpp) = aspect_bpp(kind) else {
                return fail(man, "no per-pixel size for this format".to_string());
            };
            let expected = t.width() as usize * t.height() as usize * bpp;
            if p.len() != expected {
                return fail(man, format!("packed payload is {} bytes but {}x{} at {bpp} B/px needs {expected}", p.len(), t.width(), t.height()));
            }
            let repacked = matches!(p, Cow::Owned(_));
            (p, repacked, bpp as u32)
        }
        (Payload::Blocks { bytes, expected_len }, FormatKind::Compressed(c)) => {
            match packed_blocks(bytes, t.width(), t.height(), t.bytes_per_row(), c.block, c.block_bytes, expected_len) {
                Ok(b) => {
                    let repacked = matches!(b, Cow::Owned(_));
                    (b, repacked, u32::from(c.block_bytes))
                }
                Err(e) => return fail(man, format!("compressed payload: {e}")),
            }
        }
        (Payload::Blocks { .. }, _) => return fail(man, "blocks payload for a non-compressed format".to_string()),
    };

    let kv = provenance_kv(ctx, f, rows_repacked);
    let params = Ktx2Params { vk_format: vk.value, type_size: vk.type_size, width: t.width(), height: t.height(), texel_block_bytes, dfd: &dfd, kv: &kv };
    let file = match write_ktx2(&params, &bytes) {
        Ok(b) => b,
        Err(e) => return fail(man, format!("ktx2: {e}")),
    };
    let name = file_name(f);
    if let Err(e) = std::fs::write(ctx.out.join(&name), &file) {
        return fail(man, format!("writing {name}: {e}"));
    }
    man.textures.push(TextureEntry {
        stream_ref: t.stream_ref(),
        aspect: f.aspect,
        file: name,
        mtl_pixel_format: metal_name(t.format()),
        mtl_pixel_format_raw: t.format().0 as u32,
        vk_format: vk.name.to_string(),
        width: t.width(),
        height: t.height(),
        bytes_per_row: t.bytes_per_row(),
        rows_repacked,
        descriptor: f.descriptor.as_ref().map(|a| DescriptorEntry {
            mip_levels: a.descriptor.mip_levels,
            array_length: a.descriptor.array_length,
            depth: a.descriptor.depth,
            texture_type: texture_type_name(texture_type(&a.descriptor)).to_string(),
            usage: a.descriptor.usage,
            attribution: a.attribution,
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ktx::{kv_pairs, level0, parse_header};
    use crate::tex::fake::FakeTex;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ktx2_fetch_emit_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn desc(texture_type: u32, depth: u32, mips: u32, arr: u32) -> TextureDescriptor {
        TextureDescriptor { store0_offset: 0, format: 80, texture_type, width: 4, height: 4, depth, mip_levels: mips, array_length: arr, sample_count: 1, usage: 5, texture_id: 0 }
    }

    fn run(name: &str, f: &Fetched<FakeTex>) -> (Manifest, PathBuf) {
        let out = scratch(name);
        let ctx = Context { out: &out, bundle: "cap.gputrace", force_load_unused: false };
        let mut man = Manifest::new("cap.gputrace".into(), 10, false, 60);
        emit_one(&ctx, f, &mut man);
        (man, out)
    }

    fn plain(t: FakeTex, aspect: Aspect) -> Fetched<FakeTex> {
        Fetched { texture: t, aspect, probed: false, descriptor: None }
    }

    #[test]
    fn unpadded_bgra_passes_through_untouched() {
        let px: Vec<u8> = (0..64).collect();
        let t = FakeTex::new(25, 4, 4, 16, 80, px.clone());
        let (man, out) = run("plain", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        let e = &man.textures[0];
        assert_eq!(e.file, "ref25_4x4_BGRA8Unorm.ktx2");
        assert!(!e.rows_repacked);
        assert_eq!(e.vk_format, "B8G8R8A8_UNORM");
        let bytes = std::fs::read(out.join(&e.file)).unwrap();
        assert_eq!(level0(&bytes).unwrap(), px.as_slice());
        let h = parse_header(&bytes).unwrap();
        assert_eq!((h.depth, h.layer_count, h.face_count, h.level_count), (0, 0, 1, 1));
        let kv = kv_pairs(&bytes);
        assert!(kv.iter().any(|(k, v)| k == "gputrace.streamRef" && v == "25"));
        assert!(kv.iter().any(|(k, v)| k == "gputrace.rowsRepacked" && v == "false"));
        assert!(kv.iter().all(|(k, _)| k != "gputrace.mipLevelCount"), "no descriptor keys without a descriptor");
    }

    #[test]
    fn padded_rows_are_written_tight_and_flagged() {
        let mut bytes = vec![0xEE; 24 * 2];
        for y in 0..2 {
            for x in 0..2 {
                bytes[y * 24 + x * 4..y * 24 + x * 4 + 4].copy_from_slice(&[x as u8, y as u8, 7, 7]);
            }
        }
        let t = FakeTex::new(1, 2, 2, 24, 80, bytes);
        let (man, out) = run("padded", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert!(man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(level0(&file).unwrap(), &[0, 0, 7, 7, 1, 0, 7, 7, 0, 1, 7, 7, 1, 1, 7, 7]);
        assert!(kv_pairs(&file).iter().any(|(k, v)| k == "gputrace.rowsRepacked" && v == "true"));
    }

    #[test]
    fn compressed_blocks_pass_through_and_padded_block_rows_are_tightened() {
        // 8x8 ASTC 4x4: 2x2 blocks of 16 bytes, tight row 32.
        let blocks: Vec<u8> = (0..64).collect();
        let t = FakeTex::new(2, 8, 8, 32, 204, blocks.clone());
        let (man, out) = run("astc", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert_eq!(man.textures[0].file, "ref2_8x8_ASTC_4x4_LDR.ktx2");
        assert!(!man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(level0(&file).unwrap(), blocks.as_slice());
        assert_eq!(parse_header(&file).unwrap().vk_format, 157);
        // Same blocks at a 48-byte stride (one padding block per row).
        let mut padded = Vec::new();
        for r in 0..2 {
            padded.extend_from_slice(&blocks[r * 32..r * 32 + 32]);
            padded.extend_from_slice(&[0xAA; 16]);
        }
        let t = FakeTex::new(3, 8, 8, 48, 204, padded);
        let (man, out) = run("astc_padded", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert!(man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(level0(&file).unwrap(), blocks.as_slice());
    }

    #[test]
    fn stencil_aspects_get_the_suffix_and_s8_uint() {
        let t = FakeTex::solid(4, 2, 2, 261, &[42]);
        let f = Fetched { texture: t, aspect: Aspect::Stencil, probed: true, descriptor: None };
        let (man, _) = run("stencil", &f);
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert_eq!(man.textures[0].file, "ref4_2x2_X32_Stencil8_stencil.ktx2");
        assert_eq!(man.textures[0].vk_format, "S8_UINT");
        let t = FakeTex::solid(5, 2, 2, 253, &[42]);
        let (man, _) = run("stencil_base", &plain(t, Aspect::Stencil));
        assert_eq!(man.textures[0].file, "ref5_2x2_Stencil8.ktx2", "a base stencil texture has no suffix");
    }

    #[test]
    fn unsupported_and_truncated_are_named_failures() {
        let t = FakeTex::solid(7, 2, 2, 90, &[0, 0, 0, 0]); // RGB10A2Unorm
        let (man, _) = run("packed", &plain(t, Aspect::Color));
        assert!(man.textures.is_empty());
        assert_eq!(man.failures[0].stream_ref, 7);
        assert!(man.failures[0].reason.contains("RGB10A2Unorm (90)"), "{}", man.failures[0].reason);
        assert!(man.failures[0].reason.contains("packed"));
        let t = FakeTex::new(8, 4, 4, 16, 70, vec![0; 8]);
        let (man, _) = run("short", &plain(t, Aspect::Color));
        assert!(man.failures[0].reason.contains("truncated"), "{}", man.failures[0].reason);
    }

    #[test]
    fn volumes_are_refused_only_when_certain_and_deeper_than_one() {
        let t = FakeTex::solid(9, 4, 4, 80, &[1, 2, 3, 4]);
        let deep = Fetched { texture: t.clone(), aspect: Aspect::Color, probed: false, descriptor: Some(Attributed { descriptor: desc(7, 4, 1, 1), attribution: Attribution::Certain }) };
        let (man, _) = run("vol_certain", &deep);
        assert!(man.textures.is_empty());
        assert!(man.failures[0].reason.contains("3D texture, depth 4"), "{}", man.failures[0].reason);

        let flat = Fetched { texture: t.clone(), aspect: Aspect::Color, probed: false, descriptor: Some(Attributed { descriptor: desc(7, 1, 1, 1), attribution: Attribution::Certain }) };
        let (man, out) = run("vol_depth1", &flat);
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        let d = man.textures[0].descriptor.as_ref().unwrap();
        assert_eq!(d.texture_type, "3D");
        assert_eq!(d.attribution, Attribution::Certain);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        let kv = kv_pairs(&file);
        assert!(kv.iter().any(|(k, v)| k == "gputrace.textureType" && v == "3D"));
        assert!(kv.iter().any(|(k, v)| k == "gputrace.depth" && v == "1"));
        assert!(kv.iter().any(|(k, v)| k == "gputrace.descriptorAttribution" && v == "certain"));

        let ambiguous = Fetched { texture: t, aspect: Aspect::Color, probed: false, descriptor: Some(Attributed { descriptor: desc(7, 4, 1, 1), attribution: Attribution::Ambiguous }) };
        let (man, _) = run("vol_ambiguous", &ambiguous);
        assert!(man.failures.is_empty(), "an ambiguous attribution never withholds bytes");
        assert_eq!(man.textures[0].descriptor.as_ref().unwrap().attribution, Attribution::Ambiguous);
    }

    #[test]
    fn descriptor_keys_carry_mip_and_array_counts() {
        let t = FakeTex::solid(10, 4, 4, 80, &[1, 2, 3, 4]);
        let f = Fetched { texture: t, aspect: Aspect::Color, probed: false, descriptor: Some(Attributed { descriptor: desc(3, 1, 3, 6), attribution: Attribution::Certain }) };
        let (man, out) = run("desc", &f);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        let kv = kv_pairs(&file);
        assert!(kv.iter().any(|(k, v)| k == "gputrace.mipLevelCount" && v == "3"));
        assert!(kv.iter().any(|(k, v)| k == "gputrace.arrayLength" && v == "6"));
        assert!(kv.iter().any(|(k, v)| k == "gputrace.textureType" && v == "2DArray"));
        assert_eq!(man.textures[0].descriptor.as_ref().unwrap().mip_levels, 3);
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod emit;` to `src/lib.rs`.
Run: `cargo test emit::`
Expected: 7 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/emit.rs src/lib.rs
git commit -m "feat: emitter writes one KTX2 per fetched texture with provenance

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 8: The sweep: two passes, dedupe, attribution, coverage

**Files:**
- Create: `src/sweep.rs`
- Modify: `src/lib.rs` (add `pub mod sweep;`)

**Interfaces:**
- Consumes: `tex::{Tex, Aspect, classify}`, `emit::{Fetched, Attributed}`, `manifest::*`, hl `Descriptions`, `ManifestStatus`, `TextureDescriptor`, `Error`.
- Produces: `trait sweep::Fetcher { type Tex: Tex; fn manifest_status(&self) -> ManifestStatus; fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Self::Tex>, Error>; fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>; fn describe(&self, texs: &[Self::Tex]) -> Descriptions; }`, `sweep::Sweep<T> { fetched: Vec<Fetched<T>>, probes, duplicates, failures, bundle_manifest, coverage, sweep_error }`, `sweep::run<F: Fetcher>(f: &F, max_stream_ref: u64) -> Sweep<F::Tex>`.

- [ ] **Step 1: Write `src/sweep.rs`**

```rust
//! The two-pass fetch (spec 5): pass 1 sweeps plane 0, dedupes duplicate
//! streamRefs, joins descriptors and grades them; pass 2 probes every
//! depth-format ref for a stencil aspect. Nothing here touches disk.

use crate::emit::{Attributed, Fetched};
use crate::manifest::{Attribution, BundleManifest, Coverage, Duplicate, Failure, ProbeOutcome, StencilProbe};
use crate::tex::{Aspect, Tex, classify};
use gputools_replay_hl::{Descriptions, Error, ManifestStatus, TextureDescriptor};
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

pub trait Fetcher {
    type Tex: Tex;
    fn manifest_status(&self) -> ManifestStatus;
    fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Self::Tex>, Error>;
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
    fn describe(&self, texs: &[Self::Tex]) -> Descriptions;
}

pub struct Sweep<T> {
    pub fetched: Vec<Fetched<T>>,
    pub probes: Vec<StencilProbe>,
    pub duplicates: Vec<Duplicate>,
    pub failures: Vec<Failure>,
    pub bundle_manifest: BundleManifest,
    pub coverage: Option<Coverage>,
    pub sweep_error: Option<String>,
}

type Key = (u32, u32, u32);

fn key<T: Tex>(t: &T) -> Key {
    (t.width(), t.height(), t.format().0 as u32)
}

fn desc_key(d: &TextureDescriptor) -> Key {
    (d.width, d.height, d.format)
}

/// Collapse records sharing a streamRef (spec 5 step 4). Identical copies
/// keep one; differing copies are all dropped and recorded as a failure.
fn dedupe<T: Tex>(texs: Vec<T>, duplicates: &mut Vec<Duplicate>, failures: &mut Vec<Failure>) -> Vec<T> {
    let mut groups: BTreeMap<u64, Vec<T>> = BTreeMap::new();
    for t in texs {
        groups.entry(t.stream_ref()).or_default().push(t);
    }
    let mut kept = Vec::new();
    for (stream_ref, mut group) in groups {
        if group.len() == 1 {
            kept.extend(group);
            continue;
        }
        let first = group.remove(0);
        let identical = group.iter().all(|g| g.raw_bytes() == first.raw_bytes() && key(g) == key(&first));
        duplicates.push(Duplicate { stream_ref, identical });
        if identical {
            kept.push(first);
        } else {
            failures.push(Failure {
                stream_ref,
                aspect: classify(&first.format_kind()),
                reason: format!("{} records for this streamRef differ byte-for-byte; cannot choose one", group.len() + 1),
            });
        }
    }
    kept
}

/// Spec 5 step 5: a geometry group is `certain` only when the fetched and
/// listed counts for that exact `(width, height, format)` agree.
fn grade<T: Tex>(texs: &[T], d: &Descriptions) -> HashMap<Key, Attribution> {
    let mut fetched: HashMap<Key, usize> = HashMap::new();
    for t in texs {
        *fetched.entry(key(t)).or_default() += 1;
    }
    let mut listed: HashMap<Key, usize> = HashMap::new();
    for desc in d.per_texture.iter().flatten().chain(d.unplaced.iter()) {
        *listed.entry(desc_key(desc)).or_default() += 1;
    }
    fetched
        .keys()
        .chain(listed.keys())
        .map(|k| {
            let same = fetched.get(k).copied().unwrap_or(0) == listed.get(k).copied().unwrap_or(0);
            (*k, if same { Attribution::Certain } else { Attribution::Ambiguous })
        })
        .collect()
}

pub fn run<F: Fetcher>(f: &F, max_stream_ref: u64) -> Sweep<F::Tex> {
    let status = f.manifest_status();
    let bundle_manifest = match status {
        ManifestStatus::Ok(n) => BundleManifest::Ok { textures_listed: n },
        ManifestStatus::NoDescriptors => BundleManifest::NoDescriptors,
        ManifestStatus::Unparseable => BundleManifest::Unparseable,
    };
    let mut sweep = Sweep { fetched: Vec::new(), probes: Vec::new(), duplicates: Vec::new(), failures: Vec::new(), bundle_manifest, coverage: None, sweep_error: None };

    // Pass 1.
    let pass1 = match f.textures(0..=max_stream_ref) {
        Ok(t) => t,
        Err(e) => {
            sweep.sweep_error = Some(format!("pass 1 (plane 0 sweep): {e}"));
            return sweep;
        }
    };
    let kept = dedupe(pass1, &mut sweep.duplicates, &mut sweep.failures);

    // Describe and grade.
    let described = f.describe(&kept);
    let grades = grade(&kept, &described);
    if let ManifestStatus::Ok(_) = status {
        let attributed = described.per_texture.iter().flatten().count();
        sweep.coverage = Some(Coverage {
            answered: kept.len(),
            attributed,
            unattributed: kept.len() - attributed,
            listed_not_answered: described.unplaced.len(),
        });
    }

    let depth_refs: Vec<u64> = kept.iter().filter(|t| classify(&t.format_kind()) == Aspect::Depth).map(|t| t.stream_ref()).collect();

    for (t, desc) in kept.into_iter().zip(described.per_texture) {
        let aspect = classify(&t.format_kind());
        let descriptor = desc.map(|descriptor| Attributed {
            descriptor,
            attribution: grades.get(&key(&t)).copied().unwrap_or(Attribution::Ambiguous),
        });
        sweep.fetched.push(Fetched { texture: t, aspect, probed: false, descriptor });
    }

    // Pass 2: the stencil aspect of every depth-format ref. Plane 1 is
    // inert on a plain depth texture (it echoes the depth), so only a
    // stencil-only reply counts.
    if !depth_refs.is_empty() {
        match f.stencil_aspects(&depth_refs) {
            Ok(replies) => {
                let replies = dedupe(replies, &mut sweep.duplicates, &mut sweep.failures);
                let mut written: HashMap<u64, bool> = depth_refs.iter().map(|r| (*r, false)).collect();
                for t in replies {
                    if classify(&t.format_kind()) == Aspect::Stencil && written.contains_key(&t.stream_ref()) {
                        written.insert(t.stream_ref(), true);
                        sweep.fetched.push(Fetched { texture: t, aspect: Aspect::Stencil, probed: true, descriptor: None });
                    }
                }
                for r in &depth_refs {
                    let outcome = if written.get(r).copied().unwrap_or(false) { ProbeOutcome::Written } else { ProbeOutcome::Absent };
                    sweep.probes.push(StencilProbe { stream_ref: *r, outcome });
                }
            }
            Err(e) => sweep.sweep_error = Some(format!("pass 2 (stencil aspects): {e}")),
        }
    }
    sweep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tex::fake::FakeTex;
    use std::cell::RefCell;

    struct Fake {
        status: ManifestStatus,
        pass1: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
        stencil: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
        attributed: HashMap<u64, TextureDescriptor>,
        unplaced: Vec<TextureDescriptor>,
    }

    impl Fake {
        fn new(status: ManifestStatus, pass1: Result<Vec<FakeTex>, Error>) -> Self {
            Self { status, pass1: RefCell::new(Some(pass1)), stencil: RefCell::new(Some(Ok(Vec::new()))), attributed: HashMap::new(), unplaced: Vec::new() }
        }
        fn with_stencil(self, r: Result<Vec<FakeTex>, Error>) -> Self {
            *self.stencil.borrow_mut() = Some(r);
            self
        }
        fn attribute(mut self, stream_ref: u64, d: TextureDescriptor) -> Self {
            self.attributed.insert(stream_ref, d);
            self
        }
        fn unplaced(mut self, d: TextureDescriptor) -> Self {
            self.unplaced.push(d);
            self
        }
    }

    impl Fetcher for Fake {
        type Tex = FakeTex;
        fn manifest_status(&self) -> ManifestStatus {
            self.status
        }
        fn textures(&self, _refs: RangeInclusive<u64>) -> Result<Vec<FakeTex>, Error> {
            self.pass1.borrow_mut().take().unwrap()
        }
        fn stencil_aspects(&self, _refs: &[u64]) -> Result<Vec<FakeTex>, Error> {
            self.stencil.borrow_mut().take().unwrap()
        }
        fn describe(&self, texs: &[FakeTex]) -> Descriptions {
            Descriptions {
                per_texture: texs.iter().map(|t| self.attributed.get(&t.stream_ref).copied()).collect(),
                unplaced: self.unplaced.clone(),
            }
        }
    }

    fn desc(w: u32, h: u32, fmt: u32) -> TextureDescriptor {
        TextureDescriptor { store0_offset: 0, format: fmt, texture_type: 2, width: w, height: h, depth: 1, mip_levels: 1, array_length: 1, sample_count: 1, usage: 0, texture_id: 0 }
    }
    fn bgra(r: u64, w: u32, h: u32) -> FakeTex {
        FakeTex::solid(r, w, h, 80, &[1, 2, 3, 4])
    }
    fn depth(r: u64) -> FakeTex {
        FakeTex::solid(r, 2, 2, 252, &0.5f32.to_le_bytes())
    }
    fn stencil_aspect(r: u64) -> FakeTex {
        FakeTex::solid(r, 2, 2, 261, &[42])
    }

    #[test]
    fn classifies_and_probes_depth_refs_for_stencil() {
        let f = Fake::new(ManifestStatus::NoDescriptors, Ok(vec![bgra(1, 4, 4), depth(2), depth(3)]))
            .with_stencil(Ok(vec![stencil_aspect(2), depth(3)]));
        let s = run(&f, 10);
        assert!(s.sweep_error.is_none());
        let aspects: Vec<(u64, Aspect, bool)> = s.fetched.iter().map(|x| (x.texture.stream_ref, x.aspect, x.probed)).collect();
        assert_eq!(aspects, vec![(1, Aspect::Color, false), (2, Aspect::Depth, false), (3, Aspect::Depth, false), (2, Aspect::Stencil, true)]);
        assert_eq!(s.probes, vec![StencilProbe { stream_ref: 2, outcome: ProbeOutcome::Written }, StencilProbe { stream_ref: 3, outcome: ProbeOutcome::Absent }]);
        assert!(s.coverage.is_none(), "no coverage without a parsed manifest");
        assert_eq!(s.bundle_manifest, BundleManifest::NoDescriptors);
    }

    #[test]
    fn identical_duplicates_collapse_and_conflicting_ones_fail() {
        let mut other = bgra(5, 4, 4);
        other.bytes[0] = 99;
        let f = Fake::new(ManifestStatus::Unparseable, Ok(vec![bgra(4, 4, 4), bgra(4, 4, 4), bgra(5, 4, 4), other]));
        let s = run(&f, 10);
        let refs: Vec<u64> = s.fetched.iter().map(|x| x.texture.stream_ref).collect();
        assert_eq!(refs, vec![4]);
        assert_eq!(s.duplicates, vec![Duplicate { stream_ref: 4, identical: true }, Duplicate { stream_ref: 5, identical: false }]);
        assert_eq!(s.failures.len(), 1);
        assert_eq!(s.failures[0].stream_ref, 5);
        assert!(s.failures[0].reason.contains("differ"));
    }

    #[test]
    fn grades_by_group_count_equality_and_reports_coverage() {
        // 64x64 BGRA: three fetched, two listed -> ambiguous.
        // 32x32 BGRA: one fetched, one listed -> certain.
        // 16x16 BGRA: listed, never answered -> unplaced.
        let f = Fake::new(ManifestStatus::Ok(4), Ok(vec![bgra(1, 64, 64), bgra(2, 64, 64), bgra(3, 64, 64), bgra(4, 32, 32)]))
            .attribute(1, desc(64, 64, 80))
            .attribute(2, desc(64, 64, 80))
            .attribute(4, desc(32, 32, 80))
            .unplaced(desc(16, 16, 80));
        let s = run(&f, 10);
        let grade_of = |r: u64| s.fetched.iter().find(|x| x.texture.stream_ref == r).unwrap().descriptor.map(|a| a.attribution);
        assert_eq!(grade_of(1), Some(Attribution::Ambiguous));
        assert_eq!(grade_of(2), Some(Attribution::Ambiguous));
        assert_eq!(grade_of(3), None);
        assert_eq!(grade_of(4), Some(Attribution::Certain));
        assert_eq!(s.coverage, Some(Coverage { answered: 4, attributed: 3, unattributed: 1, listed_not_answered: 1 }));
        assert_eq!(s.bundle_manifest, BundleManifest::Ok { textures_listed: 4 });
    }

    #[test]
    fn an_unanswered_descriptor_makes_its_group_ambiguous() {
        let f = Fake::new(ManifestStatus::Ok(2), Ok(vec![bgra(1, 8, 8)]))
            .attribute(1, desc(8, 8, 80))
            .unplaced(desc(8, 8, 80));
        let s = run(&f, 10);
        assert_eq!(s.fetched[0].descriptor.unwrap().attribution, Attribution::Ambiguous);
    }

    #[test]
    fn pass1_error_is_run_level_and_keeps_the_manifest_status() {
        let f = Fake::new(ManifestStatus::Ok(3), Err(Error::Truncated));
        let s = run(&f, 10);
        assert!(s.fetched.is_empty());
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 1"));
        assert_eq!(s.bundle_manifest, BundleManifest::Ok { textures_listed: 3 });
        assert!(s.coverage.is_none());
    }

    #[test]
    fn pass2_error_keeps_pass1_results() {
        let f = Fake::new(ManifestStatus::NoDescriptors, Ok(vec![bgra(1, 4, 4), depth(2)])).with_stencil(Err(Error::Truncated));
        let s = run(&f, 10);
        assert_eq!(s.fetched.len(), 2);
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 2"));
        assert!(s.probes.is_empty());
    }
}
```

- [ ] **Step 2: Register and run**

Add `pub mod sweep;` to `src/lib.rs`.
Run: `cargo test sweep::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/sweep.rs src/lib.rs
git commit -m "feat: two-pass sweep with dedupe, attribution grading, coverage

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: The binary

**Files:**
- Modify: `src/main.rs` (replace the placeholder)
- Create: `tests/cli.rs`

**Interfaces:**
- Consumes: everything above; hl `Capture`, `ReplayerConfig`, `Aspect`.
- Produces: the `ktx2-fetch` CLI per spec 3.

- [ ] **Step 1: Write `src/main.rs`**

```rust
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use clap::Parser;
use gputools_replay_hl::{Aspect as FetchAspect, Capture, Descriptions, Error, ManifestStatus, ReplayerConfig, Texture};
use ktx2_fetch::emit::{Context, emit_one};
use ktx2_fetch::manifest::Manifest;
use ktx2_fetch::sweep::{self, Fetcher};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "ktx2-fetch", version, about = "Export every texture of a .gputrace capture as lossless KTX2")]
struct Args {
    /// The .gputrace bundle.
    bundle: PathBuf,
    /// Directory to write .ktx2 files and manifest.json into.
    #[arg(long)]
    out: PathBuf,
    /// Highest streamRef to sweep. Refs are sparse and assigned at load
    /// time, so the tool asks for every value up to this and keeps what
    /// answers.
    #[arg(long, default_value_t = 2000)]
    max_stream_ref: u64,
    /// Set MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1 so textures no captured
    /// command reads still answer.
    #[arg(long)]
    force_load_unused: bool,
    /// Per-fetch timeout in seconds. A large capture can take minutes.
    #[arg(long, default_value_t = 600)]
    timeout: u64,
}

fn main() -> ExitCode {
    // FIRST, before any thread exists: both env writes are sound only while
    // the process is single-threaded. The substrate verifies the unlock
    // variable in Capture::open and refuses with a named error otherwise.
    let force = std::env::args().any(|a| a == "--force-load-unused");
    #[allow(unsafe_code)]
    // SAFETY: no threads have been spawned yet; these are the first
    // statements of main.
    unsafe {
        std::env::set_var("MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX", "0");
        Capture::configure_env(&ReplayerConfig { force_load_unused_resources: force, ..ReplayerConfig::default() });
    }
    let args = Args::parse();
    match run(args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("ktx2-fetch: {e}");
            ExitCode::from(2)
        }
    }
}

struct Live(Capture);

impl Fetcher for Live {
    type Tex = Texture;
    fn manifest_status(&self) -> ManifestStatus {
        self.0.manifest_status()
    }
    fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Texture>, Error> {
        self.0.textures(refs)
    }
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Texture>, Error> {
        self.0.texture_aspects(refs.iter().copied(), FetchAspect::Stencil)
    }
    fn describe(&self, texs: &[Texture]) -> Descriptions {
        self.0.describe(texs)
    }
}

fn run(args: Args) -> Result<u8, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&args.out).map_err(|e| format!("creating {}: {e}", args.out.display()))?;
    let bundle = args.bundle.display().to_string();
    let mut cap = Capture::open(&args.bundle)?;
    cap.set_timeout(Duration::from_secs(args.timeout));
    let live = Live(cap);

    let mut man = Manifest::new(bundle.clone(), args.max_stream_ref, args.force_load_unused, args.timeout);
    let sweep = sweep::run(&live, args.max_stream_ref);
    man.bundle_manifest = sweep.bundle_manifest;
    man.coverage = sweep.coverage;
    man.duplicates = sweep.duplicates;
    man.stencil_probes = sweep.probes;
    man.failures = sweep.failures;
    man.sweep_error = sweep.sweep_error;

    let ctx = Context { out: &args.out, bundle: &bundle, force_load_unused: args.force_load_unused };
    for f in &sweep.fetched {
        emit_one(&ctx, f, &mut man);
    }

    if man.textures.is_empty() {
        eprintln!(
            "ktx2-fetch: warning: no textures were written; check that {bundle} is the capture you meant, that --max-stream-ref ({}) is at least as high as the streamRefs it uses, and consider --force-load-unused",
            args.max_stream_ref
        );
    }
    if let Some(c) = &man.coverage
        && c.listed_not_answered > 0
    {
        eprintln!(
            "ktx2-fetch: {} of the bundle's listed textures did not answer the fetch; if they are never read by a captured command, --force-load-unused makes them answer",
            c.listed_not_answered
        );
    }

    let code = man.exit_code();
    if let Err(e) = man.write(&args.out.join("manifest.json")) {
        eprintln!("ktx2-fetch: failed to write manifest.json: {e}");
        match serde_json::to_string_pretty(&man) {
            Ok(json) => eprintln!("ktx2-fetch: the manifest was not written to disk; printing it here so the run is not lost:\n{json}"),
            Err(se) => eprintln!("ktx2-fetch: could not serialise the manifest either: {se}"),
        }
        return Ok(code.max(1));
    }
    Ok(code)
}
```

- [ ] **Step 2: Write `tests/cli.rs`** (default suite, no replayer session: a missing bundle is rejected before the framework is touched)

```rust
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ktx2-fetch"))
}

#[test]
fn help_lists_every_flag() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for flag in ["--out", "--max-stream-ref", "--force-load-unused", "--timeout"] {
        assert!(text.contains(flag), "missing {flag} in:\n{text}");
    }
}

#[test]
fn a_missing_bundle_exits_2_with_a_named_error_and_no_manifest() {
    let out_dir = std::env::temp_dir().join(format!("ktx2_fetch_cli_{}", std::process::id()));
    let out = bin().arg("/nonexistent/thing.gputrace").arg("--out").arg(&out_dir).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("ktx2-fetch:"), "{err}");
    assert!(err.contains("nonexistent"), "{err}");
    assert!(!out_dir.join("manifest.json").exists());
}
```

- [ ] **Step 3: Run**

Run: `cargo test --test cli && cargo clippy --all-targets -- -D warnings`
Expected: 2 tests PASS; clippy clean. If clippy flags `let` chains, the edition is 2024 and rustc 1.98 supports them; check `edition = "2024"` in `Cargo.toml`.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: ktx2-fetch CLI wiring the sweep and emitter over a live Capture

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: This repo's fixtures and capture tooling

**Files:**
- Create: `fixtures/known-textures.m`, `fixtures/known-depth.m`, `fixtures/known-ds-pair.m`, `fixtures/known-stencil.m`, `fixtures/known-astc.m`, `fixtures/known-ycbcr.m`, `fixtures/known-ambiguous.m`, `fixtures/known-3d.m`, `fixtures/known-mips.m`, `fixtures/capture.sh`, `fixtures/capture-late.sh`, `fixtures/build-all.sh`, `fixtures/README.md`, `captures/README.md`

- [ ] **Step 1: Copy the fixture apps and capture scripts from the sibling checkout**

```bash
mkdir -p fixtures captures
for f in known-textures known-depth known-ds-pair known-stencil known-astc known-ycbcr known-ambiguous known-3d known-mips; do
  cp ../gputools-replay/fixture-apps/$f.m fixtures/
done
cp ../gputools-replay/fixture-apps/capture.sh ../gputools-replay/fixture-apps/capture-late.sh fixtures/
chmod +x fixtures/capture.sh fixtures/capture-late.sh
# The copied headers mention `fixture-apps/`; point them at this repo's directory.
sed -i '' 's#fixture-apps/#fixtures/#g' fixtures/*.m fixtures/*.sh
```

- [ ] **Step 2: Write `fixtures/build-all.sh`**

```bash
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
build known-ds-pair;   capture known-ds-pair   known-ds-pair
build known-stencil;   capture known-stencil   known-stencil
build known-astc;      capture known-astc      known-astc
build known-ycbcr -framework CoreVideo; capture known-ycbcr known-ycbcr
build known-ambiguous; capture known-ambiguous known-ambiguous
build known-3d;        capture known-3d        known-3d
build known-mips;      capture known-mips      known-mips
echo "all captures present under captures/"
```

Then `chmod +x fixtures/build-all.sh`.

- [ ] **Step 3: Write `fixtures/README.md`**

```markdown
# Fixtures

Tiny standalone Metal programs, each producing textures with exact ground
truth, plus the scripts that capture them. Copied from the
`gputools-replay` campaign on 2026-09-02 and maintained here so this repo
is free-standing. `fixtures/build-all.sh` builds and captures all of them
into `captures/` (gitignored).

Every app is two-phase: it creates its resources, blocks on a go-file,
then runs a final command inside the capture. `capture-late.sh` starts
`gpucapture` during the block, so the resources pre-exist the capture
boundary (a resource created and destroyed inside one capture is not
snapshotted for fetch). `capture.sh` is the single-phase variant, kept for
`known-textures`' non-late mode.

Textures that no captured command reads answer a fetch only under
`--force-load-unused` (`MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1`).

| app | what it makes | ground truth | needs force-load |
| --- | --- | --- | --- |
| `known-textures.m` | 7 BGRA8Unorm textures, one distinct width each (64, 80, ...) | blit source (w=64) fully cyan `00 ff ff ff` BGRA; blit destination (w=80) cyan in its 64x64 region | yes |
| `known-depth.m` | full-screen triangle at depth 0.5 into Depth32Float, blit-stored | the stored texture reads 0.5 everywhere; the other endpoint is uninitialised | no |
| `known-ds-pair.m` | two combined Depth32Float_Stencil8 resources, 64x64 and 96x96, blit-stored | 64x64: depth 0.25, stencil 11; 96x96: depth 0.75, stencil 22; both aspects fetch from one streamRef (plane 0 depth, plane 1 stencil) | no |
| `known-stencil.m` | base Stencil8 (42) and a combined DS with an X32_Stencil8 view | the base Stencil8 reads 42; combined aspects fetch separately | no |
| `known-astc.m` | 64x64 ASTC_4x4_LDR filled with one 16-byte block pattern | raw blocks `00..0f` repeated 256 times, 4096 bytes | no |
| `known-ycbcr.m` (needs CoreVideo) | 64x64 biplanar 4:2:0 CVPixelBuffer wrapped as two textures | luma R8Unorm 64x64 all 128; chroma RG8Unorm 32x32 all (100, 150) | no |
| `known-ambiguous.m` | three 64x64 BGRA8Unorm textures, same geometry | red = 1 mip, green = 3 mips, blue = 7 mips (pixel colour pins the descriptor's mip count) | yes |
| `known-3d.m` | 16x16x4 BGRA8Unorm volume, z-slices distinct | the fetch serves one z-plane and reports depth 1; the descriptor says Type3D depth 4 | yes |
| `known-mips.m` | 2-slice 2D array, 7-level chain, red/green | slice 0 red, slice 1 green; out-of-range level/slice CLAMPS (for the mip/slice follow-up) | yes |

Latency and hygiene for anything that then reads these captures through
the replayer: see the top-level README.
```

- [ ] **Step 4: Write `captures/README.md`**

```markdown
# Captures

Not committed: each is tens of megabytes and reproducible. Regenerate all
of them with `fixtures/build-all.sh`; see `fixtures/README.md` for what
each contains. The oracle tests (`tools/oracle.sh`) look for
`captures/<name>.gputrace` and skip, naming the script, when one is
missing.

`sample.gputrace` and `retroarch-trace.gputrace` are third-party traces no
test needs. tool-2's regression figures on them: 4 files from sample; 182
records on retroarch, 10 of them RGBA32Float with min -1 and max 46250.
```

- [ ] **Step 5: Build and capture everything**

Run: `fixtures/build-all.sh`
Expected: nine `captures/*.gputrace` directories. Each capture takes seconds. If `gpucapture start` reports "invalid PID", re-run: the scripts wait for the app to become capturable, but the machine may be slow.

- [ ] **Step 6: Commit** (captures are gitignored)

```bash
git add fixtures captures/README.md
git commit -m "feat: this repo's own fixture apps and capture tooling

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 11: Oracle harness, hygiene wrapper, and the first two oracle tests

**Files:**
- Create: `tools/oracle.sh`, `tests/common/mod.rs`, `tests/oracle_textures.rs`, `tests/oracle_coverage_gap.rs`

**Interfaces:**
- Produces (in `tests/common/mod.rs`): `capture(name) -> Option<PathBuf>`, `Run { status: i32, stderr: String, out: PathBuf, manifest: serde_json::Value }`, `run_cli(bundle: &Path, tag: &str, extra: &[&str]) -> Run`, `validate_all(out: &Path) -> Vec<PathBuf>`, `entries(&Run) -> Vec<serde_json::Value>`, `file_bytes(&Run, entry: &serde_json::Value) -> Vec<u8>`, `level0_of(&Run, entry) -> Vec<u8>`, `kv_of(&Run, entry) -> Vec<(String, String)>`, `f32s(&[u8]) -> Vec<f32>`, `bgra(&[u8]) -> Vec<[u8; 4]>`.

The oracle suite is feature-gated (`oracle`), one test binary per capture. Each runs the real CLI end to end, passes every written file through `ktx validate` (required on `PATH` here), reads the payload back with the library's KTX2 reader, and checks the fixture's ground truth.

- [ ] **Step 1: Write `tools/oracle.sh`**

```bash
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
```

Then `chmod +x tools/oracle.sh`.

- [ ] **Step 2: Write `tests/common/mod.rs`**

```rust
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use std::path::{Path, PathBuf};
use std::process::Command;

/// The capture, or `None` (after printing why) so the test can return early.
pub fn capture(name: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("captures").join(format!("{name}.gputrace"));
    if p.is_dir() {
        Some(p)
    } else {
        eprintln!("SKIP: {} is missing; run fixtures/build-all.sh", p.display());
        None
    }
}

pub struct Run {
    pub status: i32,
    pub stderr: String,
    pub out: PathBuf,
    pub manifest: serde_json::Value,
}

pub fn run_cli(bundle: &Path, tag: &str, extra: &[&str]) -> Run {
    let out = std::env::temp_dir().join(format!("ktx2-fetch-oracle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let o = Command::new(env!("CARGO_BIN_EXE_ktx2-fetch"))
        .arg(bundle)
        .arg("--out")
        .arg(&out)
        .args(extra)
        .output()
        .expect("spawn ktx2-fetch");
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    eprintln!("--- ktx2-fetch stderr ---\n{stderr}--- end ---");
    let manifest = std::fs::read(out.join("manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null);
    Run { status: o.status.code().unwrap_or(-1), stderr, out, manifest }
}

/// Every `.ktx2` in `out` must pass `ktx validate`. Returns their paths.
pub fn validate_all(out: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ktx2"))
        .collect();
    files.sort();
    for f in &files {
        let o = Command::new("ktx").arg("validate").arg(f).output().expect("ktx on PATH");
        assert!(o.status.success(), "ktx validate {}:\n{}{}", f.display(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    }
    files
}

pub fn entries(r: &Run) -> Vec<serde_json::Value> {
    r.manifest["textures"].as_array().cloned().unwrap_or_default()
}

pub fn file_bytes(r: &Run, entry: &serde_json::Value) -> Vec<u8> {
    std::fs::read(r.out.join(entry["file"].as_str().unwrap())).unwrap()
}

pub fn level0_of(r: &Run, entry: &serde_json::Value) -> Vec<u8> {
    ktx2_fetch::ktx::level0(&file_bytes(r, entry)).unwrap().to_vec()
}

pub fn kv_of(r: &Run, entry: &serde_json::Value) -> Vec<(String, String)> {
    ktx2_fetch::ktx::kv_pairs(&file_bytes(r, entry))
}

pub fn f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

pub fn bgra(b: &[u8]) -> Vec<[u8; 4]> {
    b.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

/// Entries whose `mtl_pixel_format` matches.
pub fn with_format<'a>(entries: &'a [serde_json::Value], name: &str) -> Vec<&'a serde_json::Value> {
    entries.iter().filter(|e| e["mtl_pixel_format"] == name).collect()
}
```

- [ ] **Step 3: Write `tests/oracle_textures.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

const CYAN: [u8; 4] = [0x00, 0xff, 0xff, 0xff];

#[test]
fn known_textures_late_writes_seven_attributed_bgra_files() {
    let Some(cap) = capture("known-textures-late") else { return };
    let r = run_cli(&cap, "textures", &["--force-load-unused", "--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    let files = validate_all(&r.out);
    assert_eq!(files.len(), 7, "{:?}", files);

    let m = &r.manifest;
    assert_eq!(m["bundle_manifest"]["status"], "ok");
    assert_eq!(m["bundle_manifest"]["textures_listed"], 7);
    assert_eq!(m["coverage"]["answered"], 7);
    assert_eq!(m["coverage"]["attributed"], 7);
    assert_eq!(m["coverage"]["listed_not_answered"], 0);
    assert!(m["failures"].as_array().unwrap().is_empty());

    let es = entries(&r);
    let by_width = |w: u64| es.iter().find(|e| e["width"] == w).unwrap_or_else(|| panic!("no width {w} entry"));
    // Blit source: fully cyan.
    let src = by_width(64);
    assert_eq!(src["mtl_pixel_format"], "BGRA8Unorm");
    assert_eq!(src["descriptor"]["attribution"], "certain");
    let px = bgra(&level0_of(&r, src));
    assert_eq!(px.len(), 64 * 64);
    assert!(px.iter().all(|p| *p == CYAN), "blit source is not all cyan");
    // Blit destination: cyan in its top-left 64x64, undefined elsewhere.
    let dst = by_width(80);
    let px = bgra(&level0_of(&r, dst));
    let h = dst["height"].as_u64().unwrap() as usize;
    assert_eq!(px.len(), 80 * h);
    for y in 0..64.min(h) {
        for x in 0..64 {
            assert_eq!(px[y * 80 + x], CYAN, "dst pixel ({x},{y})");
        }
    }
    let kv = kv_of(&r, src);
    assert!(kv.iter().any(|(k, v)| k == "gputrace.mipLevelCount" && v == "1"));
    assert!(kv.iter().any(|(k, v)| k == "gputrace.textureType" && v == "2D"));
}
```

- [ ] **Step 4: Write `tests/oracle_coverage_gap.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Without --force-load-unused, none of known-textures' unread textures
/// answers (MEASURED by the sibling campaign). The tool must report that as
/// coverage, not as a failure.
#[test]
fn unread_textures_are_listed_but_not_answered() {
    let Some(cap) = capture("known-textures-late") else { return };
    let r = run_cli(&cap, "gap", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    let m = &r.manifest;
    assert_eq!(m["bundle_manifest"]["textures_listed"], 7);
    assert_eq!(m["coverage"]["answered"], 0);
    assert_eq!(m["coverage"]["listed_not_answered"], 7);
    assert!(entries(&r).is_empty());
    assert!(r.stderr.contains("--force-load-unused"), "{}", r.stderr);
    assert_eq!(m["sweep_error"], serde_json::Value::Null, "an empty sweep is not an error: {}", m["sweep_error"]);
}
```

If this test fails because `sweep_error` reads `fetch reply had no data`, the substrate returns `FetchError::NoData` for a reply with zero records. That is a finding for the hl implementer (the tool follows spec 8 and treats it as run-level); report it and do not paper over it here.

- [ ] **Step 5: Run the two oracle tests**

Run: `pgrep -x GPUToolsReplayService; tools/oracle.sh --test oracle_textures --test oracle_coverage_gap`
Expected: `pgrep` prints nothing first; both tests PASS; the script reports no lingering service. Each CLI run takes roughly 30 seconds on these small captures.

- [ ] **Step 6: Commit**

```bash
git add tools/oracle.sh tests/common tests/oracle_textures.rs tests/oracle_coverage_gap.rs
git commit -m "test: oracle harness with ktx validate, textures and coverage-gap runs

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Oracle tests for depth, combined depth-stencil, and stencil

**Files:**
- Create: `tests/oracle_depth.rs`, `tests/oracle_ds_pair.rs`, `tests/oracle_stencil.rs`

- [ ] **Step 1: Write `tests/oracle_depth.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn depth32float_reads_half_everywhere() {
    let Some(cap) = capture("known-depth") else { return };
    let r = run_cli(&cap, "depth", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let depths = with_format(&es, "Depth32Float");
    assert!(!depths.is_empty(), "no Depth32Float entry: {es:?}");
    // Only the blit-stored endpoint carries real data; the other reads
    // uninitialised bytes. Select by content.
    let good = depths.iter().find(|e| f32s(&level0_of(&r, e)).iter().all(|&v| v == 0.5));
    let e = good.expect("no Depth32Float file reading 0.5 everywhere");
    assert_eq!(e["aspect"], "depth");
    assert_eq!(e["vk_format"], "D32_SFLOAT");
    assert!(e["file"].as_str().unwrap().ends_with("_Depth32Float.ktx2"));
    assert_eq!(ktx2_fetch::ktx::parse_header(&file_bytes(&r, e)).unwrap().vk_format, 126);
    // Plain depth refs are probed and answer no stencil.
    let probes = r.manifest["stencil_probes"].as_array().unwrap();
    assert!(!probes.is_empty());
    assert!(probes.iter().all(|p| p["outcome"] == "absent"), "{probes:?}");
}
```

- [ ] **Step 2: Write `tests/oracle_ds_pair.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Two combined Depth32Float_Stencil8 resources: 64x64 (depth 0.25,
/// stencil 11) and 96x96 (depth 0.75, stencil 22). Each fetches as a depth
/// aspect on plane 0 and a stencil aspect on plane 1 of the same streamRef
/// (MEASURED, dossier 00 on this fixture).
#[test]
fn each_combined_resource_yields_a_depth_file_and_a_stencil_sibling() {
    let Some(cap) = capture("known-ds-pair") else { return };
    let r = run_cli(&cap, "dspair", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let probes = r.manifest["stencil_probes"].as_array().unwrap().clone();

    for (w, depth, stencil) in [(64u64, 0.25f32, 11u8), (96, 0.75, 22)] {
        // A depth aspect of this size whose content is the stored value.
        let d = es
            .iter()
            .filter(|e| e["aspect"] == "depth" && e["width"] == w)
            .find(|e| f32s(&level0_of(&r, e)).iter().all(|&v| v == depth))
            .unwrap_or_else(|| panic!("no {w}x{w} depth file reading {depth}"));
        let stream_ref = d["stream_ref"].as_u64().unwrap();
        assert_eq!(d["descriptor"], serde_json::Value::Null, "combined aspects carry no descriptor");
        // Its stencil sibling: same streamRef, the probe wrote it.
        let s = es
            .iter()
            .find(|e| e["aspect"] == "stencil" && e["stream_ref"] == stream_ref)
            .unwrap_or_else(|| panic!("no stencil sibling for ref {stream_ref}"));
        assert!(s["file"].as_str().unwrap().ends_with("_stencil.ktx2"));
        assert_eq!(s["vk_format"], "S8_UINT");
        assert_eq!(s["descriptor"], serde_json::Value::Null);
        let px = level0_of(&r, s);
        assert_eq!(px.len(), (w * w) as usize, "stencil aspect is 1 byte per pixel");
        assert!(px.iter().all(|&v| v == stencil), "stencil for ref {stream_ref} is not all {stencil}");
        assert!(probes.iter().any(|p| p["stream_ref"] == stream_ref && p["outcome"] == "written"), "{probes:?}");
        let kv = kv_of(&r, s);
        assert!(kv.iter().any(|(k, v)| k == "gputrace.aspect" && v == "stencil"));
    }
}
```

- [ ] **Step 3: Write `tests/oracle_stencil.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn base_stencil8_reads_42_and_is_a_base_file_not_a_probe() {
    let Some(cap) = capture("known-stencil") else { return };
    let r = run_cli(&cap, "stencil", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let base = with_format(&es, "Stencil8");
    assert!(!base.is_empty(), "no Stencil8 entry: {es:?}");
    let e = base
        .iter()
        .find(|e| level0_of(&r, e).iter().all(|&v| v == 42))
        .expect("no Stencil8 file reading 42 everywhere");
    assert_eq!(e["aspect"], "stencil");
    assert_eq!(e["vk_format"], "S8_UINT");
    assert!(!e["file"].as_str().unwrap().ends_with("_stencil.ktx2"), "a base stencil texture is not a probed aspect");
    assert_eq!(e["descriptor"]["attribution"], "certain");
    // The manifest lists 5 textures (hl's own live test pins this).
    assert_eq!(r.manifest["bundle_manifest"]["textures_listed"], 5);
    let probes = r.manifest["stencil_probes"].as_array().unwrap();
    assert!(probes.iter().all(|p| p["outcome"] == "written" || p["outcome"] == "absent"));
}
```

- [ ] **Step 4: Run**

Run: `tools/oracle.sh --test oracle_depth --test oracle_ds_pair --test oracle_stencil`
Expected: 3 tests PASS. If `oracle_ds_pair` finds only one aspect per size, or the values are swapped between the 64 and 96 resources, that contradicts dossier 00's measurement on this fixture: report it rather than loosening the assertion.

- [ ] **Step 5: Commit**

```bash
git add tests/oracle_depth.rs tests/oracle_ds_pair.rs tests/oracle_stencil.rs
git commit -m "test: oracle runs for depth, combined depth-stencil, and stencil

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 13: Oracle tests for compressed, planar, volume, and attribution

**Files:**
- Create: `tests/oracle_astc.rs`, `tests/oracle_ycbcr.rs`, `tests/oracle_3d.rs`, `tests/oracle_ambiguous.rs`

- [ ] **Step 1: Write `tests/oracle_astc.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn astc_blocks_are_written_raw_and_byte_exact() {
    let Some(cap) = capture("known-astc") else { return };
    let r = run_cli(&cap, "astc", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let e = with_format(&es, "ASTC_4x4_LDR").into_iter().find(|e| e["width"] == 64).expect("no 64x64 ASTC_4x4_LDR entry");
    assert_eq!(e["vk_format"], "ASTC_4x4_UNORM_BLOCK");
    assert!(e["file"].as_str().unwrap().ends_with("_ASTC_4x4_LDR.ktx2"));
    let blocks = level0_of(&r, e);
    let pattern: Vec<u8> = (0..16u8).collect::<Vec<u8>>().repeat(256);
    assert_eq!(blocks.len(), 4096);
    assert_eq!(blocks, pattern, "block bytes differ from the fixture's 00..0f pattern");
    let h = ktx2_fetch::ktx::parse_header(&file_bytes(&r, e)).unwrap();
    assert_eq!(h.vk_format, 157);
    assert_eq!(h.type_size, 1);
}
```

- [ ] **Step 2: Write `tests/oracle_ycbcr.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn ycbcr_planes_are_two_ordinary_files() {
    let Some(cap) = capture("known-ycbcr") else { return };
    let r = run_cli(&cap, "ycbcr", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let luma = with_format(&es, "R8Unorm").into_iter().find(|e| e["width"] == 64 && e["height"] == 64).expect("no 64x64 R8Unorm luma entry");
    assert!(level0_of(&r, luma).iter().all(|&v| v == 128), "luma is not all 128");
    assert_eq!(luma["vk_format"], "R8_UNORM");
    let chroma = with_format(&es, "RG8Unorm").into_iter().find(|e| e["width"] == 32 && e["height"] == 32).expect("no 32x32 RG8Unorm chroma entry");
    let px = level0_of(&r, chroma);
    assert!(px.chunks_exact(2).all(|c| c == [100, 150]), "chroma is not all (100, 150)");
    assert_eq!(chroma["vk_format"], "R8G8_UNORM");
}
```

- [ ] **Step 3: Write `tests/oracle_3d.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

/// The fetch serves one z-plane of a volume and reports depth 1; only the
/// descriptor says Type3D depth 4. With a certain attribution the tool
/// refuses to ship one plane as the whole texture.
#[test]
fn a_volume_is_a_named_failure_not_a_partial_file() {
    let Some(cap) = capture("known-3d") else { return };
    let r = run_cli(&cap, "3d", &["--force-load-unused", "--max-stream-ref", "200"]);
    assert_eq!(r.status, 1, "a refused volume is a per-texture failure: {}", r.stderr);
    validate_all(&r.out);
    let failures = r.manifest["failures"].as_array().unwrap();
    let f = failures.iter().find(|f| f["reason"].as_str().unwrap().contains("3D texture, depth 4")).unwrap_or_else(|| panic!("no volume refusal in {failures:?}"));
    assert_eq!(f["aspect"], "color");
    let es = entries(&r);
    assert!(es.iter().all(|e| !(e["width"] == 16 && e["height"] == 16)), "the 16x16 volume plane must not be written: {es:?}");
}
```

- [ ] **Step 4: Write `tests/oracle_ambiguous.rs`**

```rust
#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Three same-geometry BGRA textures whose colour pins their mip count by
/// construction (red 1, green 3, blue 7). A shifted join would put the
/// wrong count on a file.
#[test]
fn same_geometry_textures_get_the_right_mip_count_and_grade_certain() {
    let Some(cap) = capture("known-ambiguous") else { return };
    let r = run_cli(&cap, "ambiguous", &["--force-load-unused", "--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let group: Vec<&serde_json::Value> = es.iter().filter(|e| e["mtl_pixel_format"] == "BGRA8Unorm" && e["width"] == 64 && e["height"] == 64).collect();
    assert_eq!(group.len(), 3, "{es:?}");
    for e in group {
        let first = bgra(&level0_of(&r, e))[0];
        let expected_mips = match first {
            [0, 0, 255, 255] => 1, // red in BGRA
            [0, 255, 0, 255] => 3, // green
            [255, 0, 0, 255] => 7, // blue
            other => panic!("unexpected colour {other:?}"),
        };
        assert_eq!(e["descriptor"]["mip_levels"], expected_mips, "ref {}", e["stream_ref"]);
        assert_eq!(e["descriptor"]["attribution"], "certain");
        let kv = kv_of(&r, e);
        assert!(kv.iter().any(|(k, v)| k == "gputrace.mipLevelCount" && v == &expected_mips.to_string()));
    }
    assert_eq!(r.manifest["coverage"]["attributed"], 3);
}
```

- [ ] **Step 5: Run**

Run: `tools/oracle.sh --test oracle_astc --test oracle_ycbcr --test oracle_3d --test oracle_ambiguous`
Expected: 4 tests PASS.

- [ ] **Step 6: Run the whole suite once, both ways**

Run: `cargo test && cargo clippy --all-targets --features oracle -- -D warnings && tools/oracle.sh`
Expected: the default suite passes with no hardware; clippy is clean under both feature sets; all nine oracle tests pass; `pgrep -x GPUToolsReplayService` prints nothing afterwards.

- [ ] **Step 7: Commit**

```bash
git add tests/oracle_astc.rs tests/oracle_ycbcr.rs tests/oracle_3d.rs tests/oracle_ambiguous.rs
git commit -m "test: oracle runs for ASTC, YCbCr, volumes, and attribution

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 14: README

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write `README.md`**

```markdown
# ktx2-fetch

Exports every texture of an Xcode `.gputrace` capture as a lossless KTX2
file, in its native pixel format, byte for byte, with the capture's own
metadata attached. It exists because `gpudebug fetch`, the built-in export,
only writes PNG, which drops alpha and destroys float range.

Version 0.2 replaces the private-framework engine of the original tool
with the `gputools-replay-hl` crate and widens the output to every format
that crate describes and Khronos' `ktx validate` accepts: byte-aligned
colour (8/16/32-bit, all numeric kinds, sRGB variants), single-aspect depth
and stencil, and BC, ETC2, EAC, and ASTC block formats written as raw
blocks. Design: `docs/superpowers/specs/2026-09-02-ktx2-fetch-hl-design.md`.

## Requirements

- macOS 27 with Xcode Command Line Tools (the engine links the private
  `GPUToolsReplay` framework they ship; no entitlement is needed).
- A checkout of `gputools-replay` beside this repo: the only dependency is
  `../gputools-replay/crates/gputools-replay-hl` by path until that crate
  is published.
- Khronos `ktx` on `PATH` for the oracle suite (installed via Nix here).
- `clang` and `gpucapture` to regenerate the fixture captures.

## Usage

```
cargo run --release -- <bundle>.gputrace --out <dir> [--max-stream-ref N] [--force-load-unused] [--timeout SECS]
```

Writes one `.ktx2` per fetched texture (level 0, slice 0; a combined
depth-stencil resource becomes a depth file and a `_stencil` sibling) plus
`<dir>/manifest.json`. Exit 0 when nothing failed, 1 when any texture or
the sweep failed (the manifest says which), 2 when the run could not start.

- `--max-stream-ref` (default 2000): streamRefs are assigned by the
  replayer at load time and are not stored in the bundle, so the tool
  sweeps a range and keeps what answers.
- `--force-load-unused`: textures no captured command reads answer only
  with this. The manifest's `coverage.listed_not_answered` tells you when
  you need it.
- `--timeout` (default 600 s): per fetch. Slow is not hung: fetches take
  from 27 seconds to over 20 minutes on large captures.

## What each file carries

The KTX2 header describes the image in the file: one 2D image, level 0,
slice 0, whatever the resource was. The Data Format Descriptor is derived
from the format (channel layout, numeric kind, sRGB transfer) and checked
byte for byte against `ktx create`'s own output; primaries stay
UNSPECIFIED because the capture records no colour space. Key/value data
under `gputrace.` records the streamRef, aspect, fetched stride, whether
padded rows were tightened, and, when the bundle's descriptor was
attributed, the resource's mip count, array length, depth, texture type,
usage, and whether that attribution is `certain` or `ambiguous`.

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
builds and captures all nine (see `fixtures/README.md` for what each
proves). `tools/capture-dfd-fixtures.py` regenerates the reference DFDs
when a format row is added.

**The replayer is a shared, crash-prone resource.** One session per
process, one process per machine. Check `pgrep -x GPUToolsReplayService`
prints nothing before and after a run. An interrupted fetch orphans a
session that locks the replayer for two hours; recover with:

```
gpudebug --terminate all
pkill -9 -f GPUToolsReplayService
```

Pre-commit hook (rustfmt check): `git config core.hooksPath .githooks`.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: README for ktx2-fetch 0.2

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
