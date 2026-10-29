//! Persisted candidate acceptance: preparation must reach the equipped private controller,
//! selected source flag and drain, slot binding, completion and isolated presentation. The
//! revival grant belongs to a passive node on each path, separate from its Radiance Super.
//! Both equipped Super entries must reach the retained controller. Other nodes stay stock. A donor
//! graph, stock metadata mutation, unscheduled completion, unselected grant or an allocation
//! missing from the replication loading companion is a failure. Retained components must keep
//! their native allocations while authored controller and bank storage needs private allocations.
//! A missing query geometry rejects embedded construction. Turning a disconnected optional
//! sequence into a required input rejects graph connection setup after construction succeeds.
//! Property refresh requires the exact query damage action, its initialized recipient and
//! mutable tracking storage. A lookalike action or missing nested allocation is insufficient.
//! This package E2E cannot establish dead-player authority or completed resurrection.
use super::*;
use crate::subclass::{AttunementPath, import, layout};
use std::collections::BTreeMap;
mod query;

#[test]
#[ignore = "requires D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn prepared_self_revive_reloads_and_equips_private_slot_binding_and_source_grant() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("self-revive");
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
    let (view, stock_path) = stock_view(&packages, ".parhelion-self-revive-", Oodle::Required);
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
        "parhelion.self-revive-acceptance",
        donor.hash,
        &donor.name,
    )
    .unwrap();
    recipe.kind = crate::ItemKind::Subclass;
    recipe.name = "Self-Revive Acceptance".into();
    let baseline = recipe.clone();
    let prepared = import::prepare_revival(&import::Request {
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
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: stock_path,
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe.clone()],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let (readback, staged_path) = stock_view(
        &packages,
        ".parhelion-self-revive-readback-",
        Oodle::Required,
    );
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
    for entry in [11, 12, 13, 15, 16, 17, 21, 22] {
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
        assert_eq!(
            subclass.entry_modifiers.get(&entry),
            donor.entry_modifiers.get(&entry)
        );
    }
    let authored = recipe.overrides.subclass_abilities.as_ref().unwrap();
    let mut perk_ids = BTreeSet::new();
    for path in AttunementPath::ALL {
        let entry = path.entries()[3];
        assert!(
            !subclass.entry_rows.contains_key(&entry),
            "A revival perk cannot equip a second Super"
        );
        let node = authored.node(donor.hash, path, 3);
        assert_eq!(node.edits.name.as_deref(), Some("Fireborn"));
        assert!(node.edits.imported_ability.is_none());
        assert_eq!(node.edits.custom_perks.len(), 1);
        assert!(perk_ids.insert(node.edits.custom_perks[0].id.clone()));
    }
    assert_ne!(
        subclass.entry_rows[&layout::SUPER],
        subclass.entry_rows[&20]
    );
    assert_ne!(
        subclass.entry_entities[&layout::SUPER],
        subclass.entry_entities[&20]
    );
    for super_entry in [layout::SUPER, 20] {
        let edits = if super_entry == layout::SUPER {
            authored.ability(donor.hash, super_entry).edits
        } else {
            authored.node(donor.hash, AttunementPath::Middle, 0).edits
        };
        assert_eq!(edits.name.as_deref(), Some("Radiance"));
        assert!(edits.custom_perks.is_empty());
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
        let (_, bank_definition) = pair(bank, bank_tag, (0x8080417A, 0x8080417B));
        let catalog_row = catalog.ability_row(row).unwrap();
        assert_eq!(catalog_row.bank, Some(bank_tag));
        assert_eq!(catalog_row.slot, Some(program::AbilityTarget::Super));

        // Independent stock helper witness, not the writer's declaration defaults. Native A4DA20
        // rejects this absent geometry, and A20150 rejects disconnected required consumers.
        let query_witness = Payload(stock.read_tag(TagHash(0x80BC3EF8)).unwrap());
        let native_query = query_witness.pointer(0x20E0).unwrap();
        assert_eq!(query_witness.u32(native_query - 4).unwrap(), 0x808092B1);
        let query = controller.pointer(definition + 0x810).unwrap();
        assert_eq!(controller.u32(query - 4).unwrap(), 0x808092B1);
        assert_eq!(
            controller.bytes::<16>(query).unwrap(),
            query_witness.bytes::<16>(native_query).unwrap()
        );
        assert_eq!(controller.u32(query).unwrap(), 2);
        assert_eq!(controller.f32(query + 4).unwrap(), 1.0);
        assert_eq!(controller.u64(query + 8).unwrap(), 0);
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

        let source_bank = source.read(0x80A6A129).unwrap();
        let original_rows = source_bank
            .array(
                source_bank.pointer(24).unwrap() + 0xA0,
                40,
                Some(0x80802A84),
            )
            .unwrap()
            .into_iter()
            .filter(|&at| source_bank.u32(at).unwrap() == 0x550050A2)
            .collect::<Vec<_>>();
        let imported_rows = bank
            .array(bank_definition + 0x88, 56, Some(0x8080453B))
            .unwrap();
        assert_eq!(original_rows.len(), 2);
        assert_eq!(imported_rows.len(), 2);
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
                    assert_eq!(bank.u8(b + 16).unwrap(), 1);
                }
                0x8080243C => {
                    assert_eq!(bank.u32(b + 4).unwrap(), 0x8080452E);
                    assert_eq!(source_bank.u32(a).unwrap(), bank.u32(b + 16).unwrap());
                }
                other => panic!("Unexpected Fireborn companion {other:08X}"),
            }
            assert_eq!(bank.u32(to + 24).unwrap(), 255);
        }
        let globals = staged
            .read_tag(resolve_live_named_tag(&staged, "investment_globals", None).unwrap())
            .unwrap();
        let mut grants = BTreeMap::new();
        for entry in [14, 18, 23] {
            let mut granted = BTreeSet::new();
            for &perk in &subclass.entry_perks[&entry] {
                let action =
                    load_sandbox_perk_runtime_action(&staged, &globals, usize::from(perk)).unwrap();
                granted.extend(
                    program::ability_properties_in(&action.action_payload)
                        .unwrap()
                        .into_iter()
                        .filter_map(|(target, key)| {
                            (target == program::AbilityTarget::Super).then_some(key)
                        }),
                );
            }
            assert_eq!(granted, BTreeSet::from([0x550050A2]));
            grants.insert(entry, granted);
        }

        let links = graph.array(0x20, 72, Some(0x80809BC9)).unwrap();
        let mut presentation = Vec::new();
        let mut completion = 0;
        let mut animation = 0;
        for (&tag, owner) in &owners {
            if owner.u32(owner.pointer(16).unwrap() - 4).unwrap() != 0x808084D7 {
                continue;
            }
            let (_, definition) = pair(owner, tag, (0x808084D7, 0x808084E9));
            let events = owner
                .array(definition + 0x168, 24, Some(0x808093E6))
                .unwrap();
            let mut scheduled = Vec::new();
            for row in owner
                .array(definition + 0x158, 24, Some(0x808093E6))
                .unwrap()
            {
                let control = owner.pointer(row + 16).unwrap();
                for child in owner.array(control + 0x38, 4, Some(0x808093FB)).unwrap() {
                    if owner.u16(child).unwrap() != 1 {
                        continue;
                    }
                    let event = owner
                        .pointer(events[usize::from(owner.u16(child + 2).unwrap())] + 16)
                        .unwrap();
                    let kind = owner.u16(event + 0x30).unwrap();
                    assert!(
                        matches!(
                            (owner.u32(event - 4).unwrap(), kind),
                            (0x808093CB, 0)
                                | (0x80806CC6, 4)
                                | (0x80806B38, 6)
                                | (0x80803FE9, 18)
                                | (0x80804595, 19)
                        ),
                        "donor gameplay remains scheduled"
                    );
                    completion += usize::from(kind == 19);
                    animation += usize::from(kind == 18);
                    scheduled.push(kind);
                }
            }
            presentation.push(json!({"owner":tag,"scheduled":scheduled}));
        }
        assert!(
            completion > 0 && animation > 0,
            "native completion and animation must survive"
        );
        for (specialized, common) in [
            (0x630, 0x208),
            (0x660, 0x238),
            (0x690, 0x268),
            (0x6C0, 0x208),
            (0x6F0, 0x238),
            (0x720, 0x268),
        ] {
            let find = |offset| {
                links
                    .iter()
                    .copied()
                    .filter(|&at| {
                        graph.u32(at + 8).unwrap() == controller_tag
                            && graph.u64(at + 16).unwrap() == (definition + offset) as u64
                    })
                    .collect::<Vec<_>>()
            };
            let a = find(common);
            let b = find(specialized);
            assert_eq!(a.len(), 1);
            assert_eq!(b.len(), 1);
            assert_eq!(
                &graph.0[a[0] + 32..a[0] + 72],
                &graph.0[b[0] + 32..b[0] + 72]
            );
            assert_eq!(
                controller
                    .bytes::<8>(definition + specialized + 16)
                    .unwrap(),
                controller.bytes::<8>(definition + common + 16).unwrap(),
                "Sequence aliases must preserve the donor's connection policy"
            );
            if graph.u64(b[0] + 56).unwrap() == 0xFFFF {
                assert_ne!(
                    controller.u8(definition + specialized + 16).unwrap(),
                    0,
                    "Native connection setup rejects an unbound required sequence"
                );
            }
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
            let private = [controller_tag, bank_tag].contains(&tag);
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
        fs::write(output.join(format!("readback-{super_entry}.json")),serde_json::to_vec_pretty(&json!({
        "revision":revision,"source_build":"56578.17.04.12.1251.evil",
        "native_build":"86657.20.08.23.1800.d2_rc___release","source_packages":source_packages,
        "source_bank_sha256":sha(&source_bank.0),"recipe":recipe_path,"manifest":build.manifest_path,
        "root":root,"row":row,"identity":pairs[usize::from(row)].identity,"pattern":pairs[usize::from(row)].pattern,
        "controller":controller_tag,"bank":bank_tag,"slot_metadata":metadata_tag,"callbacks":callbacks,
        "super_entry":super_entry,"revival_perk_entries":[14,18,23],
        "initializers":init,"query_geometry":{"offset":query,"class":controller.u32(query-4).unwrap(),"body":hex::encode(controller.bytes::<16>(query).unwrap())},
        "query_action":query_action,
        "presentation":presentation,"grants":grants,"root_sha256":sha(&graph.0),
        "controller_sha256":sha(&controller.0),"bank_sha256":sha(&bank.0),
        "replication":replication_tag,"replication_companion":companion_tag,
        "replication_sha256":sha(&replication.0),"companion_sha256":sha(&companion),
        "allocations":allocation_evidence,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        "installed":false,"gameplay_verified":false,
        "limits":["Package readback does not prove dead-player input, authority, charge consumption, resurrection or repeated-use cleanup."]
    })).unwrap()).unwrap();
    }
}
