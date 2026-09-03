#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn astc_blocks_are_written_raw_and_byte_exact() {
    let Some(cap) = capture("known-astc") else {
        return;
    };
    let r = run_cli(&cap, "astc", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let e = with_format(&es, "ASTC_4x4_LDR")
        .into_iter()
        .find(|e| e["width"] == 64)
        .expect("no 64x64 ASTC_4x4_LDR entry");
    assert_eq!(e["vk_format"], "ASTC_4x4_UNORM_BLOCK");
    assert!(e["file"].as_str().unwrap().ends_with("_ASTC_4x4_LDR.ktx2"));
    let blocks = level0_of(&r, e);
    let pattern: Vec<u8> = (0..16u8).collect::<Vec<u8>>().repeat(256);
    assert_eq!(blocks.len(), 4096);
    assert_eq!(
        blocks, pattern,
        "block bytes differ from the fixture's 00..0f pattern"
    );
    let h = gputools_replay_ktx2_fetch::ktx::parse_header(&file_bytes(&r, e)).unwrap();
    assert_eq!(h.vk_format, 157);
    assert_eq!(h.type_size, 1);
}
