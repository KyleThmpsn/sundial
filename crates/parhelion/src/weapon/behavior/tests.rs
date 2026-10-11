//! Synthetic and package-backed checks of the catalogue, grafts, sockets and firing.
use super::*;

/// Builds a synthetic owner holding one block with a label, state and behavior triple.
fn owner_with_triple(behavior_record: bool) -> (Vec<u8>, usize) {
    let mut owner = vec![0_u8; 0x400];
    let block = 0x40;
    owner[block + 8..block + 12].copy_from_slice(&0x1C0_u32.to_le_bytes());
    owner[block + 0x10..block + 0x14].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
    for (at, class) in [
        (0x200_usize, LABEL_ARRAY_CLASS),
        (0x240, STATE_ARRAY_CLASS),
        (0x280, BEHAVIOR_ARRAY_CLASS),
    ] {
        owner[at - 4..at].copy_from_slice(&ARRAY_HEADER_CLASS.to_le_bytes());
        owner[at + 8..at + 12].copy_from_slice(&class.to_le_bytes());
    }
    let triple = block + 0x170;
    for (slot, target, present) in [
        (triple, 0x200_usize, true),
        (triple + SLOT_STRIDE, 0x240, true),
        (triple + SLOT_STRIDE * 2, 0x280, behavior_record),
    ] {
        if present {
            let relative = target as i64 - slot as i64;
            owner[slot..slot + 8].copy_from_slice(&relative.to_le_bytes());
        }
    }
    (owner, block)
}

/// A synthetic owner whose one block carries these labels, with the arrays spaced so rows
/// never overlap the next array's marker.
fn owner_with_labels(labels: &[u32]) -> (Vec<u8>, usize) {
    let mut owner = vec![0_u8; 0x400];
    let block = 0x40;
    owner[block + 8..block + 12].copy_from_slice(&0x1C0_u32.to_le_bytes());
    owner[block + 0x10..block + 0x14].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
    for (at, class) in [
        (0x200_usize, LABEL_ARRAY_CLASS),
        (0x300, STATE_ARRAY_CLASS),
        (0x340, BEHAVIOR_ARRAY_CLASS),
    ] {
        owner[at - 4..at].copy_from_slice(&ARRAY_HEADER_CLASS.to_le_bytes());
        owner[at + 8..at + 12].copy_from_slice(&class.to_le_bytes());
    }
    owner[0x200..0x208].copy_from_slice(&(labels.len() as u64).to_le_bytes());
    for (index, label) in labels.iter().enumerate() {
        let row = 0x210 + index * LABEL_ROW_SIZE;
        owner[row..row + 4].copy_from_slice(&label.to_le_bytes());
        owner[row + 16..row + 20].copy_from_slice(&0x80C7_0CA1_u32.to_le_bytes());
    }
    let triple = block + 0x170;
    for (slot, target, count) in [
        (triple, 0x200_usize, labels.len() as i64),
        (triple + SLOT_STRIDE, 0x300, 1),
        (triple + SLOT_STRIDE * 2, 0x340, 0),
    ] {
        let relative = target as i64 - slot as i64;
        owner[slot..slot + 8].copy_from_slice(&relative.to_le_bytes());
        owner[slot + 8..slot + 16].copy_from_slice(&count.to_le_bytes());
    }
    (owner, block)
}

const PULSE_RIFLE: u32 = 0x937F_E7FA;
const AUTO_RIFLE: u32 = 0xDEE5_98FE;
const BUCKET_1: u32 = 0x2E65_81A6;
const BUCKET_2: u32 = 0x2E65_81A5;

