//! The two-pass fetch (spec 5): pass 1 sweeps plane 0, dedupes duplicate
//! streamRefs, joins descriptors and grades them; pass 2 probes every
//! depth-format ref for a stencil aspect. Nothing here touches disk.

use crate::emit::{Attributed, Fetched};
use crate::manifest::{
    Attribution, BoundSource, BundleManifest, Coverage, Duplicate, Failure, ProbeOutcome,
    StencilProbe,
};
use crate::tex::{Aspect, Tex, classify};
use gputools_replay_hl::{Descriptions, Error, ManifestStatus, TextureDescriptor};
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

pub trait Fetcher {
    type Tex: Tex;
    fn manifest_status(&self) -> ManifestStatus;
    /// The bundle's index record count, an upper bound on the highest
    /// streamRef the replayer can assign (it creates at most one resource per
    /// record). `None` when the bundle cannot be read.
    fn record_count(&self) -> Option<usize>;
    fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Self::Tex>, Error>;
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
    fn describe(&self, texs: &[Self::Tex]) -> Descriptions;
}

/// The sweep ceiling when the bundle gives no record count.
pub const DEFAULT_MAX_STREAM_REF: u64 = 20_000;
/// Headroom over the record count, in case the load path assigns a few refs
/// past the records it creates resources from.
pub const RECORD_COUNT_MARGIN: u64 = 64;
/// Refs per fetch in pass 1. A fetch is all-or-nothing under a timeout or a
/// replayer error, so this bounds what one failure can lose. MEASURED: a
/// nonexistent ref costs about 17 microseconds, so the sweep width itself is
/// cheap.
pub const CHUNK: u64 = 2_000;

/// The sweep's upper bound and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    pub max_stream_ref: u64,
    pub source: BoundSource,
}

/// Spec 3: an explicit `--max-stream-ref` wins; otherwise the bundle's index
/// record count plus a margin; otherwise the built-in ceiling.
pub fn bound<F: Fetcher>(f: &F, flag: Option<u64>) -> Bound {
    if let Some(max_stream_ref) = flag {
        return Bound {
            max_stream_ref,
            source: BoundSource::Flag,
        };
    }
    match f.record_count() {
        Some(n) => Bound {
            max_stream_ref: (n as u64).saturating_add(RECORD_COUNT_MARGIN),
            source: BoundSource::BundleRecordCount,
        },
        None => Bound {
            max_stream_ref: DEFAULT_MAX_STREAM_REF,
            source: BoundSource::Default,
        },
    }
}

/// The chunk ranges pass 1 fetches for a bound.
pub fn chunks(max_stream_ref: u64) -> Vec<RangeInclusive<u64>> {
    let mut out = Vec::new();
    let mut start = 0u64;
    loop {
        let end = start.saturating_add(CHUNK - 1).min(max_stream_ref);
        out.push(start..=end);
        if end == max_stream_ref {
            break;
        }
        start = end + 1;
    }
    out
}

pub struct Sweep<T> {
    pub fetched: Vec<Fetched<T>>,
    pub probes: Vec<StencilProbe>,
    pub duplicates: Vec<Duplicate>,
    pub failures: Vec<Failure>,
    pub bundle_manifest: BundleManifest,
    pub coverage: Option<Coverage>,
    pub sweep_error: Option<String>,
}

type Key = (u32, u32, u32);

fn key<T: Tex>(t: &T) -> Key {
    (t.width(), t.height(), t.format().0 as u32)
}

fn desc_key(d: &TextureDescriptor) -> Key {
    (d.width, d.height, d.format)
}

/// Collapse records sharing a streamRef (spec 5 step 4). Identical copies
/// keep one; differing copies are all dropped and recorded as a failure.
fn dedupe<T: Tex>(
    texs: Vec<T>,
    duplicates: &mut Vec<Duplicate>,
    failures: &mut Vec<Failure>,
) -> Vec<T> {
    let mut groups: BTreeMap<u64, Vec<T>> = BTreeMap::new();
    for t in texs {
        groups.entry(t.stream_ref()).or_default().push(t);
    }
    let mut kept = Vec::new();
    for (stream_ref, mut group) in groups {
        if group.len() == 1 {
            kept.extend(group);
            continue;
        }
        let first = group.remove(0);
        let identical = group
            .iter()
            .all(|g| g.raw_bytes() == first.raw_bytes() && key(g) == key(&first));
        duplicates.push(Duplicate {
            stream_ref,
            identical,
        });
        if identical {
            kept.push(first);
        } else {
            failures.push(Failure {
                stream_ref,
                aspect: classify(&first.format_kind()),
                reason: format!(
                    "{} records for this streamRef differ byte-for-byte; cannot choose one",
                    group.len() + 1
                ),
            });
        }
    }
    kept
}

