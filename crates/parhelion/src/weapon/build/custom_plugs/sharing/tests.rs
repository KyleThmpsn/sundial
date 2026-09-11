use super::*;

#[test]
#[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES, uses an isolated package view"]
fn native_identical_frames_share_one_plug_and_changed_speed_stays_private() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap());
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let mut redacted = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../../../recipes/redacted.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    let vaultbreaker = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../../../recipes/vaultbreaker.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    let mut changed = redacted.clone();
    changed.namespace = "parhelion.shared-frame.changed".into();
    changed.identity = WeaponCloneIdentity::from_namespace(&changed.namespace).unwrap();
    changed.text.name = "Changed Frame".into();
    change_speed(&mut changed);
    let mut alternative = changed.overrides.socket_plug_variants[0].clone();
    alternative.choice_index = 1;
    redacted.overrides.socket_plug_variants.push(alternative);
    redacted.overrides.socket_columns[0]
        .as_mut()
        .unwrap()
        .choices = vec![0xDD5C_B37A; 3];
    let project = WeaponProjectSpec {
        weapons: vec![redacted.clone(), vaultbreaker.clone(), changed.clone()],
    };
    let weapons = canonical_project_weapons(&project).unwrap();
    let sources = sources::load_project_sources(view.path()).unwrap();
    let resolved = resolve::resolve_project_weapons(&sources, &weapons).unwrap();
    let templates = PerkTemplates::read(&sources).unwrap();
    let plugs = super::super::plan(&sources, &resolved, &templates.strings).unwrap();
    assert_eq!(
        plugs.len(),
        2,
        "Identical frames must share their complete native plug"
    );
    assert_eq!(plugs.iter().map(|plug| plug.uses.len()).sum::<usize>(), 4);
    let redacted_ordinal = weapons
        .iter()
        .position(|weapon| weapon.namespace == redacted.namespace)
        .unwrap();
    let shared = plug_for_choice(&plugs, redacted_ordinal, 0);
    let changed_plug = plug_for_choice(&plugs, redacted_ordinal, 1);
    assert_eq!(shared.uses.len(), 2);
    assert_eq!(changed_plug.uses.len(), 2);
    assert_ne!(shared.authored_item_hash, changed_plug.authored_item_hash);
    assert_ne!(
        shared.sandbox_perks[0].authored_runtime_key,
        changed_plug.sandbox_perks[0].authored_runtime_key
    );
    let stock_action =
        load_sandbox_perk_runtime_action(&sources.manager, &sources.globals_data, 1178).unwrap();
    let stock_owner_tag = TagHash(0x8152_82E7);
    let stock_owner = sources.manager.read_tag(stock_owner_tag).unwrap();
    let stock_plug_tag = shared.source_definition_tag;
    let stock_plug = shared.source_definition.clone();
    let shared_hash = shared.authored_item_hash;
    let changed_hash = changed_plug.authored_item_hash;
    drop(sources);

    let forward = build_weapon_project_after_catalog_validation(view.path(), &project).unwrap();
    let mut reverse_project = project.clone();
    reverse_project.weapons.reverse();
    let reverse =
        build_weapon_project_after_catalog_validation(view.path(), &reverse_project).unwrap();
    assert_eq!(forward.artifacts.len(), reverse.artifacts.len());
    for (left, right) in forward.artifacts.iter().zip(&reverse.artifacts) {
        assert_eq!(left.plan.output_file_name, right.plan.output_file_name);
        assert_eq!(
            left.bytes(),
            right.bytes(),
            "Sharing must not depend on selection order"
        );
    }
    let temporary = tempfile::tempdir().unwrap();
    for artifact in &forward.artifacts {
        let path = temporary.path().join(&artifact.plan.output_file_name);
        fs::write(&path, artifact.bytes()).unwrap();
        view.add_overlay(&path).unwrap();
    }
    let manager = open_manager(view.path()).unwrap();
    let globals = read_tag(
        &manager,
        resolve_live_named_tag(&manager, "investment_globals", None).unwrap(),
        "globals",
    )
    .unwrap();
    verify_installed_frames(
        &manager,
        &globals,
        [&redacted, &vaultbreaker, &changed],
        [shared_hash, changed_hash],
        stock_action.action_tag,
    );
    assert_eq!(manager.read_tag(stock_owner_tag).unwrap(), stock_owner);
    assert_eq!(manager.read_tag(stock_plug_tag).unwrap(), stock_plug);
    assert_eq!(
        manager.read_tag(stock_action.action_tag).unwrap(),
        stock_action.action_payload
    );
}