#[test]
fn an_absent_label_array_is_optional_but_a_malformed_one_is_an_error() {
    let (mut owner, block) = owner_with_labels(&[PULSE_RIFLE]);
    let triple = first_triple(&owner, block).unwrap();
    owner[triple..triple + 8].fill(0);
    assert!(optional_label_rows(&owner, block).unwrap().is_none());

    let (mut owner, block) = owner_with_labels(&[PULSE_RIFLE]);
    let triple = first_triple(&owner, block).unwrap();
    owner[triple + 8..triple + 16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(optional_label_rows(&owner, block).is_err());
}

#[test]
fn a_graft_appends_the_source_labels_the_host_lacks_and_keeps_the_hosts_type() {
    let (host, host_block) = owner_with_labels(&[AUTO_RIFLE, BUCKET_1]);
    let (source, source_block) = owner_with_labels(&[PULSE_RIFLE, BUCKET_2]);
    let content = content_for(host, host_block);
    let append = label_append(&content, host_block, None, &[(source, source_block)])
        .unwrap()
        .expect("bucket 2 is new to the host");
    assert_eq!(append.binding_hash, BINDING);
    assert_eq!(append.slots, vec![(0x1B0, 4, 3)]);
    assert_eq!(u32_at(&append.bytes, 0).unwrap(), ARRAY_HEADER_CLASS);
    assert_eq!(u64_at(&append.bytes, 4).unwrap(), 3);
    assert_eq!(u32_at(&append.bytes, 12).unwrap(), LABEL_ARRAY_CLASS);
    let labels = (0..3)
        .map(|index| u32_at(&append.bytes, 20 + index * LABEL_ROW_SIZE).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec![AUTO_RIFLE, BUCKET_1, BUCKET_2]);
    assert_eq!(append.bytes.len(), 20 + 3 * LABEL_ROW_SIZE);
}

fn content_for(owner: Vec<u8>, block: usize) -> Content {
    Content {
        owner_tag: 0x8152_9461,
        owner,
        resource: 0,
        blocks: vec![block],
    }
}

#[test]
fn a_block_without_a_state_array_is_rejected() {
    let mut owner = vec![0_u8; 0x400];
    owner[8..12].copy_from_slice(&0x1C0_u32.to_le_bytes());
    assert!(first_triple(&owner, 0).is_err());
}

#[test]
fn resolve_rejects_a_source_without_a_behavior_record() {
    let (owner, block) = owner_with_triple(false);
    assert!(resolve(&content_for(owner, block), 0xAABB_CCDD, true).is_err());
}

#[test]
fn resolve_rejects_a_content_group_the_owner_does_not_hold() {
    let (owner, block) = owner_with_triple(true);
    assert!(resolve(&content_for(owner, block), 0x1234_5678, true).is_err());
}

/// Pins the offsets confirmed in game, so a change to the reader cannot silently move them.
/// Hard Light and SUROS Regime share the auto rifle content owner.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn the_element_switch_graft_matches_the_offsets_verified_in_game() {
    use sundial::package_authoring::open_shadowkeep_package_manager;
    const AUTO_RIFLE_OWNER: u32 = 0x8152_9461;
    const SUROS_GROUP: u32 = 0xA581_883B;
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let owner = manager.read_tag(TagHash(AUTO_RIFLE_OWNER)).unwrap();
    let resource = 0x80_usize;
    let definition = usize::try_from(u64_at(&owner, resource + 8).unwrap()).unwrap();
    let blocks = crate::weapon::ammo::property_offsets(&owner, definition).unwrap();
    let content = Content {
        owner_tag: AUTO_RIFLE_OWNER,
        owner,
        resource,
        blocks,
    };

    // Hard Light's block holds the records at the offsets the in-game trials used.
    let source = behavior("hard-light").unwrap();
    let BehaviorSource::Record { content_group, .. } = source.source else {
        panic!("Hard Light's element switch is a record");
    };
    let graft = resolve(&content, content_group, true).unwrap();
    assert_eq!(graft.state_target, 0xC520);
    assert_eq!(graft.behavior_target, Some(0xC540));

    // SUROS Regime's block exposes its triple where the verified patches wrote.
    let target = block_for_group(&content, SUROS_GROUP).unwrap();
    assert_eq!(target, 0x1620);
    let triple = first_triple(&content.owner, target).unwrap();
    assert_eq!(triple, target + 0x170);
    assert_eq!(triple + SLOT_STRIDE - content.resource, 0x1720);
    assert_eq!(triple + SLOT_STRIDE * 2 - content.resource, 0x1730);
    assert_eq!(
        slot_bytes(graft.state_target, triple + SLOT_STRIDE, 1).unwrap(),
        hex_bytes("80AD0000000000000100000000000000")
    );
    assert_eq!(
        slot_bytes(graft.behavior_target.unwrap(), triple + SLOT_STRIDE * 2, 0,).unwrap(),
        hex_bytes("90AD0000000000000000000000000000")
    );
}

