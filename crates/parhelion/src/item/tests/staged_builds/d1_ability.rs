//! Preimplementation acceptance for a saved D1 Radiance preparation. Failure boundaries:
//! donor fallback, missing paired ability identities, nonreciprocal controller/bank blocks,
//! dangling source-build owners, a Fireborn flag with no selected consumer, missing recipient
//! defaults and stock mutations. Package acceptance does not establish dead-player activation.
use super::*;
use parhelion_import::{d1_roi::Source, tiger::payload::Payload};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use sundial::package_authoring::{ability_definition, sandbox_perk::program};
mod event;
mod lifecycle;

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn pair(payload: &Payload, tag: u32, classes: (u32, u32)) -> (usize, usize) {
    let live = payload.pointer(16).unwrap();
    let definition = usize::try_from(payload.u64(live + 8).unwrap()).unwrap();
    let boundary = payload.pointer(24).unwrap();
    assert!(live < definition && definition <= boundary);
    assert_eq!(payload.u32(live - 4).unwrap(), classes.0);
    assert_eq!(payload.u32(definition - 4).unwrap(), classes.1);
    assert_eq!(payload.u32(boundary - 4).unwrap(), classes.1);
    assert_eq!(
        &payload.0[boundary..boundary + 16],
        &payload.0[definition..definition + 16]
    );
    for (at, twin, class) in [(live, definition, classes.1), (definition, live, classes.0)] {
        assert_eq!(payload.u32(at).unwrap(), tag);
        assert_eq!(payload.u32(at + 4).unwrap(), class);
        assert_eq!(payload.u64(at + 8).unwrap(), twin as u64);
    }
    (live, definition)
}

