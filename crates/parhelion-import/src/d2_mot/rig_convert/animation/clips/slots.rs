//! Track slots: which bone each clip track drives, in each engine's own order.
//!
//! The seven arrays at 0xA8..0x108 do not hold bone indices. They hold track slots, and the
//! two engines number their slots differently even over a byte-identical skeleton. Modern
//! first-person clips carry one slot per bone. Native clips carry two fewer, the forearms,
//! which the older engine drives itself, so from the first missing slot on every modern slot
//! number addresses the bone two places later in the native table. Copying the numbers
//! unchanged gave the weapon handle a finger's transform, which drops the barrel when aiming,
//! and gave a bow's arrow the thumb's.
//!
//! No asset read so far stores the slot order, so it is derived per donor from the clips
//! themselves. A name-paired modern and native clip agree on the constant pose of every slot
//! they share. One order-preserving alignment that skips exactly as many modern slots as the
//! native header lacks has to explain every constant track, or the donor keeps its native
//! clips. The alignment is kept per clip shape, because one bank mixes rigs of different sizes.
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

/// Constant rotation and translation per slot.
type Pose = BTreeMap<u16, ([f32; 4], [f32; 3])>;
/// Marker counts per slot for the modern and native clips.
type Markers = Vec<(u16, Vec<(u32, u32)>, Vec<(u32, u32)>)>;

/// The constant stream and the classes it carries in each era.
const CONSTANT: usize = 0x10;
const CONSTANT_CLASSES: [u32; 2] = [0x8080_8B40, 0x8080_8F6F];
/// Constant scale, rotation and translation slot lists, in value order.
const CONSTANT_LISTS: [usize; 3] = [0xA8, 0xB8, 0xC8];
/// Animated scale, rotation and translation slot lists, and one that is always empty.
const ANIMATED_LISTS: [usize; 4] = [0xD8, 0xE8, 0xF8, 0x108];
const SLOT_CLASS: u32 = 0x8080_000A;
/// The marker rows, as (name hash, value).
const MARKERS: usize = 0x170;
/// Bone and slot counts. The modern header is four bytes longer before them.
const MODERN_COUNTS: usize = 0x142;
const NATIVE_COUNTS: usize = 0x13E;
/// How many of a shape's native slots must be confirmed by a constant track.
const MIN_EVIDENCE: f64 = 0.9;

fn counts(clip: &Payload, at: usize) -> Result<(u16, u16)> {
    Ok((clip.u16(at)?, clip.u16(at + 2)?))
}

fn slots(clip: &Payload, list: usize) -> Result<Vec<u16>> {
    clip.array(list, 2, Some(SLOT_CLASS))?
        .into_iter()
        .map(|at| clip.u16(at))
        .collect()
}

/// The constant stream's quantized values and the ranges that decode them.
struct Constant {
    at: usize,
    counts: [usize; 3],
    params: [f32; 8],
    values: Vec<u16>,
}

impl Constant {
    fn read(clip: &Payload) -> Result<Option<Self>> {
        if clip.u64(CONSTANT)? == 0 {
            return Ok(None);
        }
        let at = clip.pointer(CONSTANT)?;
        ensure!(
            at >= 4 && CONSTANT_CLASSES.contains(&clip.u32(at - 4)?),
            "clip constant stream has an unexpected codec"
        );
        let counts = [
            usize::from(clip.u16(at + 2)?),
            usize::from(clip.u16(at + 4)?),
            usize::from(clip.u16(at + 6)?),
        ];
        let mut params = [0f32; 8];
        for (index, value) in params.iter_mut().enumerate() {
            *value = clip.f32(at + 0x14 + index * 4)?;
        }
        let values = slots(clip, at + 0x38)?;
        ensure!(
            values.len() == counts[0] + 4 * counts[1] + 3 * counts[2],
            "clip constant stream value count differs from its track counts"
        );
        for (list, count) in CONSTANT_LISTS.into_iter().zip(counts) {
            ensure!(
                usize::try_from(clip.u64(list)?)? == count,
                "clip constant slot list differs from its track count"
            );
        }
        Ok(Some(Self {
            at,
            counts,
            params,
            values,
        }))
    }

