//! Package-backed checks of the bank edits on the stock banks the build replaces. The
//! project build that ships them is checked with the weapon staged builds.
use std::path::PathBuf;

use sundial::package_authoring::{
    PackageManager,
    ability_bank::{
        CHARGE_DEFINITION_CLASS, CHARGE_ROWS, CLASS_ABILITY_CHARGE_KEY, GRENADE_SLOT,
        MELEE_CHARGE_KEY, Modifier, Parameter, SCRIPT_DEFINITION_CLASS, handler_slot,
        instance_shift, parameters, property_rows, retarget_references, slot_banks, validate,
        with_charge_row, with_property_row,
    },
    open_shadowkeep_package_manager,
    sandbox_perk::program::AbilityTuning,
};
use tiger_pkg::TagHash;

/// The blast radius scalar every grenade bank lists, base 1.0.
const EXPLOSION_RADIUS_SCALAR: u32 = 0x99B1_D826;

/// The stock banks the charge rows go to, each with the key it gains.
fn charge_banks() -> Vec<(u32, u32)> {
    CHARGE_ROWS
        .iter()
        .flat_map(|row| row.banks.iter().map(move |&bank| (bank, row.key)))
        .collect()
}

/// Ability entities that name their bank's blocks by absolute offset: Barricade, the Titan
/// melee and one grenade ability, with the bank each owns.
const ENTITY_REFERRERS: [(u32, u32); 3] = [
    (0x80B8_00B0, 0x80BC_2BCC),
    (0x80B8_1255, 0x80BC_32F8),
    (0x80B8_0761, 0x80B8_075C),
];

/// The sixteen byte references a tag holds into `bank`: their position, class and offset.
fn offset_references(payload: &[u8], bank: u32) -> Vec<(usize, u32, u64)> {
    (0..payload.len().saturating_sub(15))
        .step_by(8)
        .filter_map(|at| {
            let tag = u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
            let class = u32::from_le_bytes(payload[at + 4..at + 8].try_into().unwrap());
            let offset = u64::from_le_bytes(payload[at + 8..at + 16].try_into().unwrap());
            (tag == bank && (0x8080_0000..=0x8080_FFFF).contains(&class))
                .then_some((at, class, offset))
        })
        .collect()
}

/// The two banks that already carry their key, Double Dodge's and The Whispers', which the
/// build must leave as they are.
const STOCK_ROW_BANKS: [(u32, u32); 2] = [
    (0x80BC_2C8A, CLASS_ABILITY_CHARGE_KEY),
    (0x80BC_3439, MELEE_CHARGE_KEY),
];

fn clean_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    )
}

/// Every bank the build replaces takes its row on the handler slot its own charge rows use
/// and reads back through the client's own path as its stock rows plus the new one, a bank
/// with no charge slot is refused, a bank that already has the key is refused, and a
/// parameter the bank does not list is refused.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_stock_banks_take_charge_and_parameter_rows() {
    let manager = open_shadowkeep_package_manager(&clean_packages())
        .expect("clean-stock manager should open");
    charge_rows_land_once(&manager);
    stock_charge_rows_stay(&manager);
    parameter_rows_land_on_every_grenade_bank(&manager);
    entity_references_follow_the_moved_instance_region(&manager);
}

/// The entity's references at or past the bank's instance block move by the edit's shift,
/// the ones before it and everything else in the entity stay, and their count holds.
fn entity_references_follow_the_moved_instance_region(manager: &PackageManager) {
    for (entity, bank) in ENTITY_REFERRERS {
        let stock = manager.read_tag(TagHash(bank)).unwrap();
        let edited = if bank == 0x80B8_075C {
            let tuning = AbilityTuning::new(GRENADE_SLOT, EXPLOSION_RADIUS_SCALAR, 0.5, true);
            with_property_row(
                &stock,
                tuning.key,
                Modifier::Parameter {
                    name: EXPLOSION_RADIUS_SCALAR,
                    applied: 0.5,
                    add: true,
                },
            )
            .unwrap()
        } else {
            let key = charge_banks()
                .into_iter()
                .find(|(candidate, _)| *candidate == bank)
                .map(|(_, key)| key)
                .unwrap();
            with_charge_row(&stock, key).unwrap()
        };
        let (instance, delta) = instance_shift(&stock, &edited).unwrap();
        assert!(delta > 0 && delta % 16 == 0, "{bank:08X} shift {delta}");
        let before = manager.read_tag(TagHash(entity)).unwrap();
        let after = retarget_references(&before, bank, &stock, &edited)
            .unwrap()
            .unwrap_or_else(|| {
                panic!("{entity:08X} has no reference past {bank:08X}'s instance block")
            });
        assert_eq!(before.len(), after.len(), "{entity:08X}");
        let was = offset_references(&before, bank);
        let now = offset_references(&after, bank);
        assert!(
            was.iter()
                .any(|&(_, _, offset)| offset as usize >= instance),
            "{entity:08X}"
        );
        assert_eq!(was.len(), now.len(), "{entity:08X}");
        for (&(at, class, offset), &(at_now, class_now, offset_now)) in was.iter().zip(&now) {
            assert_eq!((at, class), (at_now, class_now), "{entity:08X}");
            let expected = if offset as usize >= instance {
                offset + delta as u64
            } else {
                offset
            };
            assert_eq!(offset_now, expected, "{entity:08X} reference at {at:#x}");
        }
        let untouched = (0..before.len())
            .filter(|&at| before[at] != after[at])
            .all(|at| {
                was.iter()
                    .any(|&(start, _, _)| (start + 8..start + 16).contains(&at))
            });
        assert!(untouched, "{entity:08X} changed outside its references");
    }
}

