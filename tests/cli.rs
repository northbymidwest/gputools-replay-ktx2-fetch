use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gputools-replay-ktx2-fetch"))
}

#[test]
fn help_lists_every_flag() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--out",
        "--max-stream-ref",
        "--force-load-unused",
        "--timeout",
        "--fetch-at",
    ] {
        assert!(text.contains(flag), "missing {flag} in:\n{text}");
    }
}

#[test]
fn a_missing_bundle_exits_2_with_a_named_error_and_no_manifest() {
    let out_dir = std::env::temp_dir().join(format!(
        "gputools_replay_ktx2_fetch_cli_{}",
        std::process::id()
    ));
    let out = bin()
        .arg("/nonexistent/thing.gputrace")
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("gputools-replay-ktx2-fetch:"), "{err}");
    assert!(err.contains("nonexistent"), "{err}");
    assert!(!out_dir.join("manifest.json").exists());
}
