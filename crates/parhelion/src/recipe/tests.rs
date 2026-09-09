use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimeFieldLocator, WeaponRuntimePathElement, WeaponRuntimeRootKind, WeaponRuntimeValue,
};

#[test]
fn activation_recipe_round_trips_and_rejects_unsupported_effects() {
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation;
    let mut recipe = WeaponRecipe::new_weapon("parhelion.activation-test").unwrap();
    let variant: WeaponSocketPlugVariantRecipe = serde_json::from_str(
        r#"{"socket_index":3,"choice_index":0,"source_plug_hash":"0x45A0BDD7","sandbox_perks":[{"source_perk_index":421,"runtime_values":[]}]}"#,
    ).unwrap();
    recipe.overrides.socket_plug_variants.push(variant);
    assert!(!recipe.to_json_pretty().unwrap().contains("\"activation\""));
    for condition in PerkActivation::ALL {
        recipe.overrides.socket_plug_variants[0].sandbox_perks[0].activation = Some(condition);
        let encoded = recipe.to_json_pretty().unwrap();
        let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
        assert_eq!(decoded, recipe);
        assert_eq!(
            decoded.to_spec().unwrap().overrides.socket_plug_variants[0].sandbox_perks[0]
                .activation,
            Some(condition)
        );
    }
    recipe.overrides.socket_plug_variants[0].sandbox_perks[0].source_perk_index = 1178;
    assert!(
        recipe
            .validate()
            .unwrap_err()
            .to_string()
            .contains("only mapped Outlaw")
    );
}

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
            binding_hash: 0x39AF_D7D3,
            resource_index: 0,
            root: WeaponRuntimeRootKind::ComponentInstance,
            root_schema: 0x80BF_DFEA,
            path: vec![WeaponRuntimePathElement {
                name_hash: 0x1234_5678,
                type_handle: 0x8080_000F,
                byte_offset: 0xBD4,
            }],
            type_handle: 0x8080_000F,
            value_offset: 0xBD4,
            byte_size: 4,
        },
        value: WeaponRuntimeValue::Float32Bits(500.0_f32.to_bits()),
    }
}

#[test]
fn socket_plug_variants_round_trip_and_compile() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.private-perk").unwrap();
    recipe.overrides.socket_plug_variants = vec![WeaponSocketPlugVariantRecipe {
        investment_stats: vec![WeaponStatOverride {
            definition_index: 13,
            value: -5,
        }],
        socket_index: 4,
        choice_index: 0,
        source_plug_hash: HexHash::new(0xDD5C_B37A),
        name: Some("Micro-Missile Frame".to_owned()),
        classification_donor_hash: Some(HexHash::new(0xC684_24BC)),
        description: Some("A private intrinsic description.".to_owned()),
        additional_sandbox_perks: vec![405],
        sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
            source_perk_index: 1178,
            activation: None,
            runtime_values: vec![private_perk_runtime_value()],
            action_float_values: Vec::new(),
        }],
    }];

    let expected_values = recipe.overrides.socket_plug_variants[0].sandbox_perks[0]
        .runtime_values
        .clone();
    for socket_index in [0, 3, 4] {
        recipe.overrides.socket_plug_variants[0].socket_index = socket_index;
        let encoded = recipe.to_json_pretty().unwrap();
        assert!(encoded.contains(r#""socket_plug_variants""#));
        assert!(encoded.contains(r#""source_plug_hash": "0xDD5CB37A""#));
        assert!(encoded.contains(r#""source_perk_index": 1178"#));

        let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
        assert_eq!(decoded, recipe);
        let variants = &decoded.to_spec().unwrap().overrides.socket_plug_variants;
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].socket_index, socket_index);
        assert_eq!(variants[0].choice_index, 0);
        assert_eq!(variants[0].source_plug_hash, 0xDD5C_B37A);
        assert_eq!(variants[0].classification_donor_hash, Some(0xC684_24BC));
        assert_eq!(
            variants[0].description.as_deref(),
            Some("A private intrinsic description.")
        );
        assert_eq!(
            (
                &variants[0].additional_sandbox_perks,
                &variants[0].investment_stats
            ),
            (&vec![405], &vec![(13, -5)])
        );
        assert_eq!(variants[0].sandbox_perks[0].source_perk_index, 1178);
        assert_eq!(variants[0].sandbox_perks[0].runtime_values, expected_values);
    }
}

#[test]
fn socket_plug_variants_reject_invalid_positions_and_perks() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.invalid-private-perk").unwrap();
    recipe.overrides.socket_plug_variants = vec![
        WeaponSocketPlugVariantRecipe {
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
            classification_donor_hash: None,
            description: None,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
                source_perk_index: 1178,
                activation: None,
                runtime_values: vec![private_perk_runtime_value()],
                action_float_values: Vec::new(),
            }],
        },
        WeaponSocketPlugVariantRecipe {
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
            classification_donor_hash: None,
            description: None,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
                source_perk_index: 416,
                activation: None,
                runtime_values: vec![private_perk_runtime_value()],
                action_float_values: Vec::new(),
            }],
        },
    ];
    let error = recipe.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("more than one edit for socket 4 choice 0")
    );

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
fn old_custom_perks_keep_sparse_stat_serialization() {
    let json = r#"{"socket_index":0,"choice_index":0,"source_plug_hash":"0xDD5CB37A","sandbox_perks":[{"source_perk_index":1178,"runtime_values":[]}]}"#;
    let variant: WeaponSocketPlugVariantRecipe = serde_json::from_str(json).unwrap();
    assert!(variant.investment_stats.is_empty());
    assert!(
        !serde_json::to_string(&variant)
            .unwrap()
            .contains("investment_stats")
    );
}

