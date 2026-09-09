//! The two-pass fetch (spec 5): pass 1 walks the replayer's object map for
//! every loaded texture up to the bound, positions playback, fetches those
//! refs at plane 0, walks the map again for anything playback added, and
//! dedupes duplicate replies; pass 2 probes every depth-format ref for a
//! stencil aspect. Nothing here touches disk.
//!
//! MEASURED (hl 0.2.0, every fixture, with and without force-load): the map
//! is populated when the session opens, is EMPTY after `play_all` until a
//! fetch returns at least one real texture, and then holds every loaded
//! texture again. A fetch of only nonexistent refs does not repopulate it.
//! Content fetched after playback is the end state on the first and every
//! later fetch, even though the command index reads 0 after a fetch. So the
//! refs come from the walk before playback, and the descriptors from the
//! walk after the fetch.

use crate::emit::Fetched;
use crate::manifest::{BoundSource, Coverage, Duplicate, Failure, ProbeOutcome, StencilProbe};
use crate::tex::{Aspect, Tex, classify};
use gputools_replay_hl::format::format_kind;
use gputools_replay_hl::{Error, ObjectMapError, TextureDescriptor};
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

pub trait Fetcher {
    type Tex: Tex;
    /// The descriptor of the loaded texture at `stream_ref`, read off the
    /// replayer's object map: `Ok(None)` when that ref is not a loaded
    /// texture, `Err` only when the map itself cannot be reached (which
    /// takes out every descriptor at once).
    fn texture_descriptor(
        &self,
        stream_ref: u64,
    ) -> Result<Option<TextureDescriptor>, ObjectMapError>;
    /// Position playback for the fetch (`--fetch-at`); returns the command
    /// index it reached. Called once, after the first map walk and before
    /// any fetch.
    fn replay(&self) -> u32;
    /// Plane 0 of each ref, in one fetch.
    fn textures(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
    fn stencil_aspects(&self, refs: &[u64]) -> Result<Vec<Self::Tex>, Error>;
}

/// The highest streamRef looked up when `--max-stream-ref` is not given.
/// MEASURED (hl 0.2.0): an object-map lookup costs about a quarter of a
/// microsecond, so this ceiling costs about a quarter of a second.
pub const DEFAULT_MAX_STREAM_REF: u64 = 1_000_000;
/// Refs per fetch. A fetch is all-or-nothing under a timeout or a replayer
/// error, so this bounds what one failure costs before the refs of a failed
/// fetch are retried one at a time.
pub const CHUNK: usize = 2_000;
/// A loaded ref this close to the bound suggests the bound may be cutting
/// the capture off.
pub const BOUND_HEADROOM: u64 = 64;

/// The sweep's upper bound and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    pub max_stream_ref: u64,
    pub source: BoundSource,
}

/// Spec 3: an explicit `--max-stream-ref` wins; otherwise the built-in
/// ceiling.
pub fn bound(flag: Option<u64>) -> Bound {
    match flag {
        Some(max_stream_ref) => Bound {
            max_stream_ref,
            source: BoundSource::Flag,
        },
        None => Bound {
            max_stream_ref: DEFAULT_MAX_STREAM_REF,
            source: BoundSource::Default,
        },
    }
}

/// The ref ranges the fallback sweep (no object map) fetches for a bound.
pub fn chunks(max_stream_ref: u64) -> Vec<RangeInclusive<u64>> {
    let mut out = Vec::new();
    let mut start = 0u64;
    loop {
        let end = start.saturating_add(CHUNK as u64 - 1).min(max_stream_ref);
        out.push(start..=end);
        if end == max_stream_ref {
            break;
        }
        start = end + 1;
    }
    out
}