fn charge_rows_land_once(manager: &PackageManager) {
    for (bank, key) in charge_banks() {
        let stock = manager.read_tag(TagHash(bank)).unwrap();
        validate(&stock).unwrap_or_else(|error| panic!("{bank:08X}: {error}"));
        let before = property_rows(&stock).unwrap();
        assert!(
            before.iter().all(|row| row.key != key),
            "{bank:08X} already carries {key:08X}"
        );
        let Some(slot) = handler_slot(&stock, Modifier::Charges(1)).unwrap() else {
            assert!(
                with_charge_row(&stock, key).is_err(),
                "{bank:08X} took a charge row with no slot to hand it to"
            );
            continue;
        };
        assert!(
            before
                .iter()
                .filter(|row| row.modifier_class == CHARGE_DEFINITION_CLASS)
                .all(|row| row.handler == slot),
            "{bank:08X} hands charges to several slots"
        );
        let edited =
            with_charge_row(&stock, key).unwrap_or_else(|error| panic!("{bank:08X}: {error}"));
        validate(&edited).unwrap_or_else(|error| panic!("{bank:08X} edited: {error}"));
        let after = property_rows(&edited).unwrap();
        assert_eq!(after.len(), before.len() + 1, "{bank:08X}");
        assert_eq!(&after[..before.len()], &before[..], "{bank:08X}");
        let row = after.last().unwrap();
        assert_eq!(
            (row.key, row.modifier_class, row.charge, row.handler),
            (key, CHARGE_DEFINITION_CLASS, Some(1), slot),
            "{bank:08X}"
        );
        // The definition block keeps its bytes up to its rows descriptor, apart from the twin
        // offset that follows the moved instance block.
        assert_eq!(&edited[0x80..0x88], &stock[0x80..0x88], "{bank:08X}");
        assert_eq!(
            &edited[0x90..0x80 + 0x170],
            &stock[0x90..0x80 + 0x170],
            "{bank:08X}"
        );
        assert!(
            with_charge_row(&edited, key).is_err(),
            "{bank:08X} took the key twice"
        );
    }
}

fn stock_charge_rows_stay(manager: &PackageManager) {
    for (bank, key) in STOCK_ROW_BANKS {
        let stock = manager.read_tag(TagHash(bank)).unwrap();
        let rows = property_rows(&stock).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.key == key && row.charge == Some(1)),
            "{bank:08X} lost its stock charge row"
        );
        assert!(
            with_charge_row(&stock, key).is_err(),
            "{bank:08X} took a second row"
        );
    }
}

/// The scalar keeps the bank's reset value, takes the applied value, and the table grows by
/// that one entry.
fn parameter_rows_land_on_every_grenade_bank(manager: &PackageManager) {
    let tuning = AbilityTuning::new(GRENADE_SLOT, EXPLOSION_RADIUS_SCALAR, 0.5, true);
    let modifier = Modifier::Parameter {
        name: EXPLOSION_RADIUS_SCALAR,
        applied: 0.5,
        add: true,
    };
    for &bank in slot_banks(GRENADE_SLOT) {
        let stock = manager.read_tag(TagHash(bank)).unwrap();
        let table = parameters(&stock).unwrap();
        let listed = table
            .iter()
            .find(|parameter| parameter.name == EXPLOSION_RADIUS_SCALAR)
            .unwrap_or_else(|| panic!("{bank:08X} does not list the blast radius scalar"));
        let edited = with_property_row(&stock, tuning.key, modifier)
            .unwrap_or_else(|error| panic!("{bank:08X}: {error}"));
        validate(&edited).unwrap_or_else(|error| panic!("{bank:08X} edited: {error}"));
        let rows = property_rows(&edited).unwrap();
        let row = rows.last().unwrap();
        let slot = handler_slot(&stock, modifier)
            .unwrap()
            .unwrap_or_else(|| panic!("{bank:08X} has no script slot"));
        assert!(
            rows[..rows.len() - 1]
                .iter()
                .filter(|row| row.modifier_class == SCRIPT_DEFINITION_CLASS)
                .all(|row| row.handler == slot),
            "{bank:08X} hands script parameters to several slots"
        );
        assert_eq!(
            (row.key, row.modifier_class, row.handler),
            (tuning.key, SCRIPT_DEFINITION_CLASS, slot),
            "{bank:08X}"
        );
        assert_eq!(
            row.parameters,
            vec![Parameter {
                name: EXPLOSION_RADIUS_SCALAR,
                reset: listed.reset,
                applied: 0.5,
                add: true,
            }],
            "{bank:08X}"
        );
        let grown = parameters(&edited).unwrap();
        assert_eq!(&grown[..table.len()], &table[..], "{bank:08X}");
        assert_eq!(grown.len(), table.len() + 1, "{bank:08X}");
        assert!(
            with_property_row(
                &stock,
                tuning.key,
                Modifier::Parameter {
                    name: 0xDEAD_BEEF,
                    applied: 1.0,
                    add: false,
                }
            )
            .is_err(),
            "{bank:08X} took a parameter it does not list"
        );
    }
}
