//! Carrier sound cues on translated clips.
//!
//! Source clips cue their own audio events by name and reference source sequences, which native
//! audio cannot resolve, so their records are omitted. Native first-person clips cue a weapon's
//! foley with value-2 named events (`reload_1` onward, `ready_1`) and play reload sequences and
//! Wwise events through resource records. A translated clip whose source omitted cues of these
//! kinds takes the cues of the carrier's own clip for the same action, retimed onto the source
//! clip, so the carrier's foley plays with the source motion.
//!
//! The carrier clip is the one its states give the action, with the same composition and the
//! nearest length, within a factor of two. All choices of an examined carrier reload carry the
//! same ordered cues. Cues follow the weapon mechanism: when both clips move a weapon part away
//! from its resting place in the weapon root frame and back, the frames where it leaves and
//! returns anchor a piecewise linear time map, so the magazine cues land where the source magazine
//! moves. Otherwise the map scales the clip length.
use super::native::Carrier;
use super::*;

/// When a clip's mechanism leaves its rest and returns, in frames, and its last frame.
#[derive(Clone, Copy)]
pub(super) struct Timing {
    excursion: Option<(f32, f32)>,
    last: f32,
}

/// The smallest excursion that anchors timing, and the fraction of it that counts as away.
const EXCURSION: f32 = 0.05;
const AWAY: f32 = 0.1;

/// The relative scale under which a part counts as hidden. Source clips hide parts at 0.01.
const HIDDEN: f32 = 0.5;

/// A part's excursion from its last-frame place, where a reload or action comes to rest: the
/// frame it leaves, the frame it returns and the distance. Each frame holds the part's place and
/// relative scale. A part that starts away from that place, such as one an authored pop moves on
/// the first frames, or one hidden when it starts, ends or leaves, has no visible excursion.
fn excursion(path: &[[f32; 4]]) -> Option<(usize, usize, f32)> {
    let rest = *path.last()?;
    if path[0][3] < HIDDEN || rest[3] < HIDDEN {
        return None;
    }
    let travel = path
        .iter()
        .map(|p| (0..3).map(|i| (p[i] - rest[i]).powi(2)).sum::<f32>().sqrt())
        .collect::<Vec<_>>();
    let (peak, most) =
        travel.iter().enumerate().fold(
            (0, 0f32),
            |best, (f, &t)| if t > best.1 { (f, t) } else { best },
        );
    if most < EXCURSION {
        return None;
    }
    let away = most * AWAY;
    let leaves = travel.iter().position(|&t| t > away)?;
    let returns = peak + travel[peak..].iter().position(|&t| t < away)?;
    (0 < leaves && leaves < returns && returns + 1 < travel.len() && path[leaves][3] >= HIDDEN)
        .then_some((leaves, returns, most))
}

impl Timing {
    /// Timing from each weapon part's translation and relative scale in the weapon root frame,
    /// per frame. The part with the largest visible excursion sets it.
    pub fn new(paths: &[Vec<[f32; 4]>], frames: u16) -> Self {
        let last = f32::from(frames.max(1) - 1);
        let excursion = paths
            .iter()
            .filter_map(|path| excursion(path))
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(leaves, returns, _)| (leaves as f32, returns as f32));
        Self { excursion, last }
    }

    pub fn report(&self) -> Value {
        json!({"leaves":self.excursion.map(|e| e.0),"returns":self.excursion.map(|e| e.1),"last":self.last})
    }
}

/// A frame map from carrier cue timing onto the source clip.
pub(super) fn retime(from: Timing, to: Timing) -> impl Fn(u32) -> u32 {
    let points = match (from.excursion, to.excursion) {
        (Some(a), Some(b)) => vec![(0., 0.), (a.0, b.0), (a.1, b.1), (from.last, to.last)],
        _ => vec![(0., 0.), (from.last, to.last)],
    };
    move |frame| {
        let frame = frame as f32;
        let mapped = points
            .windows(2)
            .find(|w| frame <= w[1].0)
            .map_or(to.last, |w| {
                let span = w[1].0 - w[0].0;
                if span <= 0. {
                    w[1].1
                } else {
                    w[0].1 + (frame - w[0].0) / span * (w[1].1 - w[0].1)
                }
            });
        mapped.round().clamp(0., to.last) as u32
    }
}

