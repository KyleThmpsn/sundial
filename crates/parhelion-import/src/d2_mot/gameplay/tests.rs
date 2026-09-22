use super::*;

#[test]
fn kinetic_including_modern_elements_requires_a_convertible_damage_carrier() {
    for perks in [
        vec![],
        vec![0x8C011E66u32],
        vec![0x66653D11],
        vec![0x781E5D20],
    ] {
        let source = json!({"perks":perks});
        assert!(!damage_carrier_compatible(&source, &[1, 68, 8]));
        assert!(damage_carrier_compatible(&source, &[1, 8]));
    }
    assert!(damage_carrier_compatible(
        &json!({"perks":[0xCCC507A5u32]}),
        &[68]
    ));
    assert!(damage_carrier_compatible(&json!({}), &[68]));
}

fn array_fixture(at: usize, class: u32, stride: usize, count: usize) -> Payload {
    let mut bytes = vec![0; 0x80 + stride * count];
    bytes[at..at + 8].copy_from_slice(&(count as u64).to_le_bytes());
    bytes[at + 8..at + 16].copy_from_slice(&(0x70i64 - (at + 8) as i64).to_le_bytes());
    bytes[0x70..0x78].copy_from_slice(&(count as u64).to_le_bytes());
    bytes[0x78..0x7c].copy_from_slice(&class.to_le_bytes());
    Payload(bytes)
}

#[test]
fn source_rows_use_modern_strides_and_resolve_table_identities() {
    let mut table = array_fixture(8, 0x808076AE, 12, 2);
    table.0[0x80..0x84].copy_from_slice(&123u32.to_le_bytes());
    table.0[0x8c..0x90].copy_from_slice(&456u32.to_le_bytes());
    let mut item = array_fixture(0x30, 0x80807387, 0x20, 2);
    item.0[0xa0..0xa4].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(perk_hashes(&item, 0x20, &table).unwrap(), [123, 456]);
    item.0[0xa0..0xa4].copy_from_slice(&2u32.to_le_bytes());
    assert!(perk_hashes(&item, 0x20, &table).is_err());

    let mut table = array_fixture(8, 0x8080586F, 0x24, 2);
    table.0[0x80..0x84].copy_from_slice(&123u32.to_le_bytes());
    table.0[0xa4..0xa8].copy_from_slice(&456u32.to_le_bytes());
    let mut item = array_fixture(0x20, 0x80807386, 0x40, 2);
    item.0[0x84..0x88].copy_from_slice(&(-7i32).to_le_bytes());
    item.0[0xc0..0xc4].copy_from_slice(&1u32.to_le_bytes());
    item.0[0xc8..0xd0].copy_from_slice(&1u64.to_le_bytes());
    let stats = stat_values(&item, 0x20, &table).unwrap();
    assert_eq!(stats[0], json!({"hash":123,"value":-7,"literal":true}));
    assert_eq!(stats[1]["hash"], 456);
    assert_eq!(stats[1]["literal"], false);
}

fn plug(hash: u32, category: u32, default: bool) -> Value {
    json!({"hash":hash,"category":category,"default":default,"art_indices":[]})
}

#[test]
fn missing_source_stats_preserve_authored_compatibility_values() {
    let existing = json!([
        {"definition_index":7,"value":40},
        {"definition_index":8,"value":50}
    ]);
    let merged =
        retain_authored_stats(&existing, vec![json!({"definition_index":7,"value":67})]).unwrap();
    assert_eq!(
        json!(merged),
        json!([
            {"definition_index":7,"value":67},
            {"definition_index":8,"value":50}
        ])
    );
}

