//! Real package readback across compile, whole-member replay and a later changed-layout compile.
use super::*;
use sundial::package_authoring::ability_bank::{self as bank, Modifier};

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn replay_preserves_packages_and_state_for_a_later_bank_recompile() {
    let source = crate::test_support::stock_packages();
    let manager = sundial::package_authoring::open_shadowkeep_package_manager(&source).unwrap();
    let output = crate::test_support::artifact_dir("family-pass-lifecycle");
    assert!(!output.exists(), "Choose a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    let _turn = crate::package::profile::hold(
        crate::package::profile::Window::FULL,
        crate::package::profile::Host::standalone(0x0CFE),
    );
    let owner = Importer::D1Roi;
    let mut current = fixture();
    current.decisions.contributions.clear();
    let family = Family::of(&current);
    let passes = crate::turn_cache::Passes::begin();
    let stock_bank = manager.read_tag(TagHash(0x80B8_075C)).unwrap();
    let stock_entity = manager.read_tag(TagHash(0x80B8_0761)).unwrap();
    let scalar = |value| Modifier::Parameter {
        name: 0x99B1_D826,
        applied: value,
        add: true,
        gate: None,
    };
    let first_bank = bank::with_property_row(&stock_bank, 0xF100_0001, scalar(0.5)).unwrap();
    let value_bank = bank::with_property_row(&stock_bank, 0xF100_0001, scalar(0.75)).unwrap();
    let grown_bank = bank::with_property_row(&value_bank, 0xF100_0002, scalar(0.25)).unwrap();
    let begin = |payload: &[u8]| {
        passes.next();
        family.start_pass(true, BTreeMap::new);
        crate::package::profile::begin_partition(
            crate::package::profile::Host::standalone(0x0CFE),
            None,
        )
        .unwrap();
        produced(Shared::Banks, [0x80B8_075C]);
        finish_shared(0x80B8_075C, payload);
        crate::package::profile::begin_partition(
            crate::package::profile::Host::standalone(member_host(owner).unwrap()),
            Some(owner),
        )
        .unwrap();
        snapshot().unwrap()
    };
    let reads = BTreeSet::from([0x80B8_075C]);
    let before = begin(&first_bank);
    let first_inputs = member_inputs(&before, &reads).unwrap();
    let relocate = |payload: &[u8]| {
        bank::retarget_references(&stock_entity, 0x80B8_075C, &stock_bank, payload)
            .unwrap()
            .unwrap()
    };
    let emit = |payload: Vec<u8>, id, name| {
        crate::package::extend::build_standalone_package_with_references(
            &source,
            id,
            name,
            &[NewTagSpec {
                template_tag: TagHash(0x80B8_0761),
                payload,
                storage: NewTagStorageMode::InheritTemplate,
            }],
            &[],
        )
        .unwrap()
    };
    let first_entity = relocate(&first_bank);
    let first = emit(
        first_entity.clone(),
        0x0CFA,
        "w64_family_referrer_0cfa_0.pkg",
    );
    let unchanged = emit(stock_entity.clone(), 0x0CF9, "w64_family_stable_0cf9_0.pkg");
    crate::turn_cache::keep_package([1; 32], &first);
    crate::turn_cache::keep_package([2; 32], &unchanged);
    contribute(|rows| {
        rows.collections.insert(12, 1);
    });
    front(7, 0x28, 3, 2);
    reads_shared([0x80B8_075C]);
    let after = snapshot().unwrap();
    passes.built(Some(owner));
    let expected = family.finish_pass();

    let second_before = begin(&value_bank);
    assert_eq!(member_inputs(&second_before, &reads).unwrap(), first_inputs);
    replay(owner, &before, &after);
    passes.replayed(Some(owner));
    let replayed = family.finish_pass();
    assert_eq!(replayed.fronts, expected.fronts);
    assert_eq!(replayed.recorded, expected.recorded);
    assert_eq!(snapshot().unwrap().reads(owner), reads);
    let control = emit(
        relocate(&value_bank),
        0x0CFA,
        "w64_family_referrer_0cfa_0.pkg",
    );
    assert_eq!(
        control.bytes(),
        first.bytes(),
        "a fresh compile must agree with the kept member"
    );

    let third_before = begin(&grown_bank);
    assert_ne!(member_inputs(&third_before, &reads).unwrap(), first_inputs);
    let retained =
        crate::turn_cache::package(&[2; 32]).expect("replay must retain unchanged packages");
    let changed_entity = relocate(&grown_bank);
    let changed = emit(
        changed_entity.clone(),
        0x0CFA,
        "w64_family_referrer_0cfa_0.pkg",
    );
    assert_ne!(changed_entity, first_entity);
    passes.built(Some(owner));
    assert!(
        crate::turn_cache::package(&[1; 32]).is_none(),
        "a compiled member drops unused packages"
    );

    let mut artifacts = Vec::new();
    for (name, artifact, expected) in [
        ("first", &first, &first_entity),
        ("forced-second", &control, &first_entity),
        ("third", &changed, &changed_entity),
        ("retained", &retained, &stock_entity),
    ] {
        let path = artifact.write_new(&output.join(name)).unwrap();
        let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        assert_eq!(&package.read_entry(0).unwrap(), expected);
        artifacts.push(serde_json::json!({"stage": name, "file": path, "digest": crate::artifact::digest_file(&path).unwrap()}));
    }
    let checked = check_references(&stock_bank, &stock_entity, &grown_bank, &changed_entity);
    fs::write(output.join("bank.bin"), &grown_bank).unwrap();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "source_revision": std::env::var("SUNDIAL_TEST_REVISION").ok(),
        "executable": crate::artifact::digest_file(&std::env::current_exe().unwrap()).unwrap(),
        "stock_packages": source, "artifacts": artifacts, "checked_references": checked,
        "bank_digest": crate::artifact::digest_bytes(&grown_bank),
        "limits": ["Isolated family pass protocol with native payloads", "No full recipe scheduling or live loader execution"]
    })).unwrap()).unwrap();
}

fn check_references(
    stock_bank: &[u8],
    stock_entity: &[u8],
    grown_bank: &[u8],
    changed_entity: &[u8],
) -> usize {
    // Independent referrer readback follows the emitted offsets into the final bank.
    let instance = |bytes: &[u8]| {
        (0x18i64 + i64::from_le_bytes(bytes[0x18..0x20].try_into().unwrap())) as usize
    };
    let displacement = instance(grown_bank) - instance(stock_bank);
    let mut checked = 0;
    for at in (0..stock_entity.len().saturating_sub(15)).step_by(8) {
        let tag = u32::from_le_bytes(stock_entity[at..at + 4].try_into().unwrap());
        let class = u32::from_le_bytes(stock_entity[at + 4..at + 8].try_into().unwrap());
        let old = u64::from_le_bytes(stock_entity[at + 8..at + 16].try_into().unwrap()) as usize;
        if tag == 0x80B8_075C
            && (0x8080_0000..=0x8080_FFFF).contains(&class)
            && (instance(stock_bank)..stock_bank.len()).contains(&old)
            && old.is_multiple_of(8)
        {
            let new =
                u64::from_le_bytes(changed_entity[at + 8..at + 16].try_into().unwrap()) as usize;
            assert_eq!(new, old + displacement);
            assert!(new < grown_bank.len());
            checked += 1;
        }
    }
    assert!(checked > 0);
    checked
}