#[allow(clippy::cognitive_complexity)]
fn bank_rows(
    source: &Payload,
    native: &Payload,
    native_tag: u32,
    definition: usize,
    staged: &PackageManager,
) -> Vec<serde_json::Value> {
    let source_definition = source.pointer(24).unwrap();
    let original = source
        .array(source_definition + 0xA0, 40, Some(0x80802A84))
        .unwrap();
    let translated = native
        .array(definition + 0x88, 56, Some(0x8080453B))
        .unwrap();
    assert_eq!(original.len(), 14, "complete configured source fixture");
    assert_eq!(translated.len(), original.len());
    let parameters = source
        .array(source_definition + 0xB0, 16, Some(0x80802122))
        .unwrap();
    let native_parameters = native
        .array(definition + 0x98, 16, Some(0x80804519))
        .unwrap();
    assert_eq!(parameters.len(), 4);
    assert_eq!(native_parameters.len(), parameters.len());
    for (&a, &b) in parameters.iter().zip(&native_parameters) {
        assert_eq!(
            source.bytes::<16>(a).unwrap(),
            native.bytes::<16>(b).unwrap()
        );
    }
    let boundary = native.pointer(24).unwrap();
    let live = native.pointer(16).unwrap();
    let state = native.array(live + 0x190, 1, Some(0x80800009)).unwrap();
    let source_state = source
        .array(source.pointer(16).unwrap() + 0x190, 1, Some(0x80800009))
        .unwrap();
    assert!(!state.is_empty());
    assert_eq!(state.len(), source_state.len());
    assert!(state[0] >= live && *state.last().unwrap() < boundary);
    let mut evidence = Vec::new();
    for (a, b) in original.into_iter().zip(translated) {
        assert_eq!(
            source.bytes::<8>(a).unwrap(),
            native.bytes::<8>(b + 0x10).unwrap()
        );
        assert_eq!(source.u32(a + 8).unwrap(), native.u32(b + 0x18).unwrap());
        assert_eq!(source.u32(a + 0x10).unwrap(), native.u32(b + 0x20).unwrap());
        assert_eq!(
            source.u64(a + 0x18).unwrap(),
            0,
            "configured source gate is absent"
        );
        assert_eq!(native.u64(b + 0x28).unwrap(), 0);
        let source_body = source.pointer(a + 0x20).unwrap();
        let body = native.pointer(b + 0x30).unwrap();
        let class = source.u32(source_body - 4).unwrap();
        let (target, bytes) = match class {
            0x8080243C => (0x8080452E, 4),
            0x80802974 => (0x80804516, 24),
            0x808026D1 => (0x80804520, 8),
            0x80802988 => (0x80804189, 1),
            0x808029F6 => (0x80804187, 1),
            0x8080236B => (0x80804183, 16),
            0x8080216B => (0x80804181, 16),
            _ => panic!("Unexpected source Radiance property {class:08X}"),
        };
        assert_eq!(native.u32(body + 4).unwrap(), target);
        let instance = usize::try_from(native.u64(body + 8).unwrap()).unwrap();
        assert!(instance >= live && instance + 16 <= boundary);
        assert_eq!(native.u32(body).unwrap(), native_tag);
        assert_eq!(native.u32(instance).unwrap(), native_tag);
        assert_eq!(native.u64(instance + 8).unwrap(), body as u64);
        let priority_slot = native.u32(b + 0x1C).unwrap();
        assert_eq!(priority_slot, source.u32(a + 0xC).unwrap());
        assert!(usize::try_from(priority_slot).unwrap() < state.len());
        if matches!(class, 0x8080236B | 0x8080216B) {
            assert_eq!(
                source.bytes::<8>(source_body).unwrap(),
                native.bytes::<8>(body + 16).unwrap()
            );
            assert_eq!(
                source.u32(source_body + 12).unwrap(),
                native.u32(body + 28).unwrap()
            );
            let target = native.u32(body + 24).unwrap();
            assert!(!matches!(target, 0 | u32::MAX));
            assert!(
                staged.get_entry(TagHash(target)).is_some(),
                "translated property resource resolves"
            );
        } else {
            assert_eq!(
                &source.0[source_body..source_body + bytes],
                &native.0[body + 16..body + 16 + bytes]
            );
        }
        if class == 0x80802974 {
            let first = usize::try_from(native.u32(body + 0x20).unwrap()).unwrap();
            let count = usize::try_from(native.u32(body + 0x24).unwrap()).unwrap();
            assert!(
                first
                    .checked_add(count)
                    .is_some_and(|end| end <= native_parameters.len())
            );
        }
        if class == 0x808026D1 {
            assert!(instance + 40 <= boundary);
            assert_eq!(native.u64(body + 24).unwrap(), 0);
            assert_eq!(native.u64(body + 64).unwrap(), 1);
            assert_eq!(native.u64(body + 72).unwrap(), 1);
            assert_eq!(native.u64(body + 80).unwrap(), 0);
            assert_eq!(native.u32(body + 88).unwrap(), u32::MAX);
            let code = native.array(body + 32, 1, Some(0x80800009)).unwrap();
            assert_eq!(code.len(), 4);
            assert_eq!(native.bytes::<4>(code[0]).unwrap(), [0x34, 0, 0x3E, 0]);
            let values = native.array(body + 48, 16, Some(0x80800090)).unwrap();
            assert_eq!(values.len(), 1);
            for lane in 0..4 {
                assert_eq!(
                    native.u32(values[0] + lane * 4).unwrap(),
                    source.u32(source_body + 4).unwrap()
                );
            }
        }
        evidence.push(json!({"source_class":class,"native_class":target,"key":native.u32(b+0x10).unwrap(),"priority_slot":priority_slot}));
    }
    evidence
}