#[test]
fn stats_resolve_by_hash_and_missing_or_conditional_values_keep_donor() {
    let source = json!({"stats":[{"hash":42,"value":67,"literal":true},{"hash":43,"value":90,"literal":true},{"hash":44,"value":5,"literal":false}]});
    let mut fallbacks = vec![];
    let mapped =
        stat_overrides(&source, &BTreeMap::from([(42, 7), (44, 8)]), &mut fallbacks).unwrap();
    assert_eq!(
        mapped,
        json!([{"definition_index":7,"value":67}])
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(fallbacks.len(), 2);
}

#[test]
fn socket_roles_match_categories_not_positions_and_keep_missing_defaults() {
    let source = json!({"sockets":[{"choices":[plug(10,2,true)]},{"choices":[plug(11,1,true),plug(12,1,false),plug(13,1,false)]}]});
    let native = json!({"sockets":[{"choices":[plug(20,1,true)]},{"choices":[plug(21,2,true)]}]});
    let mut fallbacks = vec![];
    let mapped = columns(
        &source,
        &native,
        &BTreeMap::from([(10, 2), (12, 1), (13, 9)]),
        &mut fallbacks,
    )
    .unwrap();
    assert_eq!(mapped[0]["choices"], json!(["0x00000014", "0x0000000C"]));
    assert_eq!(mapped[1]["choices"], json!(["0x0000000A"]));
    assert_eq!(fallbacks.len(), 1);
}

#[test]
fn unsupported_and_appearance_plugs_do_not_replace_donor_columns() {
    let mut appearance = plug(10, 1, true);
    appearance["art_indices"] = json!([2]);
    let source = json!({"sockets":[{"choices":[appearance,plug(11,1,false)]}]});
    let native = json!({"sockets":[{"choices":[plug(20,1,true),plug(21,1,false)]}]});
    assert_eq!(
        columns(&source, &native, &BTreeMap::from([(10, 1)]), &mut vec![]).unwrap(),
        vec![Value::Null]
    );
}

#[test]
fn darkness_elements_become_kinetic_without_changing_slot_or_ammo() {
    for perk in [0x66653D11u32, 0x781E5D20] {
        let properties =
            properties(&json!({"perks":[perk],"bucket":0x59570ADAu32,"ammo":2,"rarity":5}));
        assert_eq!(
            properties,
            json!({"modern_damage_type":"kinetic","inventory_slot":"kinetic","ammo_type":"special","rarity":"exotic"})
        );
    }
    assert_eq!(damage(&[0xCFCF0160]), Some("solar"));
    assert_eq!(damage(&[]), Some("kinetic"));
    assert_eq!(damage(&[123]), None);
    assert_eq!(damage(&[0xCFCF0160, 0xCCC507A5]), None);
    assert_eq!(
        properties(&json!({"ammo":99,"bucket":99,"rarity":99})),
        json!({})
    );
}

#[test]
fn native_randomized_defaults_fill_only_unauthored_lanes() {
    let native = json!({"sockets":[{"curated_fallback":20},{"curated_fallback":21},{}]});
    let mut columns = json!([null,{"choices":["0x00000063"]},null]);
    let mut fallbacks = vec![];
    fill_native_defaults(&native, &mut columns, &mut fallbacks).unwrap();
    assert_eq!(
        columns,
        json!([{"choices":["0x00000014"]},{"choices":["0x00000063"]},null])
    );
    assert_eq!(fallbacks.len(), 1);
    fill_native_defaults(&native, &mut columns, &mut fallbacks).unwrap();
    assert_eq!(fallbacks.len(), 1);
    let mut empty = json!([]);
    fill_native_defaults(&native, &mut empty, &mut vec![]).unwrap();
    assert_eq!(empty.as_array().unwrap().len(), 3);
}

#[test]
#[ignore = "Requires explicitly configured installed package paths"]
fn configured_gameplay_mapping() {
    let modern = std::env::var_os("PARHELION_IMPORT_MODERN").expect("PARHELION_IMPORT_MODERN");
    let native = std::env::var_os("PARHELION_IMPORT_NATIVE").expect("PARHELION_IMPORT_NATIVE");
    let hash = std::env::var("PARHELION_IMPORT_SOURCE_HASH")
        .unwrap()
        .parse()
        .unwrap();
    let donor = std::env::var("PARHELION_IMPORT_DONOR_HASH").unwrap();
    let out = tempfile::tempdir().unwrap();
    let mut reader =
        Reader::discovery(Path::new(&modern), &out.path().join("source"), true).unwrap();
    let (index, tag) = super::super::assets::item::find(&mut reader, hash).unwrap();
    let item = reader.tag(tag, None).unwrap();
    let source = source(&mut reader, hash, index, &item).unwrap();
    let mut recipe = json!({"donor":{"item_hash":donor},"overrides":{}});
    let report = apply(
        &source,
        Path::new(&native),
        &out.path().join("target"),
        &mut recipe,
    )
    .unwrap();
    assert!(
        !report["mapped"]["investment_stats"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
