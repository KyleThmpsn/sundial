//! Follow a complete status, network and modifier attachment after portable reload and staging.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source_tag: u32,
    original: &Payload,
    graph_tag: u32,
) -> CheckResult<()> {
    let source_owners = original.array(32, 12, Some(0x80800715))?;
    assert_eq!(source_owners.len(), 3);
    let source_status = reader.read(original.u32(source_owners[0])?)?;
    let source_network = reader.read(original.u32(source_owners[1])?)?;
    let source_bank = reader.read(original.u32(source_owners[2])?)?;
    for (owner, instance, definition) in [
        (&source_status, 0x80802A75, 0x80802698),
        (&source_network, 0x808006F9, 0x80800362),
        (&source_bank, 0x808021F1, 0x808020DE),
    ] {
        assert_eq!(owner.u32(owner.pointer(16)? - 4)?, instance);
        assert_eq!(owner.u32(owner.pointer(24)? - 4)?, definition);
    }
    let source_rows = source_bank.array(source_bank.pointer(24)? + 96, 96, Some(0x8080205F))?;
    let count = source_rows.len();
    assert!(matches!(count, 1 | 2));
    status(reader, stock, emitted, &source_status, graph_tag, 3)?;
    network::check(
        reader,
        stock,
        emitted,
        &source_network,
        original,
        graph_tag,
        1,
    )?;
    let graph = emitted.tag(graph_tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    let bank_tag = graph.u32(owners[2])?;
    let bank = emitted.tag(bank_tag, Some(0x80809C36))?;
    modifier_rows(reader, stock, &source_bank, &bank, bank_tag, source_rows)?;

    let source_partition = partition(original, &source_owners, false)?;
    let target_partition = partition(&graph, &owners, true)?;
    assert_eq!(source_partition, [17, 7, 2 + 2 * count]);
    assert_eq!(target_partition, [20, 7, 3 + 2 * count]);
    assert_eq!(original.array(0x30, 88, Some(0x8080054E))?.len(), 1);
    assert_eq!(graph.array(0x20, 72, Some(0x80809BC9))?.len(), 2);
    for (payload, shift) in [(original, 16), (graph.as_ref(), 0)] {
        assert_eq!(payload.bytes::<16>(0x30 + shift)?, [0; 16]);
        let masks = payload.array(0x78 + shift, 2, Some(0x80800006))?;
        assert_eq!(masks.len(), 2);
        for at in masks {
            assert_eq!(payload.u16(at)?, 0);
        }
    }

    let replication_tag = graph.u32(0x88)?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(replication_tag))
            .is_none()
    );
    let replication = emitted.tag(replication_tag, Some(0x80809BB6))?;
    let entry = emitted
        .manager
        .get_entry(tiger_pkg::TagHash(replication_tag))
        .ok_or("Private replication entry is absent")?;
    assert_eq!((entry.file_type, entry.file_subtype), (16, 0));
    assert_eq!(replication.u64(0)?, replication.0.len() as u64);
    assert_eq!(replication.u32(8)?, graph_tag);
    assert_eq!(replication.bytes::<36>(12)?, [0; 36]);
    let rows = replication.array(0x30, 32, Some(0x80809BB8))?;
    assert_eq!(rows.len(), owners.len());
    let carrier = stock.tag(0x80F9F7CF, Some(0x80809C0F))?;
    let retained = stock.tag(carrier.u32(0x88)?, Some(0x80809BB6))?;
    let retained_rows = retained.array(0x30, 32, Some(0x80809BB8))?;
    assert_eq!(retained_rows.len(), 3);
    let (witness_tag, bank_row) = bank_witness(stock, emitted, &graph, &bank, bank_tag, count)?;
    let mut required = BTreeSet::from([graph_tag, replication_tag]);
    let mut owner_records = Vec::new();
    for (ordinal, (&owner, &row)) in owners.iter().zip(&rows).enumerate() {
        let tag = graph.u32(owner)?;
        assert!(stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none());
        let payload = emitted.tag(tag, Some(0x80809C36))?;
        let live = payload.pointer(16)?;
        let definition = payload.pointer(24)?;
        let instance_class = payload.u32(live - 4)?;
        assert_eq!(
            instance_class,
            [0x80804209, 0x808090E6, 0x80803AFD][ordinal]
        );
        assert_eq!(payload.u32(live)?, tag);
        assert_eq!(payload.u32(definition)?, tag);
        assert_eq!(payload.u64(live + 8)?, definition as u64);
        assert_eq!(payload.u64(definition + 8)?, live as u64);
        assert_eq!(replication.u32(row)?, instance_class);
        let allocation = payload.u32(0x44)?;
        assert_eq!(replication.u32(row + 4)?, allocation);
        assert_eq!(replication.bytes::<16>(row + 8)?, [0; 16]);
        // These are complete original serialized rows. The allocation-dependent
        // opaque word is not recomputed from the known class and element count.
        let expected = if ordinal == 2 {
            bank_row
        } else {
            retained.bytes::<32>(retained_rows[ordinal])?
        };
        assert_eq!(replication.bytes::<32>(row)?, expected);
        required.extend([tag, allocation]);
        owner_records.push(json!({
            "ordinal": ordinal,
            "owner": format!("{tag:08X}"),
            "instance_class": format!("{instance_class:08X}"),
            "allocation": format!("{allocation:08X}"),
            "replication_row_sha256": hex::encode(Sha256::digest(expected)),
        }));
    }
    for at in graph.array(0x58, 40, Some(0x80809C22))? {
        let metadata = graph.u32(at + 32)?;
        emitted.tag(metadata, Some(0x80809C54))?;
        required.insert(metadata);
    }
    let companion = loading(emitted, stock, replication_tag, &required)?;
    fs::write(
        emitted
            .output
            .join(format!("status-network-bank-{graph_tag:08X}.json")),
        serde_json::to_vec_pretty(&json!({
            "source_graph": format!("{source_tag:08X}"),
            "emitted_graph": format!("{graph_tag:08X}"),
            "replication": format!("{replication_tag:08X}"),
            "loading_companion": format!("{companion:08X}"),
            "modifier_rows": count,
            "source_exports": source_partition,
            "emitted_exports": target_partition,
            "bank_replication_witness": format!("{witness_tag:08X}"),
            "owners": owner_records,
            "limits": "Persisted recipe and staged package readback. Native loading, initialization, cleanup and gameplay are not executed."
        }))?,
    )?;
    Ok(())
}