    /// Where each component of a track starts in `values`: scale, rotation, translation.
    fn offset(&self, kind: usize, track: usize) -> usize {
        match kind {
            0 => track,
            1 => self.counts[0] + 4 * track,
            _ => self.counts[0] + 4 * self.counts[1] + 3 * track,
        }
    }

    fn width(kind: usize) -> usize {
        [1, 4, 3][kind]
    }

    /// Constant rotation and translation of every slot that has both.
    fn pose(&self, clip: &Payload) -> Result<Pose> {
        let unit = |v: u16| f32::from(v) / 65535.0;
        let mut rotations = BTreeMap::new();
        for (track, slot) in slots(clip, CONSTANT_LISTS[1])?.into_iter().enumerate() {
            let at = self.offset(1, track);
            let mut q = [0f32; 4];
            for (j, value) in q.iter_mut().enumerate() {
                *value = unit(self.values[at + j]) * 2.0 - 1.0;
            }
            rotations.insert(slot, q);
        }
        let mut pose = BTreeMap::new();
        for (track, slot) in slots(clip, CONSTANT_LISTS[2])?.into_iter().enumerate() {
            let at = self.offset(2, track);
            let mut t = [0f32; 3];
            for (j, value) in t.iter_mut().enumerate() {
                *value = unit(self.values[at + j]) * self.params[2 + j] + self.params[5 + j];
            }
            if let Some(q) = rotations.get(&slot) {
                pose.insert(slot, (*q, t));
            }
        }
        Ok(pose)
    }
}

fn same(a: &([f32; 4], [f32; 3]), b: &([f32; 4], [f32; 3])) -> bool {
    let dot: f32 = a.0.iter().zip(&b.0).map(|(x, y)| x * y).sum();
    let distance: f32 =
        a.1.iter()
            .zip(&b.1)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt();
    dot.abs() > 0.9999 && distance < 1e-3
}

/// One rig size's alignment: the native slot of each modern slot, or none for a skipped one.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub native_slots: u16,
    pub map: Vec<Option<u16>>,
}

impl Shape {
    pub fn skipped(&self) -> Vec<u16> {
        self.map
            .iter()
            .enumerate()
            .filter(|(_, target)| target.is_none())
            .map(|(slot, _)| slot as u16)
            .collect()
    }
}

/// The best order-preserving alignment of `native` slots onto `modern` ones that skips
/// exactly `modern - native` modern slots, scored by how many constant tracks agree.
fn align(scores: &BTreeMap<(u16, u16), u32>, modern: u16, native: u16) -> Result<Shape> {
    ensure!(
        modern >= native,
        "native clips carry more slots than modern ones"
    );
    let (m, n) = (usize::from(modern), usize::from(native));
    let skip = m - n;
    // best[i][k]: native slots 0..i placed and k modern slots skipped so far.
    let mut best = vec![vec![None::<u64>; skip + 1]; n + 1];
    let mut back = vec![vec![(false, 0usize); skip + 1]; n + 1];
    best[0][0] = Some(0);
    for i in 0..=n {
        for k in 0..=skip {
            let Some(here) = best[i][k] else {
                continue;
            };
            if k < skip && best[i][k + 1].is_none_or(|v| here > v) {
                best[i][k + 1] = Some(here);
                back[i][k + 1] = (false, k);
            }
            if i < n {
                let gain = u64::from(
                    scores
                        .get(&(i as u16, (i + k) as u16))
                        .copied()
                        .unwrap_or(0),
                );
                if best[i + 1][k].is_none_or(|v| here + gain > v) {
                    best[i + 1][k] = Some(here + gain);
                    back[i + 1][k] = (true, k);
                }
            }
        }
    }
    let mut map = vec![None; m];
    let (mut i, mut k) = (n, skip);
    while i > 0 || k > 0 {
        let (placed, previous) = back[i][k];
        if placed {
            map[i - 1 + previous] = Some((i - 1) as u16);
            i -= 1;
        }
        k = previous;
    }
    Ok(Shape {
        native_slots: native,
        map,
    })
}

