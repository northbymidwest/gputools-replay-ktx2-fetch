# Changelog

Notable changes per release. Dates are the publish date.

## Unreleased

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
- The engine dependency is pinned exactly (`gputools-replay-hl = "=0.1.1"`),
  so an unlocked `cargo install` still builds against the tested engine.

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
