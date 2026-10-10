//! An attached effect's length compiles into a private copy of its timer's owner: Devour's
//! 11 seconds become 20 in the four lanes at the nest's +0x9E0, and nothing else in the copy
//! but the owner's own retargeted words moves. Invisibility's timer scales an input first, so
//! its length is the constant the program adds, and Unlimited compiles into the form of the
//! stock timers that never end.
use super::*;
use sundial::package_authoring::{
    runtime::{
        WeaponRuntimeValueOverride, load_weapon_runtime_graph_for_entity,
        resolve_weapon_runtime_field,
    },
    sandbox_perk::entity::effect_length::{EffectLength, UNLIMITED, discover, seconds},
};

const DEVOUR: u32 = 0x80B8_0AA0;
const DEVOUR_NEST: u32 = 0x80BC_2F97;
const DEVOUR_LENGTH_OFFSET: usize = 0x9E0;
const INVISIBILITY: u32 = 0x80BC_57CB;
const INVISIBILITY_NEST: u32 = 0x8153_2CA9;
/// The nest's timer definition is at +0x750. Its byte at +0x70 is the Unlimited flag, which
/// the client tests when the length is zero or less, and its program's constants are the
/// input's scale, 2.0, then the seconds it adds, 6.0.
const INVISIBILITY_FLAG_OFFSET: usize = 0x7C0;
const INVISIBILITY_SCALE_OFFSET: usize = 0xA60;
const INVISIBILITY_LENGTH_OFFSET: usize = 0xA70;

fn clean_packages() -> PathBuf {
    crate::test_support::stock_packages()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_hop_on_effect_lengths_compile_into_private_owners() {
    let manager = open_manager(&clean_packages()).unwrap();
    devour_length_compiles(&manager);
    invisibility_length_reads(&manager);
    invisibility_unlimited_compiles(&manager);
}

/// Devour's length is 11 seconds on its nest, and an edit to 20 compiles into a private copy
/// of the nest that differs from stock in the four lanes and its own retargeted words alone.
fn devour_length_compiles(manager: &PackageManager) {
    let source = read_tag(manager, TagHash(DEVOUR), "Devour").unwrap();
    let mut graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, DEVOUR, &source).unwrap();
    graph.scope_fields();
    let lengths = discover(&graph);
    assert_eq!(lengths.len(), 1, "{lengths:#?}");
    let devour = &lengths[0];
    assert_eq!(devour.owner_tag, DEVOUR_NEST);
    assert_eq!(devour.per_input, None);
    assert_eq!(devour.stock(), 11.0);

    // The edit is one override on the constant row, which resolves to the stock bytes.
    let edit = WeaponRuntimeValueOverride {
        locator: devour.field.locator.clone(),
        value: EffectLength::encode(20.0),
    };
    let resolved = resolve_weapon_runtime_field(
        manager,
        &source,
        &edit.locator.for_graph(DEVOUR.into()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (resolved.owner_tag, resolved.owner_offset),
        (DEVOUR_NEST, DEVOUR_LENGTH_OFFSET)
    );

    // The build copies the owner with the four lanes rewritten, and the entity after it.
    let allocator = AppendedTagAllocator::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, 7000);
    let mut tags = Vec::new();
    append_private_referenced_graph(manager, TagHash(DEVOUR), &[edit], allocator, &mut tags)
        .unwrap();
    assert_eq!(tags.len(), 2, "the edited owner and the entity copy");
    assert_eq!(tags[0].template_tag.0, DEVOUR_NEST);
    assert_eq!(tags[1].template_tag.0, DEVOUR);
    let stock = read_tag(manager, TagHash(DEVOUR_NEST), "stock nest").unwrap();
    let mut expected = stock.clone();
    expected[DEVOUR_LENGTH_OFFSET..DEVOUR_LENGTH_OFFSET + 16]
        .copy_from_slice(&[20.0_f32.to_le_bytes(); 4].concat());
    let authored = allocator.assigned_tag(0, "test", "owner").unwrap();
    retarget_weapon_component_owner_payload(&mut expected, &source, DEVOUR_NEST, authored.0)
        .unwrap();
    assert_eq!(tags[0].payload, expected);
    assert_eq!(
        read_tag(manager, TagHash(DEVOUR_NEST), "unchanged stock nest").unwrap(),
        stock
    );
}

