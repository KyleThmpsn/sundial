use std::path::Path;

pub(crate) mod compatibility;
pub(crate) mod swap;

use sundial::package_authoring::{
    entity::graft_weapon_component_bindings_or_rewire,
    open_shadowkeep_package_manager,
    runtime::{
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
    /// The appearance whose rig and animations the build would move onto this runtime, as
    /// its pattern row and item hash. Set only for an appearance from another weapon family;
    /// the graft is still attempted and may be refused, exactly as the build attempts it.
    pub appearance_rig: Option<(Option<u16>, u32)>,
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
            appearance_rig: None,
        }
    }

    pub(crate) fn with_appearance_rig(mut self, appearance: Option<(Option<u16>, u32)>) -> Self {
        self.appearance_rig = appearance;
        self
    }
}

/// Resolves the compiler's baseline before value and binary edits: start from the selected runtime
/// row, then atomically promote the complete owner partitions reached by selected component bindings.
/// The graph, and whether it carries an appearance's rig and animations rather than its
/// own. The flag is what the editor shows; the graph already reflects the swap either way.
pub(crate) fn load_effective_runtime_graph(
    packages: &Path,
    key: &RuntimeGraphKey,
) -> Result<(WeaponRuntimeGraph, bool), String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let (pattern, carried) = load_effective_runtime_entity_reporting(&manager, key)?;
    let graph = load_weapon_runtime_graph_for_entity(
        &manager,
        pattern.item_hash,
        pattern.pattern_global_id_hash,
        pattern.entity_tag,
        &pattern.payload,
    )?;
    Ok((graph, carried))
}

pub(crate) fn load_effective_runtime_entity(
    manager: &sundial::package_authoring::PackageManager,
    key: &RuntimeGraphKey,
) -> Result<WeaponRuntimeEntitySource, String> {
    load_effective_runtime_entity_reporting(manager, key).map(|(pattern, _)| pattern)
}

pub(crate) fn load_effective_runtime_entity_reporting(
    manager: &sundial::package_authoring::PackageManager,
    key: &RuntimeGraphKey,
) -> Result<(WeaponRuntimeEntitySource, bool), String> {
    let mut pattern = if let Some(pattern_index) = key.pattern_index {
        load_weapon_runtime_entity_at_pattern_index_with_manager(manager, pattern_index)?
    } else {
        load_weapon_runtime_entity_with_manager(manager, key.fallback_item_hash)?
    };
    // Before the component donors, matching the order the package compiler uses so the
    // editor and the build agree about which owner every binding resolves to.
    let mut carried = false;
    if let Some((appearance_pattern, appearance_item)) = key.appearance_rig {
        let appearance = if let Some(pattern_index) = appearance_pattern {
            load_weapon_runtime_entity_at_pattern_index_with_manager(manager, pattern_index)
        } else {
            load_weapon_runtime_entity_with_manager(manager, appearance_item)
        };
        // A refusal is not an error: the build falls back to pinning the appearance's parts,
        // which leaves this entity exactly as it is.
        if let Ok(appearance) = appearance {
            carried = appearance.item_hash != pattern.item_hash
                && crate::weapon::rig::graft_presentation(
                    &mut pattern.payload,
                    &appearance.payload,
                )
                .is_ok();
        }
    }
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
    graft_weapon_component_bindings_or_rewire(&mut pattern.payload, &grafts, &|tag| {
        manager.read_tag(tag)
    })
    .map_err(|error| {
        let donors = component_donors
            .iter()
            .map(|(binding_hash, item_hash, _)| {
                format!("0x{binding_hash:08X} from 0x{item_hash:08X}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("Could not apply runtime component donors ({donors}): {error}")
    })?;
    Ok((pattern, carried))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::entity::{
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, graft_weapon_component_bindings,
        weapon_component_bindings,
    };

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
    fn mountaintop_translator_is_rewired_where_pairing_refuses_it() {
        let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
        let manager = open_shadowkeep_package_manager(Path::new(&packages)).unwrap();
        let mut baseline =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 370).unwrap();
        let donor =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 285).unwrap();
        let original = baseline.payload.clone();
        // Its nested event endpoints have no pair in this entity, so pairing leaves the entity
        // untouched. Only the rewire can connect this cross-family translator.
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
        // The editor shows the same rewired entity the compiler builds.
        let authored = load_effective_runtime_entity(&manager, &key).unwrap();
        let owner = |entity: &[u8]| {
            weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).unwrap()[0]
                .owner_tag
        };
        assert_eq!(owner(&authored.payload), owner(&donor.payload));
        load_effective_runtime_graph(Path::new(&packages), &key).unwrap();
    }
}
