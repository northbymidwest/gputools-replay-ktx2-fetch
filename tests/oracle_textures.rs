#![cfg(feature = "oracle")]
mod common;
use common::*;

const CYAN: [u8; 4] = [0x00, 0xff, 0xff, 0xff];

#[test]
fn known_textures_late_writes_seven_attributed_bgra_files() {
    let Some(cap) = capture("known-textures-late") else {
        return;
    };
    let r = run_cli(
        &cap,
        "textures",
        &["--force-load-unused", "--max-stream-ref", "200"],
    );
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
    let by_width = |w: u64| {
        es.iter()
            .find(|e| e["width"] == w)
            .unwrap_or_else(|| panic!("no width {w} entry"))
    };
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
    assert!(
        kv.iter()
            .any(|(k, v)| k == "gputrace.mipLevelCount" && v == "1")
    );
    assert!(
        kv.iter()
            .any(|(k, v)| k == "gputrace.textureType" && v == "2D")
    );
}
