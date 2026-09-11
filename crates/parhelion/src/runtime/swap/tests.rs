use super::*;
use sundial::investment::{InvestmentCatalog, WeaponAmmoType, WeaponDamageProfile, WeaponRarity};
use sundial::package_authoring::weapon_entity::{
    WEAPON_RELOAD_COMPONENT_KEY, WEAPON_TRIGGER_COMPONENT_KEY,
};

fn donor(hash: u32) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash,
        name: format!("Donor {hash}"),
        type_name: "Sidearm".into(),
        bucket_hash: 1,
        collection_backed: true,
        power_cap: None,
        damage_type: None,
        inventory_slot: None,
        ammo_type: Some(WeaponAmmoType::Primary),
        weapon_pattern_index: Some(hash as u16),
        weapon_translation_group: Some(1),
        stat_group_index: None,
        damage_profile: WeaponDamageProfile::Unknown,
        rarity: WeaponRarity::Legendary,
    }
}

#[test]
fn group_selection_replaces_conflicting_choices_and_preserves_independent_edits() {
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.group-test", 1, "Donor 1").unwrap();
    let mut key = RuntimeGraphKey::new(
        Some(1),
        1,
        [(10, Some(2), 2), (20, Some(3), 3), (30, Some(4), 4)],
    );
    for (binding, _, hash) in &key.component_donors {
        recipe.set_runtime_component_donor(
            *binding,
            Some(WeaponDonorReference {
                item_hash: (*hash).into(),
                expected_name: None,
            }),
        );
    }
    let independent = recipe.runtime_component_donor(30).cloned();
    replace_group(&mut recipe, &mut key, &[10, 20], &donor(5), false);
    assert_eq!(
        key.component_donors,
        vec![(10, Some(5), 5), (20, Some(5), 5), (30, Some(4), 4)]
    );
    assert_eq!(recipe.runtime_component_donor(30), independent.as_ref());
    for binding in [10, 20] {
        assert_eq!(
            recipe
                .runtime_component_donor(binding)
                .unwrap()
                .item_hash
                .parse_u32()
                .unwrap(),
            5
        );
    }
    replace_group(&mut recipe, &mut key, &[10, 20], &donor(1), true);
    assert_eq!(key.component_donors, vec![(30, Some(4), 4)]);
    assert!(recipe.runtime_component_donor(10).is_none());
    assert!(recipe.runtime_component_donor(20).is_none());
    assert_eq!(recipe.runtime_component_donor(30), independent.as_ref());
}

