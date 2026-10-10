use super::*;
use crate::hash::fnv1_name_hash as fnv1;

/// A recovered name is only a name if it hashes to the key it is filed under. This is what
/// makes the table safe to extend by hand.
#[test]
fn every_recovered_name_hashes_to_its_key() {
    assert!(names::NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(
        names::PATH_NAMES
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0)
    );
    for (hash, name) in names::NAMES.iter().chain(names::PATH_NAMES) {
        assert_eq!(fnv1(name), *hash, "{name}");
    }
    let hashes: BTreeSet<_> = names::NAMES
        .iter()
        .chain(names::PATH_NAMES)
        .map(|(hash, _)| *hash)
        .collect();
    assert_eq!(
        hashes.len(),
        names::NAMES.len() + names::PATH_NAMES.len(),
        "duplicate key in the table"
    );
    assert_eq!(marker_name(0xF2A0_71CC), Some("primary_fire"));
    assert_eq!(marker_name(0), None);
}

/// A hash says nothing on its own. Naming what it sits next to is the whole point of the
/// explorer, so an unnamed marker has to come back described by a named neighbour.
#[test]
fn an_unnamed_marker_is_described_by_what_it_sits_beside() {
    let at = |name: u32, position: [f32; 3]| Marker {
        name,
        position,
        orientation: [0.0, 0.0, 0.0, 1.0],
    };
    let sets = vec![MarkerSet {
        entity: 0x8087_1F2A,
        component: 0x8161_ECC9,
        markers: vec![
            at(fnv1("primary_fire"), [0.3806, 0.0, 0.1358]),
            at(0x76BC_E859, [0.3717, 0.0, 0.1421]),
            at(fnv1("grip"), [0.1, 0.0, 0.0]),
            // Far enough away that nothing on the weapon is "near" it.
            at(0x0B7B_A45D, [9.0, 9.0, 9.0]),
        ],
    }];
    let marker = sets[0].markers[1];
    let near = nearest_named(&sets, &marker).unwrap();
    // `grip` is also named but much further off, so the nearer one wins.
    assert_eq!(near.name, "primary_fire");
    assert!((near.distance - 0.0109).abs() < 1e-3, "{near:?}");
    assert_eq!(describe(&sets, &marker), "0x76BCE859 by primary_fire 11mm");
    // Markers that share a point are common, and "0mm" reads as a missing number.
    let stacked = at(0x1234_5678, [0.3806, 0.0, 0.1358]);
    assert_eq!(
        nearest_named(&sets, &stacked).unwrap().to_string(),
        "on primary_fire"
    );
    // A named marker describes itself and has no neighbour.
    assert_eq!(nearest_named(&sets, &sets[0].markers[0]), None);
    assert_eq!(describe(&sets, &sets[0].markers[0]), "primary_fire");
    // Nothing within reach means no claim is made at all.
    assert_eq!(nearest_named(&sets, &sets[0].markers[3]), None);
    assert_eq!(describe(&sets, &sets[0].markers[3]), "0x0B7BA45D");
}

/// Builds one marker-set component: a data struct at 0x20 whose descriptor at +0xB0 owns a
/// 16-byte array header followed by fixed-stride rows.
fn component(markers: &[(u32, [f32; 3])]) -> Vec<u8> {
    let data = 0x20usize;
    let descriptor = data + MARKER_DESCRIPTOR;
    let header = descriptor + 0x30;
    let rows = header + 16;
    let mut bytes = vec![0u8; rows + MARKER_STRIDE * markers.len()];
    let count = markers.len() as u64;
    write_bytes(&mut bytes, 0x18, &(data as i64 - 0x18).to_le_bytes()).unwrap();
    write_bytes(&mut bytes, data - 4, &MARKER_COMPONENT.to_le_bytes()).unwrap();
    write_bytes(&mut bytes, descriptor, &count.to_le_bytes()).unwrap();
    let delta = header as i64 - (descriptor + 8) as i64;
    write_bytes(&mut bytes, descriptor + 8, &delta.to_le_bytes()).unwrap();
    write_bytes(&mut bytes, header, &count.to_le_bytes()).unwrap();
    write_bytes(&mut bytes, header + 8, &MARKER_ROW.to_le_bytes()).unwrap();
    for (index, (name, position)) in markers.iter().enumerate() {
        let row = rows + index * MARKER_STRIDE;
        for (axis, value) in position.iter().enumerate() {
            write_bytes(
                &mut bytes,
                row + MARKER_POSITION + axis * 4,
                &value.to_le_bytes(),
            )
            .unwrap();
        }
        write_bytes(&mut bytes, row + MARKER_NAME, &name.to_le_bytes()).unwrap();
    }
    bytes
}

