//! An attached effect's length compiles into a private copy of its timer's owner: Devour's
//! 11 seconds become 20 in the four lanes at the nest's +0x9E0, and nothing else in the copy
//! but the owner's own retargeted words moves. Invisibility's timer scales an input first, so
//! its length is the constant the program adds.
use super::*;
use sundial::package_authoring::{
    runtime::{
        WeaponRuntimeValueOverride, load_weapon_runtime_graph_for_entity,
        resolve_weapon_runtime_field,
    },
    sandbox_perk::entity::effect_length::{EffectLength, discover, seconds},
};

const DEVOUR: u32 = 0x80B8_0AA0;
const DEVOUR_NEST: u32 = 0x80BC_2F97;
const DEVOUR_LENGTH_OFFSET: usize = 0x9E0;
const INVISIBILITY: u32 = 0x80BC_57CB;
const INVISIBILITY_NEST: u32 = 0x8153_2CA9;

fn clean_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    )
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_hop_on_effect_lengths_compile_into_private_owners() {
    let manager = open_manager(&clean_packages()).unwrap();
    devour_length_compiles(&manager);
    invisibility_length_reads(&manager);
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