fn stock() -> (std::path::PathBuf, Vec<WeaponDonorSummary>) {
    let packages =
        std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let cache = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &cache.path().join("catalog.json"),
        false,
        |_| {},
    )
    .unwrap();
    let donors = catalog.weapon_donors();
    (packages, donors)
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn native_group_swap_repairs_conflicts_and_previews_settings_without_mutating_the_recipe() {
    let (packages, donors) = stock();
    let baseline = donors
        .iter()
        .find(|donor| donor.hash == 0x4CE3_CE93)
        .unwrap();
    let candidates = donors
        .iter()
        .filter(|donor| {
            donor.type_name == baseline.type_name
                && donor.ammo_type == baseline.ammo_type
                && donor.weapon_translation_group == baseline.weapon_translation_group
        })
        .cloned()
        .collect::<Vec<_>>();
    let key = RuntimeGraphKey::new(baseline.weapon_pattern_index, baseline.hash, []);
    let report = crate::runtime::compatibility::assess_component_donors(
        &packages,
        &key,
        WEAPON_TRIGGER_COMPONENT_KEY,
        &candidates,
    )
    .unwrap();
    let donor = candidates
        .iter()
        .find(|donor| {
            donor.weapon_pattern_index != baseline.weapon_pattern_index
                && report.candidates[&donor.hash].status
                    != crate::runtime::compatibility::DonorCompatibility::Incompatible
        })
        .expect("a native sidearm should have a transferable component group");
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.native-group-swap",
        baseline.hash,
        &baseline.name,
    )
    .unwrap();
    recipe.overrides.weapon_pattern_index = baseline.weapon_pattern_index;
    let graph = load_effective_runtime_graph(&packages, &key).unwrap();
    let field = graph
        .fields()
        .find(|field| {
            report
                .affected_bindings
                .iter()
                .any(|(binding, _)| *binding == field.locator.binding_hash)
                && field.kind == WeaponRuntimeValueKind::Float32
                && field.locator.is_buildable()
        })
        .unwrap();
    recipe
        .overrides
        .runtime_values
        .push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: field.value.clone(),
        });
    let mut stale = recipe.overrides.runtime_values[0].clone();
    stale.locator.root_schema = 0x8080_FFFF;
    recipe.overrides.runtime_values.push(stale);
    let before = recipe.clone();
    let plan = preview(
        &packages,
        &recipe,
        &key,
        WEAPON_TRIGGER_COMPONENT_KEY,
        donor,
        &donors,
    )
    .unwrap();
    assert_eq!(recipe, before);
    assert!(plan.error.is_none(), "{:?}", plan.error);
    assert!(
        plan.kept + plan.transferred.len() > 0,
        "the supported scalar setting should survive this native swap"
    );
    assert!(
        !plan.resets.is_empty(),
        "stale settings must be disclosed before resetting"
    );
    assert!(plan.group.contains(&WEAPON_TRIGGER_COMPONENT_KEY));
    assert!(plan.group.contains(&WEAPON_RELOAD_COMPONENT_KEY));
    assert_eq!(plan.after.runtime_component_donors.len(), plan.group.len());
    assert_eq!(
        plan.after.overrides.socket_plug_variants,
        recipe.overrides.socket_plug_variants
    );
    // A saved conflicting sibling selection is replaced as part of the same operation.
    let mut broken = recipe.clone();
    let mut broken_key = key.clone();
    replace_group(&mut broken, &mut broken_key, &plan.group, donor, false);
    broken.set_runtime_component_donor(
        WEAPON_RELOAD_COMPONENT_KEY,
        Some(WeaponDonorReference {
            item_hash: 0xEE06_B019_u32.into(),
            expected_name: None,
        }),
    );
    let other = donors
        .iter()
        .find(|donor| donor.hash == 0xEE06_B019)
        .unwrap();
    broken_key
        .component_donors
        .retain(|(binding, _, _)| *binding != WEAPON_RELOAD_COMPONENT_KEY);
    broken_key.component_donors.push((
        WEAPON_RELOAD_COMPONENT_KEY,
        other.weapon_pattern_index,
        other.hash,
    ));
    let repair = preview(
        &packages,
        &broken,
        &broken_key,
        WEAPON_TRIGGER_COMPONENT_KEY,
        baseline,
        &donors,
    )
    .unwrap();
    assert!(repair.error.is_none(), "{:?}", repair.error);
    assert!(repair.after.runtime_component_donors.is_empty());
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn preview_uses_compiler_overlap_checks_for_automatic_ammo_edits() {
    let (packages, donors) = stock();
    let baseline = donors
        .iter()
        .find(|donor| donor.hash == 0x4CE3_CE93)
        .unwrap();
    let key = RuntimeGraphKey::new(baseline.weapon_pattern_index, baseline.hash, []);
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let entity = load_effective_runtime_entity(&manager, &key).unwrap();
    let patches = crate::weapon_ammo::patches(
        &manager,
        &entity.payload,
        crate::weapon::WeaponAmmoType::Special,
    )
    .unwrap();
    let patch = &patches[0];
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.ammo-preview", baseline.hash, &baseline.name)
            .unwrap();
    recipe.overrides.weapon_pattern_index = baseline.weapon_pattern_index;
    recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Special);
    recipe
        .overrides
        .runtime_resource_patches
        .push(crate::WeaponRuntimeResourcePatchRecipe {
            binding_hash: patch.binding_hash.into(),
            resource_index: patch.resource_index,
            offset: patch.offset,
            bytes: patch
                .bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>(),
            graph_values: vec![],
        });
    let plan = preview(
        &packages,
        &recipe,
        &key,
        WEAPON_TRIGGER_COMPONENT_KEY,
        baseline,
        &donors,
    )
    .unwrap();
    assert!(plan.resets.is_empty());
    assert!(
        plan.error
            .as_ref()
            .is_some_and(|error| error.contains("overlap")),
        "{:?}",
        plan.error
    );
}
