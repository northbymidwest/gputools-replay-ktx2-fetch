#![cfg(feature = "oracle")]
mod common;
use common::*;

/// One combined Depth32Float_Stencil8 resource rendered at depth 0.5 with
/// stencil 42 and blit-stored. The replayer serves it as a depth aspect on
/// plane 0 and a stencil aspect on plane 1 of the same streamRef (MEASURED,
/// hl live_hl_aspects; reproduced here 2026-09-03). Only the blit-stored
/// endpoint carries real data; the render target reads uninitialised bytes,
/// so the stored resource is selected by content.
#[test]
fn a_combined_resource_yields_a_depth_file_and_a_stencil_sibling() {
    let Some(cap) = capture("known-depth-stencil") else {
        return;
    };
    // This fixture's content was stored before the capture boundary and is not
    // reproduced by replaying its commands, so it defines the snapshot state
    // only (MEASURED: after play_all it reads NaN/zero/altered). Fetch at start.
    let r = run_cli(
        &cap,
        "depthstencil",
        &["--fetch-at", "start", "--max-stream-ref", "200"],
    );
    assert_eq!(r.status, 0, "{}", r.stderr);
    validate_all(&r.out);
    let es = entries(&r);
    let probes = r.manifest["stencil_probes"].as_array().unwrap().clone();

    let d = es
        .iter()
        .filter(|e| e["aspect"] == "depth" && e["mtl_pixel_format"] == "Depth32Float")
        .find(|e| {
            let px = f32s(&level0_of(&r, e));
            !px.is_empty() && px.iter().all(|&v| v == 0.5)
        })
        .expect("no Depth32Float file reading 0.5 everywhere");
    let stream_ref = d["stream_ref"].as_u64().unwrap();
    assert_eq!(
        d["descriptor"],
        serde_json::Value::Null,
        "combined aspects carry no descriptor"
    );
    assert_eq!(d["vk_format"], "D32_SFLOAT");

    let s = es
        .iter()
        .find(|e| e["aspect"] == "stencil" && e["stream_ref"] == stream_ref)
        .unwrap_or_else(|| panic!("no stencil sibling for ref {stream_ref}"));
    assert!(s["file"].as_str().unwrap().ends_with("_stencil.ktx2"));
    assert_eq!(s["vk_format"], "S8_UINT");
    assert_eq!(s["descriptor"], serde_json::Value::Null);
    let px = level0_of(&r, s);
    let (w, h) = (d["width"].as_u64().unwrap(), d["height"].as_u64().unwrap());
    assert_eq!(
        px.len(),
        (w * h) as usize,
        "stencil aspect is 1 byte per pixel"
    );
    assert!(
        px.iter().all(|&v| v == 42),
        "stencil for ref {stream_ref} is not all 42"
    );
    assert!(
        probes
            .iter()
            .any(|p| p["stream_ref"] == stream_ref && p["outcome"] == "written"),
        "{probes:?}"
    );
    let kv = kv_of(&r, s);
    assert!(
        kv.iter()
            .any(|(k, v)| k == "gputrace.aspect" && v == "stencil")
    );
    assert!(
        kv.iter()
            .any(|(k, v)| k == "gputrace.streamRef" && v == &stream_ref.to_string())
    );
}
