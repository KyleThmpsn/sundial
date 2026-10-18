//! A level's labelled objectives: the scenario placements and map variant objects that carry a
//! Megalo label, such as `terr_object` territories, `koth_hill` hills, `ctf_flag_return` flag
//! stands, `as_goal` bomb arming points and `oddball_ball` spawns, with the team and order the
//! label carries. A gametype reads these labels to place its objectives, so a converted map's
//! objective records take their positions.
//!
//! A placement row keeps its multiplayer data behind a 32-byte label string: the label at
//! `+0x00`, game engine flags at `+0x24`, the team at `+0x25` (8 for none) and the spawn order
//! at `+0x28`. The label sits at `+0x84` in scenery and control rows, `+0x80` in crate rows,
//! `+0x78` in vehicle and weapon rows, `+0x5C` in equipment rows, `+0x8C` in machine rows and
//! `+0x58` in effect scenery rows, after each row's palette index at `+0x00`, placement flags
//! at `+0x04`, position at `+0x08` and rotation at `+0x14`, yaw first. Hang 'Em High's crates
//! read `koth_hill`, `terr_object` with orders 1 to 6, `stp_goal` and `ctf_flag_return` for
//! teams 0 to 3, `as_bomb`, `as_goal` and `oddball_ball` at `+0x80` that way.
use super::{super::cache::Cache, METRES_PER_UNIT, variant::Variant};
use anyhow::{Context, Result};

/// A scenario placement block: its rows, their size, the label's offset in a row, and the
/// palette the rows index.
struct Block {
    rows: usize,
    size: usize,
    label: usize,
    palette: usize,
}

const BLOCKS: [Block; 8] = [
    Block {
        rows: 0xFC,
        size: 0xDC,
        label: 0x84,
        palette: 0x108,
    },
    Block {
        rows: 0x12C,
        size: 0xD0,
        label: 0x78,
        palette: 0x138,
    },
    Block {
        rows: 0x144,
        size: 0xB4,
        label: 0x5C,
        palette: 0x150,
    },
    Block {
        rows: 0x15C,
        size: 0xD0,
        label: 0x78,
        palette: 0x168,
    },
    Block {
        rows: 0x180,
        size: 0xE4,
        label: 0x8C,
        palette: 0x18C,
    },
    Block {
        rows: 0x1B0,
        size: 0xDC,
        label: 0x84,
        palette: 0x1BC,
    },
    Block {
        rows: 0x1F8,
        size: 0xB0,
        label: 0x58,
        palette: 0x204,
    },
    Block {
        rows: 0x600,
        size: 0xD8,
        label: 0x80,
        palette: 0x60C,
    },
];
const PLACEMENT_FLAGS: usize = 0x04;
const PLACEMENT_POSITION: usize = 0x08;
const PLACEMENT_ROTATION: usize = 0x14;
const NEVER_PLACED: u32 = 1 << 6;
const PALETTE_SIZE: usize = 0x10;
const LABEL_LENGTH: usize = 0x20;
const LABEL_TEAM: usize = 0x25;
const LABEL_ORDER: usize = 0x28;
/// The team of a placement that belongs to no team.
const NO_TEAM: u8 = 8;

/// One labelled placement in the map's coordinates.
#[derive(Clone, Debug)]
pub struct Objective {
    /// The Megalo label, such as `terr_object`.
    pub label: String,
    /// The team the label carries, or none.
    pub team: Option<u8>,
    /// The spawn order the label carries.
    pub order: u8,
    /// Position in meters.
    pub position: [f32; 3],
    /// Facing about the vertical axis, in radians.
    pub yaw: f32,
}

impl Objective {
    fn moved(mut self, offset: [f32; 3]) -> Self {
        for (position, shift) in self.position.iter_mut().zip(offset) {
            *position += shift;
        }
        self
    }
}

/// The label at `at`, when the row carries one.
fn label(cache: &Cache, at: usize) -> Result<Option<String>> {
    let bytes = cache.bytes(at, LABEL_LENGTH)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(LABEL_LENGTH);
    if end == 0 {
        return Ok(None);
    }
    let text = std::str::from_utf8(&bytes[..end]).context("Megalo label is not UTF-8")?;
    Ok(Some(text.to_owned()))
}

/// Reads the labelled placements of `cache`'s scenario, and with a map `variant` the variant's
/// labelled objects in place of the scenario's placements of objects its palette offers, moved by
/// `offset` meters.
pub fn read(cache: &Cache, variant: Option<&Variant>, offset: [f32; 3]) -> Result<Vec<Objective>> {
    let scenario = cache.only("scnr")?.address()?;
    let mut objectives = Vec::new();
    for block in &BLOCKS {
        let palette = cache
            .block(scenario + block.palette, PALETTE_SIZE)?
            .into_iter()
            .map(|row| cache.reference(row, None))
            .collect::<Result<Vec<_>>>()?;
        for row in cache.block(scenario + block.rows, block.size)? {
            let index = cache.i16(row)?;
            if index < 0 || cache.u32(row + PLACEMENT_FLAGS)? & NEVER_PLACED != 0 {
                continue;
            }
            let Some(tag) = palette
                .get(usize::try_from(index)?)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if variant.is_some_and(|variant| variant.replaces(tag)) {
                continue;
            }
            let Some(label) = label(cache, row + block.label)? else {
                continue;
            };
            let team = cache.u8(row + block.label + LABEL_TEAM)?;
            objectives.push(
                Objective {
                    label,
                    team: (team != NO_TEAM).then_some(team),
                    order: cache.u8(row + block.label + LABEL_ORDER)?,
                    position: cache
                        .floats::<3>(row + PLACEMENT_POSITION)?
                        .map(|value| value * METRES_PER_UNIT),
                    yaw: cache.f32(row + PLACEMENT_ROTATION)?,
                }
                .moved(offset),
            );
        }
    }
    for placement in variant.map_or(&[][..], |variant| variant.placements.as_slice()) {
        let Some(label) = placement.label.clone() else {
            continue;
        };
        let [forward, ..] = placement.axes;
        objectives.push(
            Objective {
                label,
                team: placement.team,
                order: placement.spawn_sequence,
                position: placement.position,
                yaw: forward[1].atan2(forward[0]),
            }
            .moved(offset),
        );
    }
    Ok(objectives)
}