/// Spec 5 step 5: a geometry group is `certain` only when the fetched and
/// listed counts for that exact `(width, height, format)` agree.
fn grade<T: Tex>(texs: &[T], d: &Descriptions) -> HashMap<Key, Attribution> {
    let mut fetched: HashMap<Key, usize> = HashMap::new();
    for t in texs {
        *fetched.entry(key(t)).or_default() += 1;
    }
    let mut listed: HashMap<Key, usize> = HashMap::new();
    for desc in d.per_texture.iter().flatten().chain(d.unplaced.iter()) {
        *listed.entry(desc_key(desc)).or_default() += 1;
    }
    fetched
        .keys()
        .chain(listed.keys())
        .map(|k| {
            let same = fetched.get(k).copied().unwrap_or(0) == listed.get(k).copied().unwrap_or(0);
            (
                *k,
                if same {
                    Attribution::Certain
                } else {
                    Attribution::Ambiguous
                },
            )
        })
        .collect()
}

pub fn run<F: Fetcher>(f: &F, bound: &Bound) -> Sweep<F::Tex> {
    let status = f.manifest_status();
    let bundle_manifest = match status {
        ManifestStatus::Ok(n) => BundleManifest::Ok { textures_listed: n },
        ManifestStatus::NoDescriptors => BundleManifest::NoDescriptors,
        ManifestStatus::Unparseable => BundleManifest::Unparseable,
    };
    let mut sweep = Sweep {
        fetched: Vec::new(),
        probes: Vec::new(),
        duplicates: Vec::new(),
        failures: Vec::new(),
        bundle_manifest,
        coverage: None,
        sweep_error: None,
    };

    // Pass 1, in chunks: a fetch is all-or-nothing, so a failed chunk is
    // recorded and the others still count. Coverage is reported only when
    // every chunk answered, since a gap would make its numbers meaningless.
    let ranges = chunks(bound.max_stream_ref);
    let mut pass1 = Vec::new();
    let mut chunk_errors = Vec::new();
    for range in &ranges {
        match f.textures(range.clone()) {
            Ok(t) => pass1.extend(t),
            Err(e) => chunk_errors.push(format!("refs {}..={}: {e}", range.start(), range.end())),
        }
    }
    if !chunk_errors.is_empty() {
        sweep.sweep_error = Some(format!(
            "pass 1 (plane 0 sweep) failed for {} of {} chunks: {}",
            chunk_errors.len(),
            ranges.len(),
            chunk_errors.join("; ")
        ));
    }
    let kept = dedupe(pass1, &mut sweep.duplicates, &mut sweep.failures);

    // Describe and grade.
    let described = f.describe(&kept);
    let grades = grade(&kept, &described);
    if let (ManifestStatus::Ok(_), true) = (status, chunk_errors.is_empty()) {
        let attributed = described.per_texture.iter().flatten().count();
        sweep.coverage = Some(Coverage {
            answered: kept.len(),
            attributed,
            unattributed: kept.len() - attributed,
            listed_not_answered: described.unplaced.len(),
        });
    }

    let depth_refs: Vec<u64> = kept
        .iter()
        .filter(|t| classify(&t.format_kind()) == Aspect::Depth)
        .map(|t| t.stream_ref())
        .collect();

    for (t, desc) in kept.into_iter().zip(described.per_texture) {
        let aspect = classify(&t.format_kind());
        let descriptor = desc.map(|descriptor| Attributed {
            descriptor,
            attribution: grades
                .get(&key(&t))
                .copied()
                .unwrap_or(Attribution::Ambiguous),
        });
        sweep.fetched.push(Fetched {
            texture: t,
            aspect,
            probed: false,
            descriptor,
        });
    }

    // Pass 2: the stencil aspect of every depth-format ref. Plane 1 is
    // inert on a plain depth texture (it echoes the depth), so only a
    // stencil-only reply counts.
    if !depth_refs.is_empty() {
        match f.stencil_aspects(&depth_refs) {
            Ok(replies) => {
                let replies = dedupe(replies, &mut sweep.duplicates, &mut sweep.failures);
                let mut written: HashMap<u64, bool> =
                    depth_refs.iter().map(|r| (*r, false)).collect();
                for t in replies {
                    if classify(&t.format_kind()) == Aspect::Stencil
                        && written.contains_key(&t.stream_ref())
                    {
                        written.insert(t.stream_ref(), true);
                        sweep.fetched.push(Fetched {
                            texture: t,
                            aspect: Aspect::Stencil,
                            probed: true,
                            descriptor: None,
                        });
                    }
                }
                for r in &depth_refs {
                    let outcome = if written.get(r).copied().unwrap_or(false) {
                        ProbeOutcome::Written
                    } else {
                        ProbeOutcome::Absent
                    };
                    sweep.probes.push(StencilProbe {
                        stream_ref: *r,
                        outcome,
                    });
                }
            }
            Err(e) => {
                let msg = format!("pass 2 (stencil aspects): {e}");
                sweep.sweep_error = Some(match sweep.sweep_error.take() {
                    Some(prev) => format!("{prev}; {msg}"),
                    None => msg,
                });
            }
        }
    }
    sweep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tex::fake::FakeTex;
    use std::cell::RefCell;

    struct Fake {
        status: ManifestStatus,
        /// `Err` makes every chunk fail.
        pass1: Result<Vec<FakeTex>, ()>,
        /// A chunk whose start ref equals this fails.
        fail_chunk_at: Option<u64>,
        requested: RefCell<Vec<RangeInclusive<u64>>>,
        record_count: Option<usize>,
        stencil: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
        attributed: HashMap<u64, TextureDescriptor>,
        unplaced: Vec<TextureDescriptor>,
    }

    fn ten() -> Bound {
        Bound {
            max_stream_ref: 10,
            source: BoundSource::Flag,
        }
    }

    impl Fake {
        fn new(status: ManifestStatus, pass1: Result<Vec<FakeTex>, Error>) -> Self {
            Self {
                status,
                pass1: pass1.map_err(|_| ()),
                fail_chunk_at: None,
                requested: RefCell::new(Vec::new()),
                record_count: None,
                stencil: RefCell::new(Some(Ok(Vec::new()))),
                attributed: HashMap::new(),
                unplaced: Vec::new(),
            }
        }
        fn with_record_count(mut self, n: Option<usize>) -> Self {
            self.record_count = n;
            self
        }
        fn failing_chunk_at(mut self, start: u64) -> Self {
            self.fail_chunk_at = Some(start);
            self
        }
        fn with_stencil(self, r: Result<Vec<FakeTex>, Error>) -> Self {
            *self.stencil.borrow_mut() = Some(r);
            self
        }
        fn attribute(mut self, stream_ref: u64, d: TextureDescriptor) -> Self {
            self.attributed.insert(stream_ref, d);
            self
        }
        fn unplaced(mut self, d: TextureDescriptor) -> Self {
            self.unplaced.push(d);
            self
        }
    }

    impl Fetcher for Fake {
        type Tex = FakeTex;
        fn manifest_status(&self) -> ManifestStatus {
            self.status
        }
        fn record_count(&self) -> Option<usize> {
            self.record_count
        }
        fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<FakeTex>, Error> {
            self.requested.borrow_mut().push(refs.clone());
            if self.fail_chunk_at == Some(*refs.start()) {
                return Err(Error::Truncated);
            }
            match &self.pass1 {
                Err(()) => Err(Error::Truncated),
                Ok(all) => Ok(all
                    .iter()
                    .filter(|t| refs.contains(&t.stream_ref))
                    .cloned()
                    .collect()),
            }
        }
        fn stencil_aspects(&self, _refs: &[u64]) -> Result<Vec<FakeTex>, Error> {
            self.stencil.borrow_mut().take().unwrap()
        }
        fn describe(&self, texs: &[FakeTex]) -> Descriptions {
            Descriptions {
                per_texture: texs
                    .iter()
                    .map(|t| self.attributed.get(&t.stream_ref).copied())
                    .collect(),
                unplaced: self.unplaced.clone(),
                transparent: Vec::new(),
            }
        }
    }

    fn desc(w: u32, h: u32, fmt: u32) -> TextureDescriptor {
        TextureDescriptor {
            store0_offset: 0,
            format: fmt,
            texture_type: 2,
            width: w,
            height: h,
            depth: 1,
            mip_levels: 1,
            array_length: 1,
            sample_count: 1,
            usage: 0,
            texture_id: 0,
        }
    }
    fn bgra(r: u64, w: u32, h: u32) -> FakeTex {
        FakeTex::solid(r, w, h, 80, &[1, 2, 3, 4])
    }
    fn depth(r: u64) -> FakeTex {
        FakeTex::solid(r, 2, 2, 252, &0.5f32.to_le_bytes())
    }
    fn stencil_aspect(r: u64) -> FakeTex {
        FakeTex::solid(r, 2, 2, 261, &[42])
    }

    #[test]
    fn classifies_and_probes_depth_refs_for_stencil() {
        let f = Fake::new(
            ManifestStatus::NoDescriptors,
            Ok(vec![bgra(1, 4, 4), depth(2), depth(3)]),
        )
        .with_stencil(Ok(vec![stencil_aspect(2), depth(3)]));
        let s = run(&f, &ten());
        assert!(s.sweep_error.is_none());
        let aspects: Vec<(u64, Aspect, bool)> = s
            .fetched
            .iter()
            .map(|x| (x.texture.stream_ref, x.aspect, x.probed))
            .collect();
        assert_eq!(
            aspects,
            vec![
                (1, Aspect::Color, false),
                (2, Aspect::Depth, false),
                (3, Aspect::Depth, false),
                (2, Aspect::Stencil, true)
            ]
        );
        assert_eq!(
            s.probes,
            vec![
                StencilProbe {
                    stream_ref: 2,
                    outcome: ProbeOutcome::Written
                },
                StencilProbe {
                    stream_ref: 3,
                    outcome: ProbeOutcome::Absent
                }
            ]
        );
        assert!(
            s.coverage.is_none(),
            "no coverage without a parsed manifest"
        );
        assert_eq!(s.bundle_manifest, BundleManifest::NoDescriptors);
    }

    #[test]
    fn identical_duplicates_collapse_and_conflicting_ones_fail() {
        let mut other = bgra(5, 4, 4);
        other.bytes[0] = 99;
        let f = Fake::new(
            ManifestStatus::Unparseable,
            Ok(vec![bgra(4, 4, 4), bgra(4, 4, 4), bgra(5, 4, 4), other]),
        );
        let s = run(&f, &ten());
        let refs: Vec<u64> = s.fetched.iter().map(|x| x.texture.stream_ref).collect();
        assert_eq!(refs, vec![4]);
        assert_eq!(
            s.duplicates,
            vec![
                Duplicate {
                    stream_ref: 4,
                    identical: true
                },
                Duplicate {
                    stream_ref: 5,
                    identical: false
                }
            ]
        );
        assert_eq!(s.failures.len(), 1);
        assert_eq!(s.failures[0].stream_ref, 5);
        assert!(s.failures[0].reason.contains("differ"));
    }

    #[test]
    fn grades_by_group_count_equality_and_reports_coverage() {
        // 64x64 BGRA: three fetched, two listed -> ambiguous.
        // 32x32 BGRA: one fetched, one listed -> certain.
        // 16x16 BGRA: listed, never answered -> unplaced.
        let f = Fake::new(
            ManifestStatus::Ok(4),
            Ok(vec![
                bgra(1, 64, 64),
                bgra(2, 64, 64),
                bgra(3, 64, 64),
                bgra(4, 32, 32),
            ]),
        )
        .attribute(1, desc(64, 64, 80))
        .attribute(2, desc(64, 64, 80))
        .attribute(4, desc(32, 32, 80))
        .unplaced(desc(16, 16, 80));
        let s = run(&f, &ten());
        let grade_of = |r: u64| {
            s.fetched
                .iter()
                .find(|x| x.texture.stream_ref == r)
                .unwrap()
                .descriptor
                .map(|a| a.attribution)
        };
        assert_eq!(grade_of(1), Some(Attribution::Ambiguous));
        assert_eq!(grade_of(2), Some(Attribution::Ambiguous));
        assert_eq!(grade_of(3), None);
        assert_eq!(grade_of(4), Some(Attribution::Certain));
        assert_eq!(
            s.coverage,
            Some(Coverage {
                answered: 4,
                attributed: 3,
                unattributed: 1,
                listed_not_answered: 1
            })
        );
        assert_eq!(s.bundle_manifest, BundleManifest::Ok { textures_listed: 4 });
    }

    #[test]
    fn an_unanswered_descriptor_makes_its_group_ambiguous() {
        let f = Fake::new(ManifestStatus::Ok(2), Ok(vec![bgra(1, 8, 8)]))
            .attribute(1, desc(8, 8, 80))
            .unplaced(desc(8, 8, 80));
        let s = run(&f, &ten());
        assert_eq!(
            s.fetched[0].descriptor.unwrap().attribution,
            Attribution::Ambiguous
        );
    }

    #[test]
    fn pass1_error_is_run_level_and_keeps_the_manifest_status() {
        let f = Fake::new(ManifestStatus::Ok(3), Err(Error::Truncated));
        let s = run(&f, &ten());
        assert!(s.fetched.is_empty());
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 1"));
        assert_eq!(s.bundle_manifest, BundleManifest::Ok { textures_listed: 3 });
        assert!(s.coverage.is_none(), "no coverage when a chunk failed");
    }

    #[test]
    fn pass2_error_keeps_pass1_results() {
        let f = Fake::new(
            ManifestStatus::NoDescriptors,
            Ok(vec![bgra(1, 4, 4), depth(2)]),
        )
        .with_stencil(Err(Error::Truncated));
        let s = run(&f, &ten());
        assert_eq!(s.fetched.len(), 2);
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 2"));
        assert!(s.probes.is_empty());
    }

    #[test]
    fn bound_prefers_the_flag_then_the_record_count_then_the_default() {
        let f = Fake::new(ManifestStatus::Ok(1), Ok(vec![])).with_record_count(Some(1555));
        assert_eq!(
            bound(&f, Some(42)),
            Bound {
                max_stream_ref: 42,
                source: BoundSource::Flag
            }
        );
        assert_eq!(
            bound(&f, None),
            Bound {
                max_stream_ref: 1555 + RECORD_COUNT_MARGIN,
                source: BoundSource::BundleRecordCount
            }
        );
        let f = Fake::new(ManifestStatus::Unparseable, Ok(vec![]));
        assert_eq!(
            bound(&f, None),
            Bound {
                max_stream_ref: DEFAULT_MAX_STREAM_REF,
                source: BoundSource::Default
            }
        );
    }

    #[test]
    fn pass1_is_fetched_in_chunks_covering_the_bound_exactly() {
        assert_eq!(chunks(10), vec![0..=10]);
        assert_eq!(chunks(CHUNK - 1), vec![0..=CHUNK - 1]);
        assert_eq!(chunks(CHUNK), vec![0..=CHUNK - 1, CHUNK..=CHUNK]);
        assert_eq!(chunks(4500), vec![0..=1999, 2000..=3999, 4000..=4500]);
        let f = Fake::new(
            ManifestStatus::NoDescriptors,
            Ok(vec![bgra(1, 4, 4), bgra(2500, 4, 4), bgra(4200, 4, 4)]),
        );
        let b = Bound {
            max_stream_ref: 4500,
            source: BoundSource::Flag,
        };
        let s = run(&f, &b);
        assert_eq!(
            *f.requested.borrow(),
            vec![0..=1999, 2000..=3999, 4000..=4500]
        );
        let refs: Vec<u64> = s.fetched.iter().map(|x| x.texture.stream_ref).collect();
        assert_eq!(refs, vec![1, 2500, 4200]);
        assert!(s.sweep_error.is_none());
    }

    #[test]
    fn a_failed_chunk_is_recorded_and_the_other_chunks_still_count() {
        let f = Fake::new(
            ManifestStatus::Ok(3),
            Ok(vec![bgra(1, 4, 4), bgra(2500, 4, 4), bgra(4200, 4, 4)]),
        )
        .failing_chunk_at(2000);
        let b = Bound {
            max_stream_ref: 4500,
            source: BoundSource::Flag,
        };
        let s = run(&f, &b);
        let refs: Vec<u64> = s.fetched.iter().map(|x| x.texture.stream_ref).collect();
        assert_eq!(
            refs,
            vec![1, 4200],
            "the failed chunk's ref is missing, the others are kept"
        );
        let err = s.sweep_error.as_deref().unwrap();
        assert!(err.contains("1 of 3 chunks"), "{err}");
        assert!(err.contains("refs 2000..=3999"), "{err}");
        assert!(
            s.coverage.is_none(),
            "coverage is withheld when a chunk failed"
        );
    }
}
