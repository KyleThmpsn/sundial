//! Persisted candidate acceptance: preparation must reach the equipped private controller,
//! each path's own exclusive upgrade, slot binding, completion and the source presentation.
//! Upgrade grants belong to each path's selected ability records, separate from its Radiance
//! Super. Sandbox perks alone do not enter the selected ability-property buckets. The controller's
//! activation entity and Song of Flame's two resource rows must name private buff graphs that
//! read back against their source graphs, with the resource rows on their native recipient
//! fields. Both equipped Super entries must
//! reach the retained controller. Other nodes stay stock. A donor graph, donor presentation, stock
//! metadata mutation, unscheduled completion, another path's grant or an allocation missing from
//! the replication loading companion is a failure. Retained components keep their native
//! allocations, while authored controller, bank and sequence storage needs private allocations.
//! Every controller sequence input must reach the provider occurrence the source connects, and
//! every sequence scalar input must read a bank declaration of its own name. Each particle event names a private system whose stored footer comes from a source wrapper.
//! A missing query geometry rejects embedded construction. Property refresh requires the exact
//! query damage action, its initialized recipient and mutable tracking storage. This package E2E
//! cannot establish rendering, sound, animation, buffs in play or completed resurrection.
use super::*;
use crate::subclass::{AttunementPath, import, layout};
use std::collections::{BTreeMap, BTreeSet};
mod animation;
mod camera;
mod effect;
mod feedback;
mod gate;
mod glide;
mod grid;
mod input;
mod melee;
mod query;

/// Paths from top to bottom, as the source grid orders its upgrade rows.
const PATHS: [AttunementPath; 3] = [
    AttunementPath::Top,
    AttunementPath::Middle,
    AttunementPath::Bottom,
];

/// A sequence's control and event counts, which distinguish the source occurrences.
fn signature(controls: usize, events: usize) -> (usize, usize) {
    (controls, events)
}

