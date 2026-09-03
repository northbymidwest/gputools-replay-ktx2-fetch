//! The two-pass fetch (spec 5): pass 1 sweeps plane 0, dedupes duplicate
//! streamRefs, joins descriptors and grades them; pass 2 probes every
//! depth-format ref for a stencil aspect. Nothing here touches disk.

use crate::emit::{Attributed, Fetched};
use crate::manifest::{
    Attribution, BundleManifest, Coverage, Duplicate, Failure, ProbeOutcome, StencilProbe,
};
use crate::tex::{Aspect, Tex, classify};
use gputools_replay_hl::{Descriptions, Error, ManifestStatus, TextureDescriptor};
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

pub trait Fetcher {
    type Tex: Tex;
    fn manifest_status(&self) -> ManifestStatus;
    fn textures(&self, refs: RangeInclusive<u64>) -> Result<Vec<Self::Tex>, Error>;
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
    fn describe(&self, texs: &[Self::Tex]) -> Descriptions;
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

pub fn run<F: Fetcher>(f: &F, max_stream_ref: u64) -> Sweep<F::Tex> {
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

    // Pass 1.
    let pass1 = match f.textures(0..=max_stream_ref) {
        Ok(t) => t,
        Err(e) => {
            sweep.sweep_error = Some(format!("pass 1 (plane 0 sweep): {e}"));
            return sweep;
        }
    };
    let kept = dedupe(pass1, &mut sweep.duplicates, &mut sweep.failures);

    // Describe and grade.
    let described = f.describe(&kept);
    let grades = grade(&kept, &described);
    if let ManifestStatus::Ok(_) = status {
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
            Err(e) => sweep.sweep_error = Some(format!("pass 2 (stencil aspects): {e}")),
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
        pass1: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
        stencil: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
        attributed: HashMap<u64, TextureDescriptor>,
        unplaced: Vec<TextureDescriptor>,
    }

    impl Fake {
        fn new(status: ManifestStatus, pass1: Result<Vec<FakeTex>, Error>) -> Self {
            Self {
                status,
                pass1: RefCell::new(Some(pass1)),
                stencil: RefCell::new(Some(Ok(Vec::new()))),
                attributed: HashMap::new(),
                unplaced: Vec::new(),
            }
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
        fn textures(&self, _refs: RangeInclusive<u64>) -> Result<Vec<FakeTex>, Error> {
            self.pass1.borrow_mut().take().unwrap()
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
        let s = run(&f, 10);
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
        let s = run(&f, 10);
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
        let s = run(&f, 10);
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
        let s = run(&f, 10);
        assert_eq!(
            s.fetched[0].descriptor.unwrap().attribution,
            Attribution::Ambiguous
        );
    }

    #[test]
    fn pass1_error_is_run_level_and_keeps_the_manifest_status() {
        let f = Fake::new(ManifestStatus::Ok(3), Err(Error::Truncated));
        let s = run(&f, 10);
        assert!(s.fetched.is_empty());
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 1"));
        assert_eq!(s.bundle_manifest, BundleManifest::Ok { textures_listed: 3 });
        assert!(s.coverage.is_none());
    }

    #[test]
    fn pass2_error_keeps_pass1_results() {
        let f = Fake::new(
            ManifestStatus::NoDescriptors,
            Ok(vec![bgra(1, 4, 4), depth(2)]),
        )
        .with_stencil(Err(Error::Truncated));
        let s = run(&f, 10);
        assert_eq!(s.fetched.len(), 2);
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 2"));
        assert!(s.probes.is_empty());
    }
}