fn plug_for_choice(
    plugs: &[ResolvedCustomPlug],
    weapon_ordinal: usize,
    choice_index: usize,
) -> &ResolvedCustomPlug {
    plugs
        .iter()
        .find(|plug| {
            plug.uses.iter().any(|usage| {
                usage.weapon_ordinal == weapon_ordinal && usage.choice_index == choice_index
            })
        })
        .unwrap()
}

fn verify_private_speed(
    manager: &tiger_pkg::PackageManager,
    globals: &[u8],
    definition: &[u8],
    stock_action_tag: TagHash,
    expected_speed: f32,
) {
    let perks = weapon_sandbox_perks(definition).unwrap();
    let private =
        load_sandbox_perk_runtime_action(manager, globals, usize::from(perks[0])).unwrap();
    assert_ne!(private.action_tag, stock_action_tag);
    let native_graph = private
        .graphs
        .iter()
        .find(|graph| graph.tag.pkg_id() == PRIVATE_PERK_RUNTIME_PACKAGE_ID)
        .unwrap();
    let graph = sundial::package_authoring::weapon_runtime::load_weapon_runtime_graph_for_entity(
        manager,
        0,
        0,
        native_graph.tag.0,
        &native_graph.payload,
    )
    .unwrap();
    let mut checked = 0;
    for field in graph.fields() {
        let offset = match (
            field.locator.root_schema,
            field.locator.value_offset,
            field.locator.byte_size,
        ) {
            (0x8080_388F, 0x48, 112) => 0x40,
            (0x8080_3B73, 0xA0, 256) => 0xA4,
            _ => continue,
        };
        if let sundial::package_authoring::weapon_runtime::WeaponRuntimeValue::Bytes(bytes) =
            &field.value
        {
            assert_eq!(read_u32(bytes, offset).unwrap(), expected_speed.to_bits());
            checked += 1;
        }
    }
    assert!(checked >= 2);
}

fn verify_installed_frames(
    manager: &tiger_pkg::PackageManager,
    globals: &[u8],
    weapons: [&WeaponCloneSpec; 3],
    hashes: [u32; 2],
    stock_action_tag: TagHash,
) {
    let [redacted, vaultbreaker, changed] = weapons;
    let [shared_hash, changed_hash] = hashes;
    let root = read_tag(manager, TagHash(read_u32(globals, 16).unwrap()), "root").unwrap();
    let items = read_tag(
        manager,
        root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        "items",
    )
    .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let by_hash = index_item_rows_by_hash(&items, rows, count).unwrap();
    let load_item = |hash| {
        let index = by_hash[&hash][0];
        let tag = TagHash(read_u32(&items, rows + index * ITEM_ROW_SIZE + 16).unwrap());
        manager.read_tag(tag).unwrap()
    };
    for (weapon, expected) in [
        (redacted, shared_hash),
        (vaultbreaker, shared_hash),
        (changed, changed_hash),
    ] {
        let definition = load_item(weapon.identity.item_hash);
        let index = weapon_default_plug_indices(&definition).unwrap()[0];
        assert_eq!(
            read_u32(&items, rows + usize::from(index) * ITEM_ROW_SIZE).unwrap(),
            expected
        );
    }
    assert_eq!(by_hash[&shared_hash].len(), 1);
    let definition = load_item(redacted.identity.item_hash);
    let sockets = relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, socket_rows, _) = array_at(&definition, sockets).unwrap();
    let (member_count, _, members, _) = array_at(
        &definition,
        socket_rows + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
    )
    .unwrap();
    assert_eq!(member_count, 3);
    let hashes = (0..member_count)
        .map(|index| {
            let item_index = read_u16(
                &definition,
                members + index * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE,
            )
            .unwrap();
            read_u32(&items, rows + usize::from(item_index) * ITEM_ROW_SIZE).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(hashes, vec![shared_hash, changed_hash, 0xDD5C_B37A]);
    for (hash, expected_speed) in [(shared_hash, 62.5_f32), (changed_hash, 125.0_f32)] {
        verify_private_speed(
            manager,
            globals,
            &load_item(hash),
            stock_action_tag,
            expected_speed,
        );
    }
}

fn change_speed(weapon: &mut WeaponCloneSpec) {
    for value in &mut weapon.overrides.socket_plug_variants[0].sandbox_perks[0].runtime_values {
        let sundial::package_authoring::weapon_runtime::WeaponRuntimeValue::Bytes(bytes) =
            &mut value.value
        else {
            panic!("bounded speed bytes");
        };
        let offset = match value.locator.root_schema {
            0x8080_388F => 0x40,
            0x8080_3B73 => 0xA4,
            schema => panic!("unexpected speed schema {schema:08X}"),
        };
        write_u32(bytes, offset, 125.0_f32.to_bits()).unwrap();
    }
}
