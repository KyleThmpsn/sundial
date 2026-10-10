use super::*;

fn plug_member_array(row_class: u32, item_index: u16, reserved: [u8; 6]) -> Vec<u8> {
    let mut data = vec![0_u8; 72];
    data[0..8].copy_from_slice(&1_u64.to_le_bytes());
    data[8..16].copy_from_slice(&16_i64.to_le_bytes());
    data[24..32].copy_from_slice(&1_u64.to_le_bytes());
    data[32..36].copy_from_slice(&row_class.to_le_bytes());
    data[40..42].copy_from_slice(&item_index.to_le_bytes());
    data[42..48].copy_from_slice(&reserved);
    data
}

#[test]
fn plug_member_indices_are_u16_and_require_the_native_row_class() {
    let item_hashes = [0x1111_u64, 0x2222];
    let data = plug_member_array(0x8080_2E03, 1, [0xA5; 6]);
    let decoded = plug_member_hashes(&data, 0, &item_hashes).unwrap();
    assert!(decoded.complete);
    assert_eq!(decoded.values, vec![0x2222]);

    let wrong_class = plug_member_array(0xDEAD_BEEF, 1, [0; 6]);
    let decoded = plug_member_hashes(&wrong_class, 0, &item_hashes).unwrap();
    assert!(!decoded.complete);
    assert!(decoded.values.is_empty());
}

fn item(bucket_hash: u64, sockets: Vec<SocketDef>) -> ItemDef {
    ItemDef {
        hash: bucket_hash,
        name: String::new(),
        type_name: String::new(),
        bucket_hash,
        class_type: 3,
        default_plugs: Vec::new(),
        sockets,
        abilities: Default::default(),
    }
}

fn typed_item(bucket_hash: u64, type_name: &str, sockets: Vec<SocketDef>) -> ItemDef {
    let mut item = item(bucket_hash, sockets);
    item.type_name = type_name.to_owned();
    item
}

#[test]
fn gear_type_options_pool_live_socket_data_without_cosmetic_plugs() {
    let pools = vec![vec![1, 2], vec![10, 11], vec![3], vec![20]];
    let names = HashMap::from([
        (1, "Arrowhead Brake".to_owned()),
        (2, "Rampage".to_owned()),
        (3, "Kill Clip".to_owned()),
        (10, "Default Shader".to_owned()),
        (11, "Golden Trace Shader".to_owned()),
        (20, "Mobility Mod".to_owned()),
    ]);
    let items = vec![
        item(
            1_498_876_634,
            vec![
                SocketDef {
                    socket_type: 100,
                    pool: 0,
                    ..SocketDef::default()
                },
                SocketDef {
                    socket_type: 200,
                    pool: 1,
                    ..SocketDef::default()
                },
            ],
        ),
        item(
            2_465_295_065,
            vec![SocketDef {
                socket_type: 101,
                pool: 2,
                ..SocketDef::default()
            }],
        ),
        item(
            3_448_274_439,
            vec![SocketDef {
                socket_type: 300,
                pool: 3,
                ..SocketDef::default()
            }],
        ),
    ];

    let (_, options, cosmetic_pools) =
        build_gear_type_options(&items, &pools, &names, &HashMap::new());

    assert_eq!(options.get(&GearKind::Weapon).unwrap(), &vec![1, 3, 2]);
    assert_eq!(options.get(&GearKind::Armor).unwrap(), &vec![20]);
    assert!(cosmetic_pools.contains(&1));
    assert!(
        !options
            .get(&GearKind::Weapon)
            .unwrap()
            .iter()
            .any(|hash| matches!(hash, 10 | 11))
    );
}

