//! Persisted seed recovery and the pass protocol. Failure cases are recorded before implementation.
use super::*;

mod packages;

fn fixture() -> Session {
    let owner = Importer::D1Roi;
    let own = Own {
        host: Some(0x0CFC),
        ..Own::default()
    };
    Session {
        plan: Plan {
            seed_inputs: "two recipes and their complete authoring inputs".into(),
            recipe_owners: vec![Some(owner), Some(owner)],
            imports: BTreeMap::from([(
                owner,
                Planned {
                    inputs: "prepared graph and stock fingerprints".into(),
                    source_inputs: Vec::new(),
                    installed: None,
                    kept: false,
                    previous: own.clone(),
                },
            )]),
            ..Plan::default()
        },
        decisions: Decisions {
            host: true,
            contributions: BTreeMap::from([(
                owner,
                Contributions {
                    characters: vec![HostedCharacter {
                        folder: PathBuf::from("removed-graph"),
                        clips: vec![TagHash::new(0x0CFB, 0).0],
                        digest: [0xAB; 32],
                    }],
                    collections: BTreeMap::from([(12, 2)]),
                    ..Contributions::default()
                },
            )]),
            ..Decisions::default()
        },
        pass: Pass {
            main_builds: true,
            imports: BTreeMap::from([(
                owner,
                Taken {
                    allocation: vec![0x0CFB],
                    used: BTreeSet::from([0x0CFB]),
                    state: State {
                        format: FORMAT,
                        own,
                        ..State::default()
                    },
                    conversions: 0,
                },
            )]),
            ..Pass::default()
        },
    }
}

#[test]
fn seed_reload_requires_the_request_and_allocations_that_produced_its_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("family.json");
    let current = fixture();
    seed::Seed::of(&current).save(&path);
    let load = |plan: &Plan, pass: &Pass| seed::Seed::load(&path, plan, pass);
    assert!(!load(&current.plan, &current.pass).contributions.is_empty());

    // One source still has content after its character recipe is removed.
    let mut removed = current.plan.clone();
    removed.recipe_owners.pop();
    removed.seed_inputs = "the remaining recipe".into();
    assert!(load(&removed, &current.pass).contributions.is_empty());

    let mut reallocated = current.pass.clone();
    reallocated
        .imports
        .get_mut(&Importer::D1Roi)
        .unwrap()
        .allocation = vec![0x0CFA];
    assert!(load(&current.plan, &reallocated).contributions.is_empty());
    let mut records = current.plan.clone();
    records
        .imports
        .get_mut(&Importer::D1Roi)
        .unwrap()
        .previous
        .host = Some(0x0CF9);
    assert!(load(&records, &current.pass).contributions.is_empty());

    // Old compatible-format files have no proof for allocated rows.
    let mut old: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("inputs");
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(load(&current.plan, &current.pass).contributions.is_empty());
}

#[test]
fn failed_seed_is_discarded_once_without_losing_fresh_collections_counts() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("family.json");
    let mut current = fixture();
    seed::Seed::of(&current).save(&path);
    current.decisions = seed::Seed::load(&path, &current.plan, &current.pass);
    // Planning derives the host from current content instead of persisting it in the seed.
    current.decisions.host = current.plan.importers_have_content();
    let mut family = Family::of(&current);
    family.expect_collections(&BTreeMap::from([(
        Importer::D1Roi,
        Census {
            fronts: BTreeMap::from([(7, 1)]),
            counts: BTreeMap::from([(12, 1)]),
        },
    )]));
    assert!(family.discard_seed());
    assert!(!family.discard_seed());
    let kept = &family.decisions.contributions[&Importer::D1Roi];
    assert!(kept.characters.is_empty());
    assert_eq!(kept.collections, BTreeMap::from([(12, 1)]));
    assert_eq!(
        family.decisions.fronts,
        BTreeMap::from([(
            (7, crate::progression::PRESENTATION_NODE_COLLECTIBLES_OFFSET),
            1
        )])
    );
}

