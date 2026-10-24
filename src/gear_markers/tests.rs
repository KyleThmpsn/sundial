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