#[test]
fn a_damaged_marker_component_is_refused_rather_than_read_past() {
    let bytes = component(&[(fnv1("grip"), [0.0; 3])]);
    assert!(read_component(&bytes[..bytes.len() - 1], 0x20).is_err());
    assert!(read_component(&bytes, usize::MAX).is_err());
    let header = 0x20 + MARKER_DESCRIPTOR + 0x30;
    let mut wrong_class = bytes.clone();
    write_bytes(&mut wrong_class, header + 8, &0x8080_0000u32.to_le_bytes()).unwrap();
    assert!(read_component(&wrong_class, 0x20).is_err());
    let mut infinite = bytes;
    write_bytes(
        &mut infinite,
        header + 16 + MARKER_POSITION,
        &f32::INFINITY.to_le_bytes(),
    )
    .unwrap();
    assert!(read_component(&infinite, 0x20).is_err());
}

#[test]
fn an_empty_marker_set_is_not_an_error() {
    assert_eq!(read_component(&component(&[]), 0x20), Ok(Vec::new()));
}

/// The fixtures above cannot prove the walk reaches a real weapon's markers: the
/// arrangement, assignment and relation chain only exists in installed packages.
#[test]
#[ignore = "requires installed Shadowkeep packages"]
fn installed_weapons_carry_named_markers() {
    let packages = crate::test_support::preview_packages();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let mut failures = Vec::new();
    for name in [
        "Better Devils",
        "Age-Old Bond",
        "Chroma Rush",
        "Black Talon",
    ] {
        let Some(summary) = catalog.weapon_donors().into_iter().find(|d| d.name == name) else {
            continue;
        };
        let donor = catalog.weapon_donor(summary.hash).unwrap();
        let arrangement = donor.art_arrangements[0].arrangement;
        let sets = match read_appearance(&packages, arrangement) {
            Ok(sets) => sets,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        let markers: Vec<_> = sets.iter().flat_map(|set| &set.markers).collect();
        let labels: Vec<_> = markers.iter().map(|marker| marker.label()).collect();
        println!("{name} arrangement {arrangement}:");
        for (set_index, set) in sets.iter().enumerate() {
            for marker in &set.markers {
                let [x, y, z] = marker.position;
                println!(
                    "    obj{set_index} {:<22} {x:>9.4} {y:>9.4} {z:>9.4}",
                    marker.label()
                );
            }
        }
        if markers.is_empty() {
            failures.push(format!("{name}: no markers"));
        } else if !labels.iter().any(|label| !label.starts_with("0x")) {
            failures.push(format!("{name}: no marker resolved to a name"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The index behind the explorer, against real packages. The fixtures prove the
/// arithmetic; this proves the whole-game walk produces rows worth showing.
#[test]
#[ignore = "requires installed Shadowkeep packages"]
#[allow(clippy::cognitive_complexity)]
fn the_marker_index_describes_the_unnamed_ones() {
    let packages = crate::test_support::preview_packages();
    let manager = crate::investment::discovery::open_packages(&packages).unwrap();
    let ticks = AtomicUsize::new(0);
    let index = build_index(&manager, |_, _| {
        ticks.fetch_add(1, Ordering::Relaxed);
    });
    let ticks = ticks.into_inner();
    println!(
        "{} objects read, {} carry markers, {} marker sets, {} names, {} recovered",
        index.scanned,
        index.objects.len(),
        index.sets,
        index.entries.len(),
        index.named()
    );
    assert!(ticks > 1, "progress must be reported while reading");
    assert!(!index.entries.is_empty());
    assert!(index.named() > 0);
    // The reverse lookup: every object listed carries markers, and every marker's examples
    // resolve back to an object that really holds it.
    assert!(!index.objects.is_empty() && index.objects.len() <= index.scanned);
    assert!(index.objects.iter().all(|o| !o.markers.is_empty()));
    assert!(
        index.objects.windows(2).all(|w| w[0].entity < w[1].entity),
        "objects must be sorted for lookup"
    );
    for entry in index.entries.iter().take(50) {
        for example in &entry.examples {
            assert!(
                index.placement(*example, entry.hash).is_some(),
                "{:08X} is not on its example {example:08X}",
                entry.hash
            );
        }
    }
    // Sorted by reach, so the first rows are the ones worth naming next.
    assert!(
        index
            .entries
            .windows(2)
            .all(|w| w[0].objects >= w[1].objects)
    );
    let described = index
        .entries
        .iter()
        .filter(|entry| entry.name.is_none() && entry.neighbour.is_some())
        .count();
    let unnamed = index.entries.iter().filter(|e| e.name.is_none()).count();
    println!("{described} of {unnamed} unnamed markers have a named neighbour");
    for entry in index.entries.iter().take(20) {
        println!(
            "  {:<40} {:>5} objects  examples {:08X?}",
            entry.label(),
            entry.objects,
            entry.examples.iter().take(3).collect::<Vec<_>>()
        );
    }
    for entry in &index.entries {
        assert!(!entry.examples.is_empty());
        assert!(entry.examples.len() <= MAX_EXAMPLES);
    }
}

fn carrier(entity: u32, sets: usize, markers: &[(u32, [f32; 3])]) -> Carrier {
    Carrier {
        entity,
        sets,
        markers: markers
            .iter()
            .map(|&(name, position)| Marker {
                name,
                position,
                orientation: [0.0, 0.0, 0.0, 1.0],
            })
            .collect(),
    }
}

#[test]
fn marker_scan_schedules_each_package_once() {
    let tags: Vec<u32> = (0..600)
        .map(|index| tiger_pkg::TagHash::new(0x123, index).0)
        .chain((0..300).map(|index| tiger_pkg::TagHash::new(0x456, index).0))
        .collect();
    let jobs = package_jobs(&tags);
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0], tags[..600]);
    assert_eq!(jobs[1], tags[600..]);
}

/// The index is derived from what the disk keeps, so a scan that went through the cache
/// file must describe the game exactly as the scan that was written.
#[test]
fn a_scan_read_back_from_the_cache_gives_the_same_index() {
    let named = crate::hash::fnv1_name_hash("primary_fire");
    assert!(marker_name(named).is_some());
    let scan = Scan {
        scanned: 40,
        carriers: vec![
            carrier(
                0x8080_0001,
                2,
                &[(named, [0.0, 0.0, 0.0]), (0x1234_5678, [0.0, 0.011, 0.0])],
            ),
            carrier(
                0x8080_0002,
                1,
                &[
                    (0x1234_5678, [0.1, 0.1, 0.1]),
                    (0x1234_5678, [0.2, 0.2, 0.2]),
                ],
            ),
        ],
    };
    let mut bytes = Vec::new();
    crate::package_runtime::cache_file::write(&mut bytes, &scan).unwrap();
    let read: Scan = crate::package_runtime::cache_file::read(&bytes).unwrap();
    let index = index_from(&scan);
    assert_eq!(index_from(&read), index);

    assert_eq!(index.scanned, 40);
    assert_eq!(index.sets, 3);
    assert_eq!(index.objects.len(), 2);
    assert!(index.object(0x8080_0002).is_some());
    // Counted once per object that carries it, however many rows it has there.
    let unnamed = index
        .entries
        .iter()
        .find(|e| e.hash == 0x1234_5678)
        .unwrap();
    assert_eq!(unnamed.objects, 2);
    assert_eq!(unnamed.examples, vec![0x8080_0001, 0x8080_0002]);
    assert_eq!(unnamed.neighbour.map(|n| n.name), Some("primary_fire"));
    assert_eq!(index.entries[0].hash, 0x1234_5678, "the widest name leads");
}

/// The parallel read against real packages: the same objects as reading them one by one,
/// progress that ends at the total, and a cancel that stops it without a result.
#[test]
#[ignore = "requires installed Shadowkeep packages"]
fn the_parallel_read_matches_a_serial_one_and_cancels() {
    let packages = crate::test_support::preview_packages();
    let manager = crate::investment::discovery::open_packages(&packages).unwrap();
    let last = std::sync::Mutex::new((0, 0));
    let started = std::time::Instant::now();
    let parallel = scan(
        &manager,
        &|done, total| {
            let mut last = last.lock().unwrap();
            *last = (last.0.max(done), total);
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    println!("parallel read: {:?}", started.elapsed());
    let (done, total) = *last.lock().unwrap();
    assert!(done > 0);
    assert_eq!(done, total);

    // One object in seven read the old way, one at a time, which is too slow for all.
    let started = std::time::Instant::now();
    let mut serial = Vec::new();
    for (tag, _) in manager.get_all_by_reference(ENTITY).into_iter().step_by(7) {
        if let Ok(sets) = entity_marker_sets(&manager, tag.0)
            && !sets.is_empty()
        {
            let markers: Vec<_> = sets.iter().flat_map(|set| set.markers.clone()).collect();
            serial.push((tag.0, sets.len(), markers));
        }
    }
    println!("serial read of a seventh: {:?}", started.elapsed());
    assert!(!serial.is_empty());
    for (entity, sets, markers) in serial {
        let carrier = parallel
            .carriers
            .binary_search_by_key(&entity, |c| c.entity)
            .map(|at| &parallel.carriers[at])
            .unwrap_or_else(|_| panic!("0x{entity:08X} is missing from the parallel read"));
        assert_eq!(
            (carrier.sets, &carrier.markers),
            (sets, &markers),
            "0x{entity:08X}"
        );
    }
    let sampled: BTreeSet<u32> = manager
        .get_all_by_reference(ENTITY)
        .into_iter()
        .step_by(7)
        .map(|(tag, _)| tag.0)
        .collect();
    for carrier in parallel
        .carriers
        .iter()
        .filter(|c| sampled.contains(&c.entity))
    {
        assert!(
            entity_marker_sets(&manager, carrier.entity).is_ok_and(|sets| !sets.is_empty()),
            "0x{:08X} carries no markers when read alone",
            carrier.entity
        );
    }

    let cancelled = scan(&manager, &|_, _| {}, &AtomicBool::new(true));
    assert_eq!(cancelled.unwrap_err(), CANCELLED);
}
