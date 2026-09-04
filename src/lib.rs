//! gputools-replay-ktx2-fetch: lossless KTX2 export of a `.gputrace` capture's textures,
//! on the `gputools-replay-hl` engine. See the spec in
//! `docs/superpowers/specs/2026-09-02-gputools-replay-ktx2-fetch-hl-design.md`.
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub mod dfd;
pub mod dfd_ref;
pub mod emit;
pub mod ktx;
pub mod manifest;
pub mod sweep;
pub mod tex;
pub mod vkformat;

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
    fn tool_version_is_0_1_2() {
        assert_eq!(TOOL_VERSION, "0.1.2");
    }
}