#[test]
fn every_end_preserves_shipped_identity_and_build_overrides() {
    let recipe = WeaponRecipe::every_end();
    let spec = recipe.to_spec().unwrap();

    assert_eq!(recipe.namespace, "parhelion.every-end");
    assert!(!recipe.identity_is_name_derived());
    assert_eq!(
        recipe.collection_placement,
        RecipeCollectionPlacement::SunriseBadge
    );
    assert_eq!(spec.donor_item_hash, ARC_LOGIC_DONOR_HASH);
    assert_eq!(spec.identity.item_hash, 0x5355_4E44);
    assert_eq!(spec.text.name, "Every End");
    assert_eq!(spec.text.source, DEFAULT_SOURCE_TEXT);
    assert_eq!(spec.overrides.inventory_slot, None);
    assert_eq!(spec.overrides.investment_stats.len(), 9);
    assert_eq!(
        spec.overrides.modern_damage_type,
        Some(ModernDamageType::Solar)
    );
    assert_eq!(spec.overrides.power_cap_group, Some(11));
    assert_eq!(spec.overrides.socket_columns.len(), 10);
    assert_eq!(
        spec.overrides.socket_columns[0].as_ref().unwrap().choices,
        vec![0x56E7_7AA2]
    );
}

#[test]
fn second_sun_preserves_shipped_identity_and_donors() {
    let recipe = WeaponRecipe::second_sun().unwrap();
    let spec = recipe.to_spec().unwrap();

    assert!(recipe.identity_is_name_derived());
    assert_eq!(
        (
            spec.identity.item_hash,
            spec.identity.collectible_hash,
            spec.identity.unlock_hash,
        ),
        (0x757D_33F5, 0xD347_B59A, 0xF137_B0F4)
    );
    assert_eq!(
        recipe.collection_placement,
        RecipeCollectionPlacement::SunriseBadge
    );
    assert_eq!(spec.donor_item_hash, 0x59EF_ED62);
    assert_eq!(
        spec.expected_donor_name.as_deref(),
        Some("The Wardcliff Coil")
    );
    let presentation = spec.presentation_donor.as_ref().unwrap();
    assert_eq!(presentation.item_hash, 0x47A2_7ADF);
    assert_eq!(presentation.expected_name.as_deref(), Some("Truth"));
    assert_eq!(
        (
            spec.text.name.as_str(),
            spec.text.flavor.as_str(),
            spec.text.source.as_str(),
        ),
        (
            "Second Sun",
            "A second dawn, made by our own hands.",
            DEFAULT_SOURCE_TEXT,
        )
    );
    assert_eq!(
        spec.overrides.inventory_slot,
        Some(WeaponInventorySlot::Power)
    );
    assert_eq!(
        spec.overrides.modern_damage_type,
        Some(ModernDamageType::Solar)
    );
    assert_eq!(spec.overrides.power_cap_group, Some(11));
    assert_eq!(spec.overrides.socket_columns.len(), 8);
    assert_eq!(spec.overrides.ammo_type, Some(WeaponAmmoType::Heavy));
    assert!(spec.text.inventory_hint.is_none());
}

