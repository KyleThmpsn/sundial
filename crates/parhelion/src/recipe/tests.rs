use super::*;
use sundial::package_authoring::runtime::{BindingHash, SchemaHandle};
use sundial::package_authoring::runtime::{
    WeaponRuntimeFieldLocator, WeaponRuntimePathElement, WeaponRuntimeRootKind, WeaponRuntimeValue,
};

#[test]
fn referenced_graph_edits_round_trip_and_require_a_tag() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.graph-test").unwrap();
    let mut patch = WeaponRuntimeResourcePatchRecipe {
        bytes: "B9 B1 BB 80".into(),
        graph_values: vec![private_perk_runtime_value()],
        ..Default::default()
    };
    recipe
        .overrides
        .runtime_resource_patches
        .push(patch.clone());
    recipe.validate().unwrap();
    let json = serde_json::to_string(&recipe).unwrap();
    assert_eq!(serde_json::from_str::<WeaponRecipe>(&json).unwrap(), recipe);
    patch.bytes = "00 00".into();
    recipe.overrides.runtime_resource_patches[0] = patch;
    assert!(
        recipe
            .validate()
            .unwrap_err()
            .to_string()
            .contains("four graph-tag bytes")
    );
    let old: WeaponRuntimeResourcePatchRecipe = serde_json::from_str(
        r#"{"binding_hash":"0x2D8A944C","offset":21544,"bytes":"B9 B1 BB 80"}"#,
    )
    .unwrap();
    assert!(old.graph_values.is_empty());
    assert!(
        !serde_json::to_string(&old)
            .unwrap()
            .contains("graph_values")
    );
}

fn private_perk_runtime_value() -> WeaponRuntimeValueOverride {
    WeaponRuntimeValueOverride {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: BindingHash::new(0x39AF_D7D3),
            resource_index: 0,
            root: WeaponRuntimeRootKind::ComponentInstance,
            root_schema: SchemaHandle::new(0x80BF_DFEA),
            path: vec![WeaponRuntimePathElement {
                name_hash: 0x1234_5678,
                type_handle: 0x8080_000F.into(),
                byte_offset: 0xBD4,
            }],
            type_handle: SchemaHandle::new(0x8080_000F),
            value_offset: 0xBD4,
            byte_size: 4,
        },
        value: WeaponRuntimeValue::Float32Bits(500.0_f32.to_bits()),
    }
}

#[test]
fn socket_plug_variants_reject_invalid_positions_and_perks() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.invalid-private-perk").unwrap();
    recipe.overrides.socket_plug_variants = vec![
        WeaponSocketPlugVariantRecipe {
            projectile: None,
            offer_everywhere: false,
            replace_effects: false,
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
            icon: None,
            classification_donor_hash: None,
            description: None,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
                program: None,
                projectiles: Vec::new(),
                source_perk_index: 1178,
                activation: None,
                runtime_values: vec![private_perk_runtime_value()],
                action_float_values: Vec::new(),
            }],
        },
        WeaponSocketPlugVariantRecipe {
            projectile: None,
            offer_everywhere: false,
            replace_effects: false,
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
            icon: None,
            classification_donor_hash: None,
            description: None,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
                program: None,
                projectiles: Vec::new(),
                source_perk_index: 416,
                activation: None,
                runtime_values: vec![private_perk_runtime_value()],
                action_float_values: Vec::new(),
            }],
        },
    ];
    let error = recipe.validate().unwrap_err();
    assert!(error.to_string().contains("more than one edit"));

    recipe.overrides.socket_plug_variants.truncate(1);
    let before = recipe.clone();
    for stats in [
        vec![WeaponStatOverride {
            definition_index: 256,
            value: 10,
        }],
        vec![
            WeaponStatOverride {
                definition_index: 13,
                value: 10
            };
            2
        ],
    ] {
        recipe.overrides.socket_plug_variants[0].investment_stats = stats;
        assert!(
            recipe
                .validate()
                .unwrap_err()
                .to_string()
                .contains("unique native 8-bit")
        );
    }
    recipe = before;
    recipe.overrides.socket_plug_variants[0]
        .sandbox_perks
        .clear();
    let error = recipe.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not select a finished sandbox perk")
    );
}