/// Every native slot's best-agreeing modern slot must be the one it is aligned to, and enough
/// native slots must be confirmed at all. Ties are allowed: identical rest poses repeat.
fn check(scores: &BTreeMap<(u16, u16), u32>, shape: &Shape) -> Result<()> {
    let placed: BTreeMap<u16, u16> = shape
        .map
        .iter()
        .enumerate()
        .filter_map(|(modern, native)| native.map(|native| (native, modern as u16)))
        .collect();
    let mut confirmed = 0usize;
    for native in 0..shape.native_slots {
        let candidates: Vec<(u16, u32)> = scores
            .iter()
            .filter(|((n, _), _)| *n == native)
            .map(|((_, m), v)| (*m, *v))
            .collect();
        let Some(top) = candidates.iter().map(|(_, v)| *v).max() else {
            continue;
        };
        confirmed += 1;
        let aligned = placed
            .get(&native)
            .and_then(|m| candidates.iter().find(|(c, _)| c == m))
            .map_or(0, |(_, v)| *v);
        ensure!(
            aligned == top,
            "native slot {native} agrees better with a modern slot the alignment did not choose"
        );
    }
    ensure!(
        confirmed as f64 >= f64::from(shape.native_slots) * MIN_EVIDENCE,
        "only {confirmed} of {} native slots are confirmed by constant tracks",
        shape.native_slots
    );
    Ok(())
}

/// A donor's slot alignment, one per rig size, plus the marker names whose values are slots.
#[derive(Debug, Default)]
pub struct Table {
    shapes: BTreeMap<u16, Shape>,
    slot_markers: BTreeSet<u32>,
}

