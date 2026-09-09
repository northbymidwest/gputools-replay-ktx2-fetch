//! `manifest.json` (spec 7.4) and the exit-code policy (spec 8).

use crate::tex::Aspect;
use serde::Serialize;
use std::path::Path;

/// What the replayer loaded, against what the fetch returned.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Coverage {
    /// Textures the replayer loaded (its object map at open).
    pub loaded: usize,
    /// Distinct pass-1 streamRefs after dedupe.
    pub answered: usize,
    /// Resources of any kind the replayer force-loaded that no captured
    /// command uses. The replayer tracks these only under
    /// `--force-load-unused` (MEASURED: 0 reported without it, 4 with, on a
    /// fixture with four unused textures), so this is `null` otherwise.
    pub unused_resources: Option<usize>,
}

/// Where in the captured command stream the fetch happens (spec 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchAt {
    /// After replaying the whole command stream: what the frame produced.
    End,
    /// Before any command runs: the capture's stored snapshot.
    Start,
    /// After replaying up to this command index.
    Index(u32),
}

impl std::str::FromStr for FetchAt {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "end" => Ok(FetchAt::End),
            "start" => Ok(FetchAt::Start),
            other => other
                .parse::<u32>()
                .map(FetchAt::Index)
                .map_err(|_| format!("expected `end`, `start`, or a command index, got `{other}`")),
        }
    }
}

impl std::fmt::Display for FetchAt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchAt::End => f.write_str("end"),
            FetchAt::Start => f.write_str("start"),
            FetchAt::Index(n) => write!(f, "{n}"),
        }
    }
}