#[test]
#[ignore = "requires D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
fn prepared_radiance_reloads_and_equips_private_paths_and_source_presentation() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("radiance");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("fresh artifact directory");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let source_packages =
        PathBuf::from(std::env::var_os("D1_SOURCE_PACKAGES").expect("source packages"));
    let source = Source::open(&source_packages, |_, _| {}).unwrap();
    let choices = source.revival_choices().unwrap();
    let choices = choices
        .iter()
        .filter(|choice| choice.name.trim() == "Fireborn")
        .collect::<Vec<_>>();
    assert_eq!(
        choices.len(),
        1,
        "required source choice is missing or ambiguous"
    );
    let source_catalog = source.subclass_choices().unwrap();
    let source_subclass = source_catalog
        .iter()
        .find(|subclass| subclass.item == choices[0].subclass && subclass.grid == choices[0].grid)
        .unwrap();
    let source_controller = source.read(source_subclass.controller).unwrap();
    let source_definition = source_controller.pointer(24).unwrap();
    let selection = source.radiance_selection(choices[0]).unwrap();
    assert_eq!(selection.upgrades.len(), PATHS.len());

    // Independent source presentation: sequence occurrences by signature, their particle
    // events and wrapper footers, and the controller's nine connected providers.
    let source_graph = source.read(source_subclass.graph).unwrap();
    let source_components = source_graph
        .array(0x20, 12, Some(0x80800715))
        .unwrap()
        .into_iter()
        .map(|at| source_graph.u32(at).unwrap())
        .collect::<Vec<_>>();
    let mut source_signatures = BTreeMap::new();
    let mut source_particles = 0;
    let mut source_sounds = 0;
    let mut source_wrappers = BTreeSet::new();
    let mut source_footers = BTreeSet::new();
    let mut source_feedback = Vec::new();
    let mut source_animation = Vec::new();
    let source_signature = |tag: u32| {
        let owner = source.read(tag).unwrap();
        let d = owner.pointer(24).unwrap();
        let count = |field| owner.array(d + field, 8, Some(0x80800629)).unwrap().len();
        signature(count(0x160), count(0x170) + count(0x180))
    };
    for &tag in &source_components {
        let owner = source.read(tag).unwrap();
        if owner.u32(owner.pointer(16).unwrap() - 4).unwrap() != 0x8080079A {
            continue;
        }
        let d = owner.pointer(24).unwrap();
        source_feedback.extend(feedback::source(&source, &owner));
        source_animation.extend(animation::source_events(&owner));
        for row in owner.array(d + 0x180, 8, Some(0x80800629)).unwrap() {
            let event = owner.pointer(row).unwrap();
            source_sounds += usize::from(owner.u32(event - 4).unwrap() == 0x80801F00);
            if owner.u32(event - 4).unwrap() != 0x8080037A {
                continue;
            }
            source_particles += 1;
            let wrapper = owner.u32(event + 0x20).unwrap();
            source_wrappers.insert(wrapper);
            source_footers.insert(source.read(wrapper).unwrap().bytes::<12>(0x18).unwrap());
        }
        *source_signatures
            .entry(source_signature(tag))
            .or_insert(0usize) += 1;
    }
    let mut source_inputs = BTreeMap::new();
    for row in source_graph.array(0x30, 88, Some(0x8080054E)).unwrap() {
        if source_graph.u32(row + 8).unwrap() != source_subclass.controller {
            continue;
        }
        let offset = usize::try_from(source_graph.u64(row + 16).unwrap()).unwrap();
        let provider = source_graph.u32(row + 48).unwrap();
        for (source_base, native_base) in [(0x1A8, 0x208), (0x608, 0x6C0), (0x560, 0x630)] {
            for selector in 0..3 {
                if offset == source_definition + source_base + selector * 0x38 {
                    source_inputs.insert(native_base + selector * 0x30, provider);
                }
            }
        }
    }
    assert_eq!(source_inputs.len(), 9);
    let source_scalars = input::source(&source, &source_graph);
    // Body events name source human actions, whose source clips the staged actions must carry.
    let body_actions = source_animation
        .iter()
        .filter(|(_, flags)| flags[0] == 0)
        .map(|(action, _)| *action)
        .collect::<BTreeSet<_>>();
    assert!(
        !body_actions.is_empty(),
        "required animation corpus is empty"
    );
    let source_actions = animation::source_actions(&source, &body_actions);
    let mut expected_animation = source_animation
        .iter()
        .map(|&(action, flags)| animation::expected(action, flags, &source_actions))
        .collect::<Vec<_>>();
    expected_animation.sort_by_key(serde_json::Value::to_string);
    // No stock package file may be written, only the build's own packages.
    let stock_files = animation::stock_files(&packages);
    let (view, stock_path) = stock_view(&packages, ".parhelion-radiance-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    let stock = open_manager(&stock_path).unwrap();
    let catalog = crate::test_support::catalog(view.path()).unwrap();
    let donor = catalog
        .subclasses(crate::package_profile::is_stock_item_definition)
        .into_iter()
        .find(|item| item.name == "Dawnblade")
        .unwrap();
    let original_root = donor.entry_entities[&20];
    let original_graph = Payload(stock.read_tag(TagHash(original_root)).unwrap());
    let mut originals = BTreeMap::from([(original_root, original_graph.0.clone())]);
    for at in original_graph.array(16, 12, Some(0x80809C04)).unwrap() {
        let tag = original_graph.u32(at).unwrap();
        originals.insert(tag, stock.read_tag(TagHash(tag)).unwrap());
    }
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.radiance-acceptance",
        donor.hash,
        &donor.name,
    )
    .unwrap();
    recipe.kind = crate::ItemKind::Subclass;
    recipe.name = "Radiance Acceptance".into();
    let baseline = recipe.clone();
    let prepared = import::prepare_grid(&import::Request {
        source_packages: &source_packages,
        native_packages: &stock_path,
        output: &output.join("prepared"),
        recipe: &recipe,
        choice: choices[0],
        entry: 23,
    })
    .unwrap();
    assert_eq!(
        recipe, baseline,
        "preparation must not mutate the open draft"
    );
    let mut edited = recipe.clone();
    edited.name.push_str(" Changed");
    assert!(
        prepared.apply(&mut edited).is_err(),
        "preparation must preserve concurrent edits"
    );
    prepared.apply(&mut recipe).unwrap();
    let recipe_path = output.join("prepared.parhelion.json");
    recipe.save_json(&recipe_path).unwrap();
    let recipe = crate::WeaponRecipe::load_json(&recipe_path).unwrap();
    let mut missing_property = recipe.clone();
    let abilities = missing_property
        .overrides
        .subclass_abilities
        .as_mut()
        .unwrap();
    let (mutated, mut node) = PATHS
        .into_iter()
        .map(|path| (path, abilities.node(donor.hash, path, 3)))
        .find(|(_, node)| !node.edits.modifiers.is_empty())
        .expect("an available upgrade path");
    node.edits.modifiers[0].effect = crate::subclass::ModifierEffect::Key { key: 0xDEADBEEF };
    abilities.set_path_node(mutated, donor.hash, node);
    let rejected = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: stock_path.clone(),
        staging_root: output.join("missing-property"),
        ignore_installed_authored_overlays: false,
        recipes: vec![missing_property],
        maps: Vec::new(),
    })
    .unwrap();
    let rejection = crate::build_and_stage_snapshot_with_progress(&rejected, |_| {})
        .map(|_| ())
        .expect_err("A selected key absent from the private bank must reject the build")
        .to_string();
    assert!(
        rejection.contains("lacks a selected property"),
        "{rejection}"
    );
    fs::write(output.join("missing-property-rejection.txt"), &rejection).unwrap();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: stock_path,
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe.clone()],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    assert_eq!(
        animation::stock_files(&packages),
        stock_files,
        "a stock package file changed"
    );
    let (readback, staged_path) =
        stock_view(&packages, ".parhelion-radiance-readback-", Oodle::Required);
    fs::write(readback.path().join("destiny2.exe"), []).unwrap();
    for artifact in &build.artifacts {
        let target = staged_path.join(&artifact.file_name);
        if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        fs::copy(build.run_directory.join(&artifact.file_name), target).unwrap();
    }
    let staged = open_manager(&staged_path).unwrap();
    let catalog = crate::test_support::catalog(readback.path()).unwrap();
    let subclass = catalog
        .subclasses(|_| true)
        .into_iter()
        .find(|item| item.name == recipe.name)
        .unwrap();
    // Every source step is accounted for, and placed steps carry their source presentation.
    // Entries whose source choice did not translate stay stock.
    let native_entries = grid::readback(&source, source_subclass, &prepared.provenance, &recipe);
    // The source grenades' tiles draw the source HUD's own glyph art.
    let hud = grid::hud_readback(
        &source,
        source_subclass,
        &prepared.provenance,
        &recipe,
        (&stock, &staged),
        (&donor.entry_entities, &subclass.entry_entities),
    );
    fs::write(
        output.join("hud-glyphs.json"),
        serde_json::to_vec_pretty(&hud).unwrap(),
    )
    .unwrap();
    // Rows gated on another key sit only beside a melee that applies it.
    let gated = gate::readback(&source, &prepared.provenance, &staged, &subclass, &recipe);
    fs::write(
        output.join("gated-rows.json"),
        serde_json::to_vec_pretty(&gated).unwrap(),
    )
    .unwrap();
    // Source glide choices keep their own groups in the private glide bank.
    let glides = glide::readback(
        &source,
        source_subclass,
        &prepared.provenance,
        (&stock, &staged),
        &subclass,
    );
    fs::write(
        output.join("glide-groups.json"),
        serde_json::to_vec_pretty(&glides).unwrap(),
    )
    .unwrap();
    // Scorch and each placed melee upgrade keep their source rows in the private melee bank.
    let melee_rows = melee::readback(&source, &prepared.provenance, (&stock, &staged));
    fs::write(
        output.join("melee-rows.json"),
        serde_json::to_vec_pretty(&melee_rows).unwrap(),
    )
    .unwrap();
    for entry in native_entries {
        assert_eq!(
            subclass.entry_rows.get(&entry),
            donor.entry_rows.get(&entry)
        );
        assert_eq!(
            subclass.entry_entities.get(&entry),
            donor.entry_entities.get(&entry)
        );
        assert_eq!(
            subclass.entry_perks.get(&entry),
            donor.entry_perks.get(&entry)
        );
        let original = donor
            .entry_modifiers
            .get(&entry)
            .cloned()
            .unwrap_or_default();
        let actual = subclass
            .entry_modifiers
            .get(&entry)
            .cloned()
            .unwrap_or_default();
        assert!(original.iter().all(|grant| actual.contains(grant)));
        // An authored ability, a copied Super or a source grenade, has a row of its own and
        // inherits the node's existing keys for its donor row. Original grants remain intact,
        // and added destinations may only carry those keys on such a copy's row.
        for grant in actual.iter().filter(|grant| !original.contains(grant)) {
            assert!(
                donor.entry_rows.iter().any(|(own, &row)| {
                    subclass
                        .entry_rows
                        .get(own)
                        .is_some_and(|&copy| copy != row && copy == grant.1)
                        && original.contains(&(grant.0, row))
                }),
                "unrelated node {entry} gained another grant: {grant:?}"
            );
        }
    }
    let authored = recipe.overrides.subclass_abilities.as_ref().unwrap();
    // Each tree presents its own source upgrade, never another tree's. Trees take the names of
    // the first attribute column's choices in row order. The middle tree leads with its own
    // Super, which carries that tree's upgrade.
    let mut attribute_columns = BTreeMap::<u32, Vec<(i16, i16, String)>>::new();
    for node in &source_subclass.nodes {
        for step in &node.steps {
            let variant = &step.source["variants"][0];
            if variant["ability_grants"]
                .as_array()
                .is_some_and(Vec::is_empty)
                && variant["perks"].as_array().is_some_and(Vec::is_empty)
                && variant["stat_adjustments"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
            {
                attribute_columns.entry(node.group_key).or_default().push((
                    node.column,
                    node.row,
                    step.name.trim().to_owned(),
                ));
            }
        }
    }
    let mut tree_names = attribute_columns
        .into_values()
        .filter(|steps| steps.len() == PATHS.len())
        .min_by_key(|steps| steps.iter().map(|step| step.0).min())
        .expect("the source grid has an attribute column");
    tree_names.sort_by_key(|step| step.1);
    for (path, upgrade) in PATHS.into_iter().zip(&selection.upgrades) {
        let index = PATHS.iter().position(|&own| own == path).unwrap();
        assert_eq!(
            authored
                .attunement(path)
                .and_then(|attunement| attunement.name.as_deref()),
            Some(tree_names[index].2.as_str())
        );
        let position = if path == AttunementPath::Middle { 0 } else { 3 };
        let node = authored.node(donor.hash, path, position);
        let keys = node
            .edits
            .modifiers
            .iter()
            .filter_map(|modifier| match modifier.effect {
                crate::subclass::ModifierEffect::Key { key } => Some(key),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(keys, upgrade.keys.iter().copied().collect());
        if position == 3 {
            assert!(
                !subclass.entry_rows.contains_key(&path.entries()[3]),
                "An upgrade cannot equip a second Super"
            );
            assert_eq!(node.edits.name.as_deref(), Some(upgrade.choice.name.trim()));
            assert!(node.edits.imported_ability.is_none());
            assert!(node.edits.custom_perks.is_empty());
        }
    }
    assert!(
        selection
            .upgrades
            .iter()
            .any(|upgrade| upgrade.choice == *choices[0])
    );
    assert_ne!(
        subclass.entry_rows[&layout::SUPER],
        subclass.entry_rows[&20]
    );
    for super_entry in [layout::SUPER, 20] {
        let edits = if super_entry == layout::SUPER {
            authored.ability(donor.hash, super_entry).edits
        } else {
            authored.node(donor.hash, AttunementPath::Middle, 0).edits
        };
        assert_eq!(edits.name.as_deref(), Some("Radiance"));
        // Only the Radiance step's own translated grants and actions ride on the Super.
        assert!(edits.custom_perks.len() <= 1);
        let row = subclass.entry_rows[&super_entry];
        let root = subclass.entry_entities[&super_entry];
        assert!(stock.get_entry(TagHash(root)).is_none());
        let pairs = ability_definition::definitions(
            &staged.read_tag(TagHash(0x81613D26)).unwrap(),
            &staged.read_tag(TagHash(0x81319130)).unwrap(),
        )
        .unwrap();
        let old_pairs = ability_definition::definitions(
            &stock.read_tag(TagHash(0x81613D26)).unwrap(),
            &stock.read_tag(TagHash(0x81319130)).unwrap(),
        )
        .unwrap();
        assert_eq!(&pairs[..old_pairs.len()], old_pairs.as_slice());
        assert!(usize::from(row) >= old_pairs.len());
        let graph = Payload(staged.read_tag(TagHash(root)).unwrap());
        let mut owners = BTreeMap::new();
        for at in graph.array(16, 12, Some(0x80809C04)).unwrap() {
            let tag = graph.u32(at).unwrap();
            assert!(stock.get_entry(TagHash(tag)).is_none());
            owners.insert(tag, Payload(staged.read_tag(TagHash(tag)).unwrap()));
        }
        let by_class = |class| {
            owners
                .iter()
                .filter(|(_, owner)| owner.u32(owner.pointer(16).unwrap() - 4).unwrap() == class)
                .collect::<Vec<_>>()
        };
        let controllers = by_class(0x80804176);
        let banks = by_class(0x8080417A);
        assert_eq!(controllers.len(), 1);
        assert_eq!(banks.len(), 1);
        let (&controller_tag, controller) = controllers[0];
        let (&bank_tag, bank) = banks[0];
        let (live, definition) = pair(controller, controller_tag, (0x80804176, 0x80804177));
        // Source and native consumers independently identify charge count, activation cost
        // and active drain. Native flags drop the source-only low bit.
        for (from, to) in [(0xE8, 0xC8), (0xEC, 0xCC), (0xF4, 0xD0)] {
            assert_eq!(
                source_controller.u32(source_definition + from).unwrap(),
                controller.u32(definition + to).unwrap()
            );
        }
        assert_eq!(controller.u16(definition + 0x4C).unwrap(), 0x89);
        // The Super's HUD tile names the source glyph, from definition +170 in the source, at
        // +1C8 of every native view. The native table already holds it, row and art unchanged.
        let source_glyph = source_controller.u32(source_definition + 0x170).unwrap();
        assert_eq!(source_controller.u8(source_definition + 0x174).unwrap(), 0);
        for at in [live, definition, controller.pointer(24).unwrap()] {
            assert_eq!(controller.u32(at + 0x1C8).unwrap(), source_glyph);
        }
        let glyph_row = |manager: &PackageManager| {
            let table = Payload(manager.read_tag(TagHash(0x80BC6A79)).unwrap());
            (0..usize::try_from(table.u64(8).unwrap()).unwrap())
                .map(|row| 48 + row * 112)
                .find(|&at| table.u32(at).unwrap() == source_glyph)
                .map(|at| table.bytes::<112>(at).unwrap())
        };
        assert!(
            glyph_row(&stock).is_some(),
            "the source glyph is a native row"
        );
        assert_eq!(glyph_row(&staged), glyph_row(&stock));
        let (_, bank_definition) = pair(bank, bank_tag, (0x8080417A, 0x8080417B));
        let catalog_row = catalog.ability_row(row).unwrap();
        assert_eq!(catalog_row.bank, Some(bank_tag));
        assert_eq!(catalog_row.slot, Some(program::AbilityTarget::Super));

        let query_selection =
            query::selection(controller, controller_tag, &source_controller, &stock);
        let query = controller.pointer(definition + 0x810).unwrap();
        let action_witness = Payload(stock.read_tag(TagHash(0x80BC2BF4)).unwrap());
        let (recipient, query_action) =
            query::readback(controller, controller_tag, &action_witness);

        let metadata_tag = controller.u32(definition + 0x438).unwrap();
        assert!(
            stock.get_entry(TagHash(metadata_tag)).is_none(),
            "slot binding must be private"
        );
        let metadata = Payload(staged.read_tag(TagHash(metadata_tag)).unwrap());
        assert_eq!(metadata.u32(8).unwrap(), 0x80803BEA);
        assert_eq!(metadata.u32(12).unwrap(), 0x80803C0D);
        let methods = metadata.array(16, 24, Some(0x80809C56)).unwrap();
        assert_eq!(methods.len(), 71);
        // Native registered slot-assignment dispatch, independently distinct from property callback.
        assert_eq!(metadata.u32(methods[14]).unwrap(), 0x80804176);
        assert_eq!(metadata.u32(methods[14] + 4).unwrap(), 2);
        assert_eq!(metadata.bytes::<16>(methods[14] + 8).unwrap(), [0; 16]);
        assert_eq!(metadata.u32(methods[5]).unwrap(), 0x80804176);
        assert_eq!(metadata.u32(methods[5] + 4).unwrap(), 9);
        assert_eq!(metadata.bytes::<16>(methods[5] + 8).unwrap(), [0; 16]);
        let registered = graph
            .array(88, 40, Some(0x80809C22))
            .unwrap()
            .into_iter()
            .filter(|&at| {
                graph.u32(at + 16).unwrap() == controller_tag
                    && graph.u64(at + 24).unwrap() == (definition + 0x430) as u64
            })
            .collect::<Vec<_>>();
        assert_eq!(registered.len(), 1);
        assert_eq!(graph.u32(registered[0] + 32).unwrap(), metadata_tag);
        let callbacks = callback::readback(controller, controller_tag, false);
        callback::readback(bank, bank_tag, true);
        for (offset, method) in [
            (0x698, 7),
            (0x6B0, 8),
            (0x758, 10),
            (0x788, 4),
            (0x7A0, 5),
            (0x890, 3),
        ] {
            assert_eq!(controller.u32(live + offset).unwrap(), 0x80804176);
            assert_eq!(controller.u32(live + offset + 4).unwrap(), method);
            assert!(
                controller
                    .array(32, 8, Some(0x8080919F))
                    .unwrap()
                    .iter()
                    .any(|&at| controller.pointer(at).unwrap() == live + offset + 8)
            );
        }
        let initializers = controller.array(48, 16, Some(0x808091A4)).unwrap();
        let init = initializers
            .iter()
            .map(|&at| {
                (
                    controller.pointer(at).unwrap(),
                    controller.u32(at + 8).unwrap(),
                    controller.u16(at + 12).unwrap(),
                    controller.u16(at + 14).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let position = |row| init.iter().position(|candidate| *candidate == row).unwrap();
        assert!(
            position((recipient, 0x80803B36, 0, 1))
                < position((live + 0xCF0, 0x8080422D, 0, u16::MAX))
        );
        assert!(
            position((live + 0xCF0, 0x8080422D, 0, u16::MAX)) < position((live, 0x80804176, 0, 1))
        );
        assert!(position((live, 0x80803BB7, 13, 15)) < position((live, 0x80804176, 0, 1)));
        assert!(
            position((live + 0xD30, 0x808092A4, u16::MAX, 0))
                < position((live + 0xCF0, 0x8080422D, 0, u16::MAX))
        );

        let source_bank = source.read(source_subclass.bank).unwrap();
        let mut selected = selection
            .upgrades
            .iter()
            .flat_map(|upgrade| upgrade.keys.iter().copied())
            .collect::<BTreeSet<_>>();
        // Grid choices that only grant Super-row properties join the bank beside the upgrades.
        for step in source_subclass.nodes.iter().flat_map(|node| &node.steps) {
            let variant = &step.source["variants"][0];
            if !variant["perks"].as_array().is_some_and(Vec::is_empty)
                || !variant["stat_adjustments"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                continue;
            }
            let grants = variant["ability_grants"]
                .as_array()
                .unwrap()
                .iter()
                .map(|grant| hex::decode(grant.as_str().unwrap()).unwrap())
                .collect::<Vec<_>>();
            if !grants.is_empty()
                && grants
                    .iter()
                    .all(|bytes| bytes[4] == source_subclass.ability_row && bytes[11] == u8::MAX)
            {
                selected.extend(
                    grants
                        .iter()
                        .map(|bytes| u32::from_le_bytes(bytes[..4].try_into().unwrap())),
                );
            }
        }
        // Source graph to private graph, from the activation entity and the resource rows.
        let mut effect_graphs = BTreeMap::new();
        let activation = controller.u32(definition + 0x7C0).unwrap();
        assert!(
            ![0, u32::MAX].contains(&activation),
            "every path needs the Radiance activation entity"
        );
        effect_graphs.insert(
            source_controller.u32(source_definition + 0x708).unwrap(),
            activation,
        );
        let original_rows = source_bank
            .array(
                source_bank.pointer(24).unwrap() + 0xA0,
                40,
                Some(0x80802A84),
            )
            .unwrap()
            .into_iter()
            .filter(|&at| selected.contains(&source_bank.u32(at).unwrap()))
            .collect::<Vec<_>>();
        let imported_rows = bank
            .array(bank_definition + 0x88, 56, Some(0x8080453B))
            .unwrap();
        assert!(!original_rows.is_empty());
        assert_eq!(original_rows.len(), imported_rows.len());
        for (&from, &to) in original_rows.iter().zip(&imported_rows) {
            assert_eq!(
                source_bank.bytes::<8>(from).unwrap(),
                bank.bytes::<8>(to + 16).unwrap()
            );
            let a = source_bank.pointer(from + 32).unwrap();
            let b = bank.pointer(to + 48).unwrap();
            match source_bank.u32(a - 4).unwrap() {
                0x80802988 => {
                    assert_eq!(bank.u32(b + 4).unwrap(), 0x80804189);
                    // The only selected flag-set row is Fireborn's revival flag.
                    assert_eq!(bank.u8(b + 16).unwrap(), 1);
                }
                0x8080243C => {
                    assert_eq!(bank.u32(b + 4).unwrap(), 0x8080452E);
                    assert_eq!(source_bank.u32(a).unwrap(), bank.u32(b + 16).unwrap());
                }
                0x808026D1 => assert_eq!(bank.u32(b + 4).unwrap(), 0x80804520),
                // A script parameter row, such as a duration passive's named value.
                0x80802974 => assert_eq!(bank.u32(b + 4).unwrap(), 0x80804516),
                0x808029F6 => {
                    assert_eq!(bank.u32(b + 4).unwrap(), 0x80804187);
                    assert_eq!(bank.u8(b + 16).unwrap(), source_bank.u8(a).unwrap());
                }
                // Both refresh routines send recipient +8 to the query and +C to the
                // activation entity. Native 4183 and 4181 write those fields.
                class @ (0x8080216B | 0x8080236B) => {
                    let expected = if class == 0x8080216B {
                        0x80804183
                    } else {
                        0x80804181
                    };
                    assert_eq!(bank.u32(b + 4).unwrap(), expected);
                    let target = bank.u32(b + 24).unwrap();
                    let source_target = source_bank.u32(a + 8).unwrap();
                    assert!(
                        effect_graphs
                            .insert(source_target, target)
                            .is_none_or(|previous| previous == target),
                        "one source graph became two private graphs"
                    );
                }
                other => panic!("Unexpected selected companion {other:08X}"),
            }
            // Each row keeps its source priority, wildcard 255 or a numbered priority group.
            assert_eq!(
                bank.u32(to + 24).unwrap(),
                source_bank.u32(from + 8).unwrap()
            );
        }
        // The base graph and Song of Flame's caster graph. Song's query graph is the base graph.
        assert_eq!(effect_graphs.len(), 2, "source buff graphs differ");
        let buff_graphs = effect_graphs
            .iter()
            .map(|(&from, &to)| effect::readback(&source, &staged, &stock, from, to))
            .collect::<Vec<_>>();
        let mut grants = BTreeMap::new();
        let mut upgrade_entries = Vec::new();
        for (path, upgrade) in PATHS.into_iter().zip(&selection.upgrades) {
            // The middle tree's upgrade rides on its own Super node.
            let (entry, target) = if path == AttunementPath::Middle {
                (20, 20)
            } else {
                (path.entries()[3], layout::SUPER)
            };
            let granted = subclass
                .entry_modifiers
                .get(&entry)
                .cloned()
                .unwrap_or_default();
            let mut expected = upgrade
                .keys
                .iter()
                .map(|&key| (key, subclass.entry_rows[&target]))
                .collect::<Vec<_>>();
            expected.sort_unstable();
            let mut actual = granted.clone();
            actual.sort_unstable();
            assert_eq!(actual, expected, "{} path grants", path.key());
            upgrade_entries.push(entry);
            grants.insert(entry, granted);
        }
        let upgrade_keys = selection
            .upgrades
            .iter()
            .flat_map(|upgrade| upgrade.keys.iter().copied())
            .collect::<BTreeSet<_>>();
        for (&entry, modifiers) in &subclass.entry_modifiers {
            if !upgrade_entries.contains(&entry) {
                assert!(modifiers.iter().all(|(key, _)| !upgrade_keys.contains(key)));
            }
        }

        let links = graph.array(0x20, 72, Some(0x80809BC9)).unwrap();
        let mut presentation = Vec::new();
        let mut completion = 0;
        let mut particles = 0;
        let mut sounds = 0;
        let mut cues = BTreeSet::new();
        let mut systems = BTreeSet::new();
        let mut signatures = BTreeMap::new();
        let mut sequence_owners = BTreeMap::new();
        let mut feedback_events = Vec::new();
        let mut animation_events = Vec::new();
        for (&tag, owner) in &owners {
            if owner.u32(owner.pointer(16).unwrap() - 4).unwrap() != 0x808084D7 {
                continue;
            }
            // The occurrence multiset below rejects a remaining donor sequence.
            let (_, definition) = pair(owner, tag, (0x808084D7, 0x808084E9));
            let controls = owner
                .array(definition + 0x158, 24, Some(0x808093E6))
                .unwrap();
            let events = owner
                .array(definition + 0x168, 24, Some(0x808093E6))
                .unwrap();
            let shape = signature(controls.len(), events.len());
            *signatures.entry(shape).or_insert(0usize) += 1;
            sequence_owners.insert(tag, shape);
            let mut scheduled = Vec::new();
            for row in &events {
                let event = owner.pointer(row + 16).unwrap();
                let class = owner.u32(event - 4).unwrap();
                let kind = owner.u16(event + 0x30).unwrap();
                assert!(
                    matches!(
                        (class, kind),
                        (0x808093CB, 0)
                            | (0x80806CC6, 4)
                            | (0x80806B38, 6)
                            | (0x80803D8D, 12)
                            | (0x80803F24, 13)
                            | (0x80803FE9, 18)
                            | (0x80804595, 19)
                            | (0x808066A3, 21)
                    ),
                    "an unqualified event body is scheduled"
                );
                completion += usize::from(kind == 19);
                if class == 0x80803D8D {
                    feedback_events.push(feedback::native(owner, event, &staged, &stock));
                }
                if class == 0x80803FE9 {
                    animation_events.push(animation::native(owner, event, &staged, &stock));
                }
                if class == 0x80806B38 {
                    sounds += 1;
                    let cue = owner.u32(event + 0x40).unwrap();
                    assert!(
                        stock.get_entry(TagHash(cue)).is_none(),
                        "a sound names a stock cue"
                    );
                    assert_eq!(
                        staged.get_entry(TagHash(cue)).unwrap().reference,
                        0x80809802
                    );
                    cues.insert(cue);
                }
                if class == 0x80806CC6 {
                    particles += 1;
                    for system_row in owner.array(event + 0x40, 24, Some(0x80806CC8)).unwrap() {
                        let system = owner.u32(system_row + 16).unwrap();
                        assert!(stock.get_entry(TagHash(system)).is_none());
                        assert_eq!(
                            staged.get_entry(TagHash(system)).unwrap().reference,
                            0x80806E28
                        );
                        systems.insert(system);
                    }
                }
                scheduled.push(json!({"class":class,"kind":kind}));
            }
            // Native resolvers test bit 3 or bit 4 by caller. Every source declaration has one
            // cached-point bit for all callers, so both native bits must agree.
            for row in owner
                .array(definition + 0x1C0, 16, Some(0x80808615))
                .unwrap()
            {
                let flags = owner.u8(row + 4).unwrap();
                assert_eq!((flags >> 3) & 1, (flags >> 4) & 1);
            }
            presentation.push(json!({"owner":tag,"signature":shape,"scheduled":scheduled}));
        }
        assert_eq!(
            signatures, source_signatures,
            "source sequence occurrences differ"
        );
        assert_eq!(particles, source_particles);
        // A refused cue closure keeps every sound's timing as a delay, never a partial set.
        assert!(
            sounds == source_sounds || sounds == 0,
            "sound events were partly dropped"
        );
        assert!(completion > 0, "source completion must be scheduled");
        // Every source feedback event, none kept as a delay or merged with another.
        let mut expected_feedback = source_feedback.clone();
        expected_feedback.sort_by_key(serde_json::Value::to_string);
        feedback_events.sort_by_key(serde_json::Value::to_string);
        assert!(
            !expected_feedback.is_empty(),
            "required feedback corpus is empty"
        );
        assert_eq!(
            feedback_events, expected_feedback,
            "feedback events differ from the source"
        );
        // Every source animation event, with the source motion behind each body action.
        animation_events.sort_by_key(serde_json::Value::to_string);
        assert_eq!(
            animation_events, expected_animation,
            "animation events differ from the source"
        );
        let human_tables = animation::stock_routes(&staged, &stock);
        let (scalar_inputs, extended_banks) =
            input::readback(&graph, &owners, &originals, &source_scalars);
        assert_eq!(systems.len(), source_wrappers.len());
        let mut footers = BTreeSet::new();
        for &system in &systems {
            let wrapper = Payload(staged.read_tag(TagHash(system)).unwrap());
            assert_eq!(wrapper.0.len(), 0x34);
            for (offset, class) in [(0, 0x80806E2C), (0x14, 0x808071E8), (0x18, 0x80806E2E)] {
                let tag = wrapper.u32(offset).unwrap();
                assert!(stock.get_entry(TagHash(tag)).is_none());
                assert_eq!(staged.get_entry(TagHash(tag)).unwrap().reference, class);
            }
            for offset in [4, 8, 0x10, 0x1C, 0x30] {
                assert_eq!(wrapper.u32(offset).unwrap(), u32::MAX);
            }
            assert_eq!(wrapper.u16(0x2C).unwrap(), 0, "no native cost attenuation");
            footers.insert(wrapper.bytes::<12>(0x20).unwrap());
        }
        assert_eq!(
            footers, source_footers,
            "wrapper footers differ from their sources"
        );
        for (&native_offset, &provider) in &source_inputs {
            let rows = links
                .iter()
                .copied()
                .filter(|&at| {
                    graph.u32(at + 8).unwrap() == controller_tag
                        && graph.u64(at + 16).unwrap() == (definition + native_offset) as u64
                })
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1, "controller input {native_offset:X}");
            let owner = graph.u32(rows[0] + 40).unwrap();
            let shape = *sequence_owners
                .get(&owner)
                .expect("a controller input names another component");
            let (_, provider_definition) = pair(&owners[&owner], owner, (0x808084D7, 0x808084E9));
            assert_eq!(
                graph.u64(rows[0] + 48).unwrap(),
                (provider_definition + 0x90) as u64
            );
            assert_eq!(
                shape,
                source_signature(provider),
                "controller input {native_offset:X}"
            );
            let camera = camera::readback(
                &source,
                &source.read(provider).unwrap(),
                &owners[&owner],
                &staged,
                &stock,
            );
            presentation.push(json!({"source":provider,"owner":owner,"camera":camera}));
        }
        let replication_tag = graph.u32(0x88).unwrap();
        assert!(stock.get_entry(TagHash(replication_tag)).is_none());
        let replication = Payload(staged.read_tag(TagHash(replication_tag)).unwrap());
        assert_eq!(replication.u32(8).unwrap(), root);
        let component_rows = graph.array(16, 12, Some(0x80809C04)).unwrap();
        let replication_rows = replication.array(48, 32, Some(0x80809BB8)).unwrap();
        assert_eq!(component_rows.len(), replication_rows.len());
        let chain =
            crate::chain::discover_patch_chain(&staged_path, TagHash(replication_tag).pkg_id())
                .unwrap();
        let package = fs::read(&chain.latest().path).unwrap();
        let package_layout = crate::format::PackageLayout::parse(&package).unwrap();
        let companions = package_layout
            .shared_tag_enrollment_rows(&package)
            .unwrap()
            .chunks_exact(8)
            .filter(|row| read_u32(row, 0).unwrap() == replication_tag)
            .map(|row| read_u32(row, 4).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(companions.len(), 1);
        let companion_tag = companions[0];
        assert_eq!(
            staged.get_entry(TagHash(companion_tag)).unwrap().reference,
            0x80809EF9
        );
        let companion = staged.read_tag(TagHash(companion_tag)).unwrap();
        let loading = sundial::package_authoring::loading_index::entries(
            &sundial::package_authoring::loading_index::decode(
                &companion,
                companion_tag,
                replication_tag,
            )
            .unwrap(),
        );
        for tag in [replication_tag, companion_tag] {
            assert!(loading.contains(&(TagHash(tag).pkg_id(), TagHash(tag).entry_index())));
        }
        let mut allocation_evidence = Vec::new();
        for (&component, &replicated) in component_rows.iter().zip(&replication_rows) {
            let tag = graph.u32(component).unwrap();
            let owner = &owners[&tag];
            assert_eq!(
                replication.u32(replicated).unwrap(),
                owner.u32(owner.pointer(16).unwrap() - 4).unwrap()
            );
            assert_eq!(
                replication.u32(replicated + 4).unwrap(),
                owner.u32(0x44).unwrap()
            );
            let allocation = TagHash(owner.u32(0x44).unwrap());
            assert_eq!(staged.get_entry(allocation).unwrap().reference, 0x80809BBB);
            let bytes = staged.read_tag(allocation).unwrap();
            if tag == controller_tag {
                let witness_allocation = Payload(
                    stock
                        .read_tag(TagHash(action_witness.u32(0x44).unwrap()))
                        .unwrap(),
                );
                query::allocation(&Payload(bytes.clone()), &witness_allocation);
            }
            assert!(loading.contains(&(allocation.pkg_id(), allocation.entry_index())));
            let private = [controller_tag, bank_tag].contains(&tag)
                || sequence_owners.contains_key(&tag)
                || extended_banks.contains(&tag);
            if private {
                assert!(stock.get_entry(allocation).is_none());
            } else {
                assert_eq!(bytes, stock.read_tag(allocation).unwrap());
                assert!(
                    originals.values().any(|original| {
                        let original = Payload(original.clone());
                        original.u32(0x44).unwrap() == allocation.0
                            && original.u32(original.pointer(16).unwrap() - 4).unwrap()
                                == owner.u32(owner.pointer(16).unwrap() - 4).unwrap()
                    }),
                    "retained allocation must come from a matching native component"
                );
            }
            fs::write(output.join(format!("{:08X}.bin", allocation.0)), &bytes).unwrap();
            allocation_evidence.push(json!({"owner":tag,"allocation":allocation.0,
            "private":private,"sha256":sha(&bytes),"enrolled":true}));
        }
        assert!(
            allocation_evidence
                .iter()
                .any(|row| row["private"] == false)
        );
        fs::write(
            output.join(format!("{replication_tag:08X}.bin")),
            &replication.0,
        )
        .unwrap();
        fs::write(output.join(format!("{companion_tag:08X}.bin")), &companion).unwrap();
        for (tag, bytes) in &originals {
            assert_eq!(
                staged.read_tag(TagHash(*tag)).unwrap(),
                *bytes,
                "stock donor changed"
            );
        }
        for (tag, bytes) in [
            (root, &graph.0),
            (controller_tag, &controller.0),
            (bank_tag, &bank.0),
            (metadata_tag, &metadata.0),
        ] {
            fs::write(output.join(format!("{tag:08X}.bin")), bytes).unwrap();
        }
        let source_presentation = json!({"upgrade_entries":upgrade_entries,
            "upgrades":selection.upgrades,"particle_systems":systems,
            "source_particle_events":source_particles,"sound_events":sounds,
            "source_sound_events":source_sounds,"cues":cues,"buff_graphs":buff_graphs,
            "feedback_events":feedback_events,"animation_events":animation_events,
            "human_tables":human_tables,"scalar_inputs":scalar_inputs});
        fs::write(output.join(format!("readback-{super_entry}.json")),serde_json::to_vec_pretty(&json!({
        "revision":revision,"source_build":"56578.17.04.12.1251.evil",
        "native_build":"86657.20.08.23.1800.d2_rc___release","source_packages":source_packages,
        "source_bank_sha256":sha(&source_bank.0),"recipe":recipe_path,"manifest":build.manifest_path,
        "root":root,"row":row,"identity":pairs[usize::from(row)].identity,"pattern":pairs[usize::from(row)].pattern,
        "controller":controller_tag,"bank":bank_tag,"slot_metadata":metadata_tag,"callbacks":callbacks,
        "super_entry":super_entry,"source_presentation":source_presentation,
        "initializers":init,"query_geometry":{"offset":query,"class":controller.u32(query-4).unwrap(),"body":hex::encode(controller.bytes::<16>(query).unwrap())},
        "query_action":query_action,"query_selection":query_selection,
        "presentation":presentation,"grants":grants,"root_sha256":sha(&graph.0),
        "controller_sha256":sha(&controller.0),"bank_sha256":sha(&bank.0),
        "source_controller_sha256":sha(&source_controller.0),
        "energy":{"charges":controller.u32(definition+0xC8).unwrap(),
            "activation_cost":controller.f32(definition+0xCC).unwrap(),
            "base_active_drain":controller.f32(definition+0xD0).unwrap(),
            "flags":controller.u16(definition+0x4C).unwrap()},
        "replication":replication_tag,"replication_companion":companion_tag,
        "replication_sha256":sha(&replication.0),"companion_sha256":sha(&companion),
        "allocations":allocation_evidence,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        "installed":false,"gameplay_verified":false,
        "limits":["Package readback does not prove dead-player input, authority, charge consumption, resurrection or repeated-use cleanup.",
            "Sequence translation is checked structurally. Rendering, sound, animation and buff effects in play are not observed."]
    })).unwrap()).unwrap();
    }
}
