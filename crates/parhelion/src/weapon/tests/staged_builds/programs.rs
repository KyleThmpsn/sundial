use super::*;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{Action, NativeNode, Program, Trigger},
};

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn private_graph_scopes_route_matching_fields_and_reject_the_wrong_asset() {
    use sundial::package_authoring::{
        sandbox_perk::{
            self,
            program::{Asset, Position},
        },
        weapon_runtime::*,
    };
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let mut stock = load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
    let entities = [0x8152_82E1, 0x80BB_B1B9];
    let program = Program {
        trigger: Trigger::WeaponKill,
        actions: entities
            .iter()
            .map(|&graph| Action::Spawn {
                asset: Asset {
                    graph,
                    ..Asset::default()
                },
                position: Position::Event,
            })
            .collect(),
        ..Program::default()
    };
    let compiled = sandbox_perk::program::compile(&manager, &program).unwrap();
    stock.action_payload = compiled.payload;
    stock.graphs = entities
        .iter()
        .zip(compiled.graph_offsets)
        .map(
            |(&tag, offset)| sandbox_perk::SandboxPerkRuntimeGraphSource {
                tag: TagHash(tag),
                action_offsets: vec![offset.unwrap()],
                payload: manager.read_tag(TagHash(tag)).unwrap(),
            },
        )
        .collect();
    let mut values = Vec::new();
    for (index, source) in stock.graphs.iter().enumerate() {
        let mut graph =
            load_weapon_runtime_graph_for_entity(&manager, 0, 0, source.tag.0, &source.payload)
                .unwrap();
        graph.scope_fields();
        let speed = sandbox_perk::projectile::parameters::discover(&graph)
            .into_iter()
            .find(|parameter| parameter.kind == sandbox_perk::projectile::parameters::Kind::Speed)
            .unwrap();
        speed.set(&mut values, 2.0 + index as f32).unwrap();
        assert_eq!(speed.value(&values).unwrap(), 2.0 + index as f32);
    }
    assert!(values.iter().all(|value| value.locator.graph_tag.is_some()));
    // The graph identity must be what resolves at least one otherwise ambiguous field.
    assert!(values.iter().any(|value| {
        stock
            .graphs
            .iter()
            .filter(|source| {
                let mut locator = value.locator.clone();
                locator.graph_tag = None;
                resolve_weapon_runtime_field(&manager, &source.payload, &locator).is_ok()
            })
            .count()
            > 1
    }));
    let allocator = AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0);
    let before = stock.clone();
    for form in 0..3 {
        let program = form != 0;
        let mut authored_program = Program {
            trigger: Trigger::WeaponKill,
            actions: Vec::new(),
            ..Program::default()
        };
        if program {
            authored_program.actions = entities
                .iter()
                .map(|&tag| Action::Spawn {
                    asset: Asset {
                        graph: tag,
                        path: String::new(),
                        values: values
                            .iter()
                            .filter(|value| value.locator.graph_tag == Some(tag))
                            .cloned()
                            .collect(),
                    },
                    position: Position::Event,
                })
                .collect();
        }
        if form == 2 {
            authored_program =
                Program::from_native(&stock.action_payload, "Complete Scoped Program").unwrap();
            for asset in authored_program.assets_mut() {
                asset.values = values
                    .iter()
                    .filter(|value| value.locator.graph_tag == Some(asset.graph))
                    .cloned()
                    .collect();
            }
        }
        let mut tags = Vec::new();
        clone_private_sandbox_perk_runtime(
            &manager,
            &stock,
            custom_runtime::PrivateRuntimeEdits {
                program: program.then_some(&authored_program),
                values: if program { &[] } else { &values },
                ..Default::default()
            },
            allocator,
            &mut tags,
        )
        .unwrap();
        for entity in entities {
            assert_eq!(
                tags.iter()
                    .filter(|tag| tag.template_tag == TagHash(entity))
                    .count(),
                1
            );
        }
    }
    let mut invalid = values.clone();
    invalid[0].locator.graph_tag = Some(0x8000_0001);
    assert!(
        clone_private_sandbox_perk_runtime(
            &manager,
            &stock,
            custom_runtime::PrivateRuntimeEdits {
                values: &invalid,
                ..Default::default()
            },
            allocator,
            &mut Vec::new()
        )
        .is_err()
    );
    assert_eq!(stock, before);
    assert_stock_graphs_unchanged(&manager, &stock);
}