impl Serialize for FetchAt {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// The resource as the replayer created it, read off its live `MTLTexture`
/// (spec 5 step 2). `pixel_format` is the resource's own format, which for
/// a combined depth-stencil texture differs from the aspect a file holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DescriptorEntry {
    pub pixel_format: String,
    pub texture_type: String,
    pub depth: u32,
    pub mip_levels: u32,
    pub array_length: u32,
    pub sample_count: u32,
    pub usage: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextureEntry {
    pub stream_ref: u64,
    pub aspect: Aspect,
    pub file: String,
    pub mtl_pixel_format: String,
    pub mtl_pixel_format_raw: u32,
    pub vk_format: String,
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub rows_repacked: bool,
    pub descriptor: Option<DescriptorEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Duplicate {
    pub stream_ref: u64,
    pub identical: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProbeOutcome {
    Written,
    Absent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StencilProbe {
    pub stream_ref: u64,
    pub outcome: ProbeOutcome,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub stream_ref: u64,
    pub aspect: Aspect,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub bundle: String,
    pub tool_version: String,
    pub engine: String,
    /// Where the fetch was asked to happen (`--fetch-at`).
    pub fetch_at: FetchAt,
    /// The command index playback actually reached before any fetch.
    pub replayed_to_command_index: u32,
    pub force_load_unused: bool,
    pub timeout_secs: u64,
    pub assumptions: Vec<String>,
    /// Omitted when the replayer's object map could not be read (see
    /// `sweep_error`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
    pub textures: Vec<TextureEntry>,
    pub duplicates: Vec<Duplicate>,
    pub stencil_probes: Vec<StencilProbe>,
    pub failures: Vec<Failure>,
    pub sweep_error: Option<String>,
    /// The replayer refused to load the capture, so no sweep ran. Present
    /// only on that path; everything above it still records the run's
    /// settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_error: Option<String>,
}

impl Manifest {
    pub fn new(bundle: String, force_load_unused: bool, timeout_secs: u64) -> Self {
        Self {
            bundle,
            tool_version: crate::TOOL_VERSION.to_string(),
            engine: crate::engine().to_string(),
            fetch_at: FetchAt::End,
            replayed_to_command_index: 0,
            force_load_unused,
            timeout_secs,
            assumptions: vec![
                "MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 was set; without it the replayer cannot create its command queue in an unentitled process".to_string(),
                format!("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE={}; textures no captured command reads answer only when it is 1", u8::from(force_load_unused)),
                format!("MTLREPLAYER_IGNORE_UNUSED_RESOURCE={}; when not force-loading, a texture the replayer cannot create because no captured command uses it is skipped instead of failing the whole fetch", u8::from(!force_load_unused)),
                "textures are fetched at the playback position fetch_at (default: the end of the captured command stream, what the frame produced; `start` is the capture's stored snapshot); replayed_to_command_index is the index playback reached".to_string(),
                "the textures fetched are exactly the ones the replayer's object map held when the capture opened (the resources the load created; an unused texture is absent without force-load)".to_string(),
                "each descriptor is read off the MTLTexture the replayer created for that streamRef; nothing is joined by position".to_string(),
                "alpha is assumed straight (Metal does not record premultiplication)".to_string(),
            ],
            coverage: None,
            textures: Vec::new(),
            duplicates: Vec::new(),
            stencil_probes: Vec::new(),
            failures: Vec::new(),
            sweep_error: None,
            open_error: None,
        }
    }

    /// The one-line assumptions string written into every file's KV data.
    pub fn assumptions_line(force_load_unused: bool) -> String {
        format!(
            "MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0; MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE={}; MTLREPLAYER_IGNORE_UNUSED_RESOURCE={}; alpha assumed straight (Metal does not record premultiplication)",
            u8::from(force_load_unused),
            u8::from(!force_load_unused)
        )
    }

    /// 0 when nothing failed; 1 when any per-texture failure, a sweep
    /// error, or a load failure was recorded. (2, bad arguments or an
    /// uncreatable output directory, is the binary's.)
    pub fn exit_code(&self) -> u8 {
        if self.failures.is_empty() && self.sweep_error.is_none() && self.open_error.is_none() {
            0
        } else {
            1
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_reflects_failures_and_sweep_errors_only() {
        let mut m = Manifest::new("b".into(), false, 60);
        assert_eq!(m.exit_code(), 0);
        m.duplicates.push(Duplicate {
            stream_ref: 1,
            identical: true,
        });
        m.stencil_probes.push(StencilProbe {
            stream_ref: 2,
            outcome: ProbeOutcome::Absent,
        });
        assert_eq!(m.exit_code(), 0, "informational entries are not failures");
        m.failures.push(Failure {
            stream_ref: 3,
            aspect: Aspect::Color,
            reason: "x".into(),
        });
        assert_eq!(m.exit_code(), 1);
        let mut m = Manifest::new("b".into(), false, 60);
        m.sweep_error = Some("fetch timed out".into());
        assert_eq!(m.exit_code(), 1);
        let mut m = Manifest::new("b".into(), false, 60);
        m.open_error = Some("the replayer reported an error".into());
        assert_eq!(m.exit_code(), 1);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(v["open_error"], "the replayer reported an error");
        let m = Manifest::new("b".into(), false, 60);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert!(
            v.get("open_error").is_none(),
            "open_error is omitted when the capture loaded"
        );
    }

    #[test]
    fn serialises_the_spec_shape() {
        let mut m = Manifest::new("cap.gputrace".into(), true, 600);
        m.coverage = Some(Coverage {
            loaded: 7,
            answered: 7,
            unused_resources: Some(0),
        });
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(v["coverage"]["loaded"], 7);
        assert_eq!(v["coverage"]["answered"], 7);
        assert_eq!(v["coverage"]["unused_resources"], 0);
        assert!(
            v.get("max_stream_ref").is_none(),
            "no sweep bound since hl 0.3.0"
        );
        assert_eq!(v["force_load_unused"], true);
        assert!(
            v["engine"]
                .as_str()
                .unwrap()
                .starts_with("gputools-replay-hl")
        );
        let m = Manifest::new("cap".into(), false, 1);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert!(
            v.get("coverage").is_none(),
            "coverage is omitted when the object map was not read"
        );
        assert_eq!(v["sweep_error"], serde_json::Value::Null);
    }

    #[test]
    fn fetch_at_parses_and_serialises_as_a_string() {
        assert_eq!("end".parse::<FetchAt>(), Ok(FetchAt::End));
        assert_eq!("start".parse::<FetchAt>(), Ok(FetchAt::Start));
        assert_eq!("120".parse::<FetchAt>(), Ok(FetchAt::Index(120)));
        assert!("middle".parse::<FetchAt>().is_err());
        assert_eq!(serde_json::to_string(&FetchAt::End).unwrap(), "\"end\"");
        assert_eq!(serde_json::to_string(&FetchAt::Index(7)).unwrap(), "\"7\"");
    }

    #[test]
    fn enums_serialise_lowercase() {
        assert_eq!(serde_json::to_string(&FetchAt::Start).unwrap(), "\"start\"");
        assert_eq!(
            serde_json::to_string(&ProbeOutcome::Written).unwrap(),
            "\"written\""
        );
        assert_eq!(
            serde_json::to_string(&Aspect::Stencil).unwrap(),
            "\"stencil\""
        );
    }
}
