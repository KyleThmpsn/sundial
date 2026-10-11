//! Bullets per Shot as a setting of the weapon itself. The Barrel reads how many bullets one pull
//! fires from its inputs 22 to 25, and the stat translator writes them. The translation scripts
//! name the same output `bullets_per_shot` where they publish it as a channel. Where every input
//! copies the same column of one translation, and that column holds a whole number of bullets at
//! every stat tier, the column is the weapon's bullets per shot and a private translator copy
//! can hold another at every tier. Most types hold one number there, as a fusion rifle's seven
//! bolts do. A pulse rifle's, sidearm's and rocket launcher's change with Rounds Per Minute, so
//! the weapon's own value picks the tier, and a set value holds at every one. The other inputs
//! that read the column, such as the damage a fusion rifle splits over its bolts and the
//! ammunition a burst spends, follow it as they do in stock. A column that holds 0, as an
//! automatic weapon's does, is not offered.
use super::rig::{
    TRANSLATOR_KEY_CLASS, TRANSLATOR_KEY_HASH, TRANSLATOR_KEY_SIZE, TRANSLATOR_TABLE_CLASS,
    TRANSLATOR_TABLE_SIZE, translator_array,
};
use crate::tag_payload::{array_at, read_u32};
use crate::{AuthoringResult, error::invalid, item::WeaponRuntimeResourcePatch};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::{
    WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, weapon_component_bindings,
};
use sundial::package_authoring::runtime::modifiers::{BARREL_BULLETS_PER_SHOT, BARREL_COMPONENT};
use tiger_pkg::TagHash;

/// A type's translations, each a stat converted at every tier: its rows descriptor at +0x10, the
/// Weapon Stats input it converts at +0x20, and the stat values its first and last tiers stand for
/// at +0x28 and +0x2C, with the tiers evenly between.
const TRANSLATION_CLASS: u32 = 0x8080_3979;
const TRANSLATION_SIZE: usize = 0x38;
const TRANSLATION_ROWS: usize = 0x10;
const TRANSLATION_STAT: usize = 0x20;
const TRANSLATION_RANGE: usize = 0x28;
/// Weapon Stats input 0, which the investment constants map to Rounds Per Minute, and that stat's
/// definition hash. Every stock column that changes follows it.
const ROUNDS_PER_MINUTE_INPUT: u32 = 0;
pub(crate) const ROUNDS_PER_MINUTE_HASH: u32 = 0xFF66_4809;
/// One tier's outputs: a count and a pointer to that many Float32 values.
const ROW_CLASS: u32 = 0x8080_9F2A;
const ROW_SIZE: usize = 0x10;
const OUTPUTS_CLASS: u32 = 0x8080_000F;
/// A routing entry: the component at +0, its input at +4, how it combines at +0xC and, for a
/// direct copy, the output it copies at +0x1C.
const ROUTE_CLASS: u32 = 0x8080_3981;
const ROUTE_SIZE: usize = 0x50;
const ROUTE_COMPONENT: usize = 0x0;
const ROUTE_INPUT: usize = 0x4;
const ROUTE_OPERATION: usize = 0xC;
const ROUTE_SOURCE: usize = 0x1C;
/// Copies one translation output as it is.
const DIRECT: u32 = 4;
/// Translation outputs are numbered from here, 0x20 for each translation and one for each column.
const OUTPUTS: u32 = 0x1E00;
const OUTPUTS_PER_TRANSLATION: u32 = 0x20;

/// The column a weapon's bullets per shot is kept in.
pub(crate) struct Burst {
    pub(crate) column: Column,
    /// Each tier's value, as an offset into the translator owner.
    cells: Vec<usize>,
    /// The translator resource the patches are relative to.
    resource: usize,
}

/// The bullets per shot a translator gives at each stat tier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Column {
    /// Each tier's bullets, lowest stat first.
    tiers: Vec<u16>,
    /// The stat the tiers follow, by definition hash, where they differ.
    pub(crate) stat: Option<u32>,
    /// The stat values the first and last tiers stand for.
    range: [i32; 2],
}

impl Column {
    /// The bullets at every tier, where they agree.
    pub(crate) fn fixed(&self) -> Option<u16> {
        let first = *self.tiers.first()?;
        self.tiers
            .iter()
            .all(|&bullets| bullets == first)
            .then_some(first)
    }

    /// The bullets at stat `value`, from the nearest tier. Values outside the range take its end.
    pub(crate) fn at(&self, value: i32) -> u16 {
        let [low, high] = self.range;
        let last = self.tiers.len().saturating_sub(1);
        let tier = if high > low && last > 0 {
            let position = f64::from(value.clamp(low, high) - low) / f64::from(high - low);
            (position * last as f64).round() as usize
        } else {
            0
        };
        self.tiers.get(tier.min(last)).copied().unwrap_or(1)
    }
}

/// The bullets per shot `entity` fires, converted by the table for translation `group`, when its
/// translator keeps them in one column. None for a weapon whose burst comes from elsewhere, holds
/// 0, or follows a stat other than Rounds Per Minute.
pub(crate) fn read(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
) -> AuthoringResult<Option<Burst>> {
    let bindings =
        weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Ok(None);
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Stat translator offset overflow"))?;
    Ok(column(&owner, group)?.map(|(column, cells)| Burst {
        column,
        cells,
        resource,
    }))
}

