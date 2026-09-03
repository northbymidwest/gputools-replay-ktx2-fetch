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
    let r = run_cli(&cap, "stencil", &["--max-stream-ref", "200"]);
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
    assert_eq!(e["descriptor"]["attribution"], "certain");
    // The manifest lists 5 textures (hl's own live test pins this).
    assert_eq!(r.manifest["bundle_manifest"]["textures_listed"], 5);
    let probes = r.manifest["stencil_probes"].as_array().unwrap();
    assert!(
        probes.iter().any(|p| p["outcome"] == "written"),
        "the combined resource's depth ref should probe a stencil aspect: {probes:?}"
    );
}