/// A carrier clip whose cues a translated clip takes.
pub(super) struct Donor {
    pub tag: u32,
    pub payload: std::sync::Arc<Payload>,
    pub timing: Timing,
}

/// The carrier's clips by action, with their weapon mechanism timing.
pub(super) struct Donors<'a> {
    carrier: &'a Carrier,
    rig: control::Rig,
    handle: Option<usize>,
    parts: Vec<usize>,
    timings: BTreeMap<u32, Timing>,
}

impl<'a> Donors<'a> {
    /// `root` names the weapon root bone below which weapon parts move.
    pub fn new(carrier: &'a Carrier, root: u32) -> Result<Self> {
        let rig = control::Rig::read(&carrier.skeleton.1, &carrier.controls.1)?;
        let handle = rig.names.iter().position(|n| *n == root);
        let parts = handle.map(|handle| parts(&rig, handle)).unwrap_or_default();
        Ok(Self {
            carrier,
            rig,
            handle,
            parts,
            timings: BTreeMap::new(),
        })
    }

    /// The carrier clip for a translated clip that the given actions route: same composition,
    /// carrying cues, nearest in length, then the lowest descriptor.
    pub fn choose(
        &mut self,
        native: &mut Reader,
        actions: &BTreeSet<u32>,
        frames: u16,
        composition: u8,
    ) -> Result<Option<Donor>> {
        let bank = &self.carrier.bank.1;
        let rows = bank.array(0x68, 32, Some(0x80809002))?;
        let clips = bank.array(8, 4, Some(0x80808F48))?;
        let named = self.carrier.actions()?;
        let mut descriptors = BTreeSet::new();
        for &action in actions {
            if named.contains(&action) {
                descriptors.extend(crate::tiger::animation::state::descriptors(
                    &self.carrier.states.1,
                    action,
                )?);
            }
        }
        let mut best: Option<(u16, u32, std::sync::Arc<Payload>)> = None;
        for descriptor in descriptors {
            let row = *rows
                .get(descriptor)
                .context("Carrier state names an absent descriptor")?;
            let index = bank.u16(row + 24)?;
            if index == u16::MAX {
                continue;
            }
            let tag = bank.u32(
                *clips
                    .get(usize::from(index))
                    .context("Carrier clip ordinal")?,
            )?;
            let payload = native.tag(tag, Some(0x80808F49))?;
            if payload.u8(0x181)? != composition || clip::cues(&payload)?.is_empty() {
                continue;
            }
            // A clip of a different length plays different motion, so its cues would land on
            // unrelated frames.
            let length = payload.u16(0x13C)?;
            if u32::from(length) > 2 * u32::from(frames)
                || 2 * u32::from(length) < u32::from(frames)
            {
                continue;
            }
            let distance = length.abs_diff(frames);
            if best.as_ref().is_none_or(|b| distance < b.0) {
                best = Some((distance, tag, payload));
            }
        }
        let Some((_, tag, payload)) = best else {
            return Ok(None);
        };
        let timing = match self.timings.get(&tag) {
            Some(&timing) => timing,
            None => {
                let timing = self.timing(&payload)?;
                self.timings.insert(tag, timing);
                timing
            }
        };
        Ok(Some(Donor {
            tag,
            payload,
            timing,
        }))
    }

