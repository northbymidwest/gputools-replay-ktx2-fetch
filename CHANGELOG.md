# Changelog

Notable changes per release. Dates are the publish date.

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
