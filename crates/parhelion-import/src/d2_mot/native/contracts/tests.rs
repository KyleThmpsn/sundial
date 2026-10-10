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
#[ignore = "Requires configured local native packages, never installed content writes"]
fn configured_packages_discover_contracts_and_preserve_repeated_exports() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_NATIVE_PACKAGES").unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut reader = Reader::new(&packages, dir.path(), false).unwrap();
    export(&mut reader, &packages, dir.path(), &mut |_| {}).unwrap();
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
    let alternate = tempfile::tempdir().unwrap();
    fs::write(
        alternate
            .path()
            .join(format!("{}-previous.json", Catalog::SCHEMA)),
        serde_json::to_vec(&first).unwrap(),
    )
    .unwrap();
    let reused = reusable_carriers(
        alternate.path(),
        &alternate
            .path()
            .join(format!("{}-current.json", Catalog::SCHEMA)),
        &reader,
    )
    .unwrap();
    assert_eq!(reused.carriers.len(), Role::ALL.len());
    export(&mut reader, &packages, dir.path(), &mut |_| {}).unwrap();
    let second = Catalog::read(dir.path()).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
}
