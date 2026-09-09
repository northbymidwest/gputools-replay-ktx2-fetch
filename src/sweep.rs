//! The two-pass fetch (spec 5): pass 1 takes the replayer's loaded-texture
//! snapshot, positions playback, fetches those refs at plane 0, and dedupes
//! duplicate replies; pass 2 probes every depth-format ref for a stencil
//! aspect. Nothing here touches disk.
//!
//! hl 0.3.0 snapshots the object map at `Capture::open`, when it is
//! guaranteed populated (playback clears the live map in place, MEASURED on
//! hl 0.2.0 across every fixture), so the snapshot is stable whatever the
//! playback position and there is no ref range to sweep. Content fetched
//! after playback is the end state on the first and every later fetch, even
//! though the command index reads 0 after a fetch, so the index is read
//! right after playback.

use crate::emit::Fetched;
use crate::manifest::{Coverage, Duplicate, Failure, ProbeOutcome, StencilProbe};
use crate::tex::{Aspect, Tex, classify};
use gputools_replay_hl::format::format_kind;
use gputools_replay_hl::{Error, ObjectMapError, TextureDescriptor};
use std::collections::{BTreeMap, HashMap};

pub trait Fetcher {
    type Tex: Tex;
    /// Every loaded texture with its descriptor, from the replayer's
    /// open-time snapshot of its object map. `Err` only when the map itself
    /// could not be reached, which takes out every descriptor at once.
    fn loaded_textures(&self) -> Result<Vec<(u64, TextureDescriptor)>, ObjectMapError>;
    /// The streamRefs of resources (of any kind) the replayer force-loaded
    /// that no captured command uses, from the same snapshot. Empty unless
    /// force-load is on.
    fn unused_resource_refs(&self) -> Result<Vec<u64>, ObjectMapError>;
    /// Position playback for the fetch (`--fetch-at`); returns the command
    /// index it reached. Called once, before any fetch.
    fn replay(&self) -> u32;
    /// Plane 0 of each ref, in one fetch.
    fn textures(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
}

/// Refs per fetch. A fetch is all-or-nothing under a timeout or a replayer
/// error, so this bounds what one failure costs before the refs of a failed
/// fetch are retried one at a time.
pub const CHUNK: usize = 2_000;

pub struct Sweep<T> {
    /// The command index playback reached before the first fetch.
    pub command_index: u32,
    pub fetched: Vec<Fetched<T>>,
    pub probes: Vec<StencilProbe>,
    pub duplicates: Vec<Duplicate>,
    pub failures: Vec<Failure>,
    /// Present when the snapshot answered; withheld when it did not.
    pub coverage: Option<Coverage>,
    pub sweep_error: Option<String>,
}

type Key = (u32, u32, u32);

fn key<T: Tex>(t: &T) -> Key {
    (t.width(), t.height(), t.format().0 as u32)
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

/// Fetch `refs` in chunks. A chunk that fails is retried one ref at a time
/// so a failure lands on the ref that caused it; `descs` names the aspect
/// a failure is recorded under.
fn fetch_refs<F: Fetcher>(
    f: &F,
    refs: &[u64],
    descs: &BTreeMap<u64, TextureDescriptor>,
    failures: &mut Vec<Failure>,
) -> Vec<F::Tex> {
    let mut out = Vec::new();
    for chunk in refs.chunks(CHUNK) {
        match f.textures(chunk) {
            Ok(t) => out.extend(t),
            Err(_) => {
                for r in chunk {
                    match f.textures(std::slice::from_ref(r)) {
                        Ok(t) => out.extend(t),
                        Err(e) => failures.push(Failure {
                            stream_ref: *r,
                            aspect: descs
                                .get(r)
                                .map(|d| classify(&format_kind(d.pixel_format)))
                                .unwrap_or(Aspect::Color),
                            reason: format!("fetch: {e}"),
                        }),
                    }
                }
            }
        }
    }
    out
}

/// `force_load_unused` says whether the replayer was asked to create unused
/// resources, which is the only case in which it reports them.
pub fn run<F: Fetcher>(f: &F, force_load_unused: bool) -> Sweep<F::Tex> {
    let mut sweep = Sweep {
        command_index: 0,
        fetched: Vec::new(),
        probes: Vec::new(),
        duplicates: Vec::new(),
        failures: Vec::new(),
        coverage: None,
        sweep_error: None,
    };

    // Pass 1: the snapshot names every loaded texture; nothing else can be
    // discovered, so without it the run stops here.
    let descs: BTreeMap<u64, TextureDescriptor> = match f.loaded_textures() {
        Ok(list) => list.into_iter().collect(),
        Err(e) => {
            sweep.sweep_error = Some(format!(
                "the replayer's object map could not be read ({e}); no texture can be enumerated or described, so nothing was fetched"
            ));
            return sweep;
        }
    };
    let unused = force_load_unused.then(|| f.unused_resource_refs().map(|u| u.len()).unwrap_or(0));
    sweep.command_index = f.replay();
    let refs: Vec<u64> = descs.keys().copied().collect();
    let pass1 = fetch_refs(f, &refs, &descs, &mut sweep.failures);
    let kept = dedupe(pass1, &mut sweep.duplicates, &mut sweep.failures);
    sweep.coverage = Some(Coverage {
        loaded: descs.len(),
        answered: kept.len(),
        unused_resources: unused,
    });

    let depth_refs: Vec<u64> = kept
        .iter()
        .filter(|t| classify(&t.format_kind()) == Aspect::Depth)
        .map(|t| t.stream_ref())
        .collect();

    for t in kept {
        let aspect = classify(&t.format_kind());
        let descriptor = descs.get(&t.stream_ref()).copied();
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
                        let descriptor = descs.get(&t.stream_ref()).copied();
                        sweep.fetched.push(Fetched {
                            texture: t,
                            aspect: Aspect::Stencil,
                            probed: true,
                            descriptor,
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
                sweep.sweep_error = Some(format!("pass 2 (stencil aspects): {e}"));
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
        /// The open-time snapshot; `Err` makes it unreadable.
        snapshot: Result<Vec<(u64, TextureDescriptor)>, ObjectMapError>,
        unused: Vec<u64>,
        /// What a fetch can answer, by ref. A ref listed in `refuse` fails
        /// any fetch that includes it.
        answers: Vec<FakeTex>,
        refuse: Vec<u64>,
        requested: RefCell<Vec<Vec<u64>>>,
        stencil: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
    }

    impl Fake {
        /// Every answer is also a loaded texture in the snapshot.
        fn new(answers: Vec<FakeTex>) -> Self {
            let mut seen = BTreeMap::new();
            for t in &answers {
                seen.entry(t.stream_ref)
                    .or_insert_with(|| desc(t.stream_ref, t.width, t.height, t.mtl_raw));
            }
            Self {
                snapshot: Ok(seen.into_iter().collect()),
                unused: Vec::new(),
                answers,
                refuse: Vec::new(),
                requested: RefCell::new(Vec::new()),
                stencil: RefCell::new(Some(Ok(Vec::new()))),
            }
        }
        fn unloaded(mut self, stream_ref: u64) -> Self {
            if let Ok(s) = &mut self.snapshot {
                s.retain(|(r, _)| *r != stream_ref);
            }
            self
        }
        fn unused(mut self, refs: &[u64]) -> Self {
            self.unused = refs.to_vec();
            self
        }
        fn without_map(mut self) -> Self {
            self.snapshot = Err(ObjectMapError::WrongClass);
            self
        }
        fn refusing(mut self, r: u64) -> Self {
            self.refuse.push(r);
            self
        }
        fn with_stencil(self, r: Result<Vec<FakeTex>, Error>) -> Self {
            *self.stencil.borrow_mut() = Some(r);
            self
        }
    }

    impl Fetcher for Fake {
        type Tex = FakeTex;
        fn loaded_textures(&self) -> Result<Vec<(u64, TextureDescriptor)>, ObjectMapError> {
            self.snapshot.clone()
        }
        fn unused_resource_refs(&self) -> Result<Vec<u64>, ObjectMapError> {
            self.snapshot
                .as_ref()
                .map(|_| self.unused.clone())
                .map_err(|e| *e)
        }
        fn replay(&self) -> u32 {
            self.requested.borrow_mut().push(vec![u64::MAX]);
            22
        }
        fn textures(&self, refs: &[u64]) -> Result<Vec<FakeTex>, Error> {
            self.requested.borrow_mut().push(refs.to_vec());
            if refs.iter().any(|r| self.refuse.contains(r)) {
                return Err(Error::Truncated);
            }
            Ok(self
                .answers
                .iter()
                .filter(|t| refs.contains(&t.stream_ref))
                .cloned()
                .collect())
        }
        fn stencil_aspects(&self, _refs: &[u64]) -> Result<Vec<FakeTex>, Error> {
            self.stencil.borrow_mut().take().unwrap()
        }
    }

    fn desc(stream_ref: u64, w: u32, h: u32, fmt: u32) -> TextureDescriptor {
        TextureDescriptor {
            stream_ref,
            width: w,
            height: h,
            depth: 1,
            pixel_format: fmt,
            texture_type: 2,
            mip_levels: 1,
            array_length: 1,
            sample_count: 1,
            usage: 0,
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
    fn refs_of(s: &Sweep<FakeTex>) -> Vec<u64> {
        s.fetched.iter().map(|x| x.texture.stream_ref).collect()
    }
    /// The fetches the sweep made, in order; `replay` marks playback.
    fn requests(f: &Fake) -> Vec<String> {
        f.requested
            .borrow()
            .iter()
            .map(|r| {
                if r == &[u64::MAX] {
                    "replay".to_string()
                } else {
                    format!("{r:?}")
                }
            })
            .collect()
    }

    #[test]
    fn only_loaded_refs_are_fetched_after_playback_each_with_its_own_descriptor() {
        // Ref 7 would answer a fetch but is not loaded: never asked for.
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(3, 8, 8), bgra(7, 4, 4)])
            .unloaded(7)
            .unused(&[7, 12]);
        let s = run(&f, true);
        assert!(s.sweep_error.is_none());
        assert_eq!(requests(&f), vec!["replay", "[1, 3]"]);
        assert_eq!(s.command_index, 22);
        assert_eq!(refs_of(&s), vec![1, 3]);
        for x in &s.fetched {
            let d = x.descriptor.unwrap();
            assert_eq!(d.stream_ref, x.texture.stream_ref);
            assert_eq!((d.width, d.height), (x.texture.width, x.texture.height));
        }
        assert_eq!(
            s.coverage,
            Some(Coverage {
                loaded: 2,
                answered: 2,
                unused_resources: Some(2)
            })
        );
        let s = run(&f, false);
        assert_eq!(
            s.coverage.unwrap().unused_resources,
            None,
            "the replayer reports unused resources only under force-load"
        );
    }

    #[test]
    fn classifies_and_probes_depth_refs_for_stencil() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2), depth(3)])
            .with_stencil(Ok(vec![stencil_aspect(2), depth(3)]));
        let s = run(&f, true);
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
        let stencil = s.fetched.iter().find(|x| x.probed).unwrap();
        assert_eq!(
            stencil.descriptor.map(|d| d.stream_ref),
            Some(2),
            "a probed aspect carries its ref's descriptor"
        );
    }

