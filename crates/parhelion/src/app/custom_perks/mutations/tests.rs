//! Recipe mutation regressions, including stale identities and preservation of sibling effects.
use super::*;

fn runtime_override(seed: u32) -> WeaponRuntimeValueOverride {
    WeaponRuntimeValueOverride {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: 0x1000_0000 | seed,
            resource_index: 0,
            root: sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind::Instance,
            root_schema: 0x2000_0000 | seed,
            path: Vec::new(),
            type_handle: 0x3000_0000 | seed,
            value_offset: seed,
            byte_size: 4,
        },
        value: WeaponRuntimeValue::Unsigned(u64::from(seed)),
    }
}

#[test]
fn custom_perk_lookup_and_removal_reject_a_stale_source_plug() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = PerkEditorKey {
        socket_index: 2,
        choice_index: 0,
        source_plug_hash: 10,
        source_perk_index: 7,
    };
    upsert_private_perk_runtime_values(&mut recipe, key, vec![runtime_override(1)]);
    let stale = PerkEditorKey {
        source_plug_hash: 20,
        ..key
    };
    assert!(private_perk(&recipe, stale).is_none());
    remove_private_perk_runtime_values(&mut recipe, stale);
    assert_eq!(private_perk(&recipe, key).unwrap().runtime_values.len(), 1);
}

#[test]
fn custom_perk_upsert_returns_the_requested_effect_after_sorting() {
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation;
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = PerkEditorKey {
        socket_index: 2,
        choice_index: 0,
        source_plug_hash: 10,
        source_perk_index: 7,
    };
    upsert_private_perk_runtime_values(&mut recipe, key, Vec::new());
    let earlier = PerkEditorKey {
        source_perk_index: 3,
        ..key
    };
    upsert_private_perk_runtime_values(&mut recipe, earlier, Vec::new()).activation =
        Some(PerkActivation::MeleeKill);
    assert!(private_perk(&recipe, key).unwrap().activation.is_none());
    assert_eq!(
        private_perk(&recipe, earlier).unwrap().activation,
        Some(PerkActivation::MeleeKill)
    );
}

#[test]
fn private_perk_runtime_edits_share_one_custom_plug_and_remain_sorted() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let first = PerkEditorKey {
        socket_index: 4,
        choice_index: 1,
        source_plug_hash: 0xAABB_CCDD,
        source_perk_index: 1178,
    };
    let second = PerkEditorKey {
        source_perk_index: 416,
        ..first
    };

    upsert_private_perk_runtime_values(&mut recipe, first, vec![runtime_override(1)]);
    upsert_private_perk_runtime_values(&mut recipe, second, vec![runtime_override(2)]);

    assert_eq!(recipe.overrides.socket_plug_variants.len(), 1);
    let variant = &recipe.overrides.socket_plug_variants[0];
    assert_eq!(variant.source_plug_hash, HexHash::new(0xAABB_CCDD));
    assert_eq!(
        variant
            .sandbox_perks
            .iter()
            .map(|perk| perk.source_perk_index)
            .collect::<Vec<_>>(),
        vec![416, 1178]
    );
    assert_eq!(
        private_perk_runtime_values(&recipe, first).unwrap().len(),
        1
    );
}

#[test]
fn removing_private_perks_removes_the_empty_custom_plug() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let first = PerkEditorKey {
        socket_index: 2,
        choice_index: 0,
        source_plug_hash: 0xAABB_CCDD,
        source_perk_index: 10,
    };
    let second = PerkEditorKey {
        source_perk_index: 20,
        ..first
    };
    upsert_private_perk_runtime_values(&mut recipe, first, vec![runtime_override(1)]);
    upsert_private_perk_runtime_values(&mut recipe, second, vec![runtime_override(2)]);

    remove_private_perk_runtime_values(&mut recipe, first);
    assert_eq!(recipe.overrides.socket_plug_variants.len(), 1);
    assert!(private_perk_runtime_values(&recipe, first).is_none());
    remove_private_perk_runtime_values(&mut recipe, second);
    assert!(recipe.overrides.socket_plug_variants.is_empty());
}

#[test]
fn removing_runtime_edits_resets_activation_but_keeps_equipped_stats() {
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation;
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = PerkEditorKey {
        socket_index: 3,
        choice_index: 0,
        source_plug_hash: 0x45A0_BDD7,
        source_perk_index: 421,
    };
    upsert_private_perk_runtime_values(&mut recipe, key, Vec::new());
    let variant = &mut recipe.overrides.socket_plug_variants[0];
    variant.investment_stats.push(WeaponStatOverride {
        definition_index: 26,
        value: 5,
    });
    variant.sandbox_perks[0].activation = Some(PerkActivation::MeleeKill);
    upsert_private_perk_runtime_values(&mut recipe, key, vec![runtime_override(1)]);
    assert_eq!(
        recipe.overrides.socket_plug_variants[0].sandbox_perks[0].activation,
        Some(PerkActivation::MeleeKill)
    );
    remove_private_perk_runtime_values(&mut recipe, key);
    let variant = &recipe.overrides.socket_plug_variants[0];
    assert!(variant.sandbox_perks[0].activation.is_none());
    assert!(variant.sandbox_perks[0].runtime_values.is_empty());
    assert_eq!(variant.investment_stats[0].value, 5);
}

#[test]
fn removing_a_socket_choice_reindexes_later_custom_plugs() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    for (choice_index, plug_hash) in [(0, 10), (1, 20), (2, 30)] {
        upsert_private_perk_runtime_values(
            &mut recipe,
            PerkEditorKey {
                socket_index: 0,
                choice_index,
                source_plug_hash: plug_hash,
                source_perk_index: 7,
            },
            vec![runtime_override(u32::from(choice_index) + 1)],
        );
    }

    reconcile_socket_plug_variants(&mut recipe, 0, &[10, 30], Some(1));

    assert_eq!(
        recipe
            .overrides
            .socket_plug_variants
            .iter()
            .map(|variant| (
                variant.choice_index,
                variant.source_plug_hash.parse_u32().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![(0, 10), (1, 30)]
    );
}

#[test]
fn custom_plug_runtime_is_a_regular_workbench_feature() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    upsert_private_perk_runtime_values(
        &mut recipe,
        PerkEditorKey {
            socket_index: 0,
            choice_index: 0,
            source_plug_hash: 10,
            source_perk_index: 7,
        },
        vec![runtime_override(1)],
    );

    assert!(technical_recipe_features(&recipe).is_empty());
}
