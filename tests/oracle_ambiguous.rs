#![cfg(feature = "oracle")]
mod common;
use common::*;

/// Three same-geometry BGRA textures whose colour pins their mip count by
/// construction (red 1, green 3, blue 7). A shifted join would put the
/// wrong count on a file.
#[test]
fn same_geometry_textures_get_the_right_mip_count_and_grade_certain() {
    let Some(cap) = capture("known-ambiguous") else {
        return;
    };
    let r = run_cli(
        &cap,
        "ambiguous",
        &["--force-load-unused", "--max-stream-ref", "200"],
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
        assert_eq!(e["descriptor"]["attribution"], "certain");
        let kv = kv_of(&r, e);
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.mipLevelCount" && v == &expected_mips.to_string())
        );
    }
    assert_eq!(r.manifest["coverage"]["attributed"], 3);
}
