#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use clap::Parser;
use gputools_replay_hl::{
    Aspect as FetchAspect, Capture, Descriptions, Error, ManifestStatus, ReplayerConfig, Texture,
};
use gputools_replay_ktx2_fetch::emit::{Context, emit_one};
use gputools_replay_ktx2_fetch::manifest::Manifest;
use gputools_replay_ktx2_fetch::sweep::{self, Fetcher};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "gputools-replay-ktx2-fetch",
    version,
    about = "Export every texture of a .gputrace capture as lossless KTX2"
)]
struct Args {
    /// The .gputrace bundle.
    bundle: PathBuf,
    /// Directory to write .ktx2 files and manifest.json into.
    #[arg(long)]
    out: PathBuf,
    /// Highest streamRef to sweep. Refs are sparse and assigned at load
    /// time, so the tool asks for every value up to this and keeps what
    /// answers. Default: the bundle's index record count plus a margin,
    /// which bounds the refs the replayer can assign; 20000 if the bundle
    /// cannot be read.
    #[arg(long)]
    max_stream_ref: Option<u64>,
    /// Set MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1 so textures no captured
    /// command reads still answer.
    #[arg(long)]
    force_load_unused: bool,
    /// Per-fetch timeout in seconds. A large capture can take minutes.
    #[arg(long, default_value_t = 600)]
    timeout: u64,
}

fn main() -> ExitCode {
    // FIRST, before any thread exists: both env writes are sound only while
    // the process is single-threaded. The substrate verifies the unlock
    // variable in Capture::open and refuses with a named error otherwise.
    // With force-load on, every resource is created and nothing needs
    // tolerating; with it off, tolerating unused-resource creation failures
    // is what lets the used textures answer instead of the whole batch
    // failing (MEASURED on known-textures-late: 3 of 7 answer; the ignore
    // flag also overrides force-load if both are set, so they are never set
    // together).
    let force = std::env::args_os().any(|a| a == "--force-load-unused");
    #[allow(unsafe_code)]
    // SAFETY: no threads have been spawned yet; these are the first
    // statements of main. Still single-threaded; the two removals clear
    // whatever the ambient environment may already hold so it cannot
    // contradict what the manifest and KV data record.
    unsafe {
        std::env::remove_var("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE");
        std::env::remove_var("MTLREPLAYER_IGNORE_UNUSED_RESOURCE");
        std::env::set_var("MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX", "0");
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: force,
            ignore_unused_resources: !force,
            ..ReplayerConfig::default()
        });
    }
    let args = Args::parse();
    match run(args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("gputools-replay-ktx2-fetch: {e}");
            ExitCode::from(2)
        }
    }
}

struct Live(Capture);

impl Fetcher for Live {
    type Tex = Texture;
    fn manifest_status(&self) -> ManifestStatus {
        self.0.manifest_status()
    }
    fn record_count(&self) -> Option<usize> {
        self.0.record_count()
    }
    fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Texture>, Error> {
        self.0.textures(refs)
    }
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Texture>, Error> {
        self.0
            .texture_aspects(refs.iter().copied(), FetchAspect::Stencil)
    }
    fn describe(&self, texs: &[Texture]) -> Descriptions {
        self.0.describe(texs)
    }
}

fn run(args: Args) -> Result<u8, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&args.out)
        .map_err(|e| format!("creating {}: {e}", args.out.display()))?;
    let bundle = args.bundle.display().to_string();
    let mut cap = Capture::open(&args.bundle)?;
    cap.set_timeout(Duration::from_secs(args.timeout));
    let live = Live(cap);

    let bound = sweep::bound(&live, args.max_stream_ref);
    let mut man = Manifest::new(
        bundle.clone(),
        bound.max_stream_ref,
        args.force_load_unused,
        args.timeout,
    );
    man.max_stream_ref_source = bound.source;
    let sweep = sweep::run(&live, &bound);
    man.bundle_manifest = sweep.bundle_manifest;
    man.coverage = sweep.coverage;
    man.duplicates = sweep.duplicates;
    man.stencil_probes = sweep.probes;
    man.failures = sweep.failures;
    man.sweep_error = sweep.sweep_error;

    let ctx = Context {
        out: &args.out,
        bundle: &bundle,
        force_load_unused: args.force_load_unused,
    };
    for f in &sweep.fetched {
        emit_one(&ctx, f, &mut man);
    }

    if man.textures.is_empty() {
        eprintln!(
            "gputools-replay-ktx2-fetch: warning: no textures were written; check that {bundle} is the capture you meant, that --max-stream-ref ({}) is at least as high as the streamRefs it uses, and consider --force-load-unused",
            bound.max_stream_ref
        );
    }
    if let Some(c) = &man.coverage
        && c.listed_not_answered > 0
    {
        eprintln!(
            "gputools-replay-ktx2-fetch: {} of the bundle's listed textures did not answer the fetch; if they are never read by a captured command, --force-load-unused makes them answer",
            c.listed_not_answered
        );
    }

    let code = man.exit_code();
    if let Err(e) = man.write(&args.out.join("manifest.json")) {
        eprintln!("gputools-replay-ktx2-fetch: failed to write manifest.json: {e}");
        match serde_json::to_string_pretty(&man) {
            Ok(json) => eprintln!(
                "gputools-replay-ktx2-fetch: the manifest was not written to disk; printing it here so the run is not lost:\n{json}"
            ),
            Err(se) => eprintln!(
                "gputools-replay-ktx2-fetch: could not serialise the manifest either: {se}"
            ),
        }
        return Ok(code.max(1));
    }
    Ok(code)
}