    #[test]
    fn identical_duplicates_collapse_and_conflicting_ones_fail() {
        let mut other = bgra(5, 4, 4);
        other.bytes[0] = 99;
        let f = Fake::new(vec![bgra(4, 4, 4), bgra(4, 4, 4), bgra(5, 4, 4), other]);
        let s = run(&f, true);
        assert_eq!(refs_of(&s), vec![4]);
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
        assert_eq!(
            s.coverage,
            Some(Coverage {
                loaded: 2,
                answered: 1,
                unused_resources: Some(0)
            })
        );
    }

    #[test]
    fn a_failed_chunk_is_retried_one_ref_at_a_time() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2), bgra(3, 4, 4)]).refusing(2);
        let s = run(&f, true);
        assert_eq!(
            requests(&f),
            vec!["replay", "[1, 2, 3]", "[1]", "[2]", "[3]"]
        );
        assert_eq!(refs_of(&s), vec![1, 3]);
        assert_eq!(s.failures.len(), 1);
        assert_eq!(s.failures[0].stream_ref, 2);
        assert_eq!(
            s.failures[0].aspect,
            Aspect::Depth,
            "aspect from the descriptor"
        );
        assert!(s.failures[0].reason.starts_with("fetch:"));
        assert!(
            s.sweep_error.is_none(),
            "a per-ref failure is not a run error"
        );
        assert_eq!(s.coverage.unwrap().answered, 2);
    }

    #[test]
    fn an_unreadable_map_stops_the_run_before_playback() {
        let f = Fake::new(vec![bgra(1, 4, 4)]).without_map();
        let s = run(&f, true);
        assert!(requests(&f).is_empty(), "no playback and no fetch");
        assert!(s.fetched.is_empty());
        let err = s.sweep_error.as_deref().unwrap();
        assert!(err.contains("object map could not be read"), "{err}");
        assert!(s.coverage.is_none());
    }

    #[test]
    fn pass2_error_keeps_pass1_results() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2)]).with_stencil(Err(Error::Truncated));
        let s = run(&f, true);
        assert_eq!(s.fetched.len(), 2);
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 2"));
        assert!(s.probes.is_empty());
    }

    #[test]
    fn loaded_refs_are_fetched_in_chunks() {
        let answers: Vec<FakeTex> = (0..(CHUNK as u64 + 5)).map(|r| bgra(r, 1, 1)).collect();
        let f = Fake::new(answers);
        let s = run(&f, true);
        let sizes: Vec<usize> = f.requested.borrow().iter().skip(1).map(Vec::len).collect();
        assert_eq!(sizes, vec![CHUNK, 5]);
        assert_eq!(s.fetched.len(), CHUNK + 5);
    }
}
