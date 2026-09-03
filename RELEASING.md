# Releasing

The crate releases through `.github/workflows/release.yml`. Every release is a
`0.x` pre-release.

## Prerequisites (one-time)

- **A macOS 27 runner.** `cargo publish` runs a verify build that links the
  private framework through `gputools-replay-hl`, which no macOS runner below
  27 can do. No GitHub-hosted runner is that new yet; the `publish` job runs on
  `macos-latest` and is effective once that image reaches macOS 27 (pin it to
  a `macos-27` label if GitHub ships one first).
- **A crates.io trusted-publisher entry** for `gputools-replay-ktx2-fetch`:
  owner `northbymidwest`, repo `gputools-replay-ktx2-fetch`, workflow
  `release.yml`, environment `release`.
- **A `release` environment** in repo settings with a required reviewer, and
  restricted to `main`.

## By hand, before dispatching

1. Bump `version` in `Cargo.toml` to the new version.
2. Remove the `publish = false` line (it is the deliberate safety net that
   keeps the crate off crates.io until you mean it).
3. Retitle the `## Unreleased` section of `CHANGELOG.md` to
   `## <version> - <YYYY-MM-DD>`.
4. Commit, push, and wait for CI to go green.

## Dispatch

Actions -> release -> Run workflow. Enter the version without a leading `v`.
Leave `dry_run` ticked for a rehearsal; untick it to publish.

- `preflight` (no approval) validates the version format, that the manifest
  carries it and is no longer `publish = false`, that a non-empty
  `CHANGELOG.md` section exists, and that the newest CI run for the commit is
  green.
- `publish` pauses at the `release` environment's reviewer gate. On approval it
  exchanges an OIDC token for a short-lived crates.io token, publishes the
  crate, then pushes the `v<version>` tag and creates a `--prerelease` GitHub
  Release from the changelog section.

`dry_run` defaults to true on purpose: forgetting to untick costs a re-run;
forgetting to tick would be an irreversible publish.
