//! Where a level's AI squads stand and who they are: the spawn points of the scenario's squads,
//! each with the character it places, which a converted map's combatant spawn points take.
//!
//! The scenario's squad block at `+0x398` holds `0x6C`-byte rows: a 32-byte name, then spawn points
//! as a block at `+0x3C` of `0x7C`-byte rows and designer and templated cells as blocks at `+0x54`
//! and `+0x60` of `0x6C`-byte rows. A spawn point holds its cell index at `+0x10`, its position in
//! world units at `+0x14`, the structure BSP it belongs to at `+0x22` (-1 for none), its facing in
//! radians at `+0x24`, yaw first, and its own character palette index at `+0x32` (-1 to take its
//! cell's). A cell lists character choices as a block at `+0x14` of 16-byte rows, each a palette
//! index at `+0x0C` and a chance at `+0x0E`. The character palette at `+0x3EC` holds a tag
//! reference per row (Assembly's ReachMCC scenario plugin, checked against `m10`, `m20`, `m30` and
//! `m35`: 208 of Nightfall's 213 points take their cell's most likely character, one its own, and
//! four have none).
use super::{super::cache::Cache, METRES_PER_UNIT, spawn::Spawn};
use anyhow::Result;

const SQUADS: usize = 0x398;
const SQUAD_SIZE: usize = 0x6C;
const SQUAD_NAME: usize = 0x00;
const SPAWN_POINTS: usize = 0x3C;
const DESIGNER_CELLS: usize = 0x54;
const TEMPLATED_CELLS: usize = 0x60;
const SPAWN_POINT_SIZE: usize = 0x7C;
const CELL_SIZE: usize = 0x6C;
const POINT_CELL: usize = 0x10;
const POINT_NAME: usize = 0x0C;
const POINT_POSITION: usize = 0x14;
const POINT_BSP: usize = 0x22;
const POINT_FACING: usize = 0x24;
const POINT_CHARACTER: usize = 0x32;
const CELL_CHARACTERS: usize = 0x14;
const CHOICE_SIZE: usize = 0x10;
const CHOICE_CHARACTER: usize = 0x0C;
const CHOICE_CHANCE: usize = 0x0E;
const CHARACTERS: usize = 0x3EC;
const CHARACTER_SIZE: usize = 0x10;

/// One squad of the level and its spawn points in the loaded structure BSPs.
#[derive(Clone, Debug)]
pub struct Squad {
    pub name: String,
    pub points: Vec<Point>,
}

/// One spawn point in the map's coordinates and the character it places: the last part of the
/// character tag's path, such as `grunt` or `elite_officer`, or none.
#[derive(Clone, Debug)]
pub struct Point {
    pub name: String,
    pub spawn: Spawn,
    pub character: Option<String>,
}

/// The scenario's squads with their spawn points in the structure BSPs of `mask` (bit `n` for the
/// scenario's `n`th), or in none, in meters moved by `offset`. Squads with no such point are left
/// out.
pub fn read_squads(cache: &Cache, offset: [f32; 3], mask: u32) -> Result<Vec<Squad>> {
    let scenario = cache.only("scnr")?.address()?;
    let characters = cache
        .block(scenario + CHARACTERS, CHARACTER_SIZE)?
        .into_iter()
        .map(|row| {
            Ok(cache
                .reference(row, Some("char"))?
                .map(|tag| tag.path.rsplit('\\').next().unwrap_or(&tag.path).to_owned()))
        })
        .collect::<Result<Vec<_>>>()?;
    let character = |index: i16| {
        usize::try_from(index)
            .ok()
            .and_then(|index| characters.get(index).cloned().flatten())
    };
    let mut squads = Vec::new();
    for squad in cache.block(scenario + SQUADS, SQUAD_SIZE)? {
        let mut cells = cache.block(squad + DESIGNER_CELLS, CELL_SIZE)?;
        cells.extend(cache.block(squad + TEMPLATED_CELLS, CELL_SIZE)?);
        // A cell's most likely character, the first among equals.
        let cell_character = |index: i16| -> Result<Option<String>> {
            let Some(&cell) = usize::try_from(index)
                .ok()
                .and_then(|index| cells.get(index))
            else {
                return Ok(None);
            };
            let mut best: Option<(i16, Option<String>)> = None;
            for choice in cache.block(cell + CELL_CHARACTERS, CHOICE_SIZE)? {
                let chance = cache.i16(choice + CHOICE_CHANCE)?;
                let found = character(cache.i16(choice + CHOICE_CHARACTER)?);
                if found.is_some() && best.as_ref().is_none_or(|(most, _)| chance > *most) {
                    best = Some((chance, found));
                }
            }
            Ok(best.and_then(|(_, found)| found))
        };
        let mut points = Vec::new();
        for row in cache.block(squad + SPAWN_POINTS, SPAWN_POINT_SIZE)? {
            let bsp = cache.i16(row + POINT_BSP)?;
            if (0..32).contains(&bsp) && mask & (1 << bsp) == 0 {
                continue;
            }
            let position = cache.floats::<3>(row + POINT_POSITION)?;
            let own = character(cache.i16(row + POINT_CHARACTER)?);
            let character = match own {
                Some(own) => Some(own),
                None => match cell_character(cache.i16(row + POINT_CELL)?)? {
                    Some(found) => Some(found),
                    None => cell_character(0)?,
                },
            };
            points.push(Point {
                name: cache.string_id(cache.u32(row + POINT_NAME)?)?,
                spawn: Spawn {
                    position: std::array::from_fn(|k| position[k] * METRES_PER_UNIT + offset[k]),
                    yaw: cache.f32(row + POINT_FACING)?,
                },
                character,
            });
        }
        if !points.is_empty() {
            squads.push(Squad {
                name: cache.string(squad + SQUAD_NAME, 32)?,
                points,
            });
        }
    }
    Ok(squads)
}
