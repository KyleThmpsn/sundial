//! Resolve the installed seasonal vendor and reward packages using Sunrise's native layouts.
use std::collections::{BTreeMap, HashSet};

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    catalog::{CollectibleDef, ProgressionDefinition, ProgressionScope, UnlockDefinition},
    investment::seasonal::{self, ArtifactMod, Definition, RewardGrant},
    package_payload::{array_at, u16_at, u32_at, u64_at},
};

pub(in crate::catalog) fn scan_seasonal(
    manager: &PackageManager,
    item_hashes: &[u64],
    item_tags: &[u32],
    collectibles: &[CollectibleDef],
    flags: &[UnlockDefinition],
    values: &[UnlockDefinition],
    progressions: &[ProgressionDefinition],
) -> Result<Definition, String> {
    let power = progression(
        progressions,
        seasonal::POWER_PROGRESSION,
        seasonal::POWER_VALUE,
    )?;
    let points = progression(
        progressions,
        seasonal::POINTS_PROGRESSION,
        seasonal::EARNED_VALUE,
    )?;
    let pass = progression(
        progressions,
        seasonal::PASS_PROGRESSION,
        seasonal::PASS_VALUE,
    )?;
    let hud = progression(progressions, seasonal::HUD_PROGRESSION, seasonal::HUD_VALUE)?;
    if power.steps.first().map(|step| step.cost) != Some(0)
        || points.steps.first().map(|step| step.cost) != Some(0)
        || power
            .steps
            .iter()
            .chain(&points.steps)
            .any(|step| step.cost < 0)
        || power.steps.len() > 100
        || points.steps.len() != 12
        || pass.steps.len() != 100
        || pass.steps[0].cost != 0
        || pass.steps[1..]
            .iter()
            .any(|step| step.cost != seasonal::XP_PER_RANK)
        || hud.steps.len() != 1
        || hud.steps[0].cost != seasonal::XP_PER_RANK
        || pass.reward_items.is_empty()
    {
        return Err(
            "The installed seasonal ladders do not match the current Sunrise runtime".into(),
        );
    }
    let used = values
        .get(seasonal::USED_VALUE)
        .ok_or("The artifact points-used definition is missing")?;
    if used.bank() != 2 || used.compact_slot != Some(seasonal::USED_CHARACTER_SLOT as u16) {
        return Err("The artifact points-used definition does not match its character bank".into());
    }
    for reward in &pass.reward_items {
        if let Some(index) = reward.claim_flag {
            let flag = flags
                .get(usize::from(index))
                .ok_or("A season-pass claim flag is missing")?;
            if flag.bank() != 1 || flag.compact_slot.is_none_or(|slot| slot >= 12_300) {
                return Err("A season-pass reward does not map to a native account flag".into());
            }
        }
    }
    Ok(Definition {
        power_steps: power.steps.iter().map(|step| step.cost).collect(),
        point_steps: points.steps.iter().map(|step| step.cost).collect(),
        mods: artifact_mods(manager, collectibles, flags)?,
        reward_grants: reward_grants(manager, pass, item_hashes, item_tags)?,
    })
}

fn progression(
    definitions: &[ProgressionDefinition],
    index: usize,
    value: usize,
) -> Result<&ProgressionDefinition, String> {
    definitions
        .get(index)
        .filter(|row| {
            usize::from(row.definition_index) == index
                && row.scope == ProgressionScope::Account
                && row.level_value == Some(value as u16)
        })
        .ok_or_else(|| {
            format!("Seasonal progression #{index} does not match the current Sunrise runtime")
        })
}

fn read(manager: &PackageManager, tag: u32, class: Option<u32>) -> Result<Vec<u8>, String> {
    let tag = TagHash(tag);
    if let Some(class) = class
        && manager
            .get_entry(tag)
            .is_none_or(|entry| entry.reference != class)
    {
        return Err(format!("Seasonal package {tag} has an unexpected class"));
    }
    manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read seasonal package {tag}: {error}"))
}

fn rows(
    data: &[u8],
    descriptor: usize,
    class: u32,
    stride: usize,
) -> Result<(usize, usize), String> {
    let (count, offset, actual) = array_at(data, descriptor)?;
    if actual != class
        || count
            .checked_mul(stride)
            .and_then(|size| offset.checked_add(size))
            .is_none_or(|end| end > data.len())
    {
        return Err("A seasonal package array has an unexpected layout or extent".into());
    }
    Ok((count, offset))
}

