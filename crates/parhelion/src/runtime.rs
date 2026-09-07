use std::path::Path;

use sundial::package_authoring::{
    open_shadowkeep_package_manager,
    weapon_entity::graft_weapon_component_bindings,
    weapon_runtime::{
        WeaponRuntimeGraph, load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager, load_weapon_runtime_graph_for_entity,
    },
};

/// Everything that changes the effective runtime graph shown by the editor.
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

/// Resolves the graph exactly as the compiler will see it: start from the selected runtime row,
/// then atomically promote the complete owner partitions reached by selected component bindings.
pub(crate) fn load_effective_runtime_graph(
    packages: &Path,
    key: &RuntimeGraphKey,
) -> Result<WeaponRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut pattern = if let Some(pattern_index) = key.pattern_index {
        load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern_index)?
    } else {
        load_weapon_runtime_entity_with_manager(&manager, key.fallback_item_hash)?
    };
    let mut component_donors = Vec::with_capacity(key.component_donors.len());
    for &(binding_hash, donor_pattern_index, donor_item_hash) in &key.component_donors {
        let donor = if let Some(pattern_index) = donor_pattern_index {
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern_index)?
        } else {
            load_weapon_runtime_entity_with_manager(&manager, donor_item_hash)?
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
    load_weapon_runtime_graph_for_entity(
        &manager,
        pattern.item_hash,
        pattern.pattern_global_id_hash,
        pattern.entity_tag,
        &pattern.payload,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::weapon_entity::{
        WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY,
    };
    use sundial::package_authoring::weapon_runtime::WeaponRuntimeValue;

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
    fn mountaintop_translator_structural_graft_preserves_breachlight_reload() {
        let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
        let baseline = load_effective_runtime_graph(
            Path::new(&packages),
            &RuntimeGraphKey::new(Some(370), 0x4CE3_CE93, []),
        )
        .expect("Breachlight's runtime graph should decode");
        let key = RuntimeGraphKey::new(
            Some(370),
            0x4CE3_CE93,
            [(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, Some(285), 0xEE06_B019)],
        );

        let graph = load_effective_runtime_graph(Path::new(&packages), &key)
            .expect("Mountaintop's complete translator partition should graft into Breachlight");

        let translator = graph
            .resources
            .iter()
            .find(|resource| resource.binding_hash == WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
            .expect("the grafted graph should expose Mountaintop's stat translator");
        assert_eq!(translator.owner_tag, 0x8152_8276);
        assert_eq!(translator.instance.schema, 0x80BB_B906);
        assert_eq!(translator.alias_bindings.len(), 3);
        assert!(translator.definition.as_ref().is_some_and(|definition| {
            definition.schema == 0x80BB_B907
                && definition.fields.iter().any(|field| {
                    field.path_label == "Initial Speed Scale"
                        && field.value == WeaponRuntimeValue::Float32Bits(0.5_f32.to_bits())
                })
        }));

        // The translator is the only selected owner partition. These bindings govern the
        // Breachlight firing/reload loop and must remain byte-for-byte semantically identical.
        for binding_hash in [
            WEAPON_INPUT_COMPONENT_KEY,
            WEAPON_TRIGGER_COMPONENT_KEY,
            WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
            WEAPON_BARREL_COMPONENT_KEY,
            WEAPON_CONTROLLER_COMPONENT_KEY,
            WEAPON_MAGAZINE_COMPONENT_KEY,
            WEAPON_RELOAD_COMPONENT_KEY,
        ] {
            let baseline_resources = baseline
                .resources
                .iter()
                .filter(|resource| resource.binding_hash == binding_hash)
                .collect::<Vec<_>>();
            let authored_resources = graph
                .resources
                .iter()
                .filter(|resource| resource.binding_hash == binding_hash)
                .collect::<Vec<_>>();
            assert_eq!(authored_resources, baseline_resources);
        }
    }
}
