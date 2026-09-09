#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Three same-geometry BGRA textures whose colour pins their mip count by
/// construction (red 1, green 3, blue 7). Descriptors are keyed by
/// streamRef, so nothing can shift one texture's count onto another.
#[test]
fn same_geometry_textures_get_their_own_mip_count() {
    let Some(cap) = capture("known-ambiguous") else {
        return;
    };
    // This fixture's content was stored before the capture boundary and is not
    // reproduced by replaying its commands, so it defines the snapshot state
    // only (MEASURED: after play_all it reads NaN/zero/altered). Fetch at start.
    let r = run_cli(
        &cap,
        "ambiguous",
        &["--fetch-at", "start", "--force-load-unused"],
    );
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let group: Vec<&serde_json::Value> = es
        .iter()
        .filter(|e| e["mtl_pixel_format"] == "BGRA8Unorm" && e["width"] == 64 && e["height"] == 64)
        .collect();
    assert_eq!(group.len(), 3, "{es:?}");
    for e in group {
        let first = bgra(&level0_of(&r, e))[0];
        let expected_mips = match first {
            [0, 0, 255, 255] => 1, // red in BGRA
            [0, 255, 0, 255] => 3, // green
            [255, 0, 0, 255] => 7, // blue
            other => panic!("unexpected colour {other:?}"),
        };
        assert_eq!(
            e["descriptor"]["mip_levels"], expected_mips,
            "ref {}",
            e["stream_ref"]
        );
        let kv = kv_of(&r, e);
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.mipLevelCount" && v == &expected_mips.to_string())
        );
    }
    assert_eq!(r.manifest["coverage"]["loaded"], 3);
    assert_eq!(r.manifest["coverage"]["answered"], 3);
}
