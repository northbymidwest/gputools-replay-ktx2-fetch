use std::{env, fs, path::Path};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let lock = Path::new(&manifest_dir).join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let engine = fs::read_to_string(&lock)
        .ok()
        .and_then(|text| engine_from_lock(&text))
        .unwrap_or_else(|| {
            "gputools-replay-hl (version unknown: not found in Cargo.lock)".to_string()
        });
    println!("cargo:rustc-env=KTX2_FETCH_ENGINE={engine}");
}

/// `gputools-replay-hl <version> (<source>)`: the resolved version of the
/// engine crate and its lock `source` (a registry or git URL once it is
/// published; `path` while it is a path dependency).
fn engine_from_lock(text: &str) -> Option<String> {
    let lock: toml::Table = text.parse().ok()?;
    let package = lock
        .get("package")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(toml::Value::as_str) == Some("gputools-replay-hl"))?;
    let version = package.get("version")?.as_str()?;
    let source = package
        .get("source")
        .and_then(toml::Value::as_str)
        .unwrap_or("path");
    Some(format!("gputools-replay-hl {version} ({source})"))
}