/// Invisibility's timer scales an input first, so its length is the constant the program adds.
fn invisibility_length_reads(manager: &PackageManager) {
    let source = read_tag(manager, TagHash(INVISIBILITY), "Invisibility").unwrap();
    let mut graph =
        load_weapon_runtime_graph_for_entity(manager, 0, 0, INVISIBILITY, &source).unwrap();
    graph.scope_fields();
    let lengths = discover(&graph);
    assert_eq!(lengths.len(), 1, "{lengths:#?}");
    assert_eq!(lengths[0].owner_tag, INVISIBILITY_NEST);
    assert_eq!(lengths[0].per_input, Some(2.0));
    assert_eq!(lengths[0].stock(), 6.0);
    assert_eq!(seconds(&lengths[0].field.value), Some(6.0));
}

/// Invisibility's timer has its Unlimited flag clear, so -1 alone would end it at once, and its
/// input could still make the length positive. Unlimited compiles into -1 in the length's four
/// lanes, zero in the scale's and the flag set, and nothing else in the copy but the owner's own
/// retargeted words moves.
fn invisibility_unlimited_compiles(manager: &PackageManager) {
    let source = read_tag(manager, TagHash(INVISIBILITY), "Invisibility").unwrap();
    let mut graph =
        load_weapon_runtime_graph_for_entity(manager, 0, 0, INVISIBILITY, &source).unwrap();
    graph.scope_fields();
    let lengths = discover(&graph);
    let length = &lengths[0];
    let scale = length.input_scale.as_ref().expect("the input's scale");
    let flag = length
        .unlimited
        .as_ref()
        .expect("the timer's Unlimited flag");
    assert_eq!(
        flag.field.owner_offset as usize + flag.at,
        INVISIBILITY_FLAG_OFFSET
    );
    assert!(!flag.stock() && !length.stock_unlimited());
    let edits = [
        (&length.field, EffectLength::encode(UNLIMITED)),
        (scale, EffectLength::encode(0.0)),
        (&flag.field, flag.with(&flag.field.value, true).unwrap()),
    ]
    .map(|(field, value)| WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value,
    });

    let allocator = AppendedTagAllocator::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, 7000);
    let mut tags = Vec::new();
    append_private_referenced_graph(manager, TagHash(INVISIBILITY), &edits, allocator, &mut tags)
        .unwrap();
    assert_eq!(tags.len(), 2, "the edited owner and the entity copy");
    assert_eq!(tags[0].template_tag.0, INVISIBILITY_NEST);
    let stock = read_tag(manager, TagHash(INVISIBILITY_NEST), "stock nest").unwrap();
    assert_eq!(stock[INVISIBILITY_FLAG_OFFSET], 0);
    let mut expected = stock.clone();
    let lanes = |seconds: f32| [seconds.to_le_bytes(); 4].concat();
    expected[INVISIBILITY_SCALE_OFFSET..INVISIBILITY_SCALE_OFFSET + 16]
        .copy_from_slice(&lanes(0.0));
    expected[INVISIBILITY_LENGTH_OFFSET..INVISIBILITY_LENGTH_OFFSET + 16]
        .copy_from_slice(&lanes(UNLIMITED));
    expected[INVISIBILITY_FLAG_OFFSET] = 1;
    let authored = allocator.assigned_tag(0, "test", "owner").unwrap();
    retarget_weapon_component_owner_payload(&mut expected, &source, INVISIBILITY_NEST, authored.0)
        .unwrap();
    assert_eq!(tags[0].payload, expected);
}