#[test]
fn socket_and_gear_type_options_require_both_dimensions_to_match() {
    let pools = vec![vec![1], vec![2], vec![3], vec![4], vec![5]];
    let names = HashMap::from([
        (1, "Weapon A".to_owned()),
        (2, "Armor A".to_owned()),
        (3, "Weapon B".to_owned()),
        (4, "Weapon C".to_owned()),
        (5, "Weapon D".to_owned()),
    ]);
    let items = vec![
        typed_item(
            1_498_876_634,
            "Hand Cannon",
            vec![SocketDef {
                socket_type: 100,
                pool: 0,
                ..SocketDef::default()
            }],
        ),
        typed_item(
            3_448_274_439,
            "Helmet",
            vec![SocketDef {
                socket_type: 100,
                pool: 1,
                ..SocketDef::default()
            }],
        ),
        typed_item(
            2_465_295_065,
            "Auto Rifle",
            vec![
                SocketDef {
                    socket_type: 200,
                    pool: 2,
                    ..SocketDef::default()
                },
                SocketDef {
                    socket_type: 100,
                    pool: 3,
                    ..SocketDef::default()
                },
            ],
        ),
        typed_item(
            2_465_295_065,
            "Hand Cannon",
            vec![SocketDef {
                socket_type: 100,
                pool: 4,
                ..SocketDef::default()
            }],
        ),
    ];

    let (socket_options, socket_and_gear_options) =
        build_socket_type_options(&items, &pools, &names);

    assert_eq!(socket_options.get(&100).unwrap(), &vec![2, 1, 4, 5]);
    assert_eq!(
        socket_and_gear_options
            .get("Hand Cannon")
            .and_then(|options| options.get(&100))
            .unwrap(),
        &vec![1, 5]
    );
    assert_eq!(
        socket_and_gear_options
            .get("Helmet")
            .and_then(|options| options.get(&100))
            .unwrap(),
        &vec![2]
    );
    assert_eq!(
        socket_and_gear_options
            .get("Auto Rifle")
            .and_then(|options| options.get(&200))
            .unwrap(),
        &vec![3]
    );
    assert_eq!(
        socket_and_gear_options
            .get("Auto Rifle")
            .and_then(|options| options.get(&100))
            .unwrap(),
        &vec![4]
    );
}

#[test]
fn installed_shaders_are_available_to_exotic_only_weapon_families() {
    let pools = vec![vec![11], vec![22]];
    let names = HashMap::from([(11, "Shader".into()), (22, "Trait".into())]);
    let items = vec![
        typed_item(
            1_498_876_634,
            "Auto Rifle",
            vec![
                SocketDef {
                    socket_type: 180,
                    pool: 0,
                    ..Default::default()
                },
                SocketDef {
                    socket_type: 92,
                    pool: 1,
                    ..Default::default()
                },
            ],
        ),
        typed_item(2_465_295_065, "Trace Rifle", vec![]),
        typed_item(3_448_274_439, "Helmet", vec![]),
    ];
    let (_, families) = build_socket_type_options(&items, &pools, &names);
    assert_eq!(families["Trace Rifle"][&180], vec![11]);
    assert!(!families["Trace Rifle"].contains_key(&92));
    assert!(!families.contains_key("Helmet"));
    let (_, without_shader) = build_socket_type_options(&items[1..], &pools, &names);
    assert!(!without_shader.contains_key("Trace Rifle"));
}

#[test]
fn package_sources_keep_embedded_reusable_and_randomized_members_separate() {
    let mut item = vec![0_u8; 240];
    write_u16(&mut item, 12, 0);
    write_u16(&mut item, 32, 1);
    write_plug_member_array(&mut item, 64, 2, 128);
    write_u32(&mut item, 144, 3);
    write_u32(&mut item, 176, 1);

    let mut plug_sets = vec![0_u8; 320];
    write_array_descriptor(&mut plug_sets, 8, 2, 64);
    write_plug_member_array(&mut plug_sets, 88, 1, 160);
    write_u32(&mut plug_sets, 176, 2);
    write_plug_member_array(&mut plug_sets, 112, 2, 224);
    write_u32(&mut plug_sets, 240, 4);
    write_u32(&mut plug_sets, 272, 5);

    let sources = socket_package_sources(&item, 0, &[100, 101, 102, 103, 104, 105], &plug_sets);

    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
    assert_eq!(sources[0].allowed, vec![103, 101]);
    assert_eq!(sources[0].ordered_members, vec![103, 101]);
    assert!(sources[0].valid);
    assert_eq!(
        sources[1].kind,
        SocketOptionSourceKind::ReusableSet { index: 0 }
    );
    assert_eq!(sources[1].allowed, vec![102]);
    assert_eq!(sources[1].ordered_members, vec![102]);
    assert!(sources[1].valid);
    assert_eq!(
        sources[2].kind,
        SocketOptionSourceKind::RandomizedSet { index: 1 }
    );
    assert_eq!(sources[2].allowed, vec![104, 105]);
    assert_eq!(sources[2].ordered_members, vec![104, 105]);
    assert!(sources[2].valid);
}

