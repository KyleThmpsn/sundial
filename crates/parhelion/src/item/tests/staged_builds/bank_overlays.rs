//! A weapon project whose private perk applies ability bank keys ships the sandbox packages
//! with their banks replaced. The bank edits themselves are checked in `crate::ability`.
use std::collections::BTreeSet;

use super::*;
use crate::package_profile::canonical_package;
use sundial::package_authoring::ability_bank::{
    CHARGE_ROWS, CLASS_ABILITY_CHARGE_KEY, GRENADE_SLOT, MELEE_CHARGE_KEY, MELEE_SLOT, Modifier,
    handler_slot, parameters, property_rows, slot_banks,
};
use sundial::package_authoring::sandbox_perk::program::{AbilityTuning, Action, Program, Trigger};

/// The blast radius scalar every grenade bank lists, base 1.0.
const EXPLOSION_RADIUS_SCALAR: u32 = 0x99B1_D826;

/// The stock banks the charge rows go to, each with the key it gains.
fn charge_banks() -> Vec<(u32, u32)> {
    CHARGE_ROWS
        .iter()
        .flat_map(|row| row.banks.iter().map(move |&bank| (bank, row.key)))
        .collect()
}

/// The two banks that already carry their key, Double Dodge's and The Whispers', which the
/// build must leave as they are.
const STOCK_ROW_BANKS: [u32; 2] = [0x80BC_2C8A, 0x80BC_3439];

/// Ability entities that name their bank's blocks by absolute offset, with the bank each
/// owns: Barricade, the Titan melee and one grenade ability.
const ENTITY_REFERRERS: [(u32, u32); 3] = [
    (0x80B8_00B0, 0x80BC_2BCC),
    (0x80B8_1255, 0x80BC_32F8),
    (0x80B8_0761, 0x80B8_075C),
];

