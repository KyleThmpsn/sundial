//! Shadowkeep C9E280 reads definition +160 or variant property +E0.
use crate::tag_payload::{read_u32 as u32_at, read_u64 as u64_at};
use crate::{AuthoringResult, error::invalid, weapon::WeaponRuntimeResourcePatch};
use sundial::package_authoring::weapon_entity::weapon_component_bindings;
use tiger_pkg::{PackageManager, TagHash};
const BINDING: u32 = 0x5F0DD954;
struct Content {
    owner: Vec<u8>,
    instance: usize,
    properties: Vec<usize>,
}
fn content(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Content> {
    let bindings = weapon_component_bindings(entity, BINDING).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "HUD icon authoring requires one weapon-content component",
        ));
    };
    if binding.concrete_class != 0x80803ACB {
        return Err(invalid("Unsupported HUD weapon-content class"));
    }
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|e| invalid(e.to_string()))?;
    let instance = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("HUD instance offset overflow"))?;
    if u32_at(&owner, instance)? != binding.owner_tag || u32_at(&owner, instance + 4)? != 0x80803AC9
    {
        return Err(invalid(
            "HUD content definition must be typed within its owner",
        ));
    }
    let definition = usize::try_from(u64_at(&owner, instance + 8)?)
        .map_err(|_| invalid("HUD definition offset overflow"))?;
    let properties = crate::weapon_ammo::property_offsets(&owner, definition)?;
    Ok(Content {
        owner,
        instance,
        properties,
    })
}

/// Each native variant's +10 key selects the appearance pattern's content group.
/// Select before copying the HUD key; shared sword owners also contain other swords' icons.
pub(crate) fn inherited_key(
    manager: &PackageManager,
    entity: &[u8],
    content_group: u32,
) -> AuthoringResult<u32> {
    let content = content(manager, entity)?;
    let key = selected_key(&content.owner, &content.properties, content_group)?;
    icon_layer(manager, key)?;
    Ok(key)
}

pub(super) fn icon_layer(manager: &PackageManager, key: u32) -> AuthoringResult<Option<TagHash>> {
    // Some stock appearances (including Perfect Paradox) explicitly have no
    // silhouette override. Preserve that native value when inheriting appearance.
    if key == 0x811C9DC5 {
        return Ok(None);
    }
    let table = manager
        .read_tag(super::assets::TABLE)
        .map_err(|e| invalid(e.to_string()))?;
    let (count, _, rows, class) = crate::tag_payload::array_at(&table, 8)?;
    if class != 0x80804A59 || count > 4096 || rows.checked_add(count * 112) != Some(table.len()) {
        return Err(invalid("Unsupported ammunition HUD icon table layout"));
    }
    let row = table[rows..]
        .chunks_exact(112)
        .find(|row| row[..4] == key.to_le_bytes())
        .ok_or_else(|| {
            invalid(format!(
                "Appearance donor HUD key {key:08X} is missing from the native HUD table"
            ))
        })?;
    u32_at(row, 4).map(TagHash).map(Some)
}

fn selected_key(owner: &[u8], properties: &[usize], content_group: u32) -> AuthoringResult<u32> {
    let mut selected = None;
    for &property in &properties[1..] {
        if u32_at(owner, property + 0x10)? == content_group && selected.replace(property).is_some()
        {
            return Err(invalid(
                "Appearance donor has duplicate content-group variants",
            ));
        }
    }
    u32_at(owner, selected.unwrap_or(properties[0]) + 0xE0)
}

pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    key: u32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let Content {
        instance,
        properties,
        ..
    } = content(manager, entity)?;
    properties
        .into_iter()
        .map(|property| {
            let offset = property
                .checked_add(0xE0)
                .and_then(|v| v.checked_sub(instance))
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| invalid("HUD property offset overflow"))?;
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: BINDING,
                resource_index: 0,
                offset,
                bytes: key.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_uses_group_match_then_default_and_rejects_ambiguity() {
        let mut owner = vec![0; 0x600];
        for (property, group, key) in [(0, 1u32, 10u32), (0x200, 2, 20), (0x400, 3, 30)] {
            owner[property + 0x10..property + 0x14].copy_from_slice(&group.to_le_bytes());
            owner[property + 0xE0..property + 0xE4].copy_from_slice(&key.to_le_bytes());
        }
        let properties = [0, 0x200, 0x400];
        assert_eq!(selected_key(&owner, &properties, 3).unwrap(), 30);
        assert_eq!(selected_key(&owner, &properties, 2).unwrap(), 20);
        assert_eq!(selected_key(&owner, &properties, 99).unwrap(), 10);
        owner[0x410..0x414].copy_from_slice(&2u32.to_le_bytes());
        assert!(selected_key(&owner, &properties, 2).is_err());
        assert!(selected_key(&owner[..0x300], &properties, 3).is_err());
    }
}
