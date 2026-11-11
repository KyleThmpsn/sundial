//! Checked native rider and locomotion rows with private descriptor substitutions.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub enum Kind {
    Rider,
    Stance,
}

/// A locomotion row's movement directions. Each `8080337D` entry holds its unit direction at
/// `+3C` and its speed tiers in the `8080337F` array at `+28`.
pub const GAITS: usize = 48;
/// The locomotion table's shared clip list. Turning and movement entries name a row of it.
const CLIP_LIST: usize = 8;
const CLIP_ROW: (usize, u32) = (24, 0x8080338D);
const DIRECTION: (usize, u32) = (72, 0x8080337D);
const TIER: (usize, u32) = (16, 0x8080337F);
const TIERS: usize = 40;
/// Array descriptors inside a direction entry, relocated when the entry is copied.
const DIRECTION_ARRAYS: [usize; 3] = [0, 16, TIERS];

impl Kind {
    fn layout(self) -> (usize, usize, u32, &'static [usize]) {
        match self {
            Self::Rider => (8, 48, 0x8080321E, &[0, 16, 32]),
            Self::Stance => (24, 72, 0x80803378, &[0, 32, 48]),
        }
    }

    fn field(self, field: usize) -> Result<(usize, u32)> {
        match (self, field) {
            (Self::Rider, 0 | 32) => Ok((48, 0x80803221)),
            (Self::Rider, 16) => Ok((8, 0x80803222)),
            (Self::Stance, 0) => Ok((12, 0x8080338C)),
            _ => anyhow::bail!("Unsupported character motion table field"),
        }
    }
}

/// One speed tier of a locomotion direction: the clip it plays, the planar direction it moves
/// in, its reference speed in metres per second and its clip list duration in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Gait {
    pub descriptor: usize,
    pub direction: [f32; 2],
    pub speed: f32,
    pub duration: f32,
}

fn row(p: &Payload, kind: Kind, choice: usize) -> Result<usize> {
    let (at, stride, class, _) = kind.layout();
    p.array(at, stride, Some(class))?
        .get(choice)
        .copied()
        .context("Character motion choice is outside its table")
}

/// The speed tiers of a locomotion row, in direction then tier order.
pub fn gaits(p: &Payload, choice: usize) -> Result<Vec<Gait>> {
    let row = row(p, Kind::Stance, choice)?;
    let clips = p.array(CLIP_LIST, CLIP_ROW.0, Some(CLIP_ROW.1))?;
    let mut result = Vec::new();
    for entry in p.array(row + GAITS, DIRECTION.0, Some(DIRECTION.1))? {
        let direction = [p.f32(entry + 0x3C)?, p.f32(entry + 0x40)?];
        ensure!(
            direction.iter().all(|v| v.is_finite())
                && (direction[0].hypot(direction[1]) - 1.).abs() < 1e-3,
            "Locomotion direction is not a unit vector"
        );
        for tier in p.array(entry + TIERS, TIER.0, Some(TIER.1))? {
            let clip = *clips
                .get(p.u32(tier)? as usize)
                .context("Locomotion tier names a clip outside its list")?;
            let descriptor = p.u32(clip)?;
            let speed = p.f32(tier + 4)?;
            let duration = p.f32(clip + 4)?;
            ensure!(
                descriptor != u32::MAX
                    && speed.is_finite()
                    && speed >= 0.
                    && duration.is_finite()
                    && duration > 0.,
                "Locomotion tier is invalid"
            );
            result.push(Gait {
                descriptor: descriptor as usize,
                direction,
                speed,
                duration,
            });
        }
    }
    Ok(result)
}

