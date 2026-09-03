#![cfg(feature = "oracle")]
mod common;
use common::*;

/// The fetch serves one z-plane of a volume and reports depth 1; only the
/// descriptor says Type3D depth 4. With a certain attribution the tool
/// refuses to ship one plane as the whole texture.
#[test]
fn a_volume_is_a_named_failure_not_a_partial_file() {
    let Some(cap) = capture("known-3d") else {
        return;
    };
    let r = run_cli(
        &cap,
        "3d",
        &["--force-load-unused", "--max-stream-ref", "200"],
    );
    assert_eq!(
        r.status, 1,
        "a refused volume is a per-texture failure: {}",
        r.stderr
    );
    validate_all(&r.out);
    let failures = r.manifest["failures"].as_array().unwrap();
    let f = failures
        .iter()
        .find(|f| {
            f["reason"]
                .as_str()
                .unwrap()
                .contains("3D texture, depth 4")
        })
        .unwrap_or_else(|| panic!("no volume refusal in {failures:?}"));
    assert_eq!(f["aspect"], "color");
    let es = entries(&r);
    assert!(
        es.iter().all(|e| !(e["width"] == 16 && e["height"] == 16)),
        "the 16x16 volume plane must not be written: {es:?}"
    );
}
