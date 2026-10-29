//! Follow the connected status through its retained sequence and private audio resources.
use super::*;
mod bank;
mod persisted;
mod storage;

pub(super) fn evidence(
    source: &Source,
    emitted: &mut Reader,
    action: &Payload,
    recipe: &WeaponRecipe,
) -> CheckResult<Vec<Value>> {
    let mut records = Vec::new();
    for row in action.array(0x38, 8, Some(0x808040AC))? {
        let at = action.pointer(row)?;
        if action.u8(at)? != 1 {
            continue;
        }
        let tag = action.u32(at + 16)?;
        let graph = emitted.tag(tag, Some(0x80809C0F))?;
        let owners = graph.array(16, 12, Some(0x80809C04))?;
        if owners.len() != 3 {
            continue;
        }
        let second = emitted.tag(graph.u32(owners[1])?, Some(0x80809C36))?;
        if second.u32(second.pointer(24)? - 4)? != 0x808084E9 {
            continue;
        }
        let suffix = format!("-{tag:08X}.json");
        let mut matching = Vec::new();
        for path in fs::read_dir(&emitted.output)? {
            let path = path?;
            let name = path.file_name();
            let name = name.to_str().ok_or("Readback artifact name")?;
            if name.starts_with("status-audio-") && name.ends_with(&suffix) {
                matching.push(path.path());
            }
        }
        assert_eq!(
            matching.len(),
            1,
            "Staged sound graph has no independent source readback"
        );
        let mut record: Value = serde_json::from_slice(&fs::read(&matching[0])?)?;
        assert_eq!(record["emitted_graph"].as_u64(), Some(u64::from(tag)));
        record["persisted"] = persisted::check(source, emitted, recipe, &record)?;
        fs::write(&matching[0], serde_json::to_vec_pretty(&record)?)?;
        records.push(record);
    }
    Ok(records)
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source_tag: u32,
    original: &Payload,
    graph_tag: u32,
) -> CheckResult<()> {
    let source_rows = original.array(32, 12, Some(0x80800715))?;
    assert_eq!(source_rows.len(), 3);
    let source_status = source.read(original.u32(source_rows[0])?)?;
    let source_sequence = source.read(original.u32(source_rows[1])?)?;
    let source_network = source.read(original.u32(source_rows[2])?)?;
    assert_eq!(
        source_status.u32(source_status.pointer(24)? - 4)?,
        0x80802698
    );
    assert_eq!(
        source_sequence.u32(source_sequence.pointer(24)? - 4)?,
        0x80800610
    );
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(graph_tag))
            .is_none()
    );
    let graph = emitted.tag(graph_tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), source_rows.len());
    let mut owner_tags = Vec::new();
    for &row in &owners {
        let tag = graph.u32(row)?;
        assert!(stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none());
        assert_eq!(graph.bytes::<8>(row + 4)?, [0; 8]);
        assert!(
            !owner_tags.contains(&tag),
            "Two roles share one mutable owner"
        );
        owner_tags.push(tag);
    }
    let status_owner = emitted.tag(owner_tags[0], Some(0x80809C36))?;
    let sequence_owner = emitted.tag(owner_tags[1], Some(0x80809C36))?;
    status_fields(
        source,
        stock,
        emitted,
        &source_status,
        &graph,
        &status_owner,
        3,
    )?;
    network::check(
        source,
        stock,
        emitted,
        &source_network,
        original,
        graph_tag,
        2,
    )?;
    let source_partition = replication::partition(original, &source_rows, false)?;
    let target_partition = replication::partition(&graph, &owners, true)?;
    assert_eq!(source_partition, [17, 12, 7]);
    assert_eq!(target_partition, [20, 12, 7]);
    for (payload, shift) in [(original, 16), (graph.as_ref(), 0)] {
        assert_eq!(payload.bytes::<16>(0x30 + shift)?, [0; 16]);
        let masks = payload.array(0x78 + shift, 2, Some(0x80800006))?;
        assert_eq!(masks.len(), 2);
        for at in masks {
            assert_eq!(payload.u16(at)?, 0);
        }
    }
    connections(
        original,
        &source_status,
        &source_sequence,
        &graph,
        &status_owner,
        &sequence_owner,
    )?;
    let (source_cue, cue) = sequence(&source_sequence, &sequence_owner, owner_tags[1])?;
    let (audio, mut required) = bank::check(source, stock, emitted, source_cue, cue)?;

    // A complete original three-owner graph supplies its actual allocations and opaque
    // replication rows. Following its backlink is independent of the authored patches.
    let carrier_tag = 0x80BC5833;
    let carrier = stock.tag(carrier_tag, Some(0x80809C0F))?;
    let carrier_owners = carrier.array(16, 12, Some(0x80809C04))?;
    assert_eq!(carrier_owners.len(), owners.len());
    let original_replica_tag = carrier.u32(0x88)?;
    let original_replica = stock.tag(original_replica_tag, Some(0x80809BB6))?;
    let original_replica_rows = original_replica.array(0x30, 32, Some(0x80809BB8))?;
    let replica_tag = graph.u32(0x88)?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(replica_tag))
            .is_none()
    );
    let replica = emitted.tag(replica_tag, Some(0x80809BB6))?;
    assert_eq!(replica.u64(0)?, replica.0.len() as u64);
    assert_eq!(replica.u32(8)?, graph_tag);
    assert_eq!(replica.bytes::<36>(12)?, [0; 36]);
    let replica_rows = replica.array(0x30, 32, Some(0x80809BB8))?;
    assert_eq!(replica_rows.len(), owners.len());
    assert_eq!(replica_rows.len(), original_replica_rows.len());
    required.extend([graph_tag, replica_tag]);
    let mut unchanged = BTreeMap::from([
        (carrier_tag, carrier.0.clone()),
        (original_replica_tag, original_replica.0.clone()),
    ]);
    let mut records = Vec::new();
    for (ordinal, &tag) in owner_tags.iter().enumerate() {
        let target = emitted.tag(tag, Some(0x80809C36))?;
        let original_tag = carrier.u32(carrier_owners[ordinal])?;
        let witness = stock.tag(original_tag, Some(0x80809C36))?;
        if ordinal == 1 {
            let definition = witness.pointer(24)?;
            let events = witness.array(definition + 0x168, 24, Some(0x808093E6))?;
            assert_eq!(events.len(), 1);
            let event = witness.pointer(events[0] + 16)?;
            let cue_tag = witness.u32(event + 0x40)?;
            let cue = stock.tag(cue_tag, Some(0x80809802))?;
            let mut dependencies = BTreeSet::from([cue_tag]);
            for field in [0x10, 0x14, 0x28] {
                dependencies.insert(cue.u32(field)?);
            }
            for field in cue.array(0x18, 4, Some(0x80800014))? {
                let tag = cue.u32(field)?;
                if tag != u32::MAX {
                    dependencies.insert(tag);
                }
            }
            for tag in dependencies {
                unchanged.insert(tag, stock.tag(tag, None)?.0.clone());
            }
        }
        let live = target.pointer(16)?;
        let definition = target.pointer(24)?;
        assert_eq!(target.pointer(8)?, witness.pointer(8)?);
        assert_eq!(
            (live, definition, target.0.len()),
            (witness.pointer(16)?, witness.pointer(24)?, witness.0.len())
        );
        assert_eq!(target.bytes::<16>(0x48)?, witness.bytes::<16>(0x48)?);
        for at in [live, definition] {
            assert_eq!(target.u32(at)?, tag);
            assert_eq!(target.u32(at - 4)?, witness.u32(at - 4)?);
        }
        assert_eq!(target.u64(live + 8)?, definition as u64);
        assert_eq!(target.u64(definition + 8)?, live as u64);
        assert_eq!(
            replication::providers(&graph, tag)?,
            replication::providers(&carrier, original_tag)?
        );
        let allocation = target.u32(0x44)?;
        assert_eq!(allocation, witness.u32(0x44)?);
        let allocation_before = stock.tag(allocation, Some(0x80809BBB))?;
        assert_eq!(
            emitted.tag(allocation, Some(0x80809BBB))?.0,
            allocation_before.0
        );
        assert_eq!(
            replica.bytes::<32>(replica_rows[ordinal])?,
            original_replica.bytes::<32>(original_replica_rows[ordinal])?
        );
        assert_eq!(replica.u32(replica_rows[ordinal])?, target.u32(live - 4)?);
        assert_eq!(replica.u32(replica_rows[ordinal] + 4)?, allocation);
        let lifecycle = target.array(0x30, 16, Some(0x808091A4))?;
        let prior = witness.array(0x30, 16, Some(0x808091A4))?;
        assert_eq!(lifecycle.len(), prior.len());
        assert!(!lifecycle.is_empty());
        for (&row, &before) in lifecycle.iter().zip(&prior) {
            assert_eq!(target.pointer(row)?, witness.pointer(before)?);
            assert_eq!(target.bytes::<8>(row + 8)?, witness.bytes::<8>(before + 8)?);
        }
        required.extend([tag, allocation]);
        unchanged.insert(original_tag, witness.0.clone());
        unchanged.insert(allocation, allocation_before.0.clone());
        records.push(
            json!({"ordinal":ordinal,"owner":tag,"allocation":allocation,
            "sha256":hex::encode(Sha256::digest(&target.0)),"lifecycle_rows":lifecycle.len()}),
        );
    }
    for row in graph.array(0x58, 40, Some(0x80809C22))? {
        let tag = graph.u32(row + 32)?;
        let before = stock.tag(tag, Some(0x80809C54))?;
        assert_eq!(emitted.tag(tag, Some(0x80809C54))?.0, before.0);
        required.insert(tag);
        unchanged.insert(tag, before.0.clone());
    }
    let companion = replication::loading(emitted, stock, replica_tag, &required)?;
    for (tag, before) in unchanged {
        assert_eq!(
            emitted.tag(tag, None)?.0,
            before,
            "Stock sound carrier changed at {tag:08X}"
        );
    }
    fs::write(
        emitted.output.join(format!(
            "status-audio-{source_tag:08X}-{graph_tag:08X}.json"
        )),
        serde_json::to_vec_pretty(&json!({"source_graph":source_tag,"emitted_graph":graph_tag,
            "source_cue":source_cue,"cue":cue,"replication":replica_tag,"loading_companion":companion,
            "source_exports":source_partition,"emitted_exports":target_partition,"owners":records,
            "audio":audio,"required_resources":required,
            "limits":"Persisted recipe and staged package readback. Playback, activation, cancellation, expiry and cleanup execution remain gameplay checks."}))?,
    )?;
    Ok(())
}

