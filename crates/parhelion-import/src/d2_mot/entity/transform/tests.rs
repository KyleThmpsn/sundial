//! Configured complete-package oracle, written before the adapter.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires configured transform package exports and a fresh artifact directory"]
fn transform_package_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_TRANSFORM_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_TRANSFORM_OUTPUT")?);
    ensure!(!output.exists(), "transform oracle output already exists");
    let load = |name: &str| -> Result<Payload> { Ok(Payload(fs::read(corpus.join(name))?)) };
    let template = load("native-owners/80C709E8.bin")?;
    let allocation = load("native-allocation/80C70C8B.bin")?;
    let mut metadata = BTreeMap::new();
    for directory in ["modern-metadata", "native-metadata"] {
        for entry in fs::read_dir(corpus.join(directory))? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "bin") {
                continue;
            }
            let bytes = fs::read(&path)?;
            if bytes.len() > 136 {
                continue;
            }
            let tag = u32::from_str_radix(
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .context("transform metadata name")?,
                16,
            )?;
            metadata.insert(tag, Payload(bytes));
        }
    }
    fs::create_dir_all(&output)?;
    let mut reports = Vec::new();
    for (source_tag, native_tag) in [
        (0x80C3A8AD, 0x80C709E8),
        (0x80C3ED8C, 0x80C0D8D4),
        (0x80A6D125, 0x80C0D8D4),
    ] {
        let source = load(&format!("modern-owners/{source_tag:08X}.bin"))?;
        let graph = Graph {
            components: vec![source_tag],
            connections: vec![],
            named_connections: vec![],
        };
        let native = emit(
            &source,
            &template,
            &allocation,
            &metadata,
            &graph,
            native_tag,
            0x80C70C8B,
        )?;
        ensure!(
            native.owner.0 == load(&format!("native-owners/{native_tag:08X}.bin"))?.0,
            "transform owner differs from complete native counterpart {native_tag:08X}"
        );
        ensure!(
            native.allocation.0 == allocation.0,
            "transform allocation differs"
        );
        fs::write(
            output.join(format!("{source_tag:08X}.bin")),
            &native.owner.0,
        )?;
        reports.push(serde_json::json!({"source":source_tag,"native":native_tag,
            "source_sha256":format!("{:x}",Sha256::digest(&source.0)),
            "native_sha256":format!("{:x}",Sha256::digest(&native.owner.0)),
            "objects":native.objects,"byte_identical":true}));
    }
    let source = load("modern-owners/80D8A174.bin")?;
    let parent = corpus.parent().context("transform corpus parent")?;
    for entity_tag in [
        0x80D93C0Cu32,
        0x80D93CCB,
        0x80D93CDF,
        0x80D93D02,
        0x80D93D28,
    ] {
        let entity = Payload(fs::read(
            parent.join(format!("enigma-resource-entities/{entity_tag:08X}.bin")),
        )?);
        let graph = Graph::read(&entity, true)?;
        let native = emit(
            &source,
            &template,
            &allocation,
            &metadata,
            &graph,
            0x81FD1FF7,
            0x81FD1FF6,
        )?;
        fs::write(
            output.join(format!("{entity_tag:08X}-owner.bin")),
            &native.owner.0,
        )?;
        fs::write(output.join("allocation.bin"), &native.allocation.0)?;
        reports.push(
            serde_json::json!({"entity":entity_tag,"source":0x80D8A174u32,
            "owner_sha256":format!("{:x}",Sha256::digest(&native.owner.0)),
            "objects":native.objects,"source_graph_checked":true}),
        );
    }
    let graph = Graph {
        components: vec![0x80D8A174],
        connections: vec![],
        named_connections: vec![],
    };
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let mut refused = Vec::new();
    for (name, at, value) in [
        ("Base Flags", sd + 16, 1),
        ("Runtime State", si + 16, 1),
        ("Provider Parent", sd + 0x48, 0),
        ("Provider State", sd + 0x58, 1),
        ("Input Count", sd + 0xE8, 2),
        ("Child Transforms", sd + 0x108, 1),
        ("Lifecycle Flags", source.pointer(0x48)? + 16 + 12, 3),
    ] {
        let mut changed = source.clone();
        changed.0[at] = value;
        ensure!(
            emit(
                &changed,
                &template,
                &allocation,
                &metadata,
                &graph,
                0x81FD1FF7,
                0x81FD1FF6
            )
            .is_err(),
            "unsupported transform state accepted: {name}"
        );
        refused.push(name);
    }
    let unsupported = Object {
        owner: 0x80D8A174,
        class: 0x808098C9,
        offset: (sd + 0xC8) as u64,
    };
    let mut active = graph;
    active.connections.push(super::super::links::Connection {
        consumer: super::super::links::Endpoint {
            namespace: 0,
            object: None,
            selector: 0xFFFF,
        },
        provider: super::super::links::Endpoint {
            namespace: 0,
            object: Some(unsupported),
            selector: 0,
        },
        channel: 0,
        flags: 0,
    });
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &metadata,
            &active,
            0x81FD1FF7,
            0x81FD1FF6
        )
        .is_err(),
        "active source-only transform getter accepted"
    );
    refused.push("Active Source-only Getter");
    active.connections.clear();
    let mut changed = metadata.clone();
    changed
        .get_mut(&source.u32(sd + 0x48 + 8)?)
        .context("transform lifecycle metadata")?
        .0[68] = 7;
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &changed,
            &active,
            0x81FD1FF7,
            0x81FD1FF6
        )
        .is_err(),
        "changed transform method accepted"
    );
    refused.push("Method Ordinal");
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
        "cases":reports,"refusals":refused,"installable":false,"gameplay_verified":false}))?,
    )?;
    Ok(())
}