#[test]
fn new_donor_recipe_inherits_definition_by_default() {
    let recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.falling-star",
        0x1234_5678,
        "A Different Weapon",
    )
    .unwrap();
    let spec = recipe.to_spec().unwrap();

    assert_eq!(spec.donor_item_hash, 0x1234_5678);
    assert_eq!(
        spec.expected_donor_name.as_deref(),
        Some("A Different Weapon")
    );
    assert_eq!(spec.overrides, WeaponCloneOverrides::default());
    assert!(spec.presentation_donor.is_none());
    assert_eq!(spec.text.source, DEFAULT_SOURCE_TEXT);
    assert_eq!(
        recipe.collection_placement,
        RecipeCollectionPlacement::SunriseBadge
    );
}

#[test]
fn changing_geometry_donor_restores_dependent_presentation_sources() {
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.presentation-reset", 0x1111_1111, "Gameplay")
            .unwrap();
    recipe.presentation_donor = Some(WeaponDonorReference {
        item_hash: 0x2222_2222_u32.into(),
        expected_name: Some("Old geometry".to_owned()),
    });
    recipe.render_gear_donor = Some(WeaponDonorReference {
        item_hash: 0x3333_3333_u32.into(),
        expected_name: Some("Old render gear".to_owned()),
    });
    recipe.icon_donor = Some(WeaponDonorReference {
        item_hash: 0x4444_4444_u32.into(),
        expected_name: Some("Old icon".to_owned()),
    });
    recipe.overrides.art_arrangements = Some(vec![WeaponArtArrangementRecipe {
        character_class: -1,
        arrangement: 960,
    }]);
    recipe.overrides.render_dye_rows = Some(std::array::from_fn(|_| {
        vec![WeaponDyeReferenceRecipe {
            channel_index: 0,
            dye_reference_index: 7,
        }]
    }));
    recipe.overrides.investment_stats = vec![WeaponStatOverride {
        definition_index: 14,
        value: 50,
    }];
    let icon_edit = recipe.overrides.icon_edit.clone();

    let new_donor = WeaponDonorReference {
        item_hash: 0x5555_5555_u32.into(),
        expected_name: Some("New geometry".to_owned()),
    };
    recipe.set_presentation_donor(Some(new_donor.clone()));

    assert_eq!(recipe.presentation_donor, Some(new_donor));
    assert!(recipe.render_gear_donor.is_none());
    assert!(recipe.icon_donor.is_none());
    assert!(recipe.overrides.art_arrangements.is_none());
    assert!(recipe.overrides.render_dye_rows.is_none());
    assert_eq!(recipe.overrides.icon_edit, icon_edit);
    assert_eq!(recipe.overrides.investment_stats.len(), 1);
}

