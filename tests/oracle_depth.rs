#![cfg(feature = "oracle")]
mod common;
use common::*;

#[test]
fn depth32float_reads_half_everywhere() {
    let Some(cap) = capture("known-depth") else {
        return;
    };
    let r = run_cli(&cap, "depth", &["--max-stream-ref", "200"]);
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let depths = with_format(&es, "Depth32Float");
    assert!(!depths.is_empty(), "no Depth32Float entry: {es:?}");
    // Only the blit-stored endpoint carries real data; the other reads
    // uninitialised bytes. Select by content.
    let good = depths
        .iter()
        .find(|e| f32s(&level0_of(&r, e)).iter().all(|&v| v == 0.5));
    let e = good.expect("no Depth32Float file reading 0.5 everywhere");
    assert_eq!(e["aspect"], "depth");
    assert_eq!(e["vk_format"], "D32_SFLOAT");
    assert!(e["file"].as_str().unwrap().ends_with("_Depth32Float.ktx2"));
    assert_eq!(
        gputools_replay_ktx2_fetch::ktx::parse_header(&file_bytes(&r, e))
            .unwrap()
            .vk_format,
        126
    );
    // Plain depth refs are probed and answer no stencil.
    let probes = r.manifest["stencil_probes"].as_array().unwrap();
    assert!(!probes.is_empty());
    assert!(
        probes.iter().all(|p| p["outcome"] == "absent"),
        "{probes:?}"
    );
}