/// Patches that make the translator give the Barrel `bullets` bullets per shot, in the private
/// translator copy the build makes. None for a weapon without the column, which does not offer the
/// setting, so a value saved on another Runtime stays inert until one with the column returns.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
    bullets: u16,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let Some(burst) = read(manager, entity, group)? else {
        return Ok(Vec::new());
    };
    let bytes = f32::from(bullets).to_le_bytes().to_vec();
    burst
        .cells
        .iter()
        .map(|&cell| {
            let offset = cell
                .checked_sub(burst.resource)
                .and_then(|offset| u32::try_from(offset).ok())
                .ok_or_else(|| invalid("Stat translator patch offset overflow"))?;
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset,
                bytes: bytes.clone(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            })
        })
        .collect()
}

/// The bullets and the cells of the column every Barrel bullet input copies, in the table for
/// `group`, or the first table when the translator has none for it, as the client falls back.
fn column(owner: &[u8], group: u32) -> AuthoringResult<Option<(Column, Vec<usize>)>> {
    let keys = translator_array(owner, TRANSLATOR_KEY_CLASS)?;
    let tables = translator_array(owner, TRANSLATOR_TABLE_CLASS)?;
    if keys.0 != tables.0 {
        return Ok(None);
    }
    let mut index = 0;
    for key in 0..keys.0 {
        if read_u32(
            owner,
            keys.1 + key * TRANSLATOR_KEY_SIZE + TRANSLATOR_KEY_HASH,
        )? == group
        {
            index = key;
            break;
        }
    }
    let table = tables.1 + index * TRANSLATOR_TABLE_SIZE;
    let (translations, _, translation_rows, translation_class) = array_at(owner, table)?;
    let (routes, _, route_rows, route_class) = array_at(owner, table + 0x10)?;
    if translation_class != TRANSLATION_CLASS || route_class != ROUTE_CLASS {
        return Ok(None);
    }
    let mut source = None;
    for input in BARREL_BULLETS_PER_SHOT {
        let mut found = None;
        for route in (0..routes).map(|index| route_rows + index * ROUTE_SIZE) {
            if i64::from(read_u32(owner, route + ROUTE_COMPONENT)?) == BARREL_COMPONENT
                && i64::from(read_u32(owner, route + ROUTE_INPUT)?) == input
            {
                if found.is_some() {
                    return Ok(None);
                }
                found = Some(route);
            }
        }
        let Some(route) = found else {
            return Ok(None);
        };
        let own = read_u32(owner, route + ROUTE_SOURCE)?;
        if read_u32(owner, route + ROUTE_OPERATION)? != DIRECT || source.is_some_and(|s| s != own) {
            return Ok(None);
        }
        source = Some(own);
    }
    let Some(output) = source.and_then(|source| source.checked_sub(OUTPUTS)) else {
        return Ok(None);
    };
    let translation = (output / OUTPUTS_PER_TRANSLATION) as usize;
    let column = (output % OUTPUTS_PER_TRANSLATION) as usize;
    if translation >= translations {
        return Ok(None);
    }
    let translation = translation_rows + translation * TRANSLATION_SIZE;
    let (tiers, _, tier_rows, tier_class) = array_at(owner, translation + TRANSLATION_ROWS)?;
    if tier_class != ROW_CLASS || tiers == 0 {
        return Ok(None);
    }
    let mut cells = Vec::with_capacity(tiers);
    let mut bullets = Vec::with_capacity(tiers);
    for tier in (0..tiers).map(|index| tier_rows + index * ROW_SIZE) {
        let (count, _, outputs, class) = array_at(owner, tier)?;
        if class != OUTPUTS_CLASS || column >= count {
            return Ok(None);
        }
        let cell = outputs + column * 4;
        let value = f32::from_bits(read_u32(owner, cell)?);
        if value.fract() != 0.0
            || !(1.0..=f32::from(crate::weapon::barrel::MAX_BULLETS_PER_SHOT)).contains(&value)
        {
            return Ok(None);
        }
        bullets.push(value as u16);
        cells.push(cell);
    }
    let bound = |at: usize| -> AuthoringResult<Option<i32>> {
        let value = f32::from_bits(read_u32(owner, translation + TRANSLATION_RANGE + at)?);
        Ok((value.fract() == 0.0 && value.abs() <= 1.0e6).then_some(value as i32))
    };
    let (Some(low), Some(high)) = (bound(0)?, bound(4)?) else {
        return Ok(None);
    };
    let mut column = Column {
        tiers: bullets,
        stat: None,
        range: [low, high],
    };
    if column.fixed().is_none() {
        // Only a stat the weapon's own stats name can pick the tier the editor shows.
        if read_u32(owner, translation + TRANSLATION_STAT)? != ROUNDS_PER_MINUTE_INPUT {
            return Ok(None);
        }
        column.stat = Some(ROUNDS_PER_MINUTE_HASH);
    }
    Ok(Some((column, cells)))
}
