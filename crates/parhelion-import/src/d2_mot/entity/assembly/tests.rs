//! Configured topology and refusal oracle, written before the preflight.
use super::*;
use anyhow::Context;
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
struct Config {
    source_entity: PathBuf,
    source_owners: PathBuf,
    native_entity: PathBuf,
    native_owners: PathBuf,
}

fn owners(directory: &std::path::Path, graph: &Graph) -> Result<BTreeMap<u32, Payload>> {
    graph
        .components
        .iter()
        .map(|tag| {
            Ok((
                *tag,
                Payload(fs::read(directory.join(format!("{tag:08X}.bin")))?),
            ))
        })
        .collect()
}

// Independently encode the stock Native topology in the Source row layout.
// This is a serialization control, not a claim about a paired Source asset.
fn source_layout(native: &Payload) -> Result<Payload> {
    let graph = Graph::read(native, false)?;
    let mut bytes = vec![0; 56];
    let mut append = |descriptor: usize, class: u32, rows: &[Vec<u8>]| {
        if rows.is_empty() {
            return;
        }
        let head = bytes.len();
        bytes[descriptor..descriptor + 8].copy_from_slice(&(rows.len() as u64).to_le_bytes());
        bytes[descriptor + 8..descriptor + 16]
            .copy_from_slice(&((head as i64) - (descriptor as i64 + 8)).to_le_bytes());
        bytes.extend_from_slice(&(rows.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&class.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        for row in rows {
            bytes.extend_from_slice(row);
        }
    };
    let components = native
        .array(16, 12, Some(0x80809C04))?
        .into_iter()
        .map(|at| Ok(native.bytes::<12>(at)?.to_vec()))
        .collect::<Result<Vec<_>>>()?;
    append(8, 0x80809ACD, &components);
    for (at, rows) in [(24, &graph.connections), (40, &graph.named_connections)] {
        let mut encoded = Vec::new();
        for edge in rows {
            let mut row = vec![0; 56];
            for (endpoint, offset) in [(&edge.consumer, 0), (&edge.provider, 24)] {
                let object = endpoint.object.unwrap_or(Object {
                    owner: u32::MAX,
                    class: u32::MAX,
                    offset: 0,
                });
                row[offset..offset + 4].copy_from_slice(&object.owner.to_le_bytes());
                row[offset + 4..offset + 8].copy_from_slice(&object.class.to_le_bytes());
                row[offset + 8..offset + 16].copy_from_slice(&object.offset.to_le_bytes());
                row[offset + 16..offset + 20].copy_from_slice(&endpoint.namespace.to_le_bytes());
                row[offset + 20..offset + 24]
                    .copy_from_slice(&u32::try_from(endpoint.selector)?.to_le_bytes());
            }
            row[48..52].copy_from_slice(&edge.channel.to_le_bytes());
            row[52..56].copy_from_slice(&edge.flags.to_le_bytes());
            encoded.push(row);
        }
        append(at, 0x80809A8F, &encoded);
    }
    let size = bytes.len() as u64;
    bytes[..8].copy_from_slice(&size.to_le_bytes());
    Ok(Payload(bytes))
}

#[test]
#[ignore = "requires explicitly configured exported entities and owners"]
fn configured_entity_preflight() -> Result<()> {
    let config_path = PathBuf::from(std::env::var("PARHELION_ENTITY_PREFLIGHT_CONFIG")?);
    let output = PathBuf::from(std::env::var("PARHELION_ENTITY_PREFLIGHT_OUTPUT")?);
    ensure!(!output.exists(), "preflight output already exists");
    let config: Config = serde_json::from_slice(&fs::read(&config_path)?)?;
    let base = config_path
        .parent()
        .context("preflight configuration parent")?;
    let path = |value: &PathBuf| {
        if value.is_absolute() {
            value.clone()
        } else {
            base.join(value)
        }
    };
    let actual = Payload(fs::read(path(&config.source_entity))?);
    let actual_graph = Graph::read(&actual, true)?;
    let actual_owners = owners(&path(&config.source_owners), &actual_graph)?;
    let native = Payload(fs::read(path(&config.native_entity))?);
    let native_graph = Graph::read(&native, false)?;
    let native_owners = owners(&path(&config.native_owners), &native_graph)?;
    let source = source_layout(&native)?;
    let graph = Graph::read(&source, true)?;
    let mut contracts = Contracts {
        components: graph
            .components
            .iter()
            .map(|&tag| Component {
                source: tag,
                target: tag,
            })
            .collect(),
        ..Default::default()
    };
    let objects: BTreeSet<_> = graph.objects().collect();
    let relocations: Vec<_> = objects
        .iter()
        .map(|&object| Relocation {
            source: object,
            target: object,
        })
        .collect();
    for edge in graph.connections.iter().chain(&graph.named_connections) {
        if let Some(provider) = edge.provider.object {
            if !contracts
                .channels
                .iter()
                .any(|c| c.provider == provider && c.source == edge.channel)
            {
                contracts.channels.push(Channel {
                    provider,
                    source: edge.channel,
                    target: edge.channel,
                    methods: vec![],
                });
            }
        }
        for endpoint in [&edge.consumer, &edge.provider] {
            if endpoint.selector > 0xFFFF {
                contracts.externals.push(External {
                    object: endpoint.object.context("external object")?,
                    source: endpoint.selector,
                    target: endpoint.selector,
                });
            }
        }
    }
    let report = preflight(
        &source,
        &native_owners,
        &native_owners,
        &relocations,
        &contracts,
        &[],
        &[],
    )?;
    let rows = report
        .rows
        .as_ref()
        .context("complete topology did not lower")?;
    // Read the stock serialized arrays independently of the lowering closures.
    for (descriptor, actual_rows) in [(32, &rows.connections), (48, &rows.named_connections)] {
        let expected = native.array(descriptor, 72, Some(0x80809BC9))?;
        ensure!(
            expected.len() == actual_rows.len(),
            "native connection count differs"
        );
        for (at, row) in expected.iter().zip(actual_rows) {
            ensure!(
                row.as_slice() == native.bytes::<72>(*at)?.as_slice(),
                "independent native row differs"
            );
        }
    }
    ensure!(
        !report.ready,
        "opaque component tails were declared complete"
    );
    let mut refused = Vec::new();
    let mut duplicate = relocations
        .iter()
        .map(|r| Relocation {
            source: r.source,
            target: r.target,
        })
        .collect::<Vec<_>>();
    duplicate.extend(relocations.iter().map(|r| Relocation {
        source: r.source,
        target: r.target,
    }));
    let deduplicated = preflight(
        &source,
        &native_owners,
        &native_owners,
        &duplicate,
        &contracts,
        &[],
        &[],
    )?;
    ensure!(
        deduplicated.object_map.len() == relocations.len(),
        "identical converter maps were not deduplicated"
    );
    let deduplicated_rows = deduplicated
        .rows
        .as_ref()
        .context("identical converter maps blocked topology")?;
    ensure!(
        deduplicated_rows.connections == rows.connections
            && deduplicated_rows.named_connections == rows.named_connections,
        "deduplicated topology differs"
    );
    if source.u64(24)? > 0 {
        let mut overlap = source.clone();
        let header = source.pointer(32)?;
        overlap.0[40..48].copy_from_slice(&source.bytes::<8>(24)?);
        overlap.0[48..56].copy_from_slice(&(i64::try_from(header)? - 48).to_le_bytes());
        let negative = preflight(
            &overlap,
            &native_owners,
            &native_owners,
            &relocations,
            &contracts,
            &[],
            &[],
        )?;
        ensure!(
            !negative.ready
                && negative.rows.is_none()
                && negative
                    .blockers
                    .iter()
                    .any(|b| b.kind == "entity_array_overlap"),
            "aliased entity arrays were accepted"
        );
        refused.push(negative);
    }
    if !relocations.is_empty() {
        let negative = preflight(
            &source,
            &native_owners,
            &native_owners,
            &relocations[1..],
            &contracts,
            &[],
            &[],
        )?;
        ensure!(
            negative.rows.is_none() && !negative.ready,
            "missing object was accepted"
        );
        refused.push(negative);
    }
    let mut gates = Contracts::default();
    gates.pending.push(Pending {
        owner: 0,
        kind: ObligationKind::Gate,
        offset: Some(7),
        detail: serde_json::json!({"Mask":{"index":7,"name":123}}),
    });
    let gated = preflight(
        &source,
        &native_owners,
        &native_owners,
        &relocations,
        &gates,
        &[],
        &[],
    )?;
    ensure!(
        !gated.ready
            && gated.pending[0].detail["Mask"]["name"] == 123
            && gated
                .blockers
                .iter()
                .any(|b| b.kind == "converter_obligation" && b.detail.contains("123")),
        "gate evidence was lost"
    );
    refused.push(gated);
    let mut unsupported = contracts;
    if let Some(channel) = unsupported.channels.first_mut() {
        channel.methods.push(0x7FFF);
        unsupported.unsupported.push(Unsupported {
            object: channel.provider,
            methods: vec![0x7FFF],
        });
        let negative = preflight(
            &source,
            &native_owners,
            &native_owners,
            &relocations,
            &unsupported,
            &[],
            &[],
        )?;
        ensure!(
            negative.rows.is_none() && !negative.ready,
            "unsupported active method was accepted"
        );
        refused.push(negative);
    }
    let blocked = preflight(
        &actual,
        &actual_owners,
        &BTreeMap::new(),
        &[],
        &Contracts::default(),
        &[],
        &[],
    )?;
    ensure!(
        !blocked.ready && !blocked.blockers.is_empty(),
        "incomplete actual entity became ready"
    );
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1, "native_control_sha256": digest(&native), "serialization_control": report,
            "deduplicated": deduplicated, "refusals": refused, "actual_entity": blocked, "semantic_equivalence_proven": false,
            "package_enrolled": false, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