#[allow(clippy::cognitive_complexity)]
fn sequence_inputs(
    source: &Source,
    native: &Payload,
    native_tag: u32,
    graph: &Payload,
    staged: &PackageManager,
) -> Vec<serde_json::Value> {
    let source_graph = source.read(0x80A6A073).unwrap();
    let original = source.read(0x80A6A126).unwrap();
    let original_live = original.pointer(16).unwrap();
    let native_live = native.pointer(16).unwrap();
    let native_definition = usize::try_from(native.u64(native_live + 8).unwrap()).unwrap();
    let source_links = source_graph.array(0x30, 88, Some(0x8080054E)).unwrap();
    let links = graph.array(0x20, 72, Some(0x80809BC9)).unwrap();
    let source_owners = source_graph.array(0x20, 12, Some(0x80800715)).unwrap();
    let owners = graph.array(16, 12, Some(0x80809C04)).unwrap();
    assert_eq!(source_owners.len(), owners.len());
    let mut evidence = Vec::new();
    // Original selector consumers, in mode order, independently locate these live helpers.
    // Reciprocal headers establish each source definition rather than assumed physical order.
    for (mode, source_base, native_base, native_def) in [
        (0, 0x4F0, 0x550, 0x208),
        (1, 0xC00, 0xAE0, 0x6C0),
        (2, 0xCF0, 0xBD0, 0x630),
    ] {
        for phase in 0..3 {
            let from = original_live + source_base + phase * 0x50;
            let from_def = usize::try_from(original.u64(from + 8).unwrap()).unwrap();
            let to = native_live + native_base + phase * 0x50;
            let to_def = native_definition + native_def + phase * 0x30;
            assert_eq!(native.u32(to).unwrap(), native_tag);
            assert_eq!(native.u32(to + 4).unwrap(), 0x808092F4);
            assert_eq!(native.u64(to + 8).unwrap(), to_def as u64);
            assert_eq!(native.pointer(to + 16).unwrap(), native_live);
            assert_eq!(native.u32(to_def).unwrap(), native_tag);
            assert_eq!(native.u32(to_def + 4).unwrap(), 0x808092F3);
            assert_eq!(native.u64(to_def + 8).unwrap(), to as u64);
            assert_eq!(
                native.u64(to_def + 16).unwrap(),
                original.u64(from_def + 24).unwrap()
            );
            assert_eq!(native.u64(to_def + 24).unwrap(), 0x808092F7);
            assert_eq!(native.u64(to_def + 32).unwrap(), 0);
            assert_eq!(native.u64(to_def + 40).unwrap(), u32::MAX as u64);
            assert_eq!(native.u32(to + 0x38).unwrap(), u32::MAX);
            assert_eq!(native.u64(to + 0x40).unwrap(), 0);
            let selected = source_links
                .iter()
                .copied()
                .filter(|&at| {
                    source_graph.u32(at + 8).unwrap() == 0x80A6A126
                        && source_graph.u64(at + 16).unwrap() == from_def as u64
                        && source_graph.u64(at + 24).unwrap() == 0x80800456
                })
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            let selected = selected[0];
            let translated = links
                .iter()
                .copied()
                .filter(|&at| {
                    graph.u32(at + 8).unwrap() == native_tag
                        && graph.u64(at + 16).unwrap() == to_def as u64
                        && graph.u32(at + 12).unwrap() == 0x808092F4
                })
                .collect::<Vec<_>>();
            assert_eq!(translated.len(), 1);
            let translated = translated[0];
            assert_eq!(
                graph.u64(translated).unwrap(),
                source_graph.u64(selected).unwrap()
            );
            assert_eq!(
                graph.u64(translated + 24).unwrap(),
                source_graph.u64(selected + 32).unwrap()
            );
            let occurrence = usize::try_from(source_graph.u64(selected + 72).unwrap()).unwrap();
            assert_eq!(graph.u64(translated + 56).unwrap(), occurrence as u64);
            assert_eq!(graph.u64(translated + 64).unwrap(), 0);
            let provider_tag = graph.u32(owners[occurrence]).unwrap();
            assert_eq!(graph.u32(translated + 40).unwrap(), provider_tag);
            let provider = Payload(staged.read_tag(TagHash(provider_tag)).unwrap());
            let provider_live = provider.pointer(16).unwrap();
            let provider_def = usize::try_from(provider.u64(provider_live + 8).unwrap()).unwrap();
            let endpoint = usize::try_from(graph.u64(translated + 48).unwrap()).unwrap();
            assert_eq!(provider.u32(provider_live - 4).unwrap(), 0x808084D7);
            assert_eq!(endpoint, provider_def + 0x90);
            assert_eq!(provider.pointer(endpoint).unwrap(), provider_def);
            assert_eq!(graph.u32(translated + 44).unwrap(), 0x808092F8);
            let metadata = Payload(
                staged
                    .read_tag(TagHash(provider.u32(endpoint + 8).unwrap()))
                    .unwrap(),
            );
            let methods = metadata.array(16, 24, Some(0x80809C56)).unwrap();
            assert_eq!(metadata.u32(8).unwrap(), 0x808092F8);
            assert_eq!(metadata.u32(12).unwrap(), 0x808092F7);
            assert_eq!(methods.len(), 1);
            assert_eq!(metadata.u32(methods[0]).unwrap(), 0x808084D7);
            assert_eq!(metadata.u32(methods[0] + 4).unwrap(), 3);
            assert_eq!(metadata.bytes::<16>(methods[0] + 8).unwrap(), [0; 16]);
            evidence.push(
                json!({"mode":mode,"selector":phase,"source_helper":from_def,
                "native_helper":to_def,"provider_occurrence":occurrence,"provider":provider_tag,
                "provider_sha256":sha(&provider.0)}),
            );
        }
    }
    evidence
}

