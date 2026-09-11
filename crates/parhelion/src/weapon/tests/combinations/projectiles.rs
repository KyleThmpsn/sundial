//! Cross-host projectile staging probes. These do not assert gameplay compatibility.
use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimeRootKind, WeaponRuntimeValue, load_weapon_runtime_graph_for_entity,
};

const HOSTS: &[(u32, &str)] = &[
    (0xEE06_B019, "The Mountaintop"),
    (0x7405_1969, "Truthteller"),
    (0x23DB_942F, "Age-Old Bond"),
    (0x53D5_1E72, "Agamid"),
    (0x3B16_442C, "Bad Omens"),
    (0x5038_4F33, "Coldheart"),
];

#[test]
#[ignore = "requires clean packages, control output and the user's frame recipe"]
fn native_effect_controls_stage_without_changing_stock_sources() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_EFFECT_CONTROL_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let mut frame = WeaponRecipe::load_json(PathBuf::from(
        std::env::var_os("PARHELION_FRAME_CONTROL_RECIPE").unwrap(),
    ))
    .unwrap();
    let original_overrides = frame.overrides.clone();
    frame
        .rename_authored_item("Control Established Micro-Missile Frame")
        .unwrap();
    assert_eq!(frame.overrides, original_overrides);
    let mut recipes = vec![frame];
    for (hash, name, plug, perk) in [
        (3850168899, "Martyr's Retribution", 0x53321066, 1778),
        (0x3B16442C, "Bad Omens", 0x4C0A1F31, 338),
    ] {
        let donor = catalog.weapon_donor(hash).unwrap();
        let lane = donor
            .sockets
            .iter()
            .position(|s| {
                s.native_default == Some(plug) || s.ordered_embedded_choices.contains(&plug)
            })
            .expect("native effect socket");
        for private in [false, true] {
            let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
                format!(
                    "Control {} {name}",
                    if private { "Private" } else { "Stock" }
                ),
                hash,
                name,
            )
            .unwrap();
            recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
            recipe.overrides.socket_columns[lane] = Some(WeaponSocketColumnRecipe {
                choices: vec![plug.into()],
                ..Default::default()
            });
            if private {
                let mut variant = recipes[0].overrides.socket_plug_variants[0].clone();
                variant.socket_index = lane as u16;
                variant.choice_index = 0;
                variant.source_plug_hash = plug.into();
                variant.classification_donor_hash = None;
                variant.name = Some(format!("Private {name} Control"));
                variant.description = None;
                variant.additional_sandbox_perks.clear();
                variant.investment_stats.clear();
                variant.sandbox_perks = vec![crate::WeaponSandboxPerkRuntimeRecipe {
                    program: None,
                    projectiles: Vec::new(),
                    source_perk_index: perk,
                    activation: None,
                    runtime_values: vec![],
                    action_float_values: vec![],
                }];
                recipe.overrides.socket_plug_variants = vec![variant];
            }
            recipes.push(recipe);
        }
    }
    let sword_hash = 614426548;
    let sword = catalog.weapon_donor(sword_hash).unwrap();
    for add_lunge in [false, true] {
        let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
            if add_lunge {
                "Probe Sword Melee Lunge"
            } else {
                "Control Stock Sword"
            },
            sword_hash,
            &sword.summary.name,
        )
        .unwrap();
        if add_lunge {
            let mut perks = sword.base_sandbox_perks.clone();
            perks.push(166);
            assert!(
                perks.len() <= 4,
                "lunge probe must fit definition projection"
            );
            recipe.overrides.base_sandbox_perks = Some(perks);
        }
        recipes.push(recipe);
    }
    let stock = open_manager(&packages).unwrap();
    let globals = Tables::read(&stock).globals;
    let mut identities = private::PrivateIdentities::default();
    run_batch(
        &packages,
        &output,
        "effect-controls",
        &recipes,
        |manager, tables, recipe| {
            if !recipe.overrides.socket_plug_variants.is_empty() {
                private::verify_private(manager, tables, &stock, &globals, recipe, &mut identities);
            }
            let spec = recipe.to_spec().unwrap();
            let (definition, _) = tables.load(manager, spec.identity.item_hash);
            verify_sockets(manager, tables, &definition, &spec);
            0
        },
    );
}

struct Effect {
    name: &'static str,
    plug: u32,
    perk: u16,
    projectile: Option<u32>,
    cluster: bool,
}