    /// The grip relative to the utility on the first frame of the carrier's `idle`, from its
    /// lowest absolute descriptor, or `None` when the carrier rig or idle lacks them.
    pub fn idle_grip(
        &self,
        native: &mut Reader,
        utility: u32,
        grip: u32,
    ) -> Result<Option<[f32; 3]>> {
        let index = |name| self.rig.names.iter().position(|n| *n == name);
        let (Some(utility), Some(grip)) = (index(utility), index(grip)) else {
            return Ok(None);
        };
        let idle = 0x6FB7_60FF;
        if !self.carrier.actions()?.contains(&idle) {
            return Ok(None);
        }
        let bank = &self.carrier.bank.1;
        let rows = bank.array(0x68, 32, Some(0x80809002))?;
        let clips = bank.array(8, 4, Some(0x80808F48))?;
        for descriptor in crate::tiger::animation::state::descriptors(&self.carrier.states.1, idle)?
        {
            let row = *rows
                .get(descriptor)
                .context("Carrier state names an absent descriptor")?;
            let ordinal = bank.u16(row + 24)?;
            if ordinal == u16::MAX {
                continue;
            }
            let clip = native.tag(
                bank.u32(
                    *clips
                        .get(usize::from(ordinal))
                        .context("Carrier clip ordinal")?,
                )?,
                Some(0x80808F49),
            )?;
            if clip.u8(0x181)? != 0 {
                continue;
            }
            let values =
                native_clip::sample_common(&clip, &self.rig.defaults, &self.carrier.controls.1, 0)?;
            let objects = self.rig.objects(&values)?;
            let held = objects[utility].inverse().then(objects[grip]);
            return Ok(Some([held.0[4], held.0[5], held.0[6]]));
        }
        Ok(None)
    }

    /// Carrier clips play only the control groups they share with the carrier rig, as the
    /// native mixer stops at the first group that differs.
    fn timing(&self, clip: &Payload) -> Result<Timing> {
        measure(&self.rig, self.handle, &self.parts, clip, |frame| {
            native_clip::sample_common(clip, &self.rig.defaults, &self.carrier.controls.1, frame)
        })
    }
}

/// Bones below `handle` in a rig's hierarchy.
fn parts(rig: &control::Rig, handle: usize) -> Vec<usize> {
    let mut below = BTreeSet::from([handle]);
    let mut parts = Vec::new();
    for bone in handle + 1..rig.parents.len() {
        if rig.parents[bone].is_some_and(|p| below.contains(&p)) {
            below.insert(bone);
            parts.push(bone);
        }
    }
    parts
}

/// Mechanism timing of an absolute clip on a rig, from each part's place in the weapon root
/// frame. Additive clips and rigs without the weapon root have no excursion.
fn measure(
    rig: &control::Rig,
    handle: Option<usize>,
    parts: &[usize],
    clip: &Payload,
    sample: impl Fn(usize) -> Result<Vec<[f32; 8]>>,
) -> Result<Timing> {
    let frames = clip.u16(0x13C)?;
    let Some(handle) = handle.filter(|_| clip.u8(0x181).ok() == Some(0)) else {
        return Ok(Timing::new(&[], frames));
    };
    let mut paths = vec![Vec::with_capacity(usize::from(frames)); parts.len()];
    for frame in 0..usize::from(frames) {
        let objects = rig.objects(&sample(frame)?)?;
        for (path, &part) in paths.iter_mut().zip(parts) {
            let relative = objects[handle].inverse().then(objects[part]);
            path.push([relative.0[4], relative.0[5], relative.0[6], relative.0[7]]);
        }
    }
    Ok(Timing::new(&paths, frames))
}

/// Mechanism timing of an authored clip on the native rig it was authored for, which is the
/// motion the weapon geometry shows.
pub(super) fn authored(clip: &Payload, rig: &control::Rig, handle: usize) -> Result<Timing> {
    measure(rig, Some(handle), &parts(rig, handle), clip, |frame| {
        native_clip::sample(clip, &rig.defaults, frame)
    })
}