pub fn descriptors(
    p: &Payload,
    kind: Kind,
    choice: usize,
    field: usize,
) -> Result<BTreeSet<usize>> {
    if matches!(kind, Kind::Stance) && field == GAITS {
        let result = gaits(p, choice)?
            .into_iter()
            .map(|gait| gait.descriptor)
            .collect::<BTreeSet<_>>();
        ensure!(!result.is_empty(), "Locomotion row has no speed tiers");
        return Ok(result);
    }
    let row = row(p, kind, choice)?;
    let (stride, class) = kind.field(field)?;
    let result = p
        .array(row + field, stride, Some(class))?
        .into_iter()
        .map(|row| {
            let descriptor = p.u32(row)?;
            ensure!(
                descriptor != u32::MAX,
                "Character motion table has an empty descriptor"
            );
            Ok(descriptor as usize)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !result.is_empty(),
        "Character motion field has no descriptors"
    );
    Ok(result)
}

/// Point a copied record's arrays at the original record's contents.
fn relocate(
    output: &mut Payload,
    p: &Payload,
    old: usize,
    new: usize,
    fields: &[usize],
) -> Result<()> {
    for &field in fields {
        if p.u64(old + field)? != 0 {
            let target = p.pointer(old + field + 8)?;
            put(
                &mut output.0,
                new + field + 8,
                &(target as i64 - new as i64 - field as i64 - 8).to_le_bytes(),
            )?;
        }
    }
    Ok(())
}

/// Append private rows and copy only the selected clip lists. Other relative arrays
/// continue to point at their existing contents, with their pointer bases relocated.
/// Speed tiers name rows of the shared clip list, so a translated tier gains its own
/// clip list row, a copy of the original with the private descriptor.
pub fn extend(
    p: &Payload,
    kind: Kind,
    routes: &BTreeMap<usize, BTreeMap<usize, BTreeMap<usize, usize>>>,
) -> Result<(Payload, BTreeMap<usize, usize>)> {
    ensure!(
        !routes.is_empty(),
        "Character motion table has no private rows"
    );
    let (field, stride, class, pointers) = kind.layout();
    let originals = p.array(field, stride, Some(class))?;
    let mut sources = originals.clone();
    let mut choices = BTreeMap::new();
    for &choice in routes.keys() {
        choices.insert(choice, sources.len());
        sources.push(
            *originals
                .get(choice)
                .context("Character motion choice is outside its table")?,
        );
    }
    let bytes = sources
        .iter()
        .flat_map(|&row| p.0[row..row + stride].iter().copied())
        .collect::<Vec<_>>();
    let mut output = p.clone();
    append(&mut output.0, field, u64::from(class), &bytes, stride)?;
    let rows = output.array(field, stride, Some(class))?;
    for (&old, &new) in sources.iter().zip(&rows) {
        relocate(&mut output, p, old, new, pointers)?;
    }
    let clips = if matches!(kind, Kind::Stance) {
        p.array(CLIP_LIST, CLIP_ROW.0, Some(CLIP_ROW.1))?
    } else {
        Vec::new()
    };
    let mut added = Vec::<u8>::new();
    let mut added_rows = BTreeMap::<(usize, usize), usize>::new();
    for (&choice, fields) in routes {
        let old = originals[choice];
        let new = rows[choices[&choice]];
        for (&field, mapping) in fields {
            if matches!(kind, Kind::Stance) && field == GAITS {
                let entries = p.array(old + GAITS, DIRECTION.0, Some(DIRECTION.1))?;
                let bytes = entries
                    .iter()
                    .flat_map(|&entry| p.0[entry..entry + DIRECTION.0].iter().copied())
                    .collect::<Vec<_>>();
                append(
                    &mut output.0,
                    new + GAITS,
                    u64::from(DIRECTION.1),
                    &bytes,
                    DIRECTION.0,
                )?;
                let copied = output.array(new + GAITS, DIRECTION.0, Some(DIRECTION.1))?;
                for (&old_entry, &new_entry) in entries.iter().zip(&copied) {
                    relocate(&mut output, p, old_entry, new_entry, &DIRECTION_ARRAYS)?;
                    let mut tiers = Vec::new();
                    for tier in p.array(old_entry + TIERS, TIER.0, Some(TIER.1))? {
                        let index = p.u32(tier)? as usize;
                        let clip = *clips
                            .get(index)
                            .context("Locomotion tier names a clip outside its list")?;
                        let descriptor = p.u32(clip)? as usize;
                        let replacement = *mapping
                            .get(&descriptor)
                            .context("Locomotion tier has an untranslated descriptor")?;
                        let next = clips.len() + added.len() / CLIP_ROW.0;
                        let row = *added_rows.entry((index, replacement)).or_insert(next);
                        if row == next {
                            added.extend(u32::try_from(replacement)?.to_le_bytes());
                            added.extend(&p.0[clip + 4..clip + CLIP_ROW.0]);
                        }
                        tiers.extend(u32::try_from(row)?.to_le_bytes());
                        tiers.extend(&p.0[tier + 4..tier + TIER.0]);
                    }
                    append(
                        &mut output.0,
                        new_entry + TIERS,
                        u64::from(TIER.1),
                        &tiers,
                        TIER.0,
                    )?;
                }
                continue;
            }
            let (stride, class) = kind.field(field)?;
            let mut bytes = Vec::new();
            for row in p.array(old + field, stride, Some(class))? {
                let descriptor = p.u32(row)? as usize;
                let replacement = *mapping
                    .get(&descriptor)
                    .context("Character motion field has an untranslated descriptor")?;
                bytes.extend(u32::try_from(replacement)?.to_le_bytes());
                bytes.extend(&p.0[row + 4..row + stride]);
            }
            append(&mut output.0, new + field, u64::from(class), &bytes, stride)?;
        }
    }
    if !added.is_empty() {
        let mut list = clips
            .iter()
            .flat_map(|&clip| p.0[clip..clip + CLIP_ROW.0].iter().copied())
            .collect::<Vec<_>>();
        list.extend(added);
        append(
            &mut output.0,
            CLIP_LIST,
            u64::from(CLIP_ROW.1),
            &list,
            CLIP_ROW.0,
        )?;
    }
    let length = output.0.len() as u64;
    put(&mut output.0, 0, &length.to_le_bytes())?;
    Ok((output, choices))
}