impl Table {
    /// Derive the alignment from name-paired (modern, native) clips.
    pub fn derive<'a>(pairs: impl IntoIterator<Item = (&'a Payload, &'a Payload)>) -> Result<Self> {
        struct Evidence {
            counts: (u16, u16),
            scores: BTreeMap<(u16, u16), u32>,
            animated: BTreeSet<u16>,
        }
        let mut shapes: BTreeMap<u16, Evidence> = BTreeMap::new();
        let mut markers: Markers = Vec::new();
        for (modern, native) in pairs {
            let (bones, modern_slots) = counts(modern, MODERN_COUNTS)?;
            let (native_bones, native_slots) = counts(native, NATIVE_COUNTS)?;
            // A pair across rigs of different sizes says nothing about either rig's slots.
            // Its clip is not converted either: `apply` finds no table for it.
            if bones != native_bones {
                continue;
            }
            let evidence = shapes.entry(bones).or_insert_with(|| Evidence {
                counts: (modern_slots, native_slots),
                scores: BTreeMap::new(),
                animated: BTreeSet::new(),
            });
            ensure!(
                evidence.counts == (modern_slots, native_slots),
                "clips of one {bones}-bone rig declare different slot counts"
            );
            for list in ANIMATED_LISTS {
                evidence.animated.extend(slots(modern, list)?);
            }
            let (Some(m), Some(n)) = (Constant::read(modern)?, Constant::read(native)?) else {
                continue;
            };
            let (m_pose, n_pose) = (m.pose(modern)?, n.pose(native)?);
            for (native_slot, n_value) in &n_pose {
                for (modern_slot, m_value) in &m_pose {
                    if same(n_value, m_value) {
                        *evidence
                            .scores
                            .entry((*native_slot, *modern_slot))
                            .or_default() += 1;
                    }
                }
            }
            let rows = |clip: &Payload, class: u32| -> Result<Vec<(u32, u32)>> {
                clip.array(MARKERS, 8, Some(class))?
                    .into_iter()
                    .map(|at| -> Result<(u32, u32)> { Ok((clip.u32(at)?, clip.u32(at + 4)?)) })
                    .collect()
            };
            markers.push((
                bones,
                rows(modern, 0x8080_8C5F)?,
                rows(native, 0x8080_907F)?,
            ));
        }
        let mut table = Self::default();
        for (bones, evidence) in shapes {
            let (modern, native) = evidence.counts;
            let shape = align(&evidence.scores, modern, native)?;
            check(&evidence.scores, &shape)
                .with_context(|| format!("{bones}-bone clip slot alignment"))?;
            for skipped in shape.skipped() {
                ensure!(
                    !evidence.animated.contains(&skipped),
                    "{bones}-bone clips animate modern slot {skipped}, which native clips lack"
                );
            }
            table.shapes.insert(bones, shape);
        }
        // A marker holds a slot when, in every pair, the native value is the aligned modern
        // value and at least once that differs from the modern value. Frame markers fail this.
        let mut verdicts: BTreeMap<u32, (usize, usize, usize)> = BTreeMap::new();
        for (bones, modern, native) in &markers {
            let shape = &table.shapes[bones];
            let native: BTreeMap<u32, u32> = native.iter().copied().collect();
            for (hash, value) in modern {
                let Some(target) = native.get(hash) else {
                    continue;
                };
                let aligned = usize::try_from(*value)
                    .ok()
                    .and_then(|slot| shape.map.get(slot).copied().flatten());
                let entry = verdicts.entry(*hash).or_default();
                entry.0 += 1;
                entry.1 += usize::from(aligned.map(u32::from) == Some(*target));
                entry.2 += usize::from(value != target);
            }
        }
        table.slot_markers = verdicts
            .into_iter()
            .filter(|(_, (pairs, aligned, differ))| aligned == pairs && *differ > 0)
            .map(|(hash, _)| hash)
            .collect();
        Ok(table)
    }

    pub fn shapes(&self) -> &BTreeMap<u16, Shape> {
        &self.shapes
    }

    pub fn slot_markers(&self) -> &BTreeSet<u32> {
        &self.slot_markers
    }

    /// Rewrite a converted clip, which already has the native layout, onto native slots.
    ///
    /// The skipped slots only ever carry constant tracks, so they leave the constant stream
    /// and its slot lists. The encoded animated streams are not touched: only their slot
    /// numbers change. Returns how many tracks were renumbered.
    pub fn apply(&self, clip: &mut Payload) -> Result<usize> {
        let (bones, declared) = counts(clip, NATIVE_COUNTS)?;
        let shape = self
            .shapes
            .get(&bones)
            .with_context(|| format!("no paired clip establishes slots for a {bones}-bone rig"))?;
        self.apply_shape(clip, declared, shape)
    }

    /// Project onto a calibrated prefix rig. Only constant trailing tracks can
    /// leave the clip, and both independently derived slot maps must agree on
    /// every retained slot. Animated tracks or markers outside it are errors.
    pub fn apply_prefix(&self, clip: &mut Payload, target_bones: u16) -> Result<usize> {
        let (bones, declared) = counts(clip, NATIVE_COUNTS)?;
        if bones == target_bones {
            return self.apply(clip);
        }
        ensure!(target_bones < bones, "clip prefix cannot extend its rig");
        let source = self
            .shapes
            .get(&bones)
            .context("source rig has no calibrated slot map")?;
        let target = self
            .shapes
            .get(&target_bones)
            .context("prefix rig has no calibrated slot map")?;
        ensure!(
            source.map.len().checked_sub(target.map.len())
                == Some(usize::from(bones - target_bones))
                && source.map.starts_with(&target.map),
            "clip rig mappings are not a shared prefix"
        );
        let mut map = target.map.clone();
        map.resize(source.map.len(), None);
        let count = self.apply_shape(
            clip,
            declared,
            &Shape {
                native_slots: target.native_slots,
                map,
            },
        )?;
        clip.0[NATIVE_COUNTS..NATIVE_COUNTS + 2].copy_from_slice(&target_bones.to_le_bytes());
        Ok(count)
    }

    fn apply_shape(&self, clip: &mut Payload, declared: u16, shape: &Shape) -> Result<usize> {
        ensure!(
            usize::from(declared) == shape.map.len(),
            "clip declares {declared} slots where its rig's modern clips declare {}",
            shape.map.len()
        );
        let target = |slot: u16| -> Result<u16> {
            shape
                .map
                .get(usize::from(slot))
                .copied()
                .flatten()
                .with_context(|| {
                    format!("clip track addresses modern slot {slot}, which native clips lack")
                })
        };
        let mut renumbered = 0;
        if let Some(constant) = Constant::read(clip)? {
            let mut values = Vec::with_capacity(constant.values.len());
            let mut kept = [Vec::new(), Vec::new(), Vec::new()];
            for (kind, list) in CONSTANT_LISTS.into_iter().enumerate() {
                for (track, slot) in slots(clip, list)?.into_iter().enumerate() {
                    // A skipped slot's constant track leaves with it. A slot past the rig's
                    // table is a malformed clip, not something to drop quietly.
                    let native = *shape.map.get(usize::from(slot)).with_context(|| {
                        format!("clip track addresses slot {slot}, past its rig")
                    })?;
                    let Some(native) = native else {
                        continue;
                    };
                    let at = constant.offset(kind, track);
                    values.extend_from_slice(&constant.values[at..at + Constant::width(kind)]);
                    kept[kind].push(native);
                    renumbered += 1;
                }
            }
            for (kind, list) in CONSTANT_LISTS.into_iter().enumerate() {
                shrink(clip, list, &kept[kind])?;
                let count = u16::try_from(kept[kind].len())?;
                clip.0[constant.at + 2 + kind * 2..constant.at + 4 + kind * 2]
                    .copy_from_slice(&count.to_le_bytes());
            }
            shrink(clip, constant.at + 0x38, &values)?;
        }
        for list in ANIMATED_LISTS {
            let renamed = slots(clip, list)?
                .into_iter()
                .map(target)
                .collect::<Result<Vec<_>>>()?;
            renumbered += renamed.len();
            shrink(clip, list, &renamed)?;
        }
        clip.0[NATIVE_COUNTS + 2..NATIVE_COUNTS + 4]
            .copy_from_slice(&shape.native_slots.to_le_bytes());
        for at in clip.array(MARKERS, 8, Some(0x8080_907F))? {
            if self.slot_markers.contains(&clip.u32(at)?) {
                let slot = u16::try_from(clip.u32(at + 4)?)?;
                let native = u32::from(target(slot)?);
                clip.0[at + 4..at + 8].copy_from_slice(&native.to_le_bytes());
            }
        }
        // The constant stream must read back with the counts it now declares.
        if Constant::read(clip)?.is_some_and(|c| c.values.len() != c.offset(2, c.counts[2])) {
            anyhow::bail!("rewritten constant stream does not read back");
        }
        Ok(renumbered)
    }
}