#[test]
fn invalid_shared_set_reference_is_retained_without_unsafe_members() {
    let mut item = vec![0_u8; 96];
    write_u16(&mut item, 12, 7);
    write_u16(&mut item, 32, u16::MAX);
    let mut plug_sets = vec![0_u8; 128];
    write_array_descriptor(&mut plug_sets, 8, 1, 64);

    let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

    assert_eq!(sources.len(), 1);
    assert_eq!(
        sources[0].kind,
        SocketOptionSourceKind::ReusableSet { index: 7 }
    );
    assert!(!sources[0].valid);
    assert!(sources[0].allowed.is_empty());
}

#[test]
fn referenced_shared_set_decodes_when_unreferenced_tail_rows_are_missing() {
    let mut item = vec![0_u8; 96];
    write_u16(&mut item, 12, 0);
    write_u16(&mut item, 32, u16::MAX);
    let mut plug_sets = vec![0_u8; 160];
    write_array_descriptor(&mut plug_sets, 8, 10, 48);
    write_plug_member_array(&mut plug_sets, 72, 1, 112);
    write_u32(&mut plug_sets, 128, 0);

    let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].allowed, vec![100]);
    assert!(sources[0].valid);
}

#[test]
fn partially_invalid_list_keeps_members_that_decode_safely() {
    let mut item = vec![0_u8; 208];
    write_u16(&mut item, 12, u16::MAX);
    write_u16(&mut item, 32, u16::MAX);
    write_plug_member_array(&mut item, 64, 2, 128);
    write_u32(&mut item, 144, 0);
    write_u32(&mut item, 176, 7);

    let sources = socket_package_sources(&item, 0, &[100], &[]);

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
    assert_eq!(sources[0].allowed, vec![100]);
    assert_eq!(sources[0].ordered_members, vec![100]);
    assert!(!sources[0].valid);
}

#[test]
fn derived_sources_are_separate_but_feed_the_same_combined_pool() {
    let category = 0xB134_761E;
    let mut items = vec![item(
        1_498_876_634,
        vec![SocketDef {
            socket_type: 518,
            allowed: vec![1],
            sources: vec![SocketOptionSource {
                kind: SocketOptionSourceKind::Embedded,
                pool: 0,
                valid: true,
                ordered_members: vec![1],
                allowed: vec![1],
            }],
            ..SocketDef::default()
        }],
    )];
    let names = HashMap::from([
        (2_285_418_970, "Tracker Disabled".to_owned()),
        (2_302_094_943, "Crucible Kill Tracker".to_owned()),
        (38_912_240, "Vanguard Kill Tracker".to_owned()),
    ]);
    build_socket_choices(
        &mut items,
        &HashMap::from([(1, category)]),
        &HashMap::from([(category, vec![1, 2])]),
        &names,
        &mut HashMap::new(),
    );
    let pools = intern_socket_pools(&mut items, &names).unwrap();
    let socket = &items[0].sockets[0];

    assert_eq!(socket.sources.len(), 3);
    assert!(socket.sources.iter().any(|source| matches!(
        source.kind,
        SocketOptionSourceKind::CategoryExpansion { category_hash }
            if category_hash == category
    )));
    assert!(
        socket
            .sources
            .iter()
            .any(|source| source.kind == SocketOptionSourceKind::SyntheticTracker)
    );
    assert!(pools[socket.pool as usize].contains(&2));
    assert!(pools[socket.pool as usize].contains(&2_285_418_970));
    for source in &socket.sources {
        assert!(source.pool < pools.len() as u32);
    }
}

#[test]
fn interning_sorts_picker_pool_without_erasing_package_member_order() {
    let mut items = vec![item(
        1_498_876_634,
        vec![SocketDef {
            allowed: vec![100, 200],
            sources: vec![SocketOptionSource {
                kind: SocketOptionSourceKind::Embedded,
                pool: 0,
                valid: true,
                ordered_members: vec![200, 100],
                allowed: vec![200, 100],
            }],
            ..SocketDef::default()
        }],
    )];
    let names = HashMap::from([(100, "Alpha".to_owned()), (200, "Zulu".to_owned())]);

    let pools = intern_socket_pools(&mut items, &names).unwrap();
    let source = &items[0].sockets[0].sources[0];

    assert_eq!(pools[source.pool as usize], vec![100, 200]);
    assert_eq!(source.ordered_members, vec![200, 100]);
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_i64(data: &mut [u8], offset: usize, value: i64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_array_descriptor(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
    write_u64(data, descriptor, count);
    let pointer = descriptor + 8;
    write_i64(data, pointer, i64::try_from(header - pointer).unwrap());
    write_u64(data, header, count);
}

fn write_plug_member_array(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
    write_array_descriptor(data, descriptor, count, header);
    write_u32(data, header + 8, 0x8080_2E03);
}