/// A project whose private perk applies both charge keys and a grenade tuning ships every
/// affected sandbox package with its banks replaced, read back through a real package
/// manager over the staged install, and a project that applies nothing of the kind leaves
/// those packages alone.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_private_perk_applying_bank_keys_ships_the_banks() {
    const BREACHLIGHT_ITEM_HASH: u32 = 0x4CE3_CE93;
    const MICRO_MISSILE_PLUG_HASH: u32 = 0xDD5C_B37A;
    const MICRO_MISSILE_PERK_INDEX: u16 = 1178;
    const TRAIT_SOCKET_INDEX: usize = 4;

    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let manager = open_manager(&packages).expect("clean-stock manager should open");
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = read_tag(&manager, globals_tag, "investment globals").unwrap();
    let root_table = read_tag(
        &manager,
        TagHash(read_u32(&globals, 16).unwrap()),
        "investment root",
    )
    .unwrap();
    let item_table_tag = root_child_tag(&root_table, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap();
    let item_table = read_tag(&manager, item_table_tag, "item table").unwrap();
    let (item_count, _, item_rows, _) = array_at(&item_table, 8).unwrap();
    let breachlight_index = find_u32_row_key(
        &item_table,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        BREACHLIGHT_ITEM_HASH,
    )
    .unwrap()
    .expect("Breachlight should exist");
    let definition_tag = TagHash(
        read_u32(
            &item_table,
            item_rows + breachlight_index * ITEM_ROW_SIZE + 16,
        )
        .unwrap(),
    );
    let definition = read_tag(&manager, definition_tag, "Breachlight definition").unwrap();
    let socket_count = weapon_default_plug_indices(&definition).unwrap().len();
    assert!(TRAIT_SOCKET_INDEX < socket_count);

    let spec = |namespace: &str, program: Option<Program>| {
        let mut socket_columns = vec![None; socket_count];
        socket_columns[TRAIT_SOCKET_INDEX] = Some(WeaponSocketColumnOverride {
            choices: vec![MICRO_MISSILE_PLUG_HASH],
            ..WeaponSocketColumnOverride::default()
        });
        WeaponCloneSpec {
            kind: crate::ItemKind::Weapon,
            namespace: namespace.to_owned(),
            donor_item_hash: BREACHLIGHT_ITEM_HASH,
            expected_donor_name: Some("Breachlight".to_owned()),
            presentation_donor: None,
            icon_donor: None,
            render_gear_donor: None,
            runtime_component_donors: Vec::new(),
            identity: WeaponCloneIdentity::from_namespace(namespace)
                .expect("test namespace should allocate"),
            text: WeaponCloneText {
                name: "Bank Row Fixture".to_owned(),
                flavor: "An ability bank fixture.".to_owned(),
                source: "Source: integration test".to_owned(),
                ..WeaponCloneText::default()
            },
            overrides: WeaponCloneOverrides {
                socket_columns,
                socket_plug_variants: vec![WeaponSocketPlugVariantOverride {
                    replace_effects: false,
                    investment_stats: Vec::new(),
                    socket_index: TRAIT_SOCKET_INDEX as u16,
                    choice_index: 0,
                    source_plug_hash: MICRO_MISSILE_PLUG_HASH,
                    icon: None,
                    classification_donor_hash: None,
                    description: None,
                    additional_sandbox_perks: Vec::new(),
                    name: Some("Bank Rows".to_owned()),
                    sandbox_perks: vec![WeaponSandboxPerkRuntimeOverride {
                        program,
                        projectiles: Vec::new(),
                        source_perk_index: MICRO_MISSILE_PERK_INDEX,
                        activation: None,
                        runtime_values: Vec::new(),
                        action_float_values: Vec::new(),
                    }],
                }],
                ..WeaponCloneOverrides::default()
            },
        }
    };
    let build = |spec: WeaponCloneSpec| {
        build_weapon_project_after_catalog_validation(
            &packages,
            &WeaponProjectSpec {
                weapons: vec![spec],
            },
        )
    };

    let tuning = AbilityTuning::new(GRENADE_SLOT, EXPLOSION_RADIUS_SCALAR, 0.5, true);
    let scalar = Modifier::Parameter {
        name: EXPLOSION_RADIUS_SCALAR,
        applied: 0.5,
        add: true,
    };
    // The packages of the banks the build can give a row: those whose stock rows show a slot
    // for the modifier, and for the tuning, those that list the parameter.
    let affected: BTreeSet<u16> = charge_banks()
        .iter()
        .map(|(bank, _)| (*bank, Modifier::Charges(1)))
        .chain(slot_banks(GRENADE_SLOT).iter().map(|&bank| (bank, scalar)))
        .filter(|(bank, modifier)| {
            let stock = read_tag(&manager, TagHash(*bank), "stock bank").unwrap();
            let listed = match modifier {
                Modifier::Parameter { name, .. } => parameters(&stock)
                    .unwrap()
                    .iter()
                    .any(|parameter| parameter.name == *name),
                Modifier::Charges(_) | Modifier::Melee { .. } | Modifier::Scalar { .. } => true,
            };
            listed && handler_slot(&stock, *modifier).unwrap().is_some()
        })
        .map(|(bank, _)| TagHash(bank).pkg_id())
        .collect();

    assert!(!affected.is_empty());
    // Without the keys, none of those packages is touched.
    let control = build(spec("parhelion.ability-banks.control", None))
        .expect("the control project should build");
    assert!(
        control
            .artifacts
            .iter()
            .all(|artifact| !affected.contains(&artifact.plan.chain.identity.package_id)),
        "a project applying no bank key must not replace any bank"
    );

    let program = Program {
        trigger: Trigger::Equipped,
        actions: vec![
            Action::AbilityProperty {
                target: 7.into(),
                key: CLASS_ABILITY_CHARGE_KEY,
                option: 0.into(),
            },
            Action::AbilityProperty {
                target: MELEE_SLOT,
                key: MELEE_CHARGE_KEY,
                option: 0.into(),
            },
            Action::AbilityProperty {
                target: GRENADE_SLOT,
                key: tuning.key,
                option: 0.into(),
            },
        ],
        ability_tunings: vec![tuning.clone()],
        ..Program::default()
    };
    program.validate().expect("the program should validate");
    let bundle = build(spec("parhelion.ability-banks.rows", Some(program)))
        .expect("the bank row project should build");
    overlays_keep_their_tables(&bundle, &affected);
    let view = staged_view(&packages, "parhelion-ability-banks-", &bundle);
    let staged = open_manager(&view.path().join("packages")).expect("staged view should open");
    staged_banks_carry_their_rows(&manager, &staged, &tuning);
}

/// One overlay per affected package, each keeping its entry table and canonical name.
fn overlays_keep_their_tables(bundle: &NewWeaponProjectBundle, affected: &BTreeSet<u16>) {
    for package_id in affected {
        let overlay = bundle
            .artifacts
            .iter()
            .find(|artifact| artifact.plan.chain.identity.package_id == *package_id)
            .unwrap_or_else(|| panic!("the project should replace package {package_id:04x}"));
        assert_eq!(
            overlay.plan.original_entry_count,
            overlay.plan.final_entry_count
        );
        assert!(overlay.plan.appended_tags.is_empty());
        assert_eq!(
            overlay.plan.output_file_name,
            canonical_package(*package_id).unwrap().authored_file_name
        );
    }
}