fn hex_bytes(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// Reported by a user: Graviton Lance's perks on a hand cannon never triggered, with no
/// detonation at all, while Hard Light's element switch worked. The picker offers one choice
/// per weapon and prefers the firing graph, so the record half holding the behavior array
/// was never applied. A graph choice now carries it.
#[test]
fn a_graph_choice_carries_its_weapons_behavior_record() {
    let graph = behavior("graviton-lance-graph").expect("the graph half is catalogued");
    let record = behavior("graviton-lance").expect("the record half is catalogued");
    assert!(graph.has_graph() && !record.has_graph());
    assert!(record.carries_behavior_record());
    assert_eq!(graph.source_item_hash, record.source_item_hash);

    let paired = paired_record(graph).expect("the graph pairs with its record");
    assert_eq!(paired.id, record.id);
    assert_eq!(paired.source.record_source(), record.source.record_source());

    // A weapon whose behavior is only a graph has nothing to pair, and a record never
    // pairs with itself.
    assert!(paired_record(behavior("truth-graph").expect("graph only")).is_none());
    assert!(paired_record(record).is_none());
}

fn overrides_for(id: &str) -> crate::item::WeaponCloneOverrides {
    crate::item::WeaponCloneOverrides {
        additional_behaviors: vec![id.to_owned()],
        ..Default::default()
    }
}

#[test]
fn a_graft_pins_the_source_weapons_own_intrinsic_and_trait() {
    let types = [
        INTRINSIC_SOCKET_TYPE,
        65,
        177,
        TRAIT_SOCKET_TYPE,
        TRAIT_SOCKET_TYPE,
    ];
    let expanded = expand_socket_columns(&overrides_for("malfeasance-graph"), &types).unwrap();
    let entry = behavior("malfeasance-graph").unwrap();
    assert_eq!(
        expanded.socket_columns[0]
            .as_ref()
            .map(|c| c.choices.clone()),
        Some(vec![entry.intrinsic_plug.unwrap()])
    );
    assert_eq!(
        expanded.socket_columns[3]
            .as_ref()
            .map(|c| c.choices.clone()),
        Some(vec![entry.trait_plug.unwrap()])
    );
    // The second trait column is left for the author.
    assert!(expanded.socket_columns[4].is_none());
}

/// The build must reach the same intrinsic lane the editor shows, including for a recipe the
/// editor never synced, and a custom perk keeps addressing its own plug after the trim.
#[test]
fn the_build_keeps_an_authored_intrinsic_perk_and_repoints_it() {
    let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
    let entry = behavior("malfeasance-graph").unwrap();
    let frame = entry.intrinsic_plug.unwrap();
    let mut overrides = overrides_for("malfeasance-graph");
    // Authored without the editor: the custom perk leads the lane, the donor's frame follows.
    overrides.socket_columns = vec![Some(column(vec![0x1234_5678, 0x0000_0010], None)), None];
    overrides.socket_plug_variants = vec![crate::item::WeaponSocketPlugVariantOverride {
        offer_everywhere: false,
        replace_effects: false,
        investment_stats: Vec::new(),
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: 0x1234_5678,
        name: Some("Authored Frame".into()),
        classification_donor_hash: None,
        icon: None,
        description: None,
        additional_sandbox_perks: Vec::new(),
        sandbox_perks: Vec::new(),
    }];

    let expanded = expand_socket_columns(&overrides, &types).unwrap();

    let choices = expanded.socket_columns[0]
        .as_ref()
        .map(|column| column.choices.clone())
        .expect("the intrinsic lane is written");
    assert_eq!(
        choices,
        vec![frame, 0x1234_5678],
        "the borrowed frame replaces the donor's, the authored one stays"
    );
    let variant = &expanded.socket_plug_variants[0];
    assert_eq!(
        choices.get(usize::from(variant.choice_index)),
        Some(&variant.source_plug_hash),
        "the custom perk still addresses its own plug"
    );
}

#[test]
fn the_author_can_keep_the_graft_without_its_perks() {
    let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
    let mut overrides = overrides_for("malfeasance-graph");
    overrides.skip_behavior_perks = true;
    let expanded = expand_socket_columns(&overrides, &types).unwrap();
    assert!(expanded.socket_columns.is_empty());
}

#[test]
fn a_behavior_leads_a_socket_the_author_already_chose_for() {
    let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
    let mut overrides = overrides_for("malfeasance-graph");
    overrides.socket_columns = vec![
        Some(crate::item::WeaponSocketColumnOverride {
            choices: vec![0x1234_5678],
            socket_type: None,
            choice_weight_bits: Vec::new(),
            choice_conditions: Vec::new(),
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
            randomized_selection_program: Vec::new(),
        }),
        None,
    ];
    // The frame lane holds the borrowed frame alone; a trait lane keeps the author's own
    // choice behind the perk the behavior needs first.
    overrides.socket_columns[1] = Some(column(vec![0x2345_6789], None));
    let expanded = expand_socket_columns(&overrides, &types).unwrap();
    let entry = behavior("malfeasance-graph").unwrap();
    let intrinsic = entry
        .intrinsic_plug
        .expect("the graft names an intrinsic plug");
    assert_eq!(
        expanded.socket_columns[0]
            .as_ref()
            .map(|column| column.choices.as_slice()),
        Some([intrinsic].as_slice())
    );
    assert_eq!(
        expanded.socket_columns[1]
            .as_ref()
            .map(|column| column.choices.as_slice()),
        Some([entry.trait_plug.unwrap(), 0x2345_6789].as_slice())
    );
}

#[test]
fn a_borrowed_frame_replaces_a_host_frame_left_beside_it() {
    let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
    let mut overrides = overrides_for("malfeasance-graph");
    let entry = behavior("malfeasance-graph").unwrap();
    let intrinsic = entry.intrinsic_plug.unwrap();
    // An older save that already pinned the frame ahead of the host's own.
    overrides.socket_columns = vec![
        Some(column(vec![intrinsic, 0x1234_5678], None)),
        Some(column(vec![entry.trait_plug.unwrap()], None)),
    ];
    let expanded = expand_socket_columns(&overrides, &types).unwrap();
    assert_eq!(
        expanded.socket_columns[0]
            .as_ref()
            .map(|column| column.choices.as_slice()),
        Some([intrinsic].as_slice())
    );
}

fn column(choices: Vec<u32>, socket_type: Option<u16>) -> crate::item::WeaponSocketColumnOverride {
    crate::item::WeaponSocketColumnOverride {
        choices,
        socket_type,
        choice_weight_bits: Vec::new(),
        choice_conditions: Vec::new(),
        reusable_plug_set_index: None,
        randomized_plug_set_index: None,
        randomized_selection_program: Vec::new(),
    }
}

/// An author who gives the behavior's perk a socket of their own has already satisfied it.
/// Leading the donor's own trait column with a second copy showed the same perk twice and
/// pushed the choices they had arranged down a place.
#[test]
fn a_perk_the_author_already_placed_is_not_pinned_a_second_time() {
    let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
    let entry = behavior("malfeasance-graph").unwrap();
    let mut overrides = overrides_for("malfeasance-graph");
    overrides.socket_columns = vec![
        None,
        Some(column(vec![0x1234_5678], None)),
        Some(column(
            vec![entry.trait_plug.unwrap()],
            Some(TRAIT_SOCKET_TYPE),
        )),
    ];
    let expanded = expand_socket_columns(&overrides, &types).unwrap();
    assert_eq!(expanded.socket_columns[1], overrides.socket_columns[1]);
    assert_eq!(expanded.socket_columns[2], overrides.socket_columns[2]);
}

/// The socket list draws a column the author gave the trait role as a trait socket, so the
/// build has to pin into it too. Reading only the donor's own types skipped it entirely.
#[test]
fn a_socket_the_author_turned_into_a_trait_column_is_where_the_perk_lands() {
    let types = [INTRINSIC_SOCKET_TYPE, u16::MAX];
    let entry = behavior("malfeasance-graph").unwrap();
    let mut overrides = overrides_for("malfeasance-graph");
    overrides.socket_columns = vec![
        None,
        Some(column(vec![0x1234_5678], Some(TRAIT_SOCKET_TYPE))),
    ];
    let expanded = expand_socket_columns(&overrides, &types).unwrap();
    assert_eq!(
        expanded.socket_columns[1]
            .as_ref()
            .map(|column| column.choices.as_slice()),
        Some([entry.trait_plug.unwrap(), 0x1234_5678].as_slice())
    );
}

/// Choosing a weapon's firing graph turns element switching on for the two exotics that
/// switch damage, and that request resolves to the very record the graph pairs with. Writing
/// it once per request put two edits over the same bytes and the overlap guard failed the
/// build, so Hard Light could not compile at all.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn a_graph_and_its_element_switch_apply_one_record_between_them() {
    use sundial::package_authoring::{
        open_shadowkeep_package_manager, runtime::load_weapon_runtime_entity_with_manager,
    };
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let rifle = load_weapon_runtime_entity_with_manager(&manager, 0xD84E_04AA).unwrap();
    let graft = |requested: &[String]| {
        patches(
            &manager,
            &rifle.payload,
            ContentGroups {
                selected: Some(rifle.weapon_content_group_hash),
                own: None,
                animations: None,
                kind: None,
                hold: None,
            },
            requested,
            DEFAULT_PROJECTILE_SPEED_BOOST,
        )
        .unwrap()
    };

    let both = graft(&["hard-light-graph".into(), ELEMENT_SWITCH.to_owned()]);
    let mut offsets = both
        .patches
        .iter()
        .map(|patch| patch.offset)
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    let unique = {
        let mut seen = offsets.clone();
        seen.dedup();
        seen.len()
    };
    assert_eq!(
        unique,
        offsets.len(),
        "a record was written twice: {offsets:?}"
    );

    // The record travels once whether or not element switching asks for it as well.
    let alone = graft(&["hard-light-graph".into()]);
    assert_eq!(alone.patches.len(), both.patches.len());
    assert_eq!(alone.appends.len(), both.appends.len());
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
#[allow(clippy::cognitive_complexity)]
fn state_only_and_graph_state_sources_compile_for_another_family() {
    use sundial::package_authoring::{
        open_shadowkeep_package_manager, runtime::load_weapon_runtime_entity_with_manager,
    };
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let sniper = load_weapon_runtime_entity_with_manager(&manager, 0xBB46_CCD3).unwrap();

    let drang = patches(
        &manager,
        &sniper.payload,
        ContentGroups {
            selected: Some(sniper.weapon_content_group_hash),
            own: None,
            animations: None,
            kind: None,
            hold: None,
        },
        &["drang-state".into()],
        DEFAULT_PROJECTILE_SPEED_BOOST,
    )
    .unwrap();
    assert!(drang.patches.is_empty());
    let (record, labels) = record_and_labels(&drang.appends);
    assert_eq!(record.slots.len(), 1);
    if let Some(labels) = labels {
        assert_eq!(labels.slots[0].0 + SLOT_STRIDE as u32, record.slots[0].0);
        assert!(labels.slots[0].2 >= 2);
    }

    let sturm = patches(
        &manager,
        &sniper.payload,
        ContentGroups {
            selected: Some(sniper.weapon_content_group_hash),
            own: None,
            animations: None,
            kind: None,
            hold: None,
        },
        &["sturm-graph".into()],
        DEFAULT_PROJECTILE_SPEED_BOOST,
    )
    .unwrap();
    let (record, labels) = record_and_labels(&sturm.appends);
    assert_eq!(record.slots.len(), 1);
    if let Some(labels) = labels {
        assert_eq!(labels.slots[0].0 + SLOT_STRIDE as u32, record.slots[0].0);
    }
    assert_eq!(sturm.patches.len(), 1);
    assert_eq!(sturm.patches[0].bytes, 0x80BB_C07C_u32.to_le_bytes());

    // The case reported in game: Graviton Lance onto a legendary pulse rifle. Same owner, so
    // the record is patched in place, and the one append is the label array carrying
    // "bucket 2", which Cosmology's kill condition requires and Bygones' block lacks.
    let catalog = crate::test_support::catalog(path.parent().unwrap()).unwrap();
    let pulse = catalog
        .weapon_donors()
        .into_iter()
        .filter(|donor| {
            donor.type_name == "Pulse Rifle"
                && donor.rarity == sundial::investment::WeaponRarity::Legendary
                && donor.weapon_pattern_index.is_some()
        })
        .find_map(|donor| load_weapon_runtime_entity_with_manager(&manager, donor.hash).ok())
        .expect("a legendary pulse rifle with a runtime entity");
    let graviton = patches(
        &manager,
        &pulse.payload,
        ContentGroups {
            selected: Some(pulse.weapon_content_group_hash),
            own: None,
            animations: None,
            kind: None,
            hold: None,
        },
        &["graviton-lance-graph".into()],
        DEFAULT_PROJECTILE_SPEED_BOOST,
    )
    .unwrap();
    assert_eq!(graviton.appends.len(), 1);
    let labels = &graviton.appends[0];
    assert_eq!(u32_at(&labels.bytes, 12).unwrap(), LABEL_ARRAY_CLASS);
    let rows = usize::try_from(labels.slots[0].2).unwrap();
    let carried = (0..rows)
        .map(|index| u32_at(&labels.bytes, 20 + index * LABEL_ROW_SIZE).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        carried[0], 0x937F_E7FA,
        "the host keeps its pulse rifle label first"
    );
    assert!(
        carried.contains(&0x2E65_81A5),
        "bucket 2 travels: {carried:08X?}"
    );
    assert_eq!(labels.bytes.len(), 20 + rows * LABEL_ROW_SIZE);
}

/// A cross-owner graft appends its record and, when the source has labels of its own, a
/// label array; nothing else.
fn record_and_labels(
    appends: &[crate::item::WeaponRuntimeResourceAppend],
) -> (
    &crate::item::WeaponRuntimeResourceAppend,
    Option<&crate::item::WeaponRuntimeResourceAppend>,
) {
    let is_labels = |append: &crate::item::WeaponRuntimeResourceAppend| {
        u32_at(&append.bytes, 12).ok() == Some(LABEL_ARRAY_CLASS)
    };
    let labels = appends.iter().find(|append| is_labels(append));
    let record = appends
        .iter()
        .find(|append| !is_labels(append))
        .expect("a record append");
    assert_eq!(appends.len(), 1 + usize::from(labels.is_some()));
    (record, labels)
}

/// The recorded list is a measurement, not a judgement about how a weapon looks in game, so
/// it is checked against the packages it was taken from. Hard Light fires visible bouncing
/// rounds and still exposes no launch speed to raise, which is exactly the sort of guess this
/// catches.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_launching_source_is_recorded() {
    use sundial::package_authoring::open_shadowkeep_package_manager;
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let measured = CATALOG
        .iter()
        .filter(|entry| {
            entry
                .graph_tag()
                .is_some_and(|tag| launches_its_own(&manager, tag))
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert!(!measured.is_empty());
    assert_eq!(
        measured.iter().copied().collect::<BTreeSet<_>>(),
        LAUNCHING_SOURCES.iter().copied().collect()
    );
}

/// The sources offering a firing pattern are a measurement too: each one's own plugs are read
/// from the packages, perk by perk, for a change to the barrel's Bullets per Shot.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_burst_source_is_recorded() {
    use sundial::package_authoring::runtime::modifiers::BARREL_BULLETS_PER_SHOT;
    use sundial::package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag};
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let catalog = crate::test_support::catalog(path.parent().unwrap()).unwrap();
    let measured = CATALOG
        .iter()
        .filter(|entry| {
            [entry.intrinsic_plug, entry.trait_plug]
                .into_iter()
                .flatten()
                .flat_map(|plug| catalog.item_sandbox_perk_indices(plug))
                .filter_map(|perk| perk_firing(&manager, &globals, perk).unwrap())
                .flat_map(|perk| perk.records)
                .any(|record| {
                    BARREL_BULLETS_PER_SHOT
                        .iter()
                        .any(|lane| record.in_burst_lane(*lane))
                })
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert!(!measured.is_empty());
    assert_eq!(
        measured.iter().copied().collect::<BTreeSet<_>>(),
        BURST_SOURCES.iter().copied().collect()
    );
}

/// The boost only fires when the graft launches something the host cannot speed up itself.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn the_speed_boost_scales_a_launching_graft_and_leaves_every_other_case_alone() {
    use sundial::package_authoring::{
        open_shadowkeep_package_manager,
        runtime::load_weapon_runtime_entity_at_pattern_index_with_manager,
    };
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let launching = behavior("anarchy-graph").expect("Anarchy launches grenades");
    let BehaviorSource::Graph { tag: launching } = launching.source else {
        panic!("Anarchy's half is a graph");
    };
    let instant = behavior("malfeasance-graph").expect("Malfeasance hits instantly");
    let BehaviorSource::Graph { tag: instant } = instant.source else {
        panic!("Malfeasance's half is a graph");
    };

    // Find one host of each sort by reading the graph each weapon's block names.
    let mut hitscan = None;
    let mut launcher = None;
    for index in 0..3000_u16 {
        if hitscan.is_some() && launcher.is_some() {
            break;
        }
        let Ok(source) = load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, index)
        else {
            continue;
        };
        let Ok(content) = content(&manager, &source.payload) else {
            continue;
        };
        let Ok(block) = block_for_group(&content, source.weapon_content_group_hash) else {
            continue;
        };
        let Some(graph) = host_graph(&content, block) else {
            continue;
        };
        if launches_its_own(&manager, graph) {
            launcher.get_or_insert(graph);
        } else {
            hitscan.get_or_insert(graph);
        }
    }
    let hitscan = hitscan.expect("some weapon fires instantly");
    let launcher = launcher.expect("some weapon launches its own");

    // A launching graft on a weapon that supplies no speed of its own is raised. Each
    // parameter writes two lanes, the instance and the definition.
    let values = graph_values(&manager, Some(hitscan), launching, 4.0).unwrap();
    assert_eq!(values.len(), 2);
    // A weapon with no firing graph at all is treated the same way.
    assert_eq!(
        graph_values(&manager, None, launching, 4.0).unwrap().len(),
        2
    );
    // A boost of one changes nothing, so no private clone is made.
    assert!(
        graph_values(&manager, Some(hitscan), launching, 1.0)
            .unwrap()
            .is_empty()
    );
    // A host that already launches its own supplies a real speed.
    assert!(
        graph_values(&manager, Some(launcher), launching, 4.0)
            .unwrap()
            .is_empty()
    );
    // A graft that also fires instantly has no launch speed worth raising.
    assert!(
        graph_values(&manager, Some(hitscan), instant, 4.0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn catalogue_ids_are_unique_and_the_element_switch_resolves_per_owner() {
    let mut ids = CATALOG.iter().map(|entry| entry.id).collect::<Vec<_>>();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count);
    assert_eq!(
        element_switch_for_owner(0x8152_9461).map(|entry| entry.id),
        Some("hard-light")
    );
    assert_eq!(
        element_switch_for_owner(0x8152_BC09).map(|entry| entry.id),
        Some("borealis")
    );
    assert!(element_switch_for_owner(0x8152_905A).is_none());
    assert_eq!(
        resolve_request(ELEMENT_SWITCH, 0x8152_BC09)
            .unwrap()
            .unwrap()
            .id,
        "borealis"
    );
    assert!(
        resolve_request(ELEMENT_SWITCH, 0x8152_905A)
            .unwrap()
            .is_none()
    );
    // A record from another family is copied in rather than refused.
    assert!(
        resolve_request("hard-light", 0x8152_BC09)
            .unwrap()
            .is_some()
    );
    assert!(resolve_request("nope", 0x8152_9461).is_err());
}

/// Every catalogued entry's plugs must be the ones its own weapon equips, in the sockets the
/// roles name. Three entries were checked before, which left a wrong hash free to pin another
/// weapon's perk. Two entries legitimately share a plug: Moving Target is part of both
/// Dornroeschen and this build's Arc Traps, so sharing is not evidence of a mistake and only
/// this test can tell the two apart.
/// The behavior browser lists weapons, so an entry whose weapon is not an offered donor would
/// disappear from the picker without a word. That is the silent-drop failure this area has
/// already produced twice, so it is checked rather than assumed.
/// Carrying a source weapon's labels must never cost a graft that worked before.
///
/// Reading those labels needs the source weapon's selected block. A shared pattern can have a
/// different row identity from the source item. The staged behavior-label workflow verifies
/// that those sources transfer their additional labels without changing the host's type.
/// A copied label array is only as good as the row size it was read with. Every block that
/// has labels is parsed here and its rows are required to end exactly where the next array
/// marker begins, which is what proves the count and the row stride together. A wrong stride
/// would not fail a build: it would write a corrupt array into the game.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_block_with_labels_parses_at_the_row_size_grafts_copy() {
    use sundial::package_authoring::{
        open_shadowkeep_package_manager, runtime::load_weapon_runtime_entity_with_manager,
    };
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let catalog = crate::test_support::catalog(path.parent().unwrap()).unwrap();
    let mut seen = BTreeSet::new();
    let mut checked = 0_usize;
    let mut failures = Vec::new();
    for donor in catalog.weapon_donors() {
        let Ok(runtime) = load_weapon_runtime_entity_with_manager(&manager, donor.hash) else {
            continue;
        };
        let Ok(content) = content(&manager, &runtime.payload) else {
            continue;
        };
        if !seen.insert(content.owner_tag) {
            continue;
        }
        for block in &content.blocks {
            let Ok(triple) = first_triple(&content.owner, *block) else {
                continue;
            };
            let rows = match label_rows(&content.owner, *block) {
                Ok(rows) => rows,
                Err(error) => {
                    failures.push(format!(
                        "owner 0x{:08X} block 0x{block:X}: {error}",
                        content.owner_tag
                    ));
                    continue;
                }
            };
            if rows.is_empty() {
                continue;
            }
            checked += 1;
            let target = slot_target(&content.owner, triple).expect("a parsed label array");
            let end = target + 16 + rows.len() * LABEL_ROW_SIZE;
            // The next array follows its own header marker, behind alignment padding. Only
            // zeroes may separate the two: a row stride that read short or long would leave
            // array bytes in that gap instead.
            let marker = (0..=4)
                .map(|word| end + word * 4)
                .take_while(|at| at + 4 <= content.owner.len())
                .find(|at| u32_at(&content.owner, *at).ok() != Some(0));
            if let Some(marker) = marker
                && u32_at(&content.owner, marker).ok() != Some(ARRAY_HEADER_CLASS)
            {
                failures.push(format!(
                    "owner 0x{:08X} block 0x{block:X}: {} rows end at 0x{end:X} followed by {:08X?}",
                    content.owner_tag,
                    rows.len(),
                    (0..6)
                        .map(|i| u32_at(&content.owner, end + i * 4).unwrap_or_default())
                        .collect::<Vec<_>>()
                ));
            }
            // Every row is a label and a tag, so a copied row cannot carry a relative pointer.
            for row in &rows {
                let tail = &row[4..16];
                if tail.iter().any(|byte| *byte != 0) {
                    failures.push(format!(
                        "owner 0x{:08X} block 0x{block:X}: label row is not position independent",
                        content.owner_tag
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "no blocks carried labels");
    assert!(failures.is_empty(), "label layout: {failures:#?}");
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_entry_compiles_a_graft_onto_a_real_host() {
    use sundial::package_authoring::{
        open_shadowkeep_package_manager, runtime::load_weapon_runtime_entity_with_manager,
    };
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let catalog = crate::test_support::catalog(path.parent().unwrap()).unwrap();
    let host = catalog
        .weapon_donors()
        .into_iter()
        .filter(|donor| {
            donor.type_name == "Auto Rifle"
                && donor.rarity == sundial::investment::WeaponRarity::Legendary
        })
        .find_map(|donor| load_weapon_runtime_entity_with_manager(&manager, donor.hash).ok())
        .expect("a legendary auto rifle with a runtime entity");
    let failed = CATALOG
        .iter()
        .filter_map(|entry| {
            patches(
                &manager,
                &host.payload,
                ContentGroups {
                    selected: Some(host.weapon_content_group_hash),
                    own: None,
                    animations: None,
                    kind: None,
                    hold: None,
                },
                &[entry.id.to_owned()],
                DEFAULT_PROJECTILE_SPEED_BOOST,
            )
            .err()
            .map(|error| format!("{} ({}): {error}", entry.source_name, entry.id))
        })
        .collect::<Vec<_>>();
    assert!(
        failed.is_empty(),
        "grafts that no longer compile: {failed:#?}"
    );
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn every_entry_pins_the_plugs_its_own_weapon_equips() {
    let path = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(path.parent().unwrap()).unwrap();
    let mut problems = Vec::new();
    for entry in CATALOG {
        let Some(donor) = catalog.weapon_donor(entry.source_item_hash) else {
            problems.push(format!(
                "{} ({}): item 0x{:08X} is not an installed weapon",
                entry.source_name, entry.id, entry.source_item_hash
            ));
            continue;
        };
        for (plug, role) in [
            (entry.intrinsic_plug, INTRINSIC_SOCKET_TYPE),
            (entry.trait_plug, TRAIT_SOCKET_TYPE),
        ] {
            let Some(plug) = plug else { continue };
            let equipped = donor
                .sockets
                .iter()
                .any(|socket| socket.socket_type == role && socket.native_default == Some(plug));
            if !equipped {
                let held = donor
                    .sockets
                    .iter()
                    .find(|socket| socket.native_default == Some(plug))
                    .map_or_else(
                        || "no socket of this weapon".to_owned(),
                        |socket| format!("its socket type {}", socket.socket_type),
                    );
                problems.push(format!(
                    "{} ({}): 0x{plug:08X} is not the type {role} default, it is in {held}",
                    entry.source_name, entry.id
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "plug mismatches:
{}",
        problems.join(
            "
"
        )
    );
}