/// Every export retains its actual owner ordinal and the referenced instance class.
pub(super) fn partition(
    graph: &Payload,
    owners: &[usize],
    native: bool,
) -> CheckResult<Vec<usize>> {
    let shift = if native { 0 } else { 16 };
    let exports = graph.array(0x58 + shift, 40, None)?;
    let references = graph.array(0x68 + shift, 24, None)?;
    assert_eq!(exports.len(), references.len());
    let mut counts = vec![0; owners.len()];
    for (export, reference) in exports.into_iter().zip(references) {
        let tag = graph.u32(export + 16)?;
        let ordinal = owners
            .iter()
            .position(|&row| graph.u32(row).ok() == Some(tag))
            .ok_or("Export has an owner outside the attachment")?;
        assert_eq!(graph.u32(reference)?, tag);
        assert_eq!(graph.u64(reference + 16)?, ordinal as u64);
        assert_eq!(
            graph.u32(reference + 4)?,
            if native { graph.u32(export + 12)? } else { 0 }
        );
        counts[ordinal] += 1;
    }
    Ok(counts)
}

fn bank_witness(
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    bank: &Payload,
    bank_tag: u32,
    count: usize,
) -> CheckResult<(u32, [u8; 32])> {
    // Original stock graphs provide independent row witnesses for these complete
    // allocations. Follow their host and owner instead of trusting an opaque literal.
    let witness_tag = match count {
        1 => 0x80C3DDE2,
        2 => 0x80C0020A,
        _ => return Err("No qualified replication witness for this bank".into()),
    };
    let witness = stock.tag(witness_tag, Some(0x80809BB6))?;
    let host = stock.tag(witness.u32(8)?, Some(0x80809C0F))?;
    assert_eq!(host.u32(0x88)?, witness_tag);
    let owners = host.array(16, 12, Some(0x80809C04))?;
    let rows = witness.array(0x30, 32, Some(0x80809BB8))?;
    assert_eq!(owners.len(), rows.len());
    let allocation_tag = bank.u32(0x44)?;
    let matches = rows
        .iter()
        .enumerate()
        .filter_map(|(ordinal, &row)| {
            (witness.u32(row).ok() == Some(0x80803AFD)
                && witness.u32(row + 4).ok() == Some(allocation_tag))
            .then_some((ordinal, row))
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    let (ordinal, row) = matches[0];
    let original_tag = host.u32(owners[ordinal])?;
    let original = stock.tag(original_tag, Some(0x80809C36))?;
    assert_eq!(providers(graph, bank_tag)?, providers(&host, original_tag)?);
    assert_eq!(original.u32(0x44)?, allocation_tag);
    assert_eq!(original.u32(original.pointer(16)? - 4)?, 0x80803AFD);
    assert_eq!(
        original
            .array(original.pointer(24)? + 88, 88, Some(0x80803B06))?
            .len(),
        count
    );
    assert_eq!(bank.pointer(16)?, original.pointer(16)?);
    assert_eq!(bank.pointer(24)?, original.pointer(24)?);
    assert_eq!(bank.0.len(), original.0.len());
    let live = bank.pointer(16)?;
    let definition = bank.pointer(24)?;
    assert_eq!(bank.u32(definition - 4)?, 0x80803AFE);
    assert_eq!(bank.u64(0x48)?, (definition - live) as u64);
    assert_eq!(bank.u64(0x50)?, (live - bank.pointer(8)?) as u64);
    let instances = bank.array(live + 80, 128, Some(0x80803B05))?;
    let definitions = bank.array(definition + 88, 88, Some(0x80803B06))?;
    let lifecycle = bank.array(0x30, 16, Some(0x808091A4))?;
    assert_eq!(instances.len(), count);
    assert_eq!(definitions.len(), count);
    assert_eq!(lifecycle.len(), count + 1);
    for ((&instance, &definition), &row) in instances.iter().zip(&definitions).zip(&lifecycle) {
        assert_eq!(bank.u32(instance)?, bank_tag);
        assert_eq!(bank.u32(definition)?, bank_tag);
        assert_eq!(bank.u32(instance + 4)?, 0x80803B06);
        assert_eq!(bank.u32(definition + 4)?, 0x80803B05);
        assert_eq!(bank.u64(instance + 8)?, definition as u64);
        assert_eq!(bank.u64(definition + 8)?, instance as u64);
        assert_eq!(bank.pointer(row)?, instance);
        assert_eq!(bank.u32(row + 8)?, 0x808045BE);
        assert_eq!((bank.u16(row + 12)?, bank.u16(row + 14)?), (0, 1));
    }
    let cleanup = lifecycle[count];
    assert_eq!(bank.pointer(cleanup)?, live + 48);
    assert_eq!(bank.u32(cleanup + 8)?, 0x80803B00);
    assert_eq!(
        (bank.u16(cleanup + 12)?, bank.u16(cleanup + 14)?),
        (u16::MAX, 1)
    );
    let allocation = emitted.tag(allocation_tag, Some(0x80809BBB))?;
    let members = allocation.array(32, 40, Some(0x80808852))?;
    assert_eq!(members.len(), 1);
    assert_eq!(allocation.u64(members[0])?, 0xFB05ED04);
    let elements = allocation.array(members[0] + 24, 40, Some(0x80808852))?;
    assert_eq!(elements.len(), 1);
    assert_eq!(allocation.u64(elements[0])?, 0x587BC3F9);
    assert_eq!(allocation.u32(elements[0] + 16)?, 0x80803B05);
    assert_eq!(allocation.u32(elements[0] + 20)? as usize, count);
    assert!(
        allocation
            .array(elements[0] + 24, 40, Some(0x80808852))?
            .is_empty()
    );
    Ok((witness_tag, witness.bytes(row)?))
}

/// Compare the complete bank provider contract with an independently stored native owner.
/// Private identity and placement change, while selectors, dispatch and runtime offsets do not.
pub(super) fn providers(
    graph: &Payload,
    owner: u32,
) -> CheckResult<BTreeSet<([u8; 40], [u8; 24])>> {
    let exports = graph.array(0x58, 40, Some(0x80809C22))?;
    let references = graph.array(0x68, 24, Some(0x80809C20))?;
    assert_eq!(exports.len(), references.len());
    let mut result = BTreeSet::new();
    for (export, reference) in exports.into_iter().zip(references) {
        if graph.u32(export + 16)? != owner {
            continue;
        }
        let mut declaration = graph.bytes::<40>(export)?;
        let mut binding = graph.bytes::<24>(reference)?;
        declaration[16..20].fill(0);
        binding[..4].fill(0);
        binding[16..24].fill(0);
        assert!(
            result.insert((declaration, binding)),
            "Duplicate modifier provider"
        );
    }
    assert!(!result.is_empty());
    Ok(result)
}

pub(super) fn loading(
    emitted: &mut Reader,
    stock: &Reader,
    replication: u32,
    required: &BTreeSet<u32>,
) -> CheckResult<u32> {
    // Locate the companion through the staged package's actual enrollment table.
    // A matching payload alone would miss an unregistered loading resource.
    let package_id = tiger_pkg::TagHash(replication).pkg_id();
    let path = &emitted
        .manager
        .package_paths
        .get(&package_id)
        .ok_or("Replication package is absent")?
        .path;
    let package = Payload(fs::read(path)?);
    assert_eq!(package.u16(0)?, 38);
    assert_eq!(package.u16(4)?, package_id);
    let tables = usize::try_from(package.u32(0x110)?)?;
    let count = usize::try_from(package.u64(tables + 0x30)?)?;
    assert!(count > 0);
    let header = package.pointer(tables + 0x38)?;
    assert_eq!(package.u32(header - 4)?, 0x80809FBD);
    assert!(package.u64(header)? >= count as u64);
    assert_eq!(package.u64(header + 8)?, 0x80809A13);
    let mut companions = Vec::new();
    for ordinal in 0..count {
        let row = header + 16 + ordinal * 8;
        if package.u32(row)? == replication {
            companions.push(package.u32(row + 4)?);
        }
    }
    assert_eq!(companions.len(), 1);
    let companion = companions[0];
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(companion))
            .is_none()
    );
    let payload = emitted.tag(companion, Some(0x80809EF9))?;
    let entry = emitted
        .manager
        .get_entry(tiger_pkg::TagHash(companion))
        .ok_or("Private replication companion is absent")?;
    assert_eq!((entry.file_type, entry.file_subtype), (8, 0));
    assert_eq!(payload.u64(0)?, payload.0.len() as u64);
    assert_eq!(payload.u32(8)?, companion);
    assert_eq!(payload.u32(12)?, replication);
    let mut dependencies = BTreeSet::new();
    for row in payload.array(16, 40, Some(0x80809EFB))? {
        let package = u16::try_from(payload.u64(row)?)?;
        for (word, at) in payload
            .array(row + 8, 4, Some(0x8080000B))?
            .into_iter()
            .enumerate()
        {
            for bit in 0..32 {
                if payload.u32(at)? & (1 << bit) != 0 {
                    dependencies
                        .insert(tiger_pkg::TagHash::new(package, (word * 32 + bit) as u16).0);
                }
            }
        }
        for at in payload.array(row + 24, 2, Some(0x8080000A))? {
            dependencies.insert(tiger_pkg::TagHash::new(package, payload.u16(at)?).0);
        }
    }
    assert!(dependencies.contains(&companion));
    for tag in required {
        assert!(
            dependencies.contains(tag),
            "Replication loading omits {tag:08X}"
        );
    }
    Ok(companion)
}