/// Every bank read back through the staged install carries its rows on the handler slot its
/// stock rows use, a bank with no slot for the modifier stays stock, and the banks that
/// carry their key already are byte-identical to stock.
fn staged_banks_carry_their_rows(
    manager: &PackageManager,
    staged: &PackageManager,
    tuning: &AbilityTuning,
) {
    check_charge_rows(manager, staged);
    for &bank in slot_banks(GRENADE_SLOT) {
        let rows = property_rows(&read_tag(staged, TagHash(bank), "staged grenade bank").unwrap())
            .unwrap_or_else(|error| panic!("{bank:08X}: {error}"));
        let row = rows
            .iter()
            .find(|row| row.key == tuning.key)
            .unwrap_or_else(|| panic!("{bank:08X} has no tuning row"));
        let slot = handler_slot(
            &read_tag(manager, TagHash(bank), "stock grenade bank").unwrap(),
            Modifier::Parameter {
                name: EXPLOSION_RADIUS_SCALAR,
                applied: 0.5,
                add: true,
            },
        )
        .unwrap()
        .unwrap_or_else(|| panic!("{bank:08X} has no script slot"));
        assert_eq!(row.handler, slot, "{bank:08X}");
        assert_eq!(row.parameters.len(), 1, "{bank:08X}");
        assert_eq!(
            (
                row.parameters[0].name,
                row.parameters[0].applied,
                row.parameters[0].add
            ),
            (EXPLOSION_RADIUS_SCALAR, 0.5, true),
            "{bank:08X}"
        );
    }
    for bank in STOCK_ROW_BANKS {
        assert_eq!(
            read_tag(staged, TagHash(bank), "untouched bank").unwrap(),
            read_tag(manager, TagHash(bank), "stock bank").unwrap(),
            "{bank:08X} carries its key already and must stay stock"
        );
    }
    // The entity that owns each moved bank ships with its offsets into the bank moved.
    for (entity, bank) in ENTITY_REFERRERS {
        let stock_bank = read_tag(manager, TagHash(bank), "stock bank").unwrap();
        let staged_bank = read_tag(staged, TagHash(bank), "staged bank").unwrap();
        let stock_entity = read_tag(manager, TagHash(entity), "stock entity").unwrap();
        let staged_entity = read_tag(staged, TagHash(entity), "staged entity").unwrap();
        let old_instance = relative_target(&stock_bank, 0x18).unwrap();
        let new_instance = relative_target(&staged_bank, 0x18).unwrap();
        assert!(new_instance > old_instance);
        let delta = new_instance - old_instance;
        let mut normalized = staged_entity.clone();
        let mut checked = 0;
        for at in (0..stock_entity.len().saturating_sub(15)).step_by(8) {
            let class = read_u32(&stock_entity, at + 4).unwrap();
            let offset = read_u64(&stock_entity, at + 8).unwrap() as usize;
            if read_u32(&stock_entity, at).unwrap() == bank
                && (0x8080_0000..=0x8080_FFFF).contains(&class)
                && (old_instance..stock_bank.len()).contains(&offset)
                && offset % 8 == 0
            {
                assert_eq!(
                    read_u64(&staged_entity, at + 8).unwrap(),
                    (offset + delta) as u64
                );
                normalized[at + 8..at + 16].copy_from_slice(&stock_entity[at + 8..at + 16]);
                checked += 1;
            }
        }
        assert!(checked > 0, "{entity:08X} needs a moved bank reference");
        assert_eq!(
            normalized, stock_entity,
            "unrelated entity bytes must survive"
        );
    }
}

fn check_charge_rows(manager: &PackageManager, staged: &PackageManager) {
    for (bank, key) in charge_banks() {
        let staged_bank = read_tag(staged, TagHash(bank), "staged bank").unwrap();
        let stock_bank = read_tag(manager, TagHash(bank), "stock bank").unwrap();
        let Some(slot) = handler_slot(&stock_bank, Modifier::Charges(1)).unwrap() else {
            assert_eq!(
                staged_bank, stock_bank,
                "{bank:08X} has no charge slot and must stay stock"
            );
            continue;
        };
        let rows =
            property_rows(&staged_bank).unwrap_or_else(|error| panic!("{bank:08X}: {error}"));
        let stock = property_rows(&stock_bank).unwrap();
        assert_eq!(rows.len(), stock.len() + 1, "{bank:08X}");
        assert_eq!(&rows[..stock.len()], &stock[..], "{bank:08X}");
        let row = rows.last().unwrap();
        assert_eq!(
            (row.key, row.charge, row.handler),
            (key, Some(1), slot),
            "{bank:08X}"
        );
    }
}