#[test]
#[ignore = "requires D1_SOURCE_PACKAGES, D1_RADIANCE_RECIPE, SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn saved_radiance_reaches_private_controller_bank_and_fireborn_property() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("radiance");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("fresh artifact directory");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let source_packages =
        PathBuf::from(std::env::var_os("D1_SOURCE_PACKAGES").expect("source packages"));
    let source = Source::open(&source_packages, |_, _| {}).unwrap();
    let source_bank = source.read(0x80A6A129).unwrap();
    let source_rows = source_bank
        .array(
            source_bank.pointer(24).unwrap() + 0xA0,
            40,
            Some(0x80802A84),
        )
        .unwrap();
    let flags = source_rows
        .into_iter()
        .filter(|&at| source_bank.u32(at).unwrap() == 0x550050A2)
        .filter_map(|at| {
            let body = source_bank.pointer(at + 32).unwrap();
            (source_bank.u32(body - 4).unwrap() == 0x80802988)
                .then(|| source_bank.u8(body).unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(flags, [1]);
    let recipe_path =
        PathBuf::from(std::env::var_os("D1_RADIANCE_RECIPE").expect("saved prepared recipe"));
    let recipe = crate::WeaponRecipe::load_json(&recipe_path).unwrap();
    assert_eq!(recipe.kind, crate::ItemKind::Subclass);
    recipe
        .save_json(output.join("prepared.parhelion.json"))
        .unwrap();
    let (view, source_path) = stock_view(&packages, ".parhelion-radiance-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    let stock = open_manager(&source_path).unwrap();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: source_path,
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe.clone()],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
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
    let row = subclass.entry_rows[&crate::subclass::layout::SUPER];
    let root = subclass.entry_entities[&crate::subclass::layout::SUPER];
    assert!(stock.get_entry(TagHash(root)).is_none());
    let definitions = staged.read_tag(TagHash(0x81613D26)).unwrap();
    let identities = staged.read_tag(TagHash(0x81319130)).unwrap();
    let pairs = ability_definition::definitions(&definitions, &identities).unwrap();
    let original_pairs = ability_definition::definitions(
        &stock.read_tag(TagHash(0x81613D26)).unwrap(),
        &stock.read_tag(TagHash(0x81319130)).unwrap(),
    )
    .unwrap();
    assert_eq!(&pairs[..original_pairs.len()], original_pairs.as_slice());
    assert!(usize::from(row) >= original_pairs.len());
    let graph = Payload(staged.read_tag(TagHash(root)).unwrap());
    let components = graph.array(16, 12, Some(0x80809C04)).unwrap();
    assert!(!components.is_empty());
    let mut owners = std::collections::BTreeMap::new();
    for at in components {
        let tag = graph.u32(at).unwrap();
        assert_eq!(
            staged.get_entry(TagHash(tag)).unwrap().reference,
            0x80809C36
        );
        let payload = Payload(staged.read_tag(TagHash(tag)).unwrap());
        owners.insert(
            payload.u32(payload.pointer(24).unwrap() - 4).unwrap(),
            (tag, payload),
        );
    }
    let (controller_tag, controller) = &owners[&0x80804177];
    let (bank_tag, bank) = &owners[&0x8080417B];
    let catalog_row = catalog
        .ability_row(row)
        .expect("private ability catalog row");
    assert_eq!(catalog_row.bank, Some(*bank_tag));
    assert_eq!(catalog_row.slot, Some(program::AbilityTarget::Super));
    assert!(stock.get_entry(TagHash(*controller_tag)).is_none());
    assert!(stock.get_entry(TagHash(*bank_tag)).is_none());
    let (controller_live, controller_definition) =
        pair(controller, *controller_tag, (0x80804176, 0x80804177));
    let (bank_live, bank_definition) = pair(bank, *bank_tag, (0x8080417A, 0x8080417B));
    let property_readback = bank_rows(&source_bank, bank, *bank_tag, bank_definition, &staged);
    let sequence_readback = sequence_inputs(&source, controller, *controller_tag, &graph, &staged);
    let event_readback = event::readback(&source, &graph, &staged);
    let lifecycle_readback = lifecycle::readback(&source, &graph, &staged);
    let controller_boundary = controller.pointer(24).unwrap();
    let bank_boundary = bank.pointer(24).unwrap();
    assert!(controller_live + 0xF40 <= controller_boundary);
    assert!(bank_live + 0x1B0 <= bank_boundary);
    assert!(
        controller
            .0
            .get(controller_live..controller_live + 0xF40)
            .is_some()
    );
    assert!(
        controller
            .0
            .get(controller_definition..controller_definition + 0xB30)
            .is_some()
    );
    assert!(bank.0.get(bank_live..bank_live + 0x1B0).is_some());
    assert!(
        bank.0
            .get(bank_definition..bank_definition + 0xD0)
            .is_some()
    );
    // Bank tail 80804179 is distinct from the transient 8080418D recipient at controller
    // +F20. Native execution initializes that recipient before applying selected properties.
    // Serialized defaults are checked only once their native declarations are qualified.
    assert!(
        controller
            .0
            .get(controller_live + 0xF20..controller_live + 0xF30)
            .is_some()
    );
    let mut flag_rows = Vec::new();
    for at in bank
        .array(bank_definition + 0x88, 56, Some(0x8080453B))
        .unwrap()
    {
        let definition = bank.pointer(at + 0x30).unwrap();
        if bank.u32(definition + 4).unwrap() != 0x80804189 {
            continue;
        }
        let live = usize::try_from(bank.u64(definition + 8).unwrap()).unwrap();
        assert!(bank_live <= live && live + 16 <= bank_boundary);
        assert_eq!(bank.u32(live + 4).unwrap(), 0x8080418A);
        assert_eq!(bank.u64(live + 8).unwrap(), definition as u64);
        assert_eq!(bank.u32(definition).unwrap(), *bank_tag);
        assert_eq!(bank.u32(live).unwrap(), *bank_tag);
        assert_eq!(bank.u8(definition + 16).unwrap(), flags[0]);
        flag_rows.push((bank.u32(at + 16).unwrap(), bank.u32(at + 0x1C).unwrap()));
    }
    assert_eq!(flag_rows.len(), 1, "one Fireborn Boolean property");
    let globals = staged
        .read_tag(resolve_live_named_tag(&staged, "investment_globals", None).unwrap())
        .unwrap();
    let stock_globals = stock
        .read_tag(resolve_live_named_tag(&stock, "investment_globals", None).unwrap())
        .unwrap();
    let stock_finished = stock
        .read_tag(TagHash(read_u32(&stock_globals, 16 + 71 * 16).unwrap()))
        .unwrap();
    let stock_perks = array_at(&stock_finished, 8).unwrap().0;
    let mut applied = BTreeSet::new();
    for perk in subclass.entry_perks.values().flatten() {
        // Native Rift and unchanged stock passives are outside this private Super-key check.
        if usize::from(*perk) < stock_perks {
            continue;
        }
        let action =
            load_sandbox_perk_runtime_action(&staged, &globals, usize::from(*perk)).unwrap();
        for (target, key) in program::ability_properties_in(&action.action_payload).unwrap() {
            if target == program::AbilityTarget::Super {
                applied.insert(key);
            }
        }
    }
    assert!(
        applied.contains(&flag_rows[0].0),
        "an emitted selected path applies Fireborn's private key"
    );
    for at in graph.array(0x68, 24, None).unwrap() {
        let tag = graph.u32(at).unwrap();
        assert!(
            staged.get_entry(TagHash(tag)).is_some(),
            "resource owner must resolve in native packages"
        );
    }
    for (tag, payload) in [
        (root, &graph),
        (*controller_tag, controller),
        (*bank_tag, bank),
    ] {
        fs::write(output.join(format!("{tag:08X}.bin")), &payload.0).unwrap();
    }
    fs::write(output.join("readback.json"),serde_json::to_vec_pretty(&json!({
        "revision":revision,"source_packages":source_packages,"source_bank_sha256":sha(&source_bank.0),
        "recipe":recipe_path,"manifest":build.manifest_path,"row":row,"identity":pairs[usize::from(row)].identity,
        "pattern":pairs[usize::from(row)].pattern,"root":root,"controller":controller_tag,"bank":bank_tag,
        "fireborn_rows":flag_rows,"property_readback":property_readback,"sequence_readback":sequence_readback,"event_readback":event_readback,"lifecycle_readback":lifecycle_readback,"root_sha256":sha(&graph.0),"controller_sha256":sha(&controller.0),"bank_sha256":sha(&bank.0),
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),"installed":false,"gameplay_verified":false
    })).unwrap()).unwrap();
}
