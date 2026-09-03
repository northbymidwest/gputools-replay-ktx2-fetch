#![cfg(feature = "oracle")]
#![allow(clippy::chunks_exact_to_as_chunks)]
mod common;
use common::*;

#[test]
fn ycbcr_planes_are_two_ordinary_files() {
    let Some(cap) = capture("known-ycbcr") else {
        return;
    };
    let r = run_cli(&cap, "ycbcr", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let luma = with_format(&es, "R8Unorm")
        .into_iter()
        .find(|e| e["width"] == 64 && e["height"] == 64)
        .expect("no 64x64 R8Unorm luma entry");
    assert!(
        level0_of(&r, luma).iter().all(|&v| v == 128),
        "luma is not all 128"
    );
    assert_eq!(luma["vk_format"], "R8_UNORM");
    let chroma = with_format(&es, "RG8Unorm")
        .into_iter()
        .find(|e| e["width"] == 32 && e["height"] == 32)
        .expect("no 32x32 RG8Unorm chroma entry");
    let px = level0_of(&r, chroma);
    assert!(
        px.chunks_exact(2).all(|c| c == [100, 150]),
        "chroma is not all (100, 150)"
    );
    assert_eq!(chroma["vk_format"], "R8G8_UNORM");
}
