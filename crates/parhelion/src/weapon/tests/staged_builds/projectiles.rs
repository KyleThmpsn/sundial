use super::*;

#[test]
#[ignore = "requires PARHELION_PROJECTILE_TEST_PACKAGES pointing to Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Independent native-byte audit checks the owner, graph, both caster links, and stale recipe rejection together"
)]
fn private_referenced_telesto_graph_preserves_stock_and_shares_both_caster_links() {
    use sundial::package_authoring::weapon_runtime::load_weapon_runtime_graph_for_entity;
    let packages = PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_TEST_PACKAGES").unwrap());
    let manager = open_manager(&packages).unwrap();
    let graph_tag = TagHash(0x80BB_B1B9);
    let stock_graph = read_tag(&manager, graph_tag, "Telesto projectile graph").unwrap();
    let graph =
        load_weapon_runtime_graph_for_entity(&manager, 0, 0, graph_tag.0, &stock_graph).unwrap();
    let mut values = Vec::new();
    for (root, schema, offset) in [
        (WeaponRuntimeRootKind::ComponentInstance, 0x8080_3B73, 0x144),
        (
            WeaponRuntimeRootKind::ComponentDefinition,
            0x8080_388F,
            0x88,
        ),
    ] {
        let field = graph
            .fields()
            .find(|field| {
                field.locator.root == root
                    && field.locator.root_schema == schema
                    && field.locator.value_offset <= offset
                    && field.locator.value_offset + field.locator.byte_size >= offset + 4
            })
            .unwrap();
        let WeaponRuntimeValue::Bytes(mut bytes) = field.value.clone() else {
            panic!("bounded field")
        };
        let relative = (offset - field.locator.value_offset) as usize;
        assert_eq!(read_u32(&bytes, relative).unwrap(), 1f32.to_bits());
        write_u32(&mut bytes, relative, 30f32.to_bits()).unwrap();
        let mut locator = field.locator.clone();
        locator.binding_hash = 0xB176_70ED;
        locator.resource_index = 0;
        values.push(WeaponRuntimeValueOverride {
            locator,
            value: WeaponRuntimeValue::Bytes(bytes),
        });
    }
    if let Some(path) = std::env::var_os("PARHELION_ERGO_SPEED_RECIPE") {
        let recipe = crate::WeaponRecipe::load_json(path).unwrap();
        let spec = recipe.to_spec().unwrap();
        assert_eq!(spec.overrides.runtime_resource_patches.len(), 2);
        for patch in &spec.overrides.runtime_resource_patches {
            assert_eq!(patch.graph_values, values);
        }
    }
    let pattern = sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 395).unwrap();
    let mut entity = pattern.payload;
    let patches = [21544, 22056].map(|offset| WeaponRuntimeResourcePatch {
        binding_hash: 0x2D8A_944C,
        resource_index: 0,
        offset,
        bytes: graph_tag.0.to_le_bytes().to_vec(),
        graph_values: values.clone(),
    });
    let allocator = AppendedTagAllocator::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, 7000);
    let mut tags = Vec::new();
    append_patched_runtime_resource_owners(
        &manager,
        &mut entity,
        &[],
        &patches,
        allocator,
        &mut tags,
    )
    .unwrap();
    assert_eq!(
        tags.len(),
        3,
        "both caster references must share one private graph"
    );
    assert_eq!(tags[0].template_tag.0, 0x8152_9502);
    assert_eq!(tags[1].template_tag, graph_tag);
    assert_eq!(tags[2].template_tag.0, 0x81A6_ABFB);
    let normalize = |data: &mut [u8], from: u32, to: u32| {
        for word in data.chunks_exact_mut(4) {
            if word == from.to_le_bytes() {
                word.copy_from_slice(&to.to_le_bytes());
            }
        }
    };
    let owner_tag = allocator.assigned_tag(0, "test", "owner").unwrap();
    let private_graph = allocator.assigned_tag(1, "test", "graph").unwrap();
    let sword_tag = allocator.assigned_tag(2, "test", "sword").unwrap();
    let mut owner = tags[0].payload.clone();
    for offset in [0x274, 0x12F8] {
        assert_eq!(read_u32(&owner, offset).unwrap(), 30f32.to_bits());
        write_u32(&mut owner, offset, 1f32.to_bits()).unwrap();
    }
    normalize(&mut owner, owner_tag.0, 0x8152_9502);
    assert_eq!(
        owner,
        read_tag(&manager, TagHash(0x8152_9502), "stock Telesto owner").unwrap()
    );
    let mut normalized_graph = tags[1].payload.clone();
    normalize(&mut normalized_graph, owner_tag.0, 0x8152_9502);
    assert_eq!(normalized_graph, stock_graph);
    let mut sword = tags[2].payload.clone();
    for (offset, source) in [(0x54A8, 0x81A6_AB70), (0x56A8, 0x81A6_ABA5)] {
        assert_eq!(read_u32(&sword, offset).unwrap(), private_graph.0);
        write_u32(&mut sword, offset, source).unwrap();
    }
    normalize(&mut sword, sword_tag.0, 0x81A6_ABFB);
    assert_eq!(
        sword,
        read_tag(&manager, TagHash(0x81A6_ABFB), "stock caster owner").unwrap()
    );
    let mut stale = patches[0].clone();
    stale.graph_values[0].locator.root_schema = 0xDEAD_BEEF;
    assert!(
        append_patched_runtime_resource_owners(
            &manager,
            &mut stock_graph.clone(),
            &[],
            &[stale],
            allocator,
            &mut Vec::new()
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
#[allow(clippy::cognitive_complexity)]
fn real_breachlight_private_micro_missile_perk_chain_round_trips_when_configured() {
    const BREACHLIGHT_ITEM_HASH: u32 = 0x4CE3_CE93;
    const MICRO_MISSILE_PLUG_HASH: u32 = 0xDD5C_B37A;
    const MICRO_MISSILE_PERK_INDEX: usize = 1178;
    const TRAIT_SOCKET_INDEX: usize = 4;

    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let packages = PathBuf::from(packages);
    let source_manager = open_manager(&packages).expect("clean-stock manager should open");
    let source_globals_tag = resolve_live_named_tag(&source_manager, "investment_globals", None)
        .expect("investment globals should be named");
    let source_globals = read_tag(&source_manager, source_globals_tag, "investment globals")
        .expect("investment globals should load");
    let source_action = sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action(
        &source_manager,
        &source_globals,
        MICRO_MISSILE_PERK_INDEX,
    )
    .expect("Micro-Missile's finished-perk runtime chain should resolve");
    assert_eq!(source_action.finished_perk.perk_hash, 0xDD1B_DD38);
    assert_eq!(source_action.action_tag, TagHash(0x80BC_2BBD));
    assert!(!source_action.graphs.is_empty());

    let projectile_speed_scale = WeaponSandboxPerkActionFloatOverride {
        node_type_handle: 0x8080_2F16,
        node_occurrence: 1,
        value_pointer_offset: 0x140,
        value_type_handle: 0x8080_2F1A,
        expected_bits: 0.5_f32.to_bits(),
        value_bits: 5.0_f32.to_bits(),
    };

    let source_root = packages
        .parent()
        .expect("configured package directory should have an install root");
    let source_root_table_tag = TagHash(read_u32(&source_globals, 16).unwrap());
    let source_root_table = read_tag(&source_manager, source_root_table_tag, "investment root")
        .expect("investment root should load");
    let source_item_table_tag = root_child_tag(&source_root_table, ROOT_ITEM_DEFINITION_TABLE_SLOT)
        .expect("item table tag should resolve");
    let source_item_table = read_tag(&source_manager, source_item_table_tag, "item table")
        .expect("item table should load");
    let (source_item_count, _, source_item_rows, _) =
        array_at(&source_item_table, 8).expect("item table should decode");
    let source_micro_item_index = find_u32_row_key(
        &source_item_table,
        source_item_rows,
        source_item_count,
        ITEM_ROW_SIZE,
        MICRO_MISSILE_PLUG_HASH,
    )
    .expect("Micro-Missile item lookup should decode")
    .expect("Micro-Missile plug should exist");
    let source_micro_definition_tag = TagHash(
        read_u32(
            &source_item_table,
            source_item_rows + source_micro_item_index * ITEM_ROW_SIZE + 16,
        )
        .unwrap(),
    );
    let breachlight_item_index = find_u32_row_key(
        &source_item_table,
        source_item_rows,
        source_item_count,
        ITEM_ROW_SIZE,
        BREACHLIGHT_ITEM_HASH,
    )
    .expect("Breachlight item lookup should decode")
    .expect("Breachlight should exist");
    let breachlight_definition_tag = TagHash(
        read_u32(
            &source_item_table,
            source_item_rows + breachlight_item_index * ITEM_ROW_SIZE + 16,
        )
        .unwrap(),
    );
    let breachlight_definition = read_tag(
        &source_manager,
        breachlight_definition_tag,
        "Breachlight definition",
    )
    .expect("Breachlight definition should load");
    let breachlight_socket_count = weapon_default_plug_indices(&breachlight_definition)
        .expect("Breachlight socket topology should decode")
        .len();
    assert!(TRAIT_SOCKET_INDEX < breachlight_socket_count);
    let source_finished_tag =
        globals_child_tag(&source_globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)
            .expect("finished-perk catalog tag should resolve");
    let source_finished = read_tag(
        &source_manager,
        source_finished_tag,
        "finished sandbox perks",
    )
    .expect("finished-perk catalog should load");
    let source_finished_count = finished_sandbox_perk_count(&source_finished).unwrap();
    let source_micro_finished =
        finished_sandbox_perk_at(&source_finished, MICRO_MISSILE_PERK_INDEX).unwrap();
    let source_runtime_map = read_tag(
        &source_manager,
        TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG),
        "sandbox-perk runtime map",
    )
    .expect("sandbox-perk runtime map should load");
    let source_micro_assignment =
        sandbox_perk_runtime_assignment(&source_runtime_map, source_micro_finished.runtime_key)
            .unwrap()
            .expect("Micro-Missile runtime key should resolve");

    let namespace = "parhelion.private-perk-chain.integration";
    let mut socket_columns = vec![None; breachlight_socket_count];
    socket_columns[TRAIT_SOCKET_INDEX] = Some(WeaponSocketColumnOverride {
        choices: vec![MICRO_MISSILE_PLUG_HASH],
        ..WeaponSocketColumnOverride::default()
    });
    let spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: BREACHLIGHT_ITEM_HASH,
        expected_donor_name: Some("Breachlight".to_owned()),
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("integration namespace should allocate"),
        text: WeaponCloneText {
            name: "Private Perk Integration".to_owned(),
            flavor: "A compiler integration fixture.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            socket_columns,
            socket_plug_variants: vec![WeaponSocketPlugVariantOverride {
                investment_stats: vec![(13, 10)],
                socket_index: TRAIT_SOCKET_INDEX as u16,
                choice_index: 0,
                source_plug_hash: MICRO_MISSILE_PLUG_HASH,
                classification_donor_hash: None,
                description: None,
                additional_sandbox_perks: Vec::new(),
                name: Some("Micro-Missile Frame".to_owned()),
                sandbox_perks: vec![WeaponSandboxPerkRuntimeOverride {
                    source_perk_index: MICRO_MISSILE_PERK_INDEX as u16,
                    activation: None,
                    runtime_values: Vec::new(),
                    action_float_values: vec![projectile_speed_scale.clone()],
                }],
            }],
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("private Micro-Missile plug/perk project should compile");
    assert_eq!(bundle.plan.weapons.len(), 1);
    assert_eq!(bundle.artifacts.len(), 9);
    let asset_artifact = bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == PARHELION_ASSET_PACKAGE_ID)
        .expect("project should contain its asset package");
    assert_eq!(asset_artifact.plan.appended_tags.len(), 7);
    let private_runtime_artifact = bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == PRIVATE_PERK_RUNTIME_PACKAGE_ID)
        .expect("project should contain its private perk-runtime package");
    assert_eq!(
        private_runtime_artifact.plan.original_entry_count,
        PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT
    );
    assert_eq!(
        private_runtime_artifact.plan.append_start_entry_count,
        6_469
    );
    assert_eq!(private_runtime_artifact.plan.reserved_entry_count, 1);
    assert_eq!(private_runtime_artifact.plan.final_entry_count, 6_474);
    assert_eq!(private_runtime_artifact.plan.appended_tags.len(), 5);
    let authored_action_tag = TagHash(0x80B7_7945);
    let authored_b9_tag = TagHash(0x80B7_7946);
    let authored_ba_tag = TagHash(0x80B7_7947);
    let authored_residency_root_tag = TagHash(0x80B7_7948);
    let authored_residency_companion_tag = TagHash(0x80B7_7949);
    let loading_index_artifact = bundle
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.plan.chain.identity.package_id == RUNTIME_DEPENDENCY_COMPANION.pkg_id()
        })
        .expect("private runtime edits require an authored native loading index");
    assert!(
        loading_index_artifact.plan.appended_tags.is_empty(),
        "an index update must not reuse any stock action slot"
    );
    let expected_private_runtime_tail = [
        (
            authored_action_tag,
            source_action.action_tag,
            source_action.action_payload.len(),
        ),
        (
            authored_b9_tag,
            PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_B9_SIZE,
        ),
        (
            authored_ba_tag,
            PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_BA_SIZE,
        ),
        (
            authored_residency_root_tag,
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_ROOT_SIZE,
        ),
        (
            authored_residency_companion_tag,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_COMPANION_SIZE,
        ),
    ];
    for (appended, (tag, template_tag, file_size)) in private_runtime_artifact
        .plan
        .appended_tags
        .iter()
        .zip(expected_private_runtime_tail)
    {
        assert_eq!(appended.tag, tag);
        assert_eq!(appended.template_tag, template_tag);
        assert_eq!(appended.file_size, file_size);
    }
    assert_eq!(
        private_runtime_artifact.plan.appended_tags[0].tag, authored_action_tag,
        "the private action must use the first index never occupied by any 01bb generation"
    );
    let private_runtime_layout =
        crate::format::PackageLayout::parse(private_runtime_artifact.bytes())
            .expect("the private runtime overlay should parse with its first shared-tag table");
    assert_eq!(private_runtime_layout.shared_tag_enrollment_count(), 1);
    let mut expected_shared_tag_row = Vec::new();
    expected_shared_tag_row.extend_from_slice(&authored_residency_root_tag.0.to_le_bytes());
    expected_shared_tag_row.extend_from_slice(&authored_residency_companion_tag.0.to_le_bytes());
    assert_eq!(
        private_runtime_layout
            .shared_tag_enrollment_rows(private_runtime_artifact.bytes())
            .expect("the private runtime shared-tag row should decode"),
        expected_shared_tag_row,
    );

    let view = tempfile::Builder::new()
        .prefix(".parhelion-private-perk-test-")
        .tempdir_in(source_root)
        .expect("temporary package view should be created on the package volume");
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).expect("temporary packages directory should be created");
    for entry in fs::read_dir(&packages).expect("clean-stock packages should be listable") {
        let entry = entry.expect("clean-stock package entry should be readable");
        let source = entry.path();
        if source.extension().and_then(|value| value.to_str()) != Some("pkg") {
            continue;
        }
        fs::hard_link(&source, view_packages.join(entry.file_name()))
            .expect("clean-stock package should hard-link into the temporary view");
    }
    let source_oodle = source_root
        .join("bin")
        .join("x64")
        .join("oo2core_3_win64.dll");
    if source_oodle.is_file() {
        let target_bin = view.path().join("bin").join("x64");
        fs::create_dir_all(&target_bin).expect("temporary Oodle directory should be created");
        fs::hard_link(&source_oodle, target_bin.join("oo2core_3_win64.dll"))
            .expect("Oodle runtime should hard-link into the temporary view");
    }
    let staged = bundle
        .write_new(&view_packages)
        .expect("private-perk overlays should stage create-new in the temporary view");
    assert_eq!(staged.len(), bundle.artifacts.len());

    let authored_manager = open_manager(&view_packages).expect("staged package view should open");
    let source_b9 = read_tag(
        &source_manager,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        "stock private perk residency B9",
    )
    .expect("stock private perk residency B9 should load");
    let source_ba = read_tag(
        &source_manager,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        "stock private perk residency BA",
    )
    .expect("stock private perk residency BA should load");
    let source_residency_root = read_tag(
        &source_manager,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        "stock private perk residency root",
    )
    .expect("stock private perk residency root should load");
    let source_residency_companion = read_tag(
        &source_manager,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        "stock private perk residency companion",
    )
    .expect("stock private perk residency companion should load");

    assert_eq!(
        matching_u32_offsets(&source_residency_root, PRIVATE_PERK_RESIDENCY_BA_TEMPLATE.0),
        PRIVATE_PERK_RESIDENCY_ROOT_BA_OFFSETS
    );
    assert!(
        matching_u32_offsets(
            &source_residency_root,
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE.0
        )
        .is_empty()
    );
    assert_eq!(
        matching_u32_offsets(&source_ba, PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE.0),
        PRIVATE_PERK_RESIDENCY_BA_ROOT_OFFSETS
    );
    assert_eq!(
        matching_u32_offsets(&source_ba, PRIVATE_PERK_RESIDENCY_B9_TEMPLATE.0),
        PRIVATE_PERK_RESIDENCY_BA_B9_OFFSETS
    );
    assert!(matching_u32_offsets(&source_ba, PRIVATE_PERK_RESIDENCY_BA_TEMPLATE.0).is_empty());
    assert_eq!(
        matching_u32_offsets(&source_b9, PRIVATE_PERK_RESIDENCY_B9_TEMPLATE.0),
        PRIVATE_PERK_RESIDENCY_B9_SELF_OFFSETS
    );
    assert_eq!(
        matching_u32_offsets(&source_b9, PRIVATE_PERK_RESIDENCY_DONOR_ACTION_TAG.0),
        PRIVATE_PERK_RESIDENCY_B9_ACTION_OFFSETS
    );
    assert_eq!(
        matching_u32_offsets(
            &source_residency_companion,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE.0
        ),
        PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS
    );
    assert_eq!(
        matching_u32_offsets(
            &source_residency_companion,
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE.0
        ),
        PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS
    );

    let mut expected_b9 = source_b9;
    for offset in PRIVATE_PERK_RESIDENCY_B9_SELF_OFFSETS {
        write_u32(&mut expected_b9, offset, authored_b9_tag.0).unwrap();
    }
    for offset in PRIVATE_PERK_RESIDENCY_B9_ACTION_OFFSETS {
        write_u32(&mut expected_b9, offset, authored_action_tag.0).unwrap();
    }
    let mut expected_ba = source_ba;
    for offset in PRIVATE_PERK_RESIDENCY_BA_ROOT_OFFSETS {
        write_u32(&mut expected_ba, offset, authored_residency_root_tag.0).unwrap();
    }
    for offset in PRIVATE_PERK_RESIDENCY_BA_B9_OFFSETS {
        write_u32(&mut expected_ba, offset, authored_b9_tag.0).unwrap();
    }
    let mut expected_residency_root = source_residency_root;
    for offset in PRIVATE_PERK_RESIDENCY_ROOT_BA_OFFSETS {
        write_u32(&mut expected_residency_root, offset, authored_ba_tag.0).unwrap();
    }
    assert_eq!(
        read_tag(
            &authored_manager,
            authored_b9_tag,
            "authored private perk residency B9"
        )
        .expect("authored private perk residency B9 should load"),
        expected_b9,
    );
    assert_eq!(
        read_tag(
            &authored_manager,
            authored_ba_tag,
            "authored private perk residency BA"
        )
        .expect("authored private perk residency BA should load"),
        expected_ba,
    );
    assert_eq!(
        read_tag(
            &authored_manager,
            authored_residency_root_tag,
            "authored private perk residency root",
        )
        .expect("authored private perk residency root should load"),
        expected_residency_root,
    );
    let authored_residency_companion = read_tag(
        &authored_manager,
        authored_residency_companion_tag,
        "authored private perk residency companion",
    )
    .expect("authored private perk residency companion should load");
    assert_eq!(
        authored_residency_companion.len(),
        PRIVATE_PERK_RESIDENCY_COMPANION_SIZE
    );
    assert_eq!(
        matching_u32_offsets(
            &authored_residency_companion,
            authored_residency_companion_tag.0
        ),
        PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS
    );
    assert_eq!(
        matching_u32_offsets(&authored_residency_companion, authored_residency_root_tag.0),
        PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS
    );
    let stock_common_dependencies = [
        TagHash::new(0x0238, 0x0B90),
        TagHash::new(0x0238, 0x0EDC),
        TagHash::new(0x0238, 0x0EDD),
    ];
    assert_eq!(
        validate_shared_tag_companion_payload(
            &source_residency_companion,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        )
        .expect("stock private perk residency companion should parse"),
        dependency_set(
            [
                PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
                PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
            ]
            .into_iter()
            .chain(stock_common_dependencies)
        )
    );
    assert_eq!(
        validate_shared_tag_companion_payload(
            &authored_residency_companion,
            authored_residency_companion_tag,
            authored_residency_root_tag,
        )
        .expect("authored private perk residency companion should parse"),
        dependency_set(
            [
                authored_residency_root_tag,
                authored_residency_companion_tag,
            ]
            .into_iter()
            .chain(stock_common_dependencies)
        ),
        "the authored companion must retain the exact stock topology: new owner/self plus only the three common 0238 dependencies"
    );
    for (authored_tag, template_tag) in [
        (authored_action_tag, source_action.action_tag),
        (authored_b9_tag, PRIVATE_PERK_RESIDENCY_B9_TEMPLATE),
        (authored_ba_tag, PRIVATE_PERK_RESIDENCY_BA_TEMPLATE),
        (
            authored_residency_root_tag,
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        ),
        (
            authored_residency_companion_tag,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        ),
    ] {
        let authored_entry = authored_manager
            .get_entry(authored_tag)
            .expect("authored private runtime entry should exist");
        let template_entry = source_manager
            .get_entry(template_tag)
            .expect("private runtime template entry should exist");
        assert_eq!(authored_entry.file_type, template_entry.file_type);
        assert_eq!(authored_entry.file_subtype, template_entry.file_subtype);
        assert_eq!(
            authored_entry.reference, template_entry.reference,
            "private residency entries must inherit their template references without overrides"
        );
    }
    let stock_loading_index = source_manager
        .read_tag(RUNTIME_DEPENDENCY_COMPANION)
        .unwrap();
    let authored_loading_index = authored_manager
        .read_tag(RUNTIME_DEPENDENCY_COMPANION)
        .unwrap();
    let stock_dependencies = crate::shared_tag_dependency_index::dependencies(
        &stock_loading_index,
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
    )
    .unwrap();
    let authored_dependencies = crate::shared_tag_dependency_index::dependencies(
        &authored_loading_index,
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
    )
    .unwrap();
    assert!(stock_dependencies.contains(&source_action.action_tag.0));
    assert!(!stock_dependencies.contains(&authored_action_tag.0));
    assert_eq!(
        authored_dependencies,
        stock_dependencies
            .into_iter()
            .chain([
                authored_action_tag.0,
                authored_b9_tag.0,
                authored_ba_tag.0,
                authored_residency_root_tag.0,
                authored_residency_companion_tag.0,
            ])
            .collect(),
        "the global loading index must preserve stock content and root every private runtime entry"
    );
    let weapon_plan = &bundle.plan.weapons[0];
    let source_breachlight_runtime =
        sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager(
            &source_manager,
            BREACHLIGHT_ITEM_HASH,
        )
        .expect("Breachlight runtime entity should load");
    let authored_runtime =
        sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager(
            &authored_manager,
            weapon_plan.item_hash,
        )
        .expect("authored Breachlight-based runtime entity should load");
    assert_eq!(
        authored_runtime.payload, source_breachlight_runtime.payload,
        "a private perk-action edit must not alter Breachlight's weapon runtime entity"
    );
    let source_breachlight_graph =
        sundial::package_authoring::weapon_runtime::load_weapon_runtime_graph_for_entity(
            &source_manager,
            source_breachlight_runtime.item_hash,
            source_breachlight_runtime.pattern_global_id_hash,
            source_breachlight_runtime.entity_tag,
            &source_breachlight_runtime.payload,
        )
        .expect("Breachlight runtime graph should decode");
    let authored_graph =
        sundial::package_authoring::weapon_runtime::load_weapon_runtime_graph_for_entity(
            &authored_manager,
            authored_runtime.item_hash,
            authored_runtime.pattern_global_id_hash,
            authored_runtime.entity_tag,
            &authored_runtime.payload,
        )
        .expect("authored runtime graph should decode");
    assert_eq!(
        authored_graph.resources, source_breachlight_graph.resources,
        "the private perk action must preserve every Breachlight runtime resource"
    );
    let authored_weapon = read_tag(
        &authored_manager,
        weapon_plan.definition_tag,
        "authored weapon definition",
    )
    .expect("authored weapon definition should load");
    let authored_weapon_plugs =
        weapon_default_plug_indices(&authored_weapon).expect("authored sockets should decode");
    let custom_plug_index = usize::from(authored_weapon_plugs[TRAIT_SOCKET_INDEX]);
    assert!(custom_plug_index >= source_item_count);

    let authored_globals_tag =
        resolve_live_named_tag(&authored_manager, "investment_globals", None)
            .expect("authored investment globals should remain named");
    let authored_globals = read_tag(
        &authored_manager,
        authored_globals_tag,
        "authored investment globals",
    )
    .expect("authored investment globals should load");
    let authored_root_tag = TagHash(read_u32(&authored_globals, 16).unwrap());
    let authored_root = read_tag(
        &authored_manager,
        authored_root_tag,
        "authored investment root",
    )
    .expect("authored investment root should load");
    let authored_item_table = read_tag(
        &authored_manager,
        root_child_tag(&authored_root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        "authored item table",
    )
    .expect("authored item table should load");
    let (authored_item_count, _, authored_item_rows, _) =
        array_at(&authored_item_table, 8).expect("authored item table should decode");
    assert_eq!(authored_item_count, source_item_count + 2);
    assert_eq!(
        read_u32(
            &authored_item_table,
            authored_item_rows + source_micro_item_index * ITEM_ROW_SIZE,
        )
        .unwrap(),
        MICRO_MISSILE_PLUG_HASH,
        "the stock Micro-Missile item row must remain intact"
    );
    assert_eq!(
        TagHash(
            read_u32(
                &authored_item_table,
                authored_item_rows + source_micro_item_index * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        ),
        source_micro_definition_tag,
        "the stock Micro-Missile definition tag must remain intact"
    );
    let custom_plug_hash = read_u32(
        &authored_item_table,
        authored_item_rows + custom_plug_index * ITEM_ROW_SIZE,
    )
    .unwrap();
    assert_ne!(custom_plug_hash, MICRO_MISSILE_PLUG_HASH);
    let custom_plug_definition_tag = TagHash(
        read_u32(
            &authored_item_table,
            authored_item_rows + custom_plug_index * ITEM_ROW_SIZE + 16,
        )
        .unwrap(),
    );
    let custom_plug_definition = read_tag(
        &authored_manager,
        custom_plug_definition_tag,
        "private plug definition",
    )
    .expect("private plug definition should load");
    let resource =
        relative_target(&custom_plug_definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (count, _, rows, _) = array_at(&custom_plug_definition, resource).unwrap();
    assert!(
        (0..count).any(|index| {
            let row = rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
            read_u8(&custom_plug_definition, row).unwrap() == 13
                && read_i32(&custom_plug_definition, row + 4).unwrap() == 10
        }),
        "custom stat must survive package emission and reloading"
    );
    let private_perk_indices =
        weapon_sandbox_perks(&custom_plug_definition).expect("private plug perks should decode");
    let private_perk_index = private_perk_indices
        .iter()
        .copied()
        .find(|index| usize::from(*index) >= source_finished_count)
        .expect("private plug should point at an appended finished-perk row");

    let authored_finished = read_tag(
        &authored_manager,
        globals_child_tag(&authored_globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT).unwrap(),
        "authored finished sandbox perks",
    )
    .expect("authored finished-perk catalog should load");
    assert_eq!(
        finished_sandbox_perk_count(&authored_finished).unwrap(),
        source_finished_count + 1
    );
    assert_eq!(
        finished_sandbox_perk_at(&authored_finished, MICRO_MISSILE_PERK_INDEX).unwrap(),
        source_micro_finished,
        "the stock Micro-Missile finished-perk row must remain intact"
    );
    let private_finished =
        finished_sandbox_perk_at(&authored_finished, usize::from(private_perk_index)).unwrap();
    assert_ne!(private_finished.perk_hash, source_micro_finished.perk_hash);
    assert_ne!(
        private_finished.runtime_key,
        source_micro_finished.runtime_key
    );

    let authored_runtime_map = read_tag(
        &authored_manager,
        TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG),
        "authored sandbox-perk runtime map",
    )
    .expect("authored sandbox-perk runtime map should load");
    assert_eq!(
        sandbox_perk_runtime_assignment_count(&authored_runtime_map).unwrap(),
        sandbox_perk_runtime_assignment_count(&source_runtime_map).unwrap() + 2,
        "the shared runtime map should gain one weapon-pattern identity and one private-perk key"
    );
    let authored_pattern_id = WeaponCloneIdentity::from_namespace(namespace)
        .unwrap()
        .pattern_global_id_hash;
    assert!(
        weapon_entity_assignment(&authored_runtime_map, authored_pattern_id)
            .unwrap()
            .is_some(),
        "the authored weapon-pattern assignment must survive beside the private-perk assignment"
    );
    let authored_source_assignment =
        sandbox_perk_runtime_assignment(&authored_runtime_map, source_micro_finished.runtime_key)
            .unwrap()
            .unwrap();
    assert_eq!(
        (
            authored_source_assignment.runtime_key,
            authored_source_assignment.runtime_tag,
        ),
        (
            source_micro_assignment.runtime_key,
            source_micro_assignment.runtime_tag,
        ),
        "the stock Micro-Missile runtime mapping must remain intact"
    );
    let private_assignment =
        sandbox_perk_runtime_assignment(&authored_runtime_map, private_finished.runtime_key)
            .unwrap()
            .expect("private runtime key should resolve");
    assert_ne!(
        private_assignment.runtime_tag, source_micro_assignment.runtime_tag,
        "an edited private finished perk must receive a private runtime action"
    );
    assert_eq!(
        TagHash(private_assignment.runtime_tag).pkg_id(),
        PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        "the private action must live in the dedicated optional runtime host"
    );
    assert_eq!(
        private_assignment.runtime_tag, 0x80B7_7945,
        "the private runtime-map row must target the virgin 01bb datum"
    );

    let private_action =
        sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action(
            &authored_manager,
            &authored_globals,
            usize::from(private_perk_index),
        )
        .expect("the staged private finished-perk chain should resolve");
    assert_eq!(private_action.action_tag.0, private_assignment.runtime_tag);
    let speed_offset = sandbox_perk_action_boxed_value_offset(
        &source_action.action_payload,
        projectile_speed_scale.node_type_handle,
        projectile_speed_scale.node_occurrence,
        projectile_speed_scale.value_pointer_offset,
        projectile_speed_scale.value_type_handle,
        size_of::<f32>(),
    )
    .expect("Micro-Missile's projectile speed scalar should resolve structurally");
    assert_eq!(
        read_u32(&source_action.action_payload, speed_offset).unwrap(),
        projectile_speed_scale.expected_bits
    );
    assert_eq!(
        read_u32(&private_action.action_payload, speed_offset).unwrap(),
        projectile_speed_scale.value_bits
    );
    let mut expected_private_action = source_action.action_payload.clone();
    write_u32(
        &mut expected_private_action,
        speed_offset,
        projectile_speed_scale.value_bits,
    )
    .unwrap();
    assert_eq!(
        private_action.action_payload, expected_private_action,
        "the private action must differ from stock Micro-Missile by only the selected scalar"
    );
}

#[test]
#[ignore = "requires PARHELION_PROJECTILE_TEST_PACKAGES pointing to Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Independent integration audit compares every native graph and action field with stock"
)]
fn real_private_projectile_speed_clone_preserves_stock_graph_and_action() {
    use sundial::package_authoring::weapon_runtime::{
        WeaponRuntimeRootKind, WeaponRuntimeValue, load_weapon_runtime_graph_for_entity,
    };
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_PROJECTILE_TEST_PACKAGES")
            .expect("set PARHELION_PROJECTILE_TEST_PACKAGES"),
    );
    let manager = open_manager(&packages).unwrap();
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = read_tag(&manager, globals_tag, "globals").unwrap();
    let action = load_sandbox_perk_runtime_action(&manager, &globals, 1178).unwrap();
    assert_eq!(action.action_tag.0, 0x80BC_2BBD);
    let graph = action
        .graphs
        .iter()
        .find(|graph| graph.tag.0 == 0x8152_82E1)
        .unwrap();
    let decoded =
        load_weapon_runtime_graph_for_entity(&manager, 0, 0, graph.tag.0, &graph.payload).unwrap();
    let field = decoded
        .fields()
        .find(|field| {
            field.locator.root_schema == 0x8080_388F
                && field.locator.root == WeaponRuntimeRootKind::ComponentDefinition
                && field.locator.value_offset == 0x48
                && field.locator.byte_size == 112
        })
        .unwrap();
    assert_eq!(field.locator.root_schema, 0x8080_388F);
    assert_eq!(field.owner_offset, 0x17D8);
    let WeaponRuntimeValue::Bytes(mut bytes) = field.value.clone() else {
        panic!("expected bounded raw field");
    };
    assert_eq!(read_u32(&bytes, 0x40).unwrap(), 1.0_f32.to_bits());
    write_u32(&mut bytes, 0x40, 62.5_f32.to_bits()).unwrap();
    let mut locator = field.locator.clone();
    locator.binding_hash = 0xB176_70ED; // Select the unambiguous alias used by the recipe.
    locator.resource_index = 0;
    let value = WeaponRuntimeValueOverride {
        locator,
        value: WeaponRuntimeValue::Bytes(bytes),
    };
    // Launch can copy prebuilt component state without resetting it from the definition.
    // Keep the serialized prototype and the reset source in agreement.
    let instance_field = decoded
        .fields()
        .find(|field| {
            field.locator.root_schema == 0x8080_3B73
                && field.locator.root == WeaponRuntimeRootKind::ComponentInstance
                && field.locator.value_offset == 0xA0
                && field.locator.byte_size == 256
        })
        .unwrap();
    assert_eq!(instance_field.owner_offset, 0x1D0);
    let WeaponRuntimeValue::Bytes(mut instance_bytes) = instance_field.value.clone() else {
        panic!("expected bounded prototype field");
    };
    assert_eq!(read_u32(&instance_bytes, 0xA4).unwrap(), 1.0_f32.to_bits());
    write_u32(&mut instance_bytes, 0xA4, 62.5_f32.to_bits()).unwrap();
    let mut instance_locator = instance_field.locator.clone();
    instance_locator.binding_hash = 0xB176_70ED;
    instance_locator.resource_index = 0;
    let values = vec![
        value,
        WeaponRuntimeValueOverride {
            locator: instance_locator,
            value: WeaponRuntimeValue::Bytes(instance_bytes),
        },
    ];
    if let Some(path) = std::env::var_os("PARHELION_PRIVATE_SPEED_RECIPE") {
        let recipe = crate::WeaponRecipe::load_json(path).unwrap();
        let spec = recipe.to_spec().unwrap();
        let variant = spec
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| variant.name.as_deref() == Some("Micro-Missile Frame"))
            .unwrap();
        assert_eq!(variant.sandbox_perks.len(), 1);
        assert_eq!(variant.sandbox_perks[0].source_perk_index, 1178);
        assert!(variant.sandbox_perks[0].action_float_values.is_empty());
        assert_eq!(variant.sandbox_perks[0].runtime_values, values);
    }
    let mut tags = Vec::new();
    let allocator = AppendedTagAllocator::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, 6469);
    let private_action = clone_private_sandbox_perk_runtime(
        &manager,
        &action,
        &values,
        &[],
        None,
        allocator,
        &mut tags,
    )
    .unwrap();
    assert_eq!(tags.len(), 7); // Owner, graph, action and four residency records.
    assert_eq!(tags[0].template_tag.0, 0x8152_82E7);
    assert_eq!(tags[1].template_tag, graph.tag);
    assert_eq!(tags[2].template_tag, action.action_tag);
    let owner_tag = allocator.assigned_tag(0, "test", "owner").unwrap();
    let graph_tag = allocator.assigned_tag(1, "test", "graph").unwrap();
    assert_eq!(
        private_action,
        allocator.assigned_tag(2, "test", "action").unwrap()
    );
    let normalize = |bytes: &mut [u8], from: u32, to: u32| {
        for word in bytes.chunks_exact_mut(4) {
            if word == from.to_le_bytes() {
                word.copy_from_slice(&to.to_le_bytes());
            }
        }
    };
    let stock_owner = read_tag(&manager, TagHash(0x8152_82E7), "stock projectile owner").unwrap();
    let mut normalized_owner = tags[0].payload.clone();
    assert_eq!(
        read_u32(&normalized_owner, 0x1818).unwrap(),
        62.5_f32.to_bits()
    );
    write_u32(&mut normalized_owner, 0x1818, 1.0_f32.to_bits()).unwrap();
    assert_eq!(
        read_u32(&normalized_owner, 0x274).unwrap(),
        62.5_f32.to_bits()
    );
    write_u32(&mut normalized_owner, 0x274, 1.0_f32.to_bits()).unwrap();
    normalize(&mut normalized_owner, owner_tag.0, 0x8152_82E7);
    assert_eq!(
        normalized_owner, stock_owner,
        "only both speed fields and self references may change"
    );
    let mut normalized_graph = tags[1].payload.clone();
    normalize(&mut normalized_graph, owner_tag.0, 0x8152_82E7);
    assert_eq!(
        normalized_graph, graph.payload,
        "all other graph slots must remain stock"
    );
    let mut normalized_action = tags[2].payload.clone();
    normalize(&mut normalized_action, graph_tag.0, graph.tag.0);
    assert_eq!(
        normalized_action, action.action_payload,
        "restore the unrelated boxed-float experiment"
    );
    assert_eq!(
        read_tag(&manager, TagHash(0x8152_82E7), "unchanged stock owner").unwrap(),
        stock_owner
    );
    assert!(
        !tags
            .iter()
            .any(|tag| tag.template_tag.0 == 0x8157_93FC || tag.template_tag.0 == 0x8152_8276)
    );
    assert_eq!(1.26_f32 * 62.5_f32, 78.75_f32);

    if std::env::var_os("PARHELION_VERIFY_INSTALLED_PRIVATE_SPEED").is_some() {
        let installed = load_sandbox_perk_runtime_action(&manager, &globals, 2481).unwrap();
        assert_eq!(installed.action_tag, private_action);
        assert!(installed.graphs.iter().any(|graph| graph.tag == graph_tag));
        let loading_index = read_tag(
            &manager,
            RUNTIME_DEPENDENCY_COMPANION,
            "installed runtime loading index",
        )
        .unwrap();
        let dependencies = crate::shared_tag_dependency_index::dependencies(
            &loading_index,
            RUNTIME_DEPENDENCY_COMPANION,
            RUNTIME_DEPENDENCY_ROOT,
        )
        .unwrap();
        assert!(dependencies.contains(&action.action_tag.0));
        let redacted =
            read_tag(&manager, TagHash(0x81A2_9550), "installed Redacted fixture").unwrap();
        assert_eq!(weapon_sandbox_perks(&redacted).unwrap(), vec![449]);
        let resource = relative_target(&redacted, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
        let (_, header, _, _) =
            array_at(&redacted, resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET).unwrap();
        assert_eq!(
            &redacted[header - 8..header],
            NESTED_ARRAY_TRAILER.as_slice()
        );
        if let Some(path) = std::env::var_os("PARHELION_EXPORT_REDACTED_FIXTURE") {
            std::fs::write(path, &redacted).unwrap();
        }
        for (index, tag) in tags.iter().enumerate() {
            let assigned = allocator
                .assigned_tag(index, "test", "installed tag")
                .unwrap();
            assert_eq!(
                read_tag(&manager, assigned, "installed private runtime tag").unwrap(),
                tag.payload,
                "installed tag {assigned:?} must match the validated clone"
            );
            assert!(
                dependencies.contains(&assigned.0),
                "missing loading dependency {assigned:?}"
            );
        }
    }
}