#[test]
fn gameplay_profile_overrides_round_trip_and_compile() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.profiled").unwrap();
    recipe.overrides.rarity = Some(RecipeRarity::Exotic);
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Void);
    recipe.overrides.weapon_pattern_index = Some(285);
    recipe.overrides.weapon_pattern_donor_hash = Some(HexHash::new(MOUNTAINTOP_DONOR_HASH));
    recipe.overrides.stat_group_index = Some(42);
    recipe.overrides.stat_group_donor_hash = Some(HexHash::new(MOUNTAINTOP_DONOR_HASH));
    recipe.render_gear_donor = Some(WeaponDonorReference {
        item_hash: HexHash::new(MOUNTAINTOP_DONOR_HASH),
        expected_name: Some(MOUNTAINTOP_DONOR_NAME.to_owned()),
    });

    let encoded = recipe.to_json_pretty().unwrap();
    let document: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(document["overrides"]["rarity"], serde_json::json!("exotic"));
    assert_eq!(
        document["overrides"]["modern_damage_type"],
        serde_json::json!("void")
    );
    let legacy = encoded.replace("weapon_pattern_index", "gear_art_index");
    let canonical = WeaponRecipe::from_json_str(&legacy)
        .unwrap()
        .to_json_pretty()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&canonical).unwrap(),
        document
    );
    assert!(!canonical.contains("gear_art_index"));
    assert_eq!(
        document["overrides"]["weapon_pattern_index"],
        serde_json::json!(285)
    );
    assert_eq!(
        document["overrides"]["weapon_pattern_donor_hash"],
        serde_json::json!("0xEE06B019")
    );
    assert!(encoded.contains(r#""render_gear_donor""#));
    assert_eq!(
        document["overrides"]["stat_group_index"],
        serde_json::json!(42)
    );
    assert_eq!(
        document["overrides"]["stat_group_donor_hash"],
        serde_json::json!("0xEE06B019")
    );
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(decoded, recipe);
    let overrides = decoded.to_spec().unwrap().overrides;
    assert_eq!(
        (overrides.rarity, overrides.modern_damage_type),
        (
            Some(AuthoredWeaponRarity::Exotic),
            Some(ModernDamageType::Void)
        ),
    );
    assert_eq!(overrides.weapon_pattern_index, Some(285));
    assert_eq!(overrides.stat_group_index, Some(42));
    assert_eq!(
        decoded
            .to_spec()
            .unwrap()
            .render_gear_donor
            .unwrap()
            .item_hash,
        MOUNTAINTOP_DONOR_HASH
    );
}

#[test]
fn recipe_schema_rejects_unsupported_versions_missing_placement_and_retired_columns() {
    let current =
        serde_json::to_value(WeaponRecipe::new_weapon("parhelion.schema-unsupported").unwrap())
            .unwrap();
    for invalid in ["schema", "collection_placement", "default_plug_hashes"] {
        let mut value = current.clone();
        match invalid {
            "schema" => value["schema"] = serde_json::json!(2),
            "collection_placement" => {
                value.as_object_mut().unwrap().remove(invalid);
            }
            _ => {
                let overrides = value["overrides"].as_object_mut().unwrap();
                overrides.remove("socket_columns");
                overrides.insert(invalid.into(), serde_json::json!(["0x11111111"]));
            }
        }
        assert!(
            WeaponRecipe::from_json_str(&value.to_string()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn rename_updates_name_namespace_and_all_hashes_atomically() {
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        "First Light",
        ARC_LOGIC_DONOR_HASH,
        ARC_LOGIC_DONOR_NAME,
    )
    .unwrap();
    let before = recipe.clone();

    recipe.rename_authored_item("A Better Tomorrow").unwrap();

    assert_eq!(recipe.name, "A Better Tomorrow");
    assert_eq!(recipe.namespace, "parhelion.a-better-tomorrow");
    assert_ne!(recipe.identity, before.identity);
    assert_eq!(recipe.donor, before.donor);
    assert_eq!(recipe.overrides, before.overrides);
    assert!(recipe.identity_is_name_derived());

    let valid = recipe.clone();
    assert!(recipe.rename_authored_item("☀☀☀").is_err());
    assert_eq!(recipe, valid);
}

#[test]
fn name_namespace_normalization_has_explicit_boundaries() {
    assert_eq!(
        namespace_for_weapon_name("  Every__END...Again!  ").unwrap(),
        "parhelion.every-end-again"
    );
    assert_eq!(
        namespace_for_weapon_name(&"a".repeat(54)).unwrap().len(),
        64
    );
    assert!(namespace_for_weapon_name(&"a".repeat(55)).is_err());
    assert!(namespace_for_weapon_name("---").is_err());
    assert!(namespace_for_weapon_name("太陽").is_err());
}

#[test]
fn duplicate_locale_and_conflicting_stat_removal_are_rejected() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.invalid-technical").unwrap();
    recipe.locale_overrides = vec![
        WeaponLocaleTextRecipe {
            locale_index: 3,
            name: Some("One".to_owned()),
            ..Default::default()
        },
        WeaponLocaleTextRecipe {
            locale_index: 3,
            name: Some("Two".to_owned()),
            ..Default::default()
        },
    ];
    assert!(recipe.validate().is_err());

    recipe.locale_overrides.clear();
    recipe.overrides.investment_stats.push(WeaponStatOverride {
        definition_index: 15,
        value: 10,
    });
    recipe.overrides.removed_investment_stats.push(15);
    assert!(recipe.validate().is_err());
}
/// A weapon built on another library weapon starts from that weapon's recipe: its stock donor
/// and its changes, with this recipe's own settings on top and its own identity throughout.
#[test]
fn rebasing_keeps_the_bases_donor_and_changes_underneath_the_recipes_own() {
    let mut base = WeaponRecipe::every_end();
    base.overrides.ammo_type = Some(RecipeAmmoType::Special);
    base.overrides.rarity = Some(RecipeRarity::Exotic);
    base.overrides.max_stack_size = Some(3);
    base.type_name = Some("Base Type".into());
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        "Built On Base",
        base.identity.item_hash.parse_u32().unwrap(),
        &base.name,
    )
    .unwrap();
    recipe.overrides.rarity = Some(RecipeRarity::Legendary);
    recipe.overrides.additional_behaviors = vec![AdditionalBehaviorRecipe {
        behavior: "graviton-lance-graph".into(),
    }];
    recipe.flavor = "Its own story.".into();

    let rebased = recipe.rebased_onto(&base).unwrap();

    assert_eq!(
        rebased.donor, base.donor,
        "the stock donor comes from the base"
    );
    assert_eq!(
        rebased.identity, recipe.identity,
        "the identity stays its own"
    );
    assert_eq!(rebased.namespace, recipe.namespace);
    assert_eq!(rebased.name, "Built On Base");
    assert_eq!(rebased.flavor, "Its own story.");
    assert_eq!(
        rebased.type_name.as_deref(),
        Some("Base Type"),
        "unset text inherits"
    );
    assert_eq!(rebased.overrides.ammo_type, Some(RecipeAmmoType::Special));
    assert_eq!(rebased.overrides.max_stack_size, Some(3));
    assert_eq!(
        rebased.overrides.rarity,
        Some(RecipeRarity::Legendary),
        "own settings win"
    );
    assert_eq!(
        rebased.overrides.socket_columns,
        base.overrides.socket_columns
    );
    assert_eq!(
        rebased.overrides.additional_behaviors,
        recipe.overrides.additional_behaviors
    );
    assert_eq!(rebased.presentation_donor, base.presentation_donor);
    assert!(rebased.to_spec().is_ok());
}