#[test]
fn collections_overflow_merges_without_holes_or_losing_stock_entries() {
    use crate::progression::{self as p, presentation};
    use crate::tag_payload::{write_relative_pointer, write_u16, write_u32, write_u64};
    let host = crate::package::profile::Host::standalone(0x0CFE);
    let _turn = crate::package::profile::hold(crate::package::profile::Window::FULL, host);
    let family = Family::of(&fixture());
    family.start_pass(true, BTreeMap::new);
    let mut stock = vec![0; 24 + 16 + p::PRESENTATION_NODE_ROW_SIZE];
    write_u64(&mut stock, 8, 1).unwrap();
    write_relative_pointer(&mut stock, 16, 24).unwrap();
    write_u64(&mut stock, 24, 1).unwrap();
    write_u32(&mut stock, 32, p::PRESENTATION_NODE_DEFINITION_ROW_CLASS).unwrap();
    let descriptor = 40 + p::PRESENTATION_NODE_COLLECTIBLES_OFFSET;
    stock.resize(stock.len().div_ceil(16) * 16, 0);
    let header = stock.len();
    stock.resize(
        header + 16 + 2 * p::PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
        0,
    );
    write_u64(&mut stock, descriptor, 2).unwrap();
    write_relative_pointer(&mut stock, descriptor + 8, header).unwrap();
    write_u64(&mut stock, header, 2).unwrap();
    write_u32(
        &mut stock,
        header + 8,
        p::PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS,
    )
    .unwrap();
    write_u16(&mut stock, header + 16, 10).unwrap();
    write_u16(
        &mut stock,
        header + 16 + p::PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
        11,
    )
    .unwrap();
    presentation::append_presentation_child_array_terminator(&mut stock).unwrap();
    let row = |id: u16| {
        let mut row = [0; p::PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE];
        row[..2].copy_from_slice(&id.to_le_bytes());
        row
    };
    let read = |data: &[u8]| {
        // Read the emitted descriptor and child rows directly, separately from the writer.
        let pointer = i64::from_le_bytes(data[descriptor + 8..descriptor + 16].try_into().unwrap());
        let header = (descriptor as i64 + 8 + pointer) as usize;
        let count =
            u64::from_le_bytes(data[descriptor..descriptor + 8].try_into().unwrap()) as usize;
        (0..count)
            .map(|i| {
                let at = header + 16 + i * p::PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE;
                u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
            })
            .collect::<Vec<_>>()
    };
    let mut main = stock.clone();
    assert_eq!(
        front(0, p::PRESENTATION_NODE_COLLECTIBLES_OFFSET, 1, 2).row,
        0
    );
    presentation::insert_collectible_children(&mut main, 0, 0, &[row(100)]).unwrap();
    presentation::insert_collectible_children(&mut main, 0, 1, &[row(11)]).unwrap();
    let mut first = stock.clone();
    crate::package::profile::begin_partition(host, Some(Importer::HaloReach)).unwrap();
    let slot = front(0, p::PRESENTATION_NODE_COLLECTIBLES_OFFSET, 2, 2);
    presentation::write_collectible_children_from(
        &mut first,
        0,
        &[row(200), row(201)],
        slot.row,
        slot.stock,
    )
    .unwrap();
    let mut second = stock.clone();
    crate::package::profile::begin_partition(host, Some(Importer::D1Roi)).unwrap();
    let slot = front(0, p::PRESENTATION_NODE_COLLECTIBLES_OFFSET, 1, 2);
    presentation::write_collectible_children_from(
        &mut second,
        0,
        &[row(300)],
        slot.row,
        slot.stock,
    )
    .unwrap();
    let slot = front(0, p::PRESENTATION_NODE_COLLECTIBLES_OFFSET, 1, 3);
    presentation::write_collectible_children_from(
        &mut second,
        0,
        &[row(301)],
        slot.row,
        slot.stock,
    )
    .unwrap();
    assert_eq!(read(&second), vec![10, 11, 300, 301]);

    let original = read(&stock);
    let members = [read(&main), read(&first), read(&second)];
    for order in [
        [0, 1, 2],
        [2, 1, 0],
        [1, 0, 2],
        [1, 2, 0],
        [0, 2, 1],
        [2, 0, 1],
    ] {
        // Independent stock-row merge, followed by each mod's appended suffix in load order.
        let mut merged = original.clone();
        for index in order {
            for row in 0..original.len() {
                if members[index][row] != original[row] {
                    assert!(merged[row] == original[row] || merged[row] == members[index][row]);
                    merged[row] = members[index][row];
                }
            }
            merged.extend_from_slice(&members[index][original.len()..]);
        }
        assert_eq!(&merged[..2], &[100, 200]);
        merged.sort_unstable();
        assert_eq!(merged, vec![10, 11, 100, 200, 201, 300, 301]);
    }
}
