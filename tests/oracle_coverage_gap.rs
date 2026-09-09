#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Without --force-load-unused the replayer creates only the three textures
/// a captured command uses; the four it does not are absent from its object
/// map (MEASURED here, 2026-09-09: 3 loaded without force-load, 7 with), so
/// the tool neither looks for nor fails on them. The ignore flag is set only
/// when force-load is off because it overrides force-load when both are set.
#[test]
fn unread_textures_are_absent_without_force_load() {
    let Some(cap) = capture("known-textures-late") else {
        return;
    };
    let r = run_cli(&cap, "gap", &[]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    let m = &r.manifest;
    assert_eq!(m["coverage"]["loaded"], 3);
    assert_eq!(m["coverage"]["answered"], 3);
    assert_eq!(
        m["coverage"]["unused_resources"],
        serde_json::Value::Null,
        "the replayer reports unused resources only under force-load"
    );
    assert_eq!(entries(&r).len(), 3);
    assert!(m["failures"].as_array().unwrap().is_empty());
    assert_eq!(
        m["sweep_error"],
        serde_json::Value::Null,
        "an empty sweep is not an error: {}",
        m["sweep_error"]
    );
}