fn assert_stock_graphs_unchanged(
    manager: &PackageManager,
    stock: &sundial::package_authoring::sandbox_perk::SandboxPerkRuntimeAction,
) {
    for source in &stock.graphs {
        assert_eq!(manager.read_tag(source.tag).unwrap(), source.payload);
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn authored_catalog_nodes_keep_private_allocation_stock_bytes_and_residency() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = manager.read_tag(globals_tag).unwrap();
    let stock = load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
    let allocator = AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0);
    let mut tags = Vec::new();
    let mut timer = NativeNode::condition(1).unwrap();
    timer.bytes[8..12].copy_from_slice(&1.5_f32.to_le_bytes());
    let programs = [
        Program {
            trigger: Trigger::Native,
            native_trigger: Some(timer),
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        },
        Program {
            trigger: Trigger::WeaponKill,
            actions: vec![
                Action::property(0x5EE2_66FC),
                Action::ExtendTimers {
                    extend_ms: 1000,
                    cap_ms: 5000,
                },
            ],
            ..Program::default()
        },
        Program {
            trigger: Trigger::Native,
            native_trigger: NativeNode::condition(26),
            actions: vec![Action::native(13).unwrap()],
            ..Program::default()
        },
    ];
    let mut identities = BTreeSet::new();
    for program in &programs {
        let first = tags.len();
        let authored = clone_private_sandbox_perk_runtime(
            &manager,
            &stock,
            custom_runtime::PrivateRuntimeEdits {
                program: Some(program),
                ..Default::default()
            },
            allocator,
            &mut tags,
        )
        .unwrap();
        assert_eq!(authored.pkg_id(), PARHELION_ASSET_PACKAGE_ID);
        assert_ne!(authored, stock.action_tag);
        assert!(identities.insert(authored));
        assert_eq!(
            tags.len() - first,
            5,
            "an action and four residency records"
        );
        assert_eq!(tags[first].template_tag, stock.action_tag);
        let owner = allocator
            .assigned_tag(first + 3, "Test Residency Root", "test residency root")
            .unwrap();
        let companion = allocator
            .assigned_tag(
                first + 4,
                "Test Residency Companion",
                "test residency companion",
            )
            .unwrap();
        let companion_payload = &tags[first + 4].payload;
        assert_eq!(
            read_u64(companion_payload, 0).unwrap(),
            companion_payload.len() as u64
        );
        assert_eq!(
            validate_shared_tag_companion_payload(companion_payload, companion, owner).unwrap(),
            dependency_set([
                owner,
                companion,
                TagHash::new(0x0238, 0x0B90),
                TagHash::new(0x0238, 0x0EDC),
                TagHash::new(0x0238, 0x0EDD),
            ])
        );
        assert_compiled_catalog_node(&manager, program, &tags[first].payload);
        assert_eq!(
            manager.read_tag(stock.action_tag).unwrap(),
            stock.action_payload
        );
    }
}

fn assert_compiled_catalog_node(manager: &PackageManager, program: &Program, payload: &[u8]) {
    let decoded = action::decode(payload).unwrap();
    if program
        .native_trigger
        .as_ref()
        .is_some_and(|node| node.kind == 26)
    {
        assert_eq!(decoded.groups[0].activation[0].kind, 26);
        assert_eq!(decoded.effects().next().unwrap().kind, 13);
        let native = decoded.effects().next().unwrap();
        let graph = native.referenced_tag.expect("weighted spawn graph");
        assert!(manager.get_entry(TagHash(graph)).is_some());
    } else if program.trigger == Trigger::Native {
        assert_eq!(decoded.groups[0].activation[0].kind, 1);
        assert_eq!(decoded.timer_budget, 2);
    } else {
        assert_eq!(read_u64(payload, 0xA0).unwrap(), 1);
        assert_eq!(decoded.effects().next().unwrap().kind, 32);
    }
}
