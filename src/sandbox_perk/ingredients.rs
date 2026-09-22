//! Perk indices supplied by native subclass ability definitions.
use crate::package_payload::{native_array_at, u16_at, u32_at, u64_at};
use crate::package_runtime::reader::PackageManager;
use std::collections::BTreeSet;
use tiger_pkg::TagHash;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AbilitySource {
    pub list: u16,
    pub entry: u16,
    pub definition: u32,
    pub perk_index: u16,
}

fn rows(
    bytes: &[u8],
    at: usize,
    class: u32,
    stride: usize,
) -> Result<std::ops::Range<usize>, String> {
    if u64_at(bytes, at)? == 0 {
        return Ok(0..0);
    }
    let (count, _, start, found) = native_array_at(bytes, at)?;
    let end = start
        .checked_add(
            count
                .checked_mul(stride)
                .ok_or("Ability array size overflow.")?,
        )
        .ok_or("Ability array end overflow.")?;
    if found != class || count > 4096 || end > bytes.len() {
        return Err(format!("Invalid ability array of class 0x{class:08X}."));
    }
    Ok(start..end)
}

/// A definition owns modifier groups whose perk lists use 16-bit finished indices.
pub fn definition_perks(bytes: &[u8]) -> Result<Vec<u16>, String> {
    let mut result = BTreeSet::new();
    for group in rows(bytes, 8, 0x80807A8D, 144)?.step_by(144) {
        for modifier in rows(bytes, group, 0x80807A97, 88)?.step_by(88) {
            for perk in rows(bytes, modifier + 32, 0x80807C5F, 2)?.step_by(2) {
                result.insert(u16_at(bytes, perk)?);
            }
        }
    }
    Ok(result.into_iter().collect())
}

pub fn abilities(manager: &PackageManager) -> Result<Vec<AbilitySource>, String> {
    let globals = manager
        .read_tag(crate::package_runtime::resolve_live_named_tag(
            manager,
            "investment_globals",
            None,
        )?)
        .map_err(|e| e.to_string())?;
    let root = manager
        .read_tag(TagHash(u32_at(&globals, 16)?))
        .map_err(|e| e.to_string())?;
    let table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 97 * 16)?))
        .map_err(|e| e.to_string())?;
    let (count, _, start, _) = native_array_at(&table, 8)?;
    if count > 4096
        || start
            .checked_add(count * 24)
            .is_none_or(|end| end > table.len())
    {
        return Err("Invalid ability list table.".into());
    }
    let mut result = BTreeSet::new();
    for list in 0..count {
        let tag = TagHash(u32_at(&table, start + list * 24 + 16)?);
        // Ordinary weapon and armor socket lists share this index table.
        if manager
            .get_entry(tag)
            .is_none_or(|entry| entry.reference != 0x80807A80)
        {
            continue;
        }
        let bytes = manager.read_tag(tag).map_err(|e| e.to_string())?;
        for (entry, at) in rows(&bytes, 16, 0x80807A86, 64)?.step_by(64).enumerate() {
            let definition = u32_at(&bytes, at + 56)?;
            let tag = TagHash(definition);
            if manager
                .get_entry(tag)
                .is_none_or(|entry| entry.reference != 0x80807A8B)
            {
                return Err(format!(
                    "Ability list {list}, entry {entry} has an invalid definition."
                ));
            }
            let definition_bytes = manager.read_tag(tag).map_err(|e| e.to_string())?;
            for perk_index in definition_perks(&definition_bytes)? {
                result.insert(AbilitySource {
                    list: list as u16,
                    entry: entry as u16,
                    definition,
                    perk_index,
                });
            }
        }
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn native_ability_ingredients_follow_modifier_perk_lists() {
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = crate::package_authoring::open_shadowkeep_package_manager(&path).unwrap();
        let sources = abilities(&manager).unwrap();
        let unique = sources
            .iter()
            .map(|source| source.perk_index)
            .collect::<BTreeSet<_>>();
        assert!(unique.len() >= 134);
        for index in [88, 89, 90, 535, 536, 537] {
            assert!(unique.contains(&index));
        }
        assert!(
            sources
                .iter()
                .any(|source| source.list == 7 && source.entry == 12 && source.perk_index == 90)
        );
        println!(
            "{} ability ingredient references, {} distinct perks",
            sources.len(),
            unique.len()
        );
    }
}