fn connections(
    original: &Payload,
    source_status: &Payload,
    source_sequence: &Payload,
    graph: &Payload,
    status: &Payload,
    sequence: &Payload,
) -> CheckResult<()> {
    let from = original.array(0x30, 88, Some(0x8080054E))?;
    assert_eq!(from.len(), 1);
    let from = from[0];
    assert_eq!(original.u32(from)?, 0x811C9DC5);
    assert_eq!(
        original.u32(from + 8)?,
        source_status.u32(source_status.pointer(16)?)?
    );
    assert_eq!(
        original.u64(from + 16)?,
        (source_status.pointer(24)? + 0xB8) as u64
    );
    assert_eq!(original.u32(from + 24)?, 0x80800456);
    assert_eq!(original.u64(from + 32)?, 0);
    assert_eq!(original.u32(from + 40)?, 0x811C9DC5);
    assert_eq!(
        original.u32(from + 48)?,
        source_sequence.u32(source_sequence.pointer(16)?)?
    );
    assert_eq!(
        original.u64(from + 56)?,
        (source_sequence.pointer(24)? + 0xA8) as u64
    );
    assert_eq!(original.u32(from + 64)?, 0x80800988);
    assert_eq!(original.u64(from + 72)?, 1);
    assert_eq!(original.u64(from + 80)?, 0);
    let rows = graph.array(0x20, 72, Some(0x80809BC9))?;
    assert_eq!(rows.len(), 2);
    let mut receivers = BTreeSet::new();
    for row in rows {
        assert_eq!(graph.u32(row)?, 0x811C9DC5);
        assert_eq!(graph.u32(row + 8)?, status.u32(status.pointer(16)?)?);
        assert_eq!(graph.u32(row + 12)?, 0x808092F4);
        assert_eq!(graph.u64(row + 24)?, 0);
        assert_eq!(graph.u32(row + 32)?, 0x811C9DC5);
        assert_eq!(graph.u64(row + 64)?, 0);
        let offset = usize::try_from(graph.u64(row + 16)?)? - status.pointer(24)?;
        assert!(receivers.insert(offset));
        match offset {
            0x60 => {
                assert_eq!(graph.u32(row + 40)?, sequence.u32(sequence.pointer(16)?)?);
                assert_eq!(graph.u32(row + 44)?, 0x808092F8);
                assert_eq!(graph.u64(row + 48)?, (sequence.pointer(24)? + 0x90) as u64);
                assert_eq!(graph.u64(row + 56)?, 1);
            }
            0x3B0 => {
                assert_eq!(graph.bytes::<8>(row + 40)?, [255; 8]);
                assert_eq!(graph.u64(row + 48)?, 0);
                assert_eq!(graph.u64(row + 56)?, u16::MAX as u64);
            }
            _ => return Err("Unexpected status execution output".into()),
        }
    }
    assert_eq!(receivers, BTreeSet::from([0x60, 0x3B0]));
    Ok(())
}