const EFFECTS: &[Effect] = &[
    Effect {
        name: "Micro-Missile",
        plug: 0xDD5C_B37A,
        perk: 1178,
        projectile: Some(0x8152_82E1),
        cluster: false,
    },
    Effect {
        name: "Wave Frame",
        plug: 0x5332_1066,
        perk: 1778,
        projectile: Some(0x8161_F4DE),
        cluster: false,
    },
    Effect {
        name: "Cosmology",
        plug: 0x131A_F65A,
        perk: 501,
        projectile: None,
        cluster: false,
    },
    Effect {
        name: "Cluster Bomb",
        plug: 0x4C0A_1F31,
        perk: 338,
        projectile: None,
        cluster: false,
    },
    Effect {
        name: "Micro-Missile With Cluster Bomb",
        plug: 0xDD5C_B37A,
        perk: 1178,
        projectile: Some(0x8152_82E1),
        cluster: true,
    },
    Effect {
        name: "Wave Frame With Cluster Bomb",
        plug: 0x5332_1066,
        perk: 1778,
        projectile: Some(0x8161_F4DE),
        cluster: true,
    },
];

fn speed_values(
    manager: &tiger_pkg::PackageManager,
    globals: &[u8],
    effect: &Effect,
    multiplier: f32,
) -> Vec<WeaponRuntimeValueOverride> {
    let Some(tag) = effect.projectile else {
        return Vec::new();
    };
    let action =
        load_sandbox_perk_runtime_action(manager, globals, usize::from(effect.perk)).unwrap();
    let graph = action
        .graphs
        .iter()
        .find(|graph| graph.tag.0 == tag)
        .unwrap();
    let decoded =
        load_weapon_runtime_graph_for_entity(manager, effect.plug, 0, tag, &graph.payload).unwrap();
    [
        (WeaponRuntimeRootKind::ComponentInstance, 0x8080_3B73, 0x144),
        (
            WeaponRuntimeRootKind::ComponentDefinition,
            0x8080_388F,
            0x88,
        ),
    ]
    .into_iter()
    .map(|(root, schema, offset)| {
        let fields = decoded
            .fields()
            .filter(|field| {
                field.locator.root == root
                    && field.locator.root_schema == schema
                    && field.locator.value_offset <= offset
                    && field.locator.value_offset + field.locator.byte_size >= offset + 4
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            1,
            "{} has ambiguous speed storage",
            effect.name
        );
        let field = fields[0];
        let WeaponRuntimeValue::Bytes(mut bytes) = field.value.clone() else {
            panic!("bounded speed bytes")
        };
        let at = (offset - field.locator.value_offset) as usize;
        assert_eq!(read_u32(&bytes, at).unwrap(), 1.0f32.to_bits());
        write_u32(&mut bytes, at, multiplier.to_bits()).unwrap();
        WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: WeaponRuntimeValue::Bytes(bytes),
        }
    })
    .collect()
}

fn recipe(
    catalog: &InvestmentCatalog,
    host: (u32, &str),
    effect: &Effect,
    values: &[WeaponRuntimeValueOverride],
    index: usize,
) -> WeaponRecipe {
    let donor = catalog
        .weapon_donor(host.0)
        .expect("stock projectile test host");
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        format!("Probe {} {}", effect.name, host.1),
        host.0,
        host.1,
    )
    .unwrap();
    recipe.overrides.modern_damage_type = Some(if effect.perk == 1778 {
        // Matched live controls spawn the follow-up Wave with Solar, but not Arc.
        // Keep element isolation separate from the cross-host baseline.
        RecipeDamageType::Solar
    } else {
        [
            RecipeDamageType::Arc,
            RecipeDamageType::Solar,
            RecipeDamageType::Void,
        ][index % 3]
    });
    recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
    if effect.perk == 1178 {
        let frame = WeaponRecipe::from_json_str(include_str!(
            "../../../../recipes/redacted.parhelion.json"
        ))
        .unwrap();
        let mut variant = frame.overrides.socket_plug_variants[0].clone();
        assert_eq!(variant.source_plug_hash.parse_u32().unwrap(), effect.plug);
        assert_eq!(variant.socket_index, 0);
        variant.name = Some(format!("Private {} {} Probe", effect.name, host.1));
        if effect.cluster {
            variant.additional_sandbox_perks.push(338);
        }
        recipe.overrides.socket_columns[0] = frame.overrides.socket_columns[0].clone();
        for (lane, socket) in donor.sockets.iter().enumerate().skip(1) {
            if socket.native_default == Some(effect.plug) {
                recipe.overrides.socket_columns[lane] = Some(WeaponSocketColumnRecipe {
                    socket_type: Some(u16::MAX),
                    ..Default::default()
                });
            }
        }
        recipe.overrides.socket_plug_variants = vec![variant];
        recipe.validate().unwrap();
        return recipe;
    }
    recipe
        .overrides
        .socket_columns
        .push(Some(WeaponSocketColumnRecipe {
            socket_type: Some(92),
            choices: vec![effect.plug.into()],
            ..Default::default()
        }));
    recipe
        .overrides
        .socket_plug_variants
        .push(crate::WeaponSocketPlugVariantRecipe {
            replace_effects: false,
            socket_index: u16::try_from(donor.sockets.len()).unwrap(),
            choice_index: 0,
            source_plug_hash: effect.plug.into(),
            name: Some(format!("Private {} {} Probe", effect.name, host.1)),
            description: Some(
                "A staged projectile experiment. Gameplay compatibility is unverified.".into(),
            ),
            classification_donor_hash: None,
            investment_stats: Vec::new(),
            additional_sandbox_perks: if effect.cluster {
                vec![338]
            } else {
                Vec::new()
            },
            sandbox_perks: vec![crate::WeaponSandboxPerkRuntimeRecipe {
                program: None,
                projectiles: Vec::new(),
                source_perk_index: effect.perk,
                activation: None,
                runtime_values: values.to_vec(),
                action_float_values: Vec::new(),
            }],
        });
    recipe.validate().unwrap();
    recipe
}