/// Overwrite a u16 array in place with no more elements than it had. The descriptor and the
/// header both carry the count; an emptied array also loses its pointer, as native ones do.
fn shrink(clip: &mut Payload, descriptor: usize, values: &[u16]) -> Result<()> {
    let rows = clip.array(descriptor, 2, Some(SLOT_CLASS))?;
    ensure!(
        values.len() <= rows.len(),
        "a slot array can only shrink in place"
    );
    if rows.is_empty() {
        return Ok(());
    }
    let header = clip.pointer(descriptor + 8)?;
    for (index, row) in rows.iter().enumerate() {
        let value = values.get(index).copied().unwrap_or(0);
        clip.0[*row..*row + 2].copy_from_slice(&value.to_le_bytes());
    }
    let count = values.len() as u64;
    clip.0[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
    clip.0[header..header + 8].copy_from_slice(&count.to_le_bytes());
    if values.is_empty() {
        clip.0[descriptor + 8..descriptor + 16].fill(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(pairs: &[(u16, u16, u32)]) -> BTreeMap<(u16, u16), u32> {
        pairs.iter().map(|(n, m, v)| ((*n, *m), *v)).collect()
    }

    #[test]
    fn the_skipped_modern_slots_are_the_ones_no_native_slot_matches() {
        // Six modern slots, four native: modern 2 and 3 have no native counterpart, the way the
        // forearms do, and everything after them sits two higher in modern.
        let evidence = scores(&[(0, 0, 9), (1, 1, 9), (2, 4, 9), (3, 5, 9), (2, 2, 1)]);
        let shape = align(&evidence, 6, 4).unwrap();
        assert_eq!(
            shape.map,
            vec![Some(0), Some(1), None, None, Some(2), Some(3)]
        );
        assert_eq!(shape.skipped(), vec![2, 3]);
        check(&evidence, &shape).unwrap();
    }

    #[test]
    fn an_alignment_contradicted_by_the_tracks_is_refused() {
        // Native slot 2 matches modern 5 and native slot 3 matches modern 3. No order-preserving
        // alignment can place both, so whichever it keeps, the other contradicts it.
        let evidence = scores(&[(0, 0, 9), (1, 1, 9), (2, 5, 9), (3, 3, 9)]);
        let shape = align(&evidence, 6, 4).unwrap();
        assert!(check(&evidence, &shape).is_err());
    }

    #[test]
    fn too_little_evidence_is_refused() {
        let evidence = scores(&[(0, 0, 9)]);
        let shape = align(&evidence, 6, 4).unwrap();
        assert!(check(&evidence, &shape).is_err());
    }
}
