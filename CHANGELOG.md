# Changelog

Notable changes per release. Dates are the publish date.

## 0.2.0 - 2026-09-09

Built on gputools-replay-hl 0.2.0, whose texture descriptors now come off
the replayer's live object map, keyed by streamRef, instead of a parse of
the bundle joined by creation order.

### Added

- The manifest's `descriptor` carries `pixel_format` (the resource's own
  format, which for a combined depth-stencil resource names the combined
  format while each file holds one aspect) and `sample_count`; every file
  carries `gputrace.resourcePixelFormat` and `gputrace.sampleCount`.
- A combined depth-stencil resource's depth file and `_stencil` sibling
  both carry its descriptor. A texture view is exported as its own
  texture with its own descriptor.
- A ref the replayer refuses to fetch is a per-texture failure naming that
  ref: a failed batch is retried one ref at a time instead of losing the
  whole chunk.
- A warning when the highest loaded streamRef is within 64 of
  `--max-stream-ref`.

### Changed

- Textures are discovered by asking the replayer's object map about every
  streamRef up to the bound and fetching the ones it names, instead of
  fetching every ref and keeping what answers. A lookup costs about a
  quarter of a microsecond, so `--max-stream-ref` now defaults to
  1000000 and the bundle-derived bound is gone (`max_stream_ref_source`
  is `flag` or `default`).
- Every exported texture carries its own descriptor. The descriptor is
  the texture's own by construction, so the `attribution` field and the
  `gputrace.descriptorAttribution` key are gone, and a 3D volume with
  depth greater than 1 is always refused rather than only when its
  attribution was certain.
- `coverage` is `{loaded, answered, highest_stream_ref}`: textures the
  replayer loaded within the bound, distinct refs fetched, and the highest
  loaded ref. It is present whenever the object map was readable.
- The engine is pinned to `gputools-replay-hl = "=0.2.0"`.

### Removed

- `bundle_manifest` and the bundle-parse coverage fields (`attributed`,
  `unattributed`, `listed_not_answered`) from the manifest, and the
  offline bundle reader they came from. Without `--force-load-unused` an
  unused texture is simply absent from the replayer, not counted.

### Fixed

- Texture descriptors on captures the bundle parser could not read (an
  SDL3 capture reported zero descriptors while its textures fetched fine)
  and on captures where the creation-order join could have shifted a
  descriptor onto a same-geometry neighbour.

- A capture the replayer refuses to load (for example a wgpu capture whose
  unused compute pipeline cannot be rebuilt under `--force-load-unused`)
  now writes a manifest carrying `open_error` and exits 1, and the message
  names `--force-load-unused` as the likely cause when it was on. Previously
  the run exited 2 with nothing written.

### Changed

- Textures are fetched after replaying the captured command stream, so each
  file holds the texture as the frame left it, which is what gpudebug shows.
  Previously fetches happened at command 0, where render targets and
  drawables still held their pre-frame contents (a wgpu capture's rendered
  drawable exported as solid black; its compute output differed from
  gpudebug's in 0.38% of pixels). New flag `--fetch-at end|start|<index>`
  selects the playback position; the manifest records `fetch_at` and
  `replayed_to_command_index`, and every file carries
  `gputrace.commandIndex`.
- The engine dependency is pinned exactly, so an unlocked `cargo install`
  still builds against the tested engine.

## 0.1.2 - 2026-09-04

### Fixed

- docs.rs builds: target `aarch64-apple-darwin`, as the engine crates do,
  since docs.rs builds on Linux and the engine's build script refuses any
  other target.

## 0.1.1 - 2026-09-04

### Changed

- The sweep bound defaults to the bundle's index record count plus a margin
  (`Capture::record_count()`, gputools-replay-hl 0.1.1), or 20000 when the
  bundle cannot be read; `--max-stream-ref` is now an override. The manifest
  records the bound's source as `max_stream_ref_source`.
- Pass 1 fetches in chunks of 2000 refs; a chunk that fails is recorded and
  the rest still count, and coverage is withheld when any chunk failed.

## 0.1.0 - 2026-09-03

### Added

- Initial release: lossless KTX2 export of every texture in a `.gputrace`
  capture, on the `gputools-replay-hl` engine, with per-file provenance, a
  run manifest with coverage and attribution, this repo's own fixture apps
  and capture tooling, and an oracle suite checked with `ktx validate`.