fn verify_stock_graphs(
    manager: &tiger_pkg::PackageManager,
    stock: &tiger_pkg::PackageManager,
    globals: &[u8],
    recipe: &WeaponRecipe,
) {
    let perk = recipe.overrides.socket_plug_variants[0].sandbox_perks[0].source_perk_index;
    let source = load_sandbox_perk_runtime_action(stock, globals, usize::from(perk)).unwrap();
    for graph in source.graphs {
        assert_eq!(
            manager.read_tag(graph.tag).unwrap(),
            graph.payload,
            "stock projectile graph changed"
        );
        let decoded =
            load_weapon_runtime_graph_for_entity(stock, 0, 0, graph.tag.0, &graph.payload).unwrap();
        for owner in decoded.owners {
            let tag = TagHash(owner.owner_tag);
            assert_eq!(
                manager.read_tag(tag).unwrap(),
                stock.read_tag(tag).unwrap(),
                "stock projectile owner changed"
            );
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_PROJECTILE_MATRIX_ROOT"]
fn native_projectile_matrix_preserves_private_speed_and_stock_sources() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_MATRIX_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let install = std::env::var_os("PARHELION_PROJECTILE_CATALOG_INSTALL")
        .map(PathBuf::from)
        .unwrap_or_else(|| packages.parent().unwrap().to_path_buf());
    let catalog = InvestmentCatalog::load(&install, false, |_| {}).unwrap();
    let stock = open_manager(&packages).unwrap();
    let globals = Tables::read(&stock).globals;
    let mut recipes = Vec::new();
    for effect in EFFECTS {
        for (index, &host) in HOSTS.iter().enumerate() {
            let values = speed_values(&stock, &globals, effect, [0.5, 2.0][index % 2]);
            recipes.push(recipe(&catalog, host, effect, &values, index));
        }
    }
    let mut identities = private::PrivateIdentities::default();
    let (build, _) = run_batch(
        &packages,
        &output,
        "projectile-matrix",
        &recipes,
        |manager, tables, recipe| {
            verify_stock_graphs(manager, &stock, &globals, recipe);
            private::verify_private(manager, tables, &stock, &globals, recipe, &mut identities);
            let spec = recipe.to_spec().unwrap();
            let (definition, _) = tables.load(manager, spec.identity.item_hash);
            verify_sockets(manager, tables, &definition, &spec);
            assert_eq!(
                weapon_damage_descriptor(&definition).unwrap(),
                WeaponDamageDescriptor::Elemental(spec.overrides.modern_damage_type.unwrap())
            );
            0
        },
    );
    assert_eq!(identities.plugs.len(), HOSTS.len() * EFFECTS.len());
    eprintln!(
        "PROJECTILE_MATRIX_PASS hosts={} effects={} weapons={} stage={}. Gameplay remains unverified.",
        HOSTS.len(),
        EFFECTS.len(),
        recipes.len(),
        build.run_directory.display()
    );
}
