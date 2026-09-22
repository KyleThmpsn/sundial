use super::*;

fn material() -> Payload {
    let mut p = Payload(vec![0; 0x410]);
    for (at, value) in [
        (8, 1),
        (12, 2),
        (20, 0x400000),
        (24, 0x06000083),
        (28, 0x86000083),
        (36, 0x7F7F80),
        (40, u32::MAX),
        (0x48, 111),
        (0x2C8, 222),
    ] {
        put(&mut p.0, at, &u32::to_le_bytes(value)).unwrap();
    }
    for at in [0xE8, 0x188, 0x228, 0x368] {
        put(&mut p.0, at, &u32::MAX.to_le_bytes()).unwrap();
    }
    p
}

fn model(material: u32, layout: i16) -> Payload {
    let mut p = Payload(vec![0; 272]);
    // One native mesh and one draw, with checked array descriptors.
    for (at, value) in [
        (16, 1u64),
        (24, 40),
        (64, 1),
        (72, 0x80807378),
        (104, 1),
        (112, 112),
        (224, 1),
        (232, 0x8080737E),
    ] {
        put(&mut p.0, at, &value.to_le_bytes()).unwrap();
    }
    put(&mut p.0, 122, &1u16.to_le_bytes()).unwrap();
    put(&mut p.0, 168, &layout.to_le_bytes()).unwrap();
    put(&mut p.0, 240, &material.to_le_bytes()).unwrap();
    p
}

#[test]
fn discovers_equivalent_contracts_after_asset_ids_change() {
    for (model_tag, material_tag) in [(100, 200), (900, 800)] {
        let mut found = BTreeMap::new();
        select_model(model_tag, &model(material_tag, 139), &mut found, |tag| {
            (tag == material_tag).then(material)
        })
        .unwrap();
        let carrier = &found[&Role::Surface];
        assert_eq!(carrier.material, material_tag);
        assert_eq!(carrier.model, model_tag);
        assert_eq!(&carrier.record[..4], &material_tag.to_le_bytes());
    }
}

#[test]
fn rejects_wrong_layout_state_and_extra_shader_stages() {
    let mut found = BTreeMap::new();
    select_model(100, &model(200, 138), &mut found, |_| Some(material())).unwrap();
    assert!(found.is_empty());
    let mut wrong_state = material();
    put(&mut wrong_state.0, 32, &0x88u32.to_le_bytes()).unwrap();
    assert!(!Role::Surface.accepts(&wrong_state).unwrap());
    let mut geometry = material();
    put(&mut geometry.0, 0x228, &333u32.to_le_bytes()).unwrap();
    assert!(!Role::Surface.accepts(&geometry).unwrap());
    assert!(Role::Surface.accepts(&Payload(vec![0; 16])).is_err());
}

#[test]
fn draw_requires_actual_material_in_the_requested_stage() {
    let p = model(200, 139);
    assert!(draw(&p, 100, 80, 0, 201).is_err());
    assert!(draw(&p, 100, 80, 1, 200).is_err());
    let mut bad = p.clone();
    put(&mut bad.0, 122, &2u16.to_le_bytes()).unwrap();
    assert!(draw(&bad, 100, 80, 0, 200).is_err());
}

#[test]
fn missing_carrier_reports_the_required_contract() {
    let catalog = Catalog {
        schema: Catalog::SCHEMA,
        stamp: "fixture".into(),
        carriers: BTreeMap::new(),
        cube: Value::Null,
        samplers: vec![],
    };
    assert!(
        catalog
            .carrier(Role::AlphaDecal)
            .unwrap_err()
            .to_string()
            .contains("AlphaDecal")
    );
}

#[test]
#[ignore = "Requires configured local native packages, never installed content writes"]
fn configured_packages_discover_and_reuse_contracts() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_NATIVE_PACKAGES").unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut reader = Reader::new(&packages, dir.path(), false).unwrap();
    let mut messages = Vec::new();
    export(&mut reader, &packages, dir.path(), &mut |m| {
        messages.push(m)
    })
    .unwrap();
    let first = Catalog::read(dir.path()).unwrap();
    for role in Role::ALL {
        let carrier = first.carrier(role).unwrap();
        println!(
            "{role:?}: material {:08X}, model {:08X}",
            carrier.material, carrier.model
        );
        first.material(dir.path(), role).unwrap();
    }
    assert!(!first.cube.is_null());
    messages.clear();
    export(&mut reader, &packages, dir.path(), &mut |m| {
        messages.push(m)
    })
    .unwrap();
    assert!(messages.iter().any(|m| m.contains("Using cached")));
    let second = Catalog::read(dir.path()).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
}
