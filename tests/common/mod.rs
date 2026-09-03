#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::chunks_exact_to_as_chunks
)]
use std::path::{Path, PathBuf};
use std::process::Command;

/// The capture, or `None` (after printing why) so the test can return early.
pub fn capture(name: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("captures")
        .join(format!("{name}.gputrace"));
    if p.is_dir() {
        Some(p)
    } else {
        eprintln!(
            "SKIP: {} is missing; run fixtures/build-all.sh",
            p.display()
        );
        None
    }
}

pub struct Run {
    pub status: i32,
    pub stderr: String,
    pub out: PathBuf,
    pub manifest: serde_json::Value,
}

pub fn run_cli(bundle: &Path, tag: &str, extra: &[&str]) -> Run {
    let out = std::env::temp_dir().join(format!(
        "gputools-replay-ktx2-fetch-oracle-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out);
    let o = Command::new(env!("CARGO_BIN_EXE_gputools-replay-ktx2-fetch"))
        .arg(bundle)
        .arg("--out")
        .arg(&out)
        .args(extra)
        .output()
        .expect("spawn gputools-replay-ktx2-fetch");
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    eprintln!("--- gputools-replay-ktx2-fetch stderr ---\n{stderr}--- end ---");
    let manifest = std::fs::read(out.join("manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null);
    Run {
        status: o.status.code().unwrap_or(-1),
        stderr,
        out,
        manifest,
    }
}

/// Every `.ktx2` in `out` must pass `ktx validate`. Returns their paths.
pub fn validate_all(out: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ktx2"))
        .collect();
    files.sort();
    for f in &files {
        let o = Command::new("ktx")
            .arg("validate")
            .arg(f)
            .output()
            .expect("ktx on PATH");
        assert!(
            o.status.success(),
            "ktx validate {}:\n{}{}",
            f.display(),
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
    files
}

pub fn entries(r: &Run) -> Vec<serde_json::Value> {
    r.manifest["textures"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

pub fn file_bytes(r: &Run, entry: &serde_json::Value) -> Vec<u8> {
    std::fs::read(r.out.join(entry["file"].as_str().unwrap())).unwrap()
}

pub fn level0_of(r: &Run, entry: &serde_json::Value) -> Vec<u8> {
    gputools_replay_ktx2_fetch::ktx::level0(&file_bytes(r, entry))
        .unwrap()
        .to_vec()
}

pub fn kv_of(r: &Run, entry: &serde_json::Value) -> Vec<(String, String)> {
    gputools_replay_ktx2_fetch::ktx::kv_pairs(&file_bytes(r, entry))
}

pub fn f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn bgra(b: &[u8]) -> Vec<[u8; 4]> {
    b.chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
}

/// Entries whose `mtl_pixel_format` matches.
pub fn with_format<'a>(entries: &'a [serde_json::Value], name: &str) -> Vec<&'a serde_json::Value> {
    entries
        .iter()
        .filter(|e| e["mtl_pixel_format"] == name)
        .collect()
}