pub struct Sweep<T> {
    /// The command index playback reached before the first fetch.
    pub command_index: u32,
    pub fetched: Vec<Fetched<T>>,
    pub probes: Vec<StencilProbe>,
    pub duplicates: Vec<Duplicate>,
    pub failures: Vec<Failure>,
    /// Present when the object map answered; withheld on the fallback sweep.
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

/// Every loaded texture the object map holds at a ref up to the bound,
/// keyed by ref. `Err` carries the map error (spec 5 step 2).
fn walk_map<F: Fetcher>(
    f: &F,
    max_stream_ref: u64,
) -> Result<BTreeMap<u64, TextureDescriptor>, ObjectMapError> {
    let mut descs = BTreeMap::new();
    for r in 0..=max_stream_ref {
        if let Some(d) = f.texture_descriptor(r)? {
            descs.insert(r, d);
        }
    }
    Ok(descs)
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

/// Pass 1 without the object map: fetch every ref up to the bound in
/// chunks, recording each failed chunk.
fn fetch_range<F: Fetcher>(f: &F, max_stream_ref: u64) -> (Vec<F::Tex>, Vec<String>) {
    let ranges = chunks(max_stream_ref);
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for range in &ranges {
        let refs: Vec<u64> = range.clone().collect();
        match f.textures(&refs) {
            Ok(t) => out.extend(t),
            Err(e) => errors.push(format!("refs {}..={}: {e}", range.start(), range.end())),
        }
    }
    if !errors.is_empty() {
        let n = errors.len();
        errors = vec![format!(
            "pass 1 (plane 0 sweep) failed for {n} of {} chunks: {}",
            ranges.len(),
            errors.join("; ")
        )];
    }
    (out, errors)
}

pub fn run<F: Fetcher>(f: &F, bound: &Bound) -> Sweep<F::Tex> {
    let mut sweep = Sweep {
        command_index: 0,
        fetched: Vec::new(),
        probes: Vec::new(),
        duplicates: Vec::new(),
        failures: Vec::new(),
        coverage: None,
        sweep_error: None,
    };

    // Pass 1, step 1: the map names every loaded texture before playback
    // empties it (see the module doc).
    let before = walk_map(f, bound.max_stream_ref);
    sweep.command_index = f.replay();

    // Step 2: fetch those refs. Without a usable map (unreadable, or empty
    // at open), every ref up to the bound is fetched instead.
    let mut descs = before.clone().unwrap_or_default();
    let mut pass1 = match &before {
        Ok(map) if !map.is_empty() => {
            let refs: Vec<u64> = map.keys().copied().collect();
            fetch_refs(f, &refs, map, &mut sweep.failures)
        }
        Ok(_) => fetch_range(f, bound.max_stream_ref).0,
        Err(e) => {
            let (texs, errors) = fetch_range(f, bound.max_stream_ref);
            let mut parts = vec![format!(
                "the replayer's object map could not be read ({e}), so descriptors are unavailable; every streamRef 0..={} was fetched instead and no descriptor metadata was written",
                bound.max_stream_ref
            )];
            parts.extend(errors);
            sweep.sweep_error = Some(parts.join("; "));
            texs
        }
    };

    // Step 3: the map is repopulated once a real texture has been fetched.
    // Walk it again for the descriptors of the fetched state and for any
    // ref that only playback made loadable.
    if before.is_ok()
        && let Ok(after) = walk_map(f, bound.max_stream_ref)
    {
        let have: std::collections::BTreeSet<u64> = descs
            .keys()
            .copied()
            .chain(pass1.iter().map(Tex::stream_ref))
            .collect();
        let new: Vec<u64> = after
            .keys()
            .copied()
            .filter(|r| !have.contains(r))
            .collect();
        descs.extend(after);
        if !new.is_empty() {
            pass1.extend(fetch_refs(f, &new, &descs, &mut sweep.failures));
        }
    }
    let kept = dedupe(pass1, &mut sweep.duplicates, &mut sweep.failures);
    if before.is_ok() {
        sweep.coverage = Some(Coverage {
            loaded: descs.len(),
            answered: kept.len(),
            highest_stream_ref: descs.keys().next_back().copied(),
        });
    }

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
        /// What the object map holds; `Err` makes every lookup fail.
        map: RefCell<Result<BTreeMap<u64, TextureDescriptor>, ObjectMapError>>,
        /// When set, `replay()` empties the map, like playback does to the
        /// real one, and the first fetch that returns a texture restores it
        /// (plus these extra entries).
        emptied_by_replay: bool,
        extra_after_fetch: BTreeMap<u64, TextureDescriptor>,
        primed: RefCell<Option<BTreeMap<u64, TextureDescriptor>>>,
        /// What a fetch can answer, by ref. A ref listed in `refuse` fails
        /// any fetch that includes it.
        answers: Vec<FakeTex>,
        refuse: Vec<u64>,
        requested: RefCell<Vec<Vec<u64>>>,
        stencil: RefCell<Option<Result<Vec<FakeTex>, Error>>>,
    }

    fn ten() -> Bound {
        Bound {
            max_stream_ref: 10,
            source: BoundSource::Flag,
        }
    }

    impl Fake {
        fn new(answers: Vec<FakeTex>) -> Self {
            Self {
                map: RefCell::new(Ok(BTreeMap::new())),
                emptied_by_replay: false,
                extra_after_fetch: BTreeMap::new(),
                primed: RefCell::new(None),
                answers,
                refuse: Vec::new(),
                requested: RefCell::new(Vec::new()),
                stencil: RefCell::new(Some(Ok(Vec::new()))),
            }
        }
        /// Every answer is also a loaded texture in the map.
        fn mapped(self) -> Self {
            let mut map = BTreeMap::new();
            for t in &self.answers {
                map.entry(t.stream_ref)
                    .or_insert_with(|| desc(t.stream_ref, t.width, t.height, t.mtl_raw));
            }
            *self.map.borrow_mut() = Ok(map);
            self
        }
        fn without_map(self) -> Self {
            *self.map.borrow_mut() = Err(ObjectMapError::WrongClass);
            self
        }
        /// Playback empties the map until a real fetch repopulates it,
        /// as measured on the real replayer.
        fn emptied_by_replay(mut self) -> Self {
            self.emptied_by_replay = true;
            self
        }
        /// After the repopulating fetch the map also holds `d`.
        fn added_after_fetch(mut self, d: TextureDescriptor) -> Self {
            self.extra_after_fetch.insert(d.stream_ref, d);
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
        fn texture_descriptor(
            &self,
            stream_ref: u64,
        ) -> Result<Option<TextureDescriptor>, ObjectMapError> {
            match &*self.map.borrow() {
                Ok(map) => Ok(map.get(&stream_ref).copied()),
                Err(e) => Err(*e),
            }
        }
        fn replay(&self) -> u32 {
            self.requested.borrow_mut().push(vec![u64::MAX]);
            if self.emptied_by_replay {
                let before = std::mem::replace(&mut *self.map.borrow_mut(), Ok(BTreeMap::new()));
                let mut full = before.unwrap_or_default();
                full.extend(self.extra_after_fetch.clone());
                *self.primed.borrow_mut() = Some(full);
            }
            22
        }
        fn textures(&self, refs: &[u64]) -> Result<Vec<FakeTex>, Error> {
            self.requested.borrow_mut().push(refs.to_vec());
            if refs.iter().any(|r| self.refuse.contains(r)) {
                return Err(Error::Truncated);
            }
            let out: Vec<FakeTex> = self
                .answers
                .iter()
                .filter(|t| refs.contains(&t.stream_ref))
                .cloned()
                .collect();
            if !out.is_empty()
                && let Some(full) = self.primed.borrow_mut().take()
            {
                *self.map.borrow_mut() = Ok(full);
            }
            Ok(out)
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
    fn only_mapped_refs_are_fetched_and_each_carries_its_own_descriptor() {
        // Ref 7 answers a fetch but is not in the map: never asked for.
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(3, 8, 8), bgra(7, 4, 4)]).mapped();
        if let Ok(map) = &mut *f.map.borrow_mut() {
            map.remove(&7);
        }
        let s = run(&f, &ten());
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
                highest_stream_ref: Some(3)
            })
        );
    }

    #[test]
    fn refs_come_from_the_walk_before_playback_and_descriptors_from_after() {
        // Playback empties the map; the fetch of the pre-playback refs
        // repopulates it, now also holding ref 9, which is then fetched too.
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(9, 2, 2)])
            .mapped()
            .emptied_by_replay()
            .added_after_fetch(desc(9, 2, 2, 80));
        if let Ok(map) = &mut *f.map.borrow_mut() {
            map.remove(&9);
        }
        let s = run(&f, &ten());
        assert_eq!(requests(&f), vec!["replay", "[1]", "[9]"]);
        assert_eq!(refs_of(&s), vec![1, 9]);
        assert!(s.fetched.iter().all(|x| x.descriptor.is_some()));
        assert_eq!(
            s.coverage,
            Some(Coverage {
                loaded: 2,
                answered: 2,
                highest_stream_ref: Some(9)
            })
        );
    }

    #[test]
    fn an_empty_map_at_open_falls_back_to_the_range_sweep_with_descriptors() {
        // Empty before replay too: nothing to prime with; the range sweep's
        // real answers repopulate the map with both refs.
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(3, 4, 4)])
            .emptied_by_replay()
            .added_after_fetch(desc(1, 4, 4, 80))
            .added_after_fetch(desc(3, 4, 4, 80));
        let s = run(&f, &ten());
        assert_eq!(
            requests(&f),
            vec!["replay", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]"]
        );
        assert_eq!(refs_of(&s), vec![1, 3]);
        assert!(
            s.fetched.iter().all(|x| x.descriptor.is_some()),
            "the walk after the fetch still supplies descriptors"
        );
        assert!(s.sweep_error.is_none());
        assert_eq!(s.coverage.unwrap().loaded, 2);
    }

    #[test]
    fn classifies_and_probes_depth_refs_for_stencil() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2), depth(3)])
            .mapped()
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
        let f = Fake::new(vec![bgra(4, 4, 4), bgra(4, 4, 4), bgra(5, 4, 4), other]).mapped();
        let s = run(&f, &ten());
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
                highest_stream_ref: Some(5)
            })
        );
    }

    #[test]
    fn a_failed_chunk_is_retried_one_ref_at_a_time() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2), bgra(3, 4, 4)])
            .mapped()
            .refusing(2);
        let s = run(&f, &ten());
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
    fn without_the_object_map_every_ref_is_fetched_and_the_run_is_marked() {
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(2500, 4, 4), bgra(4200, 4, 4)]).without_map();
        let b = Bound {
            max_stream_ref: 4500,
            source: BoundSource::Flag,
        };
        let s = run(&f, &b);
        let asked: Vec<(u64, u64)> = f
            .requested
            .borrow()
            .iter()
            .skip(1)
            .map(|c| (c[0], c[c.len() - 1]))
            .collect();
        assert_eq!(asked, vec![(0, 1999), (2000, 3999), (4000, 4500)]);
        assert_eq!(refs_of(&s), vec![1, 2500, 4200]);
        assert!(s.fetched.iter().all(|x| x.descriptor.is_none()));
        let err = s.sweep_error.as_deref().unwrap();
        assert!(err.contains("object map could not be read"), "{err}");
        assert!(s.coverage.is_none(), "no coverage without the map");
    }

    #[test]
    fn a_failed_fallback_chunk_is_recorded_and_the_others_still_count() {
        let f = Fake::new(vec![bgra(1, 4, 4), bgra(2500, 4, 4), bgra(4200, 4, 4)])
            .without_map()
            .refusing(2000);
        let b = Bound {
            max_stream_ref: 4500,
            source: BoundSource::Flag,
        };
        let s = run(&f, &b);
        assert_eq!(refs_of(&s), vec![1, 4200]);
        let err = s.sweep_error.as_deref().unwrap();
        assert!(err.contains("1 of 3 chunks"), "{err}");
        assert!(err.contains("refs 2000..=3999"), "{err}");
    }

    #[test]
    fn pass2_error_keeps_pass1_results() {
        let f = Fake::new(vec![bgra(1, 4, 4), depth(2)])
            .mapped()
            .with_stencil(Err(Error::Truncated));
        let s = run(&f, &ten());
        assert_eq!(s.fetched.len(), 2);
        assert!(s.sweep_error.as_deref().unwrap().starts_with("pass 2"));
        assert!(s.probes.is_empty());
    }

    #[test]
    fn bound_prefers_the_flag_then_the_default() {
        assert_eq!(
            bound(Some(42)),
            Bound {
                max_stream_ref: 42,
                source: BoundSource::Flag
            }
        );
        assert_eq!(
            bound(None),
            Bound {
                max_stream_ref: DEFAULT_MAX_STREAM_REF,
                source: BoundSource::Default
            }
        );
    }

    #[test]
    fn fallback_chunks_cover_the_bound_exactly() {
        let c = CHUNK as u64;
        assert_eq!(chunks(10), vec![0..=10]);
        assert_eq!(chunks(c - 1), vec![0..=c - 1]);
        assert_eq!(chunks(c), vec![0..=c - 1, c..=c]);
        assert_eq!(chunks(4500), vec![0..=1999, 2000..=3999, 4000..=4500]);
    }

    #[test]
    fn loaded_refs_are_fetched_in_chunks() {
        let answers: Vec<FakeTex> = (0..(CHUNK as u64 + 5)).map(|r| bgra(r, 1, 1)).collect();
        let f = Fake::new(answers).mapped();
        let b = Bound {
            max_stream_ref: CHUNK as u64 + 10,
            source: BoundSource::Flag,
        };
        let s = run(&f, &b);
        let sizes: Vec<usize> = f.requested.borrow().iter().skip(1).map(Vec::len).collect();
        assert_eq!(sizes, vec![CHUNK, 5]);
        assert_eq!(s.fetched.len(), CHUNK + 5);
    }
}
