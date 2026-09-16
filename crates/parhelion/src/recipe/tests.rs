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
    assert!(variant.investment_stats.is_empty());
    assert!(
        !serde_json::to_string(&variant)
            .unwrap()
            .contains("investment_stats")
    );
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
            graph_tag: None,
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
        replace_effects: false,
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
            program: None,
            projectiles: Vec::new(),
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
            replace_effects: false,
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
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
            replace_effects: false,
            investment_stats: Vec::new(),
            socket_index: 4,
            choice_index: 0,
            source_plug_hash: HexHash::new(0xDD5C_B37A),
            name: None,
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
fn changing_base_keeps_collection_story_and_independent_artwork() {
    let mut recipe = WeaponRecipe::every_end();
    recipe.overrides.collection_destination = Some(crate::collection::Destination {
        ammo: crate::collection::Ammo::Special,
        family: crate::collection::Family::Sidearms,
    });
    recipe.overrides.exclude_from_sunrise_badge = true;
    recipe.overrides.badge = Some(crate::presentation::Badge {
        name: "Travelers".into(),
        ..Default::default()
    });
    recipe.overrides.lore = Some("A story to keep.".into());
    let art = crate::presentation::Artwork::from_png(include_bytes!(
        "../../../../assets/parhelion/watermark/sunrise-watermark-0-96x96.png"
    ))
    .unwrap();
    recipe.overrides.corner_icon = Some(art);
    let before = recipe.clone();
    recipe.set_donor(ARC_LOGIC_DONOR_HASH, "Arc Logic");
    assert_eq!(
        recipe.overrides.collection_destination,
        before.overrides.collection_destination
    );
    assert_eq!(recipe.overrides.badge, before.overrides.badge);
    assert_eq!(recipe.overrides.corner_icon, before.overrides.corner_icon);
    assert_eq!(recipe.overrides.lore, before.overrides.lore);
    assert!(recipe.overrides.exclude_from_sunrise_badge);
    assert_eq!(recipe.overrides.icon_edit, before.overrides.icon_edit);
    assert_eq!(recipe.overrides.hud_icon, before.overrides.hud_icon);
    assert!(recipe.overrides.socket_columns.is_empty());
    assert!(recipe.overrides.socket_plug_variants.is_empty());
    assert!(recipe.overrides.runtime_values.is_empty());
    assert_eq!(
        WeaponRecipe::from_json_str(&recipe.to_json_pretty().unwrap()).unwrap(),
        recipe
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
    assert!(encoded.contains(r#""rarity": "exotic""#));
    assert!(encoded.contains(r#""modern_damage_type": "void""#));
    let legacy = encoded.replace("weapon_pattern_index", "gear_art_index");
    let canonical = WeaponRecipe::from_json_str(&legacy)
        .unwrap()
        .to_json_pretty()
        .unwrap();
    assert_eq!(canonical, encoded);
    assert!(!canonical.contains("gear_art_index"));
    assert!(encoded.contains(r#""weapon_pattern_index": 285"#));
    assert!(encoded.contains(r#""weapon_pattern_donor_hash": "0xEE06B019""#));
    assert!(encoded.contains(r#""render_gear_donor""#));
    assert!(encoded.contains(r#""stat_group_index": 42"#));
    assert!(encoded.contains(r#""stat_group_donor_hash": "0xEE06B019""#));
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
    recipe.overrides.investment_stats.push(WeaponStatOverride {
        definition_index: 31,
        value: 21,
    });
    assert!(recipe.validate().is_err());
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
#[test]
fn custom_choices_sharing_a_template_round_trip_while_duplicate_definitions_are_rejected() {
    let mut recipe = WeaponRecipe::new_weapon("parhelion.custom-choice-round-trip").unwrap();
    let mut perk = crate::perk::PerkRecipe::new();
    perk.name = "Private Choice".into();
    perk.description = "A private alternative to the stock template.".into();
    perk.effects.push(crate::perk::PerkRecipe::effect(405));
    recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        socket_type: Some(92),
        choices: vec![perk.template_plug.clone(); 2],
        ..Default::default()
    })];
    assert!(
        recipe.validate().is_err(),
        "Stock duplicates remain invalid"
    );
    recipe.overrides.socket_plug_variants = vec![perk.at_socket(0, 1)];
    let saved = recipe.to_json_pretty().unwrap();
    assert_eq!(WeaponRecipe::from_json_str(&saved).unwrap(), recipe);
    recipe
        .overrides
        .socket_plug_variants
        .push(perk.at_socket(0, 0));
    assert!(
        recipe.validate().is_err(),
        "Identical private perks remain duplicates"
    );
    recipe.overrides.socket_plug_variants[1].name = Some("Different Private Choice".into());
    let saved = recipe.to_json_pretty().unwrap();
    assert_eq!(WeaponRecipe::from_json_str(&saved).unwrap(), recipe);
}

#[test]
fn variable_damage_round_trips_and_needs_a_carrier_appearance() {
    use crate::weapon::variable_damage::HARD_LIGHT_ITEM_HASH;
    let mut recipe = WeaponRecipe::new_weapon("parhelion.variable").unwrap();
    recipe.overrides.variable_damage = Some(VariableDamageRecipe {
        elements: vec![RecipeDamageType::Arc, RecipeDamageType::Solar],
    });
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Arc);

    // Nothing but Hard Light or Borealis carries the reload hold, and serialization validates.
    let error = recipe.validate().unwrap_err();
    assert!(format!("{error:?}").contains("Hard Light"), "{error:?}");
    assert!(recipe.to_json_pretty().is_err());
    recipe.presentation_donor = Some(WeaponDonorReference {
        item_hash: HexHash::new(HARD_LIGHT_ITEM_HASH),
        expected_name: Some("Hard Light".to_owned()),
    });

    let encoded = recipe.to_json_pretty().unwrap();
    assert!(encoded.contains(r#""variable_damage": {"#));
    assert!(encoded.contains(r#""elements": ["#));
    assert_eq!(WeaponRecipe::from_json_str(&encoded).unwrap(), recipe);
    assert!(
        WeaponRecipe::from_json_str(&encoded.replace(r#""elements""#, r#""members""#)).is_err()
    );
    let spec = recipe.to_spec().unwrap();
    assert_eq!(
        spec.overrides.variable_damage,
        Some(WeaponVariableDamage {
            elements: vec![ModernDamageType::Arc, ModernDamageType::Solar],
        })
    );

    // The resting element must be one of the set, and a single element is a fixed type.
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Void);
    assert!(recipe.validate().is_err());
    recipe.overrides.modern_damage_type = None;
    assert!(recipe.validate().is_ok());
    recipe.overrides.variable_damage = Some(VariableDamageRecipe {
        elements: vec![RecipeDamageType::Void],
    });
    assert!(recipe.validate().is_err());
    recipe.overrides.variable_damage = None;
    assert!(!recipe.to_json_pretty().unwrap().contains("variable_damage"));
}
