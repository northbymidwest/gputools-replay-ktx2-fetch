#![cfg(feature = "oracle")]
mod common;
use common::*;

// known-stencil has no plain depth texture; its combined
// Depth32Float_Stencil8 resource surfaces as a depth aspect in pass 1 and
// its stencil aspect answers the pass-2 probe.
#[test]
fn base_stencil8_reads_42_and_is_a_base_file_not_a_probe() {
    let Some(cap) = capture("known-stencil") else {
        return;
    };
    // This fixture's content was stored before the capture boundary and is not
    // reproduced by replaying its commands, so it defines the snapshot state
    // only (MEASURED: after play_all it reads NaN/zero/altered). Fetch at start.
    let r = run_cli(
        &cap,
        "stencil",
        &["--fetch-at", "start", "--max-stream-ref", "200"],
    );
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let base = with_format(&es, "Stencil8");
    assert!(!base.is_empty(), "no Stencil8 entry: {es:?}");
    let e = base
        .iter()
        .find(|e| level0_of(&r, e).iter().all(|&v| v == 42))
        .expect("no Stencil8 file reading 42 everywhere");
    assert_eq!(e["aspect"], "stencil");
    assert_eq!(e["vk_format"], "S8_UINT");
    assert!(
        !e["file"].as_str().unwrap().ends_with("_stencil.ktx2"),
        "a base stencil texture is not a probed aspect"
    );
    assert_eq!(e["descriptor"]["pixel_format"], "Stencil8");
    // The replayer's object map holds 6 textures: the fixture's five
    // `newTextureWithDescriptor` resources plus its X32_Stencil8 view of the
    // combined resource, which is a texture with its own streamRef (MEASURED
    // 2026-09-09; the offline bundle manifest listed only the five).
    assert_eq!(r.manifest["coverage"]["loaded"], 6);
    assert!(
        es.iter().any(|e| e["mtl_pixel_format"] == "X32_Stencil8"
            && e["descriptor"]["pixel_format"] == "X32_Stencil8"
            && !e["file"].as_str().unwrap().ends_with("_stencil.ktx2")),
        "the stencil view is exported as its own texture: {es:?}"
    );
    let probes = r.manifest["stencil_probes"].as_array().unwrap();
    assert!(
        probes.iter().any(|p| p["outcome"] == "written"),
        "the combined resource's depth ref should probe a stencil aspect: {probes:?}"
    );
}