fn sequence(source: &Payload, target: &Payload, tag: u32) -> CheckResult<(u32, u32)> {
    let (s, n) = (source.pointer(24)?, target.pointer(24)?);
    assert_eq!(target.u32(n - 4)?, 0x808084E9);
    assert_eq!(source.u32(s + 0x2C0)?, 0);
    assert_eq!(target.u32(n + 0x2D0)?, 0);
    assert_eq!(source.u32(s + 0x2C4)?, 0);
    assert_eq!(source.u64(s + 0x2C8)?, 0);
    assert_eq!(target.u64(n + 0x2D8)?, 0);
    assert_eq!(
        source.bytes::<8>(s + 0x2D0)?,
        [0, 0x20, 0x25, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        target.bytes::<8>(n + 0x2E0)?,
        [8, 0x20, 0x25, 0, 0, 0, 0, 0]
    );
    for (from, to) in [(0x170, 0x178), (0x190, 0x198), (0x1A0, 0x188)] {
        assert_eq!(source.u64(s + from)?, 0);
        assert_eq!(target.u64(n + to)?, 0);
    }
    let controls = source.array(s + 0x160, 8, Some(0x80800629))?;
    let events = source.array(s + 0x180, 8, Some(0x80800629))?;
    let native_controls = target.array(n + 0x158, 24, Some(0x808093E6))?;
    let native_events = target.array(n + 0x168, 24, Some(0x808093E6))?;
    assert_eq!(
        (
            controls.len(),
            events.len(),
            native_controls.len(),
            native_events.len()
        ),
        (1, 1, 1, 1)
    );
    let control = source.pointer(controls[0])?;
    let event = source.pointer(events[0])?;
    let native_control = target.pointer(native_controls[0] + 16)?;
    let native_event = target.pointer(native_events[0] + 16)?;
    for (from, to, source_class, native_class, runtime_class) in [
        (control, native_control, 0x80800691, 0x808093D7, 0x808093D6),
        (event, native_event, 0x80801F00, 0x80806B38, 0x80806B37),
    ] {
        assert_eq!(source.u32(from - 4)?, source_class);
        assert_eq!(target.u32(to - 4)?, native_class);
        assert_eq!(source.bytes::<28>(from)?, target.bytes::<28>(to + 16)?);
        let live = usize::try_from(target.u64(to + 8)?)?;
        assert_eq!(target.u32(to)?, tag);
        assert_eq!(target.u32(to + 4)?, runtime_class);
        assert_eq!(target.u32(live)?, tag);
        assert_eq!(target.u32(live - 4)?, runtime_class);
        assert_eq!(target.u32(live + 4)?, native_class);
        assert_eq!(target.u64(live + 8)?, to as u64);
    }
    let children = source.array(control + 32, 4, Some(0x8080052C))?;
    let native_children = target.array(native_control + 56, 4, Some(0x808093FB))?;
    assert_eq!((children.len(), native_children.len()), (1, 1));
    assert_eq!(source.bytes::<4>(children[0])?, [2, 0, 0, 0]);
    assert_eq!(target.bytes::<4>(native_children[0])?, [1, 0, 0, 0]);
    assert_eq!(source.u32(event + 0x1C)?, 2);
    assert_eq!(target.u32(native_event + 0x30)?, 6);
    assert_eq!(source.u64(event + 0x28)?, 0);
    assert_eq!(target.u64(native_event + 0x48)?, 0);
    assert_eq!(source.bytes::<16>(event + 0x38)?, [0; 16]);
    assert_eq!(target.bytes::<16>(native_event + 0x60)?, [0; 16]);
    assert_eq!(source.u8(event + 0x48)?, 0);
    assert_eq!(target.u8(native_event + 0x70)?, 0);
    let live = usize::try_from(target.u64(native_event + 8)?)?;
    assert_eq!(target.u32(live + 0x40)?, u32::MAX);
    for (offset, source_width, native_width, source_bytes, native_bytes) in [
        (
            0x1B0,
            5,
            6,
            &[0, 1, 0, 1, 0x1F][..],
            &[0, 1, 0, 1, 0x3F, 0][..],
        ),
        (
            0x1C0,
            4,
            8,
            &[0, 0, 1, 0x1F][..],
            &[0, 0, 1, 0, 0x3F, 0, 0, 0][..],
        ),
        (
            0x1D0,
            8,
            8,
            &[0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0][..],
            &[0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0][..],
        ),
    ] {
        let from = source.array(s + offset, source_width, None)?;
        let to = target.array(n + offset, native_width, None)?;
        assert_eq!((from.len(), to.len()), (1, 1));
        assert_eq!(&source.0[from[0]..from[0] + source_width], source_bytes);
        assert_eq!(&target.0[to[0]..to[0] + native_width], native_bytes);
    }
    Ok((source.u32(event + 0x20)?, target.u32(native_event + 0x40)?))
}
