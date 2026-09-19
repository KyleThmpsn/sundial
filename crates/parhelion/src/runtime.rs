use std::path::Path;

pub(crate) mod compatibility;
pub(crate) mod swap;

use sundial::package_authoring::{
    open_shadowkeep_package_manager,
    weapon_entity::graft_weapon_component_bindings,
    weapon_runtime::{
        WeaponRuntimeEntitySource, WeaponRuntimeGraph,
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager, load_weapon_runtime_graph_for_entity,
    },
};

/// Everything that changes the donor-grafted baseline graph shown by the editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeGraphKey {
    pub pattern_index: Option<u16>,
    pub fallback_item_hash: u32,
    pub component_donors: Vec<(u32, Option<u16>, u32)>,
}

impl RuntimeGraphKey {
    pub(crate) fn new(
        pattern_index: Option<u16>,
        fallback_item_hash: u32,
        component_donors: impl IntoIterator<Item = (u32, Option<u16>, u32)>,
    ) -> Self {
        let mut component_donors = component_donors.into_iter().collect::<Vec<_>>();
        component_donors.sort_unstable();
        component_donors.dedup_by_key(|(binding_hash, _, _)| *binding_hash);
        Self {
            pattern_index,
            fallback_item_hash,
            component_donors,
        }
    }
}

/// Resolves the compiler's baseline before value and binary edits: start from the selected runtime
/// row, then atomically promote the complete owner partitions reached by selected component bindings.
pub(crate) fn load_effective_runtime_graph(
    packages: &Path,
    key: &RuntimeGraphKey,
) -> Result<WeaponRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let pattern = load_effective_runtime_entity(&manager, key)?;
    load_weapon_runtime_graph_for_entity(
        &manager,
        pattern.item_hash,
        pattern.pattern_global_id_hash,
        pattern.entity_tag,
        &pattern.payload,
    )
}

pub(crate) fn load_effective_runtime_entity(
    manager: &tiger_pkg::PackageManager,
    key: &RuntimeGraphKey,
) -> Result<WeaponRuntimeEntitySource, String> {
    let mut pattern = if let Some(pattern_index) = key.pattern_index {
        load_weapon_runtime_entity_at_pattern_index_with_manager(manager, pattern_index)?
    } else {
        load_weapon_runtime_entity_with_manager(manager, key.fallback_item_hash)?
    };
    let mut component_donors = Vec::with_capacity(key.component_donors.len());
    for &(binding_hash, donor_pattern_index, donor_item_hash) in &key.component_donors {
        let donor = if let Some(pattern_index) = donor_pattern_index {
            load_weapon_runtime_entity_at_pattern_index_with_manager(manager, pattern_index)?
        } else {
            load_weapon_runtime_entity_with_manager(manager, donor_item_hash)?
        };
        if donor.item_hash == pattern.item_hash {
            continue;
        }
        component_donors.push((binding_hash, donor_item_hash, donor));
    }
    let grafts = component_donors
        .iter()
        .map(|(binding_hash, _, donor)| (*binding_hash, donor.payload.as_slice()))
        .collect::<Vec<_>>();
    graft_weapon_component_bindings(&mut pattern.payload, &grafts).map_err(|error| {
        let donors = component_donors
            .iter()
            .map(|(binding_hash, item_hash, _)| {
                format!("0x{binding_hash:08X} from 0x{item_hash:08X}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("Could not apply runtime component donors ({donors}): {error}")
    })?;
    Ok(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::weapon_entity::WEAPON_STAT_TRANSLATOR_COMPONENT_KEY;

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
    fn mountaintop_translator_with_unmapped_events_is_rejected_before_authoring() {
        let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
        let manager = open_shadowkeep_package_manager(Path::new(&packages)).unwrap();
        let mut baseline =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 370).unwrap();
        let donor =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 285).unwrap();
        let original = baseline.payload.clone();
        // The old structural-only check allowed this cross-family translator. Its nested event
        // endpoints do not have a proven mapping, so the whole entity must now remain untouched.
        let error = graft_weapon_component_bindings(
            &mut baseline.payload,
            &[(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, &donor.payload)],
        )
        .unwrap_err();
        assert!(error.contains("event connection"), "{error}");
        assert_eq!(baseline.payload, original);
        let key = RuntimeGraphKey::new(
            Some(370),
            0x4CE3_CE93,
            [(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, Some(285), 0xEE06_B019)],
        );

        assert!(
            load_effective_runtime_graph(Path::new(&packages), &key)
                .unwrap_err()
                .contains("event connection")
        );
    }
}