fn artifact_mods(
    manager: &PackageManager,
    collectibles: &[CollectibleDef],
    flags: &[UnlockDefinition],
) -> Result<Vec<ArtifactMod>, String> {
    let index = read(manager, 0x8131_931D, Some(0x8080_784A))?;
    let (count, at) = rows(&index, 8, 0x8080_784E, 24)?;
    let vendor = (0..count)
        .find(|row| {
            u32_at(&index, at + row * 24).ok().map(u64::from) == Some(seasonal::ARTIFACT_VENDOR)
        })
        .ok_or("The seasonal artifact vendor is missing")?;
    let data = read(
        manager,
        u32_at(&index, at + vendor * 24 + 16)?,
        Some(0x8080_7850),
    )?;
    let (count, at) = rows(&data, 48, 0x8080_7861, 184)?;
    if count == 0 || count > 32 {
        return Err("The artifact vendor exceeds Sunrise's sale-mask capacity".into());
    }
    let mut seen = HashSet::new();
    let mut output = Vec::with_capacity(25);
    for sale_index in 0..count {
        let item_index = u16_at(&data, at + sale_index * 184 + 70)?;
        // Sunrise's granting lookup keeps the first collectible in definition order.
        let Some(collectible) = collectibles
            .iter()
            .filter(|entry| entry.item_definition_index == item_index)
            .min_by_key(|entry| entry.index)
        else {
            // The reset sale has no granting collectible and owns no mod flag.
            continue;
        };
        let category = u32_at(&data, at + sale_index * 184 + 100)?;
        if category >= 5 {
            return Err("An artifact mod belongs to an unknown display column".into());
        }
        let expression = collectible
            .conditions
            .iter()
            .find(|entry| entry.field == 4)
            .ok_or("An artifact collectible has no acquired-state expression")?;
        let [instruction] = expression.tokens.as_slice() else {
            return Err("An artifact acquired-state expression is not a single flag read".into());
        };
        if instruction.kind != 1 || instruction.operand >= u32::from(u16::MAX) {
            return Err("An artifact acquired-state expression does not name a flag".into());
        }
        let flag_definition = instruction.operand as u16;
        let flag = flags
            .get(usize::from(flag_definition))
            .ok_or("An artifact flag definition is missing")?;
        let character_slot = flag
            .compact_slot
            .filter(|&slot| flag.bank() == 3 && slot < 4096)
            .ok_or("An artifact mod does not map to a native character acquired flag")?;
        if !seen.insert(character_slot) {
            return Err("Two artifact mods share a character acquired flag".into());
        }
        output.push(ArtifactMod {
            sale_index: sale_index as u16,
            category_index: category as u8,
            item_hash: collectible.item_hash,
            collectible_hash: collectible.hash,
            flag_definition,
            character_slot,
        });
    }
    if output.len() != 25
        || (0..5).any(|column| {
            output
                .iter()
                .filter(|entry| entry.column() == column)
                .count()
                != 5
        })
    {
        return Err("The installed artifact does not contain five complete mod columns".into());
    }
    Ok(output)
}

fn reward_grants(
    manager: &PackageManager,
    pass: &ProgressionDefinition,
    item_hashes: &[u64],
    item_tags: &[u32],
) -> Result<BTreeMap<u64, RewardGrant>, String> {
    let mut output = BTreeMap::new();
    for reward in &pass.reward_items {
        if output.contains_key(&reward.item_hash) {
            continue;
        }
        let index = item_hashes
            .iter()
            .position(|&hash| hash == reward.item_hash)
            .ok_or("A season-pass reward item is missing")?;
        let tag = *item_tags
            .get(index)
            .ok_or("A season-pass item tag is missing")?;
        // Sunrise recognizes a wrapper only when its complete nonempty item set reads.
        // Short definitions and absent or malformed optional sets use its plain-item path.
        let package = read(manager, tag, None)
            .and_then(|data| reward_package(&data, item_hashes))
            .ok()
            .filter(|hashes| !hashes.is_empty());
        let grant = if let Some(hashes) = package {
            RewardGrant::ClassPackage(hashes)
        } else {
            match reward.item_hash {
                3_104_539_653 => RewardGrant::DestinationResources,
                2_223_145_359 => RewardGrant::LegendaryEngram,
                3_875_551_374 => RewardGrant::ExoticEngram,
                _ => RewardGrant::Item,
            }
        };
        output.insert(reward.item_hash, grant);
    }
    Ok(output)
}

fn reward_package(data: &[u8], item_hashes: &[u64]) -> Result<Vec<u64>, String> {
    if u64_at(data, 392)? == 0 {
        return Ok(Vec::new());
    }
    let (count, at) = rows(data, 392, 0x8080_87DB, 2)?;
    if count > 8 {
        return Err("A season-pass package exceeds Sunrise's item capacity".into());
    }
    (0..count)
        .map(|row| {
            let item = usize::from(u16_at(data, at + row * 2)?);
            item_hashes
                .get(item)
                .copied()
                .ok_or_else(|| "A season-pass package member is missing".to_owned())
        })
        .collect()
}