#[test]
fn canonical_hash_json_is_uppercase_and_string_typed() {
    let encoded = serde_json::to_string(&HexHash::new(0x000A_BC12)).unwrap();
    assert_eq!(encoded, r#""0x000ABC12""#);
    assert_eq!(
        serde_json::from_str::<HexHash>(&encoded)
            .unwrap()
            .parse_u32()
            .unwrap(),
        0x000A_BC12
    );
}

#[test]
fn dynamic_recipe_json_round_trips() {
    let recipe = WeaponRecipe::every_end();
    let encoded = recipe.to_json_pretty().unwrap();

    assert!(encoded.contains(r#""item_hash": "0xA25B8F8F""#));
    assert!(encoded.contains(r#""collection_placement": "sunrise_badge""#));
    assert!(encoded.contains(r#""modern_damage_type": "solar""#));
    assert!(encoded.contains(r#""socket_columns""#));
    assert!(!encoded.contains("default_plug_hashes"));
    assert_eq!(WeaponRecipe::from_json_str(&encoded).unwrap(), recipe);
}

#[test]
fn icon_image_edit_round_trips_and_identity_is_omitted() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.icon-edit").unwrap();
    let identity = recipe.to_json_pretty().unwrap();
    assert!(!identity.contains("icon_edit"));

    recipe.overrides.icon_edit = WeaponIconEdit {
        hue_shift_degrees: 24,
        brightness: 8,
        invert: true,
        ..WeaponIconEdit::default()
    };
    let encoded = recipe.to_json_pretty().unwrap();
    assert!(encoded.contains(r#""icon_edit""#));
    assert!(encoded.contains(r#""hue_shift_degrees": 24"#));
    assert!(!encoded.contains("imported_image"));
    assert_eq!(WeaponRecipe::from_json_str(&encoded).unwrap(), recipe);
    assert_eq!(
        recipe.to_spec().unwrap().overrides.icon_edit,
        recipe.overrides.icon_edit
    );

    recipe.overrides.icon_edit.hue_shift_degrees = 181;
    assert!(recipe.validate().is_err());
}

#[test]
fn modern_void_recipe_round_trips_and_compiles() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.void").unwrap();
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Void);
    let encoded = recipe.to_json_pretty().unwrap();
    assert!(encoded.contains(r#""modern_damage_type": "void""#));
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(decoded, recipe);
    assert_eq!(
        decoded.to_spec().unwrap().overrides.modern_damage_type,
        Some(ModernDamageType::Void)
    );
}

#[test]
fn gameplay_profile_overrides_round_trip_and_compile() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.profiled").unwrap();
    recipe.overrides.rarity = Some(RecipeRarity::Exotic);
    recipe.overrides.weapon_pattern_index = Some(285);
    recipe.overrides.weapon_pattern_donor_hash = Some(HexHash::new(MOUNTAINTOP_DONOR_HASH));
    recipe.overrides.stat_group_index = Some(42);
    recipe.overrides.stat_group_donor_hash = Some(HexHash::new(MOUNTAINTOP_DONOR_HASH));
    recipe.render_gear_donor = Some(WeaponDonorReference {
        item_hash: HexHash::new(MOUNTAINTOP_DONOR_HASH),
        expected_name: Some(MOUNTAINTOP_DONOR_NAME.to_owned()),
    });

    let encoded = recipe.to_json_pretty().unwrap();
    assert!(encoded.contains(r#""rarity": "exotic""#));
    assert!(encoded.contains(r#""weapon_pattern_index": 285"#));
    assert!(encoded.contains(r#""weapon_pattern_donor_hash": "0xEE06B019""#));
    assert!(encoded.contains(r#""render_gear_donor""#));
    assert!(encoded.contains(r#""stat_group_index": 42"#));
    assert!(encoded.contains(r#""stat_group_donor_hash": "0xEE06B019""#));
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(decoded, recipe);
    let overrides = decoded.to_spec().unwrap().overrides;
    assert_eq!(overrides.rarity, Some(AuthoredWeaponRarity::Exotic));
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
fn legacy_misnamed_gear_art_field_loads_but_saves_canonically() {
    let recipe = WeaponRecipe::new_weapon("parhelion.legacy-weapon-pattern").unwrap();
    let encoded = recipe.to_json_pretty().unwrap().replace(
        "\"investment_stats\": []",
        "\"investment_stats\": [],\n    \"gear_art_index\": 108",
    );

    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(decoded.overrides.weapon_pattern_index, Some(108));
    let canonical = decoded.to_json_pretty().unwrap();
    assert!(canonical.contains("\"weapon_pattern_index\": 108"));
    assert!(!canonical.contains("gear_art_index"));
}

#[test]
fn socket_columns_round_trip_in_order_and_reject_malformed_choices() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.columns").unwrap();
    let ordered_choices = (1..=8)
        .map(|value| HexHash::new(0x1111_1100 + value))
        .collect::<Vec<_>>();
    recipe.overrides.socket_columns = vec![
        Some(WeaponSocketColumnRecipe {
            choices: ordered_choices.clone(),
            ..WeaponSocketColumnRecipe::default()
        }),
        None,
    ];
    let encoded = recipe.to_json_pretty().unwrap();
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(
        decoded.overrides.socket_columns,
        recipe.overrides.socket_columns
    );
    assert_eq!(
        decoded.to_spec().unwrap().overrides.socket_columns[0]
            .as_ref()
            .unwrap()
            .choices,
        ordered_choices
            .iter()
            .map(|choice| choice.parse_u32().unwrap())
            .collect::<Vec<_>>()
    );

    for malformed in [
        Vec::new(),
        vec![HexHash::new(0)],
        vec![HexHash::new(1), HexHash::new(1)],
        (1..=MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES + 1)
            .map(|value| HexHash::new(u32::try_from(value).unwrap()))
            .collect(),
    ] {
        let mut invalid = recipe.clone();
        invalid.overrides.socket_columns[0] = Some(WeaponSocketColumnRecipe {
            choices: malformed,
            ..WeaponSocketColumnRecipe::default()
        });
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn removed_default_plug_field_is_not_a_second_schema() {
    let mut value =
        serde_json::to_value(WeaponRecipe::new_weapon("parhelion.schema-columns").unwrap())
            .unwrap();
    let overrides = value["overrides"].as_object_mut().unwrap();
    overrides.remove("socket_columns");
    overrides.insert(
        "default_plug_hashes".to_owned(),
        serde_json::json!(["0x11111111"]),
    );

    assert!(WeaponRecipe::from_json_str(&value.to_string()).is_err());
}

#[test]
fn non_current_recipe_schema_is_rejected() {
    let current = WeaponRecipe::new_weapon("parhelion.schema-unsupported").unwrap();
    let mut value = serde_json::to_value(current).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("schema".to_owned(), serde_json::Value::from(2));

    assert!(WeaponRecipe::from_json_str(&value.to_string()).is_err());
}

#[test]
fn duplicate_recipe_fields_are_rejected_before_validation() {
    let encoded = WeaponRecipe::new_weapon("parhelion.schema-duplicate")
        .unwrap()
        .to_json_pretty()
        .unwrap();
    let duplicate = encoded.replacen('{', r#"{"schema": 1,"#, 1);

    let error = WeaponRecipe::from_json_str(&duplicate)
        .expect_err("duplicate recipe fields must not have implicit precedence");
    assert!(
        error
            .to_string()
            .contains("duplicate object member \"schema\"")
    );
}

#[test]
fn current_schema_requires_explicit_collection_placement() {
    let current = WeaponRecipe::new_weapon("parhelion.missing-placement").unwrap();
    let mut value = serde_json::to_value(current).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("collection_placement");

    assert!(WeaponRecipe::from_json_str(&value.to_string()).is_err());
}

#[test]
fn duplicate_stat_overrides_are_rejected_by_the_compiler_boundary() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.duplicate").unwrap();
    recipe.overrides.investment_stats = vec![
        WeaponStatOverride {
            definition_index: 15,
            value: 50,
        },
        WeaponStatOverride {
            definition_index: 15,
            value: 60,
        },
    ];
    assert!(recipe.validate().is_err());
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
    assert_eq!(
        recipe.identity,
        WeaponIdentity::from(
            WeaponCloneIdentity::from_namespace("parhelion.a-better-tomorrow").unwrap()
        )
    );
    assert!(recipe.identity_is_name_derived());

    let valid = recipe.clone();
    assert!(recipe.rename_authored_item("☀☀☀").is_err());
    assert_eq!(recipe, valid);
}

#[test]
fn custom_identity_is_valid_and_renames_atomically() {
    let mut every_end = WeaponRecipe::every_end();
    let original = every_end.clone();
    assert!(every_end.validate().is_ok());
    assert!(!every_end.identity_is_name_derived());

    every_end.rename_authored_item("Different Name").unwrap();
    assert_eq!(every_end.namespace, "parhelion.different-name");
    assert_ne!(every_end.identity, original.identity);
    assert!(every_end.identity_is_name_derived());

    let another = WeaponRecipe::new_named_weapon_for_donor(
        "Every End",
        ARC_LOGIC_DONOR_HASH,
        ARC_LOGIC_DONOR_NAME,
    )
    .unwrap();
    assert_eq!(another.namespace, "parhelion.every-end");
    assert!(another.identity_is_name_derived());
}

#[test]
fn slug_is_stable_and_filesystem_safe() {
    let mut recipe = WeaponRecipe::every_end();
    assert_eq!(recipe.slug(), "every-end");
    recipe.name = "Alpha__Beta.Gamma".to_owned();
    assert_eq!(recipe.slug(), "alpha-beta-gamma");

    recipe.name = "CON".to_owned();
    assert_eq!(recipe.slug(), "weapon-con");

    recipe.name = format!("{}---tail", "a".repeat(200));
    assert_eq!(recipe.slug().len(), 80);
    assert!(!recipe.slug().ends_with('-'));
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
fn authored_namespace_validation_has_one_canonical_policy() {
    assert!(validate_parhelion_namespace("parhelion.every-end").is_ok());
    assert!(validate_parhelion_namespace("parhelion.weapon_2.test").is_ok());
    assert!(validate_parhelion_namespace("example.every-end").is_err());
    assert!(validate_parhelion_namespace("parhelion.").is_err());
    assert!(validate_parhelion_namespace("parhelion.Uppercase").is_err());
    assert!(validate_parhelion_namespace(&format!("parhelion.{}", "a".repeat(55))).is_err());
}

#[test]
fn investment_stats_are_canonicalized_by_definition_index() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.canonical-stats").unwrap();
    recipe.overrides.investment_stats = vec![
        WeaponStatOverride {
            definition_index: 31,
            value: 20,
        },
        WeaponStatOverride {
            definition_index: 15,
            value: 100,
        },
        WeaponStatOverride {
            definition_index: 22,
            value: 75,
        },
    ];

    let encoded = recipe.to_json_pretty().unwrap();
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    let compiled = recipe.to_spec().unwrap();

    assert_eq!(
        decoded
            .overrides
            .investment_stats
            .iter()
            .map(|stat| stat.definition_index)
            .collect::<Vec<_>>(),
        vec![15, 22, 31]
    );
    assert!(
        encoded.find("\"definition_index\": 15").unwrap()
            < encoded.find("\"definition_index\": 31").unwrap()
    );
    assert_eq!(
        compiled
            .overrides
            .investment_stats
            .iter()
            .map(|(definition_index, _)| *definition_index)
            .collect::<Vec<_>>(),
        vec![15, 22, 31]
    );
}

#[test]
fn locale_payload_overrides_round_trip_and_are_canonicalized() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.localized").unwrap();
    recipe.locale_overrides = vec![
        WeaponLocaleTextRecipe {
            locale_index: 12,
            name: Some("Final locale".to_owned()),
            ..Default::default()
        },
        WeaponLocaleTextRecipe {
            locale_index: 2,
            flavor: Some("A different description.".to_owned()),
            ..Default::default()
        },
    ];

    let decoded = WeaponRecipe::from_json_str(&recipe.to_json_pretty().unwrap()).unwrap();
    assert_eq!(
        decoded
            .locale_overrides
            .iter()
            .map(|locale| locale.locale_index)
            .collect::<Vec<_>>(),
        [2, 12]
    );
    assert_eq!(
        decoded.to_spec().unwrap().text.locale_overrides[1]
            .name
            .as_deref(),
        Some("Final locale")
    );

    decoded
        .clone()
        .to_json_pretty()
        .expect("canonical locale overrides remain serializable");
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
