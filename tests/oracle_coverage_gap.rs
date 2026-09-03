#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Without --force-load-unused the replayer answers the three textures a
/// captured command uses and skips the four it does not (MEASURED here,
/// 2026-09-03); the tool reports that as coverage, not as a failure. The
/// ignore flag is set only when force-load is off because it overrides
/// force-load when both are set.
#[test]
fn unread_textures_are_listed_but_not_answered() {
    let Some(cap) = capture("known-textures-late") else {
        return;
    };
    let r = run_cli(&cap, "gap", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    let m = &r.manifest;
    assert_eq!(m["bundle_manifest"]["textures_listed"], 7);
    assert_eq!(m["coverage"]["answered"], 3);
    assert_eq!(m["coverage"]["listed_not_answered"], 4);
    assert_eq!(entries(&r).len(), 3);
    assert!(r.stderr.contains("--force-load-unused"), "{}", r.stderr);
    assert_eq!(
        m["sweep_error"],
        serde_json::Value::Null,
        "an empty sweep is not an error: {}",
        m["sweep_error"]
    );
}
