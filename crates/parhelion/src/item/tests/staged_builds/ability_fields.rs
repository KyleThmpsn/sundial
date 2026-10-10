//! A native survey is the independent oracle. Discovery selects the editor control, while
//! readback follows the emitted graph and component references and checks raw package bytes.
use super::*;
use crate::subclass::{Place, SubclassAbilities};
use sundial::package_authoring::{ability_movement, ability_settings, runtime};

mod readback;

#[derive(Clone, serde::Deserialize)]
struct Case {
    id: String,
    label: String,
    subclass: String,
    entry: u8,
    root: u32,
    graph: Option<u32>,
    owner: u32,
    offset: usize,
    size: usize,
    stock: String,
    value: f32,
    expected: String,
    bank_row: Option<u16>,
    bank_key: Option<u32>,
    lane: u16,
    scale: f32,
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and writes independent staged native readback"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn verified_ability_fields_survive_saved_recipes_and_private_packages() {
    let packages = PathBuf::from(
        std::env::var_os("SUNDIAL_STOCK_PACKAGES").expect("configure SUNDIAL_STOCK_PACKAGES"),
    );
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache = temporary.path().join("catalog.json");
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let bases = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let manager = open_manager(view.path()).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("ability_fields/shadowkeep.json")).unwrap();
    let cases: Vec<Case> = serde_json::from_value(oracle["cases"].clone()).unwrap();
    assert!(
        !cases.is_empty(),
        "the configured native corpus is required"
    );
    let mut recipes = BTreeMap::new();
    let mut anchors = Vec::new();
    for case in &cases {
        let base = bases
            .iter()
            .find(|base| base.name == case.subclass)
            .unwrap();
        assert_eq!(
            base.entry_entities[&case.entry], case.root,
            "{} source route",
            case.id
        );
        let recipe = recipes.entry(base.hash).or_insert_with(|| {
            let mut recipe =
                crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
            recipe.set_donor(base.hash, base.name.clone());
            recipe
                .rename_authored_item(format!("Verified {}", base.name))
                .unwrap();
            recipe.overrides.subclass_abilities = Some(SubclassAbilities::default());
            recipe
        });
        let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
        let place = crate::subclass::entry_place(case.entry).unwrap();
        let mut edits = abilities.edits(base.hash, place);
        let original = manager.read_tag(tiger_pkg::TagHash(case.owner)).unwrap();
        assert_eq!(
            &original[case.offset..case.offset + case.size],
            bytes(&case.stock),
            "{} oracle input",
            case.id
        );
        if let Some(row) = case.bank_row {
            let lane =
                ability_movement::row_lane(&original, (case.bank_key.unwrap(), row, case.lane))
                    .unwrap();
            let bits = lane.unit.bits(case.value);
            edits.set_bank_value(lane.key, lane.row, lane.lane, Some(bits));
            anchors.push(None);
        } else {
            let graph_tag = case.graph.unwrap();
            let graph_payload = manager.read_tag(tiger_pkg::TagHash(graph_tag)).unwrap();
            let mut graph = runtime::load_weapon_runtime_graph_for_entity(
                &manager,
                0,
                0,
                graph_tag,
                &graph_payload,
            )
            .unwrap();
            graph.scope_fields();
            let settings = ability_settings::discover(&manager, &graph);
            if let Some(setting) = settings.iter().find(|setting| {
                setting.kind.label() == case.label
                    && setting.owner_tag == case.owner
                    && setting.field.owner_offset as usize + setting.lane.map_or(0, |lane| lane.at)
                        == case.offset
            }) {
                setting
                    .set(&mut edits.ability_values, case.value * case.scale)
                    .unwrap();
            } else {
                let movements = ability_movement::discover(&graph);
                let value = movements
                    .iter()
                    .find(|value| {
                        value.label == case.label
                            && value.field.owner_offset as usize <= case.offset
                            && case.offset + case.size
                                <= value.field.owner_offset as usize
                                    + value.field.locator.byte_size as usize
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "{} has no editable control at its verified storage",
                            case.id
                        )
                    });
                value.write(&mut edits.ability_values, value.unit.bits(case.value));
            }
            let resource = graph
                .resources
                .iter()
                .find(|resource| resource.owner_tag == case.owner)
                .unwrap();
            anchors.push(Some((
                readback::route(&manager, case.root, graph_tag),
                resource.binding_hash,
                resource.resource_index,
            )));
        }
        abilities.set_edits(base.hash, place, edits);
    }
    // Hidden, always-applied stat perks need the same private replacement route as a visible node.
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = manager.read_tag(globals_tag).unwrap();
    let mut passive_oracles = Vec::new();
    for (ordinal, source_perk) in [1_u16, 2, 3].into_iter().enumerate() {
        let base = bases.iter().find(|base| base.name == "Sentinel").unwrap();
        let recipe = recipes.get_mut(&base.hash).unwrap();
        assert!(base.entry_perks[&19].contains(&source_perk));
        let stock =
            load_sandbox_perk_runtime_action(&manager, &globals, usize::from(source_perk)).unwrap();
        let mut program = sundial::package_authoring::sandbox_perk::program::Program::from_native(
            &stock.action_payload,
            "Private Stat Input",
        )
        .unwrap();
        let mut changed = 0;
        for block in &mut program.native.as_mut().unwrap().graph.blocks {
            if block.class == 0x8080_3E44 {
                let original = block.bytes[0x50];
                assert!((3..=5).contains(&original));
                block.bytes[0x50] = if original == 5 { 3 } else { original + 1 };
                passive_oracles.push((
                    source_perk,
                    ordinal,
                    block.bytes[0x50],
                    block.bytes[0x51],
                    stock.action_tag,
                    stock.action_payload.clone(),
                ));
                changed += 1;
            }
        }
        assert_eq!(changed, 1, "each stat passive has one translated input");
        let mut effect = crate::perk::PerkRecipe::effect(source_perk);
        effect.program = Some(program);
        let mut perk = crate::perk::PerkRecipe::new();
        perk.name = format!("Private Stat Input {source_perk}");
        perk.effects.push(effect);
        let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
        let place = Place::Ability(19);
        let mut edits = abilities.edits(base.hash, place);
        edits.removed_perks.push(source_perk);
        edits.custom_perks.push(perk);
        abilities.set_edits(base.hash, place, edits);
    }
    let recipes = recipes
        .into_values()
        .map(|recipe| {
            let json = recipe.to_json_pretty().unwrap();
            let reloaded = crate::WeaponRecipe::from_json_str(&json).unwrap();
            assert_eq!(recipe, reloaded);
            crate::test_support::artifact(
                &format!("verified-{}.recipe.json", recipe.donor.item_hash),
                &reloaded,
            );
            reloaded
        })
        .collect();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: crate::test_support::artifacts("ability-fields-staged")
            .unwrap_or_else(|| temporary.path().join("staging")),
        ignore_installed_authored_overlays: false,
        recipes,
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap();
    crate::test_support::artifact("verified-ability-fields-build.json", &manifest);
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let authored = catalog.subclasses(|_| true);
    let globals = staged.read_tag(globals_tag).unwrap();
    let sentinel = authored
        .iter()
        .find(|base| base.name == "Verified Sentinel")
        .unwrap();
    let stock_passives = &bases
        .iter()
        .find(|base| base.name == "Sentinel")
        .unwrap()
        .entry_perks[&19];
    let private_passives = sentinel.entry_perks[&19]
        .iter()
        .filter(|perk| !stock_passives.contains(perk))
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(private_passives.len(), 3);
    let mut passive_receipt = Vec::new();
    for (source, ordinal, selector, normalization, stock_tag, stock_payload) in passive_oracles {
        let perks = &sentinel.entry_perks[&19];
        assert!(
            !perks.contains(&source),
            "the private copy replaces the granted stock passive"
        );
        let private = load_sandbox_perk_runtime_action(
            &staged,
            &globals,
            usize::from(private_passives[ordinal]),
        )
        .unwrap();
        assert_ne!(private.action_tag, stock_tag);
        // Follow the actual 40B5 group and 40AC effect pointer row, independently of Program.
        let (_, _, effects, _) = array_at(&private.action_payload, 0x38).unwrap();
        let node = relative_target(&private.action_payload, effects).unwrap();
        assert_eq!(private.action_payload[node], 2);
        assert_eq!(private.action_payload[node + 0x50], selector);
        assert_eq!(private.action_payload[node + 0x51], normalization);
        assert_eq!(staged.read_tag(stock_tag).unwrap(), stock_payload);
        passive_receipt.push(serde_json::json!({"source_perk":source,"private_perk":private_passives[ordinal],"selector":selector,"normalization":normalization}));
    }
    let mut receipt = Vec::new();
    let mut failures = Vec::new();
    for (case, anchor) in cases.iter().zip(anchors) {
        let subclass = authored
            .iter()
            .find(|base| base.name == format!("Verified {}", case.subclass))
            .unwrap();
        let private_root = subclass.entry_entities[&case.entry];
        assert_ne!(private_root, case.root);
        let private_owner = if let Some((route, binding, resource_index)) = anchor {
            readback::copied_owner(
                &staged,
                private_root,
                &route,
                binding,
                usize::from(resource_index),
            )
        } else {
            super::stock::copied_bank(&staged, case.root, private_root)
        };
        assert_ne!(
            private_owner, case.owner,
            "{} owns its edited payload",
            case.id
        );
        let emitted = staged.read_tag(tiger_pkg::TagHash(private_owner)).unwrap();
        let stock = staged.read_tag(tiger_pkg::TagHash(case.owner)).unwrap();
        let unchanged = manager.read_tag(tiger_pkg::TagHash(case.owner)).unwrap();
        if stock != unchanged {
            failures.push(format!("{} changed its stock donor", case.id));
        }
        let written = &emitted[case.offset..case.offset + case.size];
        if written != bytes(&case.expected) {
            failures.push(format!(
                "{} emitted {} instead of {}",
                case.id,
                hex::encode(written),
                case.expected
            ));
        }
        // Configuration neighbors are compared unless another case deliberately edits them.
        for offset in case.offset.saturating_sub(4)..(case.offset + case.size + 4).min(stock.len())
        {
            let edited = cases.iter().any(|other| {
                other.owner == case.owner
                    && other.entry == case.entry
                    && other.subclass == case.subclass
                    && (other.offset..other.offset + other.size).contains(&offset)
            });
            if !edited {
                if let Some(identity) = readback::owner_identity(&stock, offset, case.owner) {
                    if read_u32(&emitted, identity).unwrap() != private_owner {
                        failures.push(format!(
                            "{} did not relocate the adjacent paired-object owner",
                            case.id
                        ));
                    }
                } else if emitted[offset] != stock[offset] {
                    failures.push(format!("{} changed neighbor {offset:#X}", case.id));
                }
            }
        }
        receipt.push(serde_json::json!({"id":case.id,"private_root":private_root,"private_owner":private_owner,
            "offset":case.offset,"stock":case.stock,"emitted":hex::encode(written),
            "expected":case.expected,"stock_unchanged":stock == unchanged}));
    }
    crate::test_support::artifact(
        "verified-ability-fields.json",
        &serde_json::json!({
            "build":oracle["build"],"inputs":oracle,"readback":receipt,"stat_passives":passive_receipt,
            "failures":failures,
            "limit":"Package behavior only. Native gameplay activation and cleanup remain unobserved."
        }),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
