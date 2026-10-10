//! Configured package and entity-graph contract written before the adapter.
use super::*;
use crate::d2_mot::entity::links::Interface;
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires explicitly configured source and native effect exports"]
fn spatial_package_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_SPATIAL_OUTPUT")?);
    ensure!(!output.exists(), "spatial oracle output already exists");
    let root = corpus.parent().context("spatial corpus parent")?;
    let source = Payload(fs::read(root.join("enigma-resource-owners/80D8A379.bin"))?);
    let entity = Payload(fs::read(
        root.join("enigma-resource-entities/80D93DFA.bin"),
    )?);
    let graph = Graph::read(&entity, true)?;
    let template = Payload(fs::read(corpus.join("native/80EF62A3.bin"))?);
    let authored = Payload(fs::read(corpus.join("native/80EF8431.bin"))?);
    let allocation = Payload(fs::read(corpus.join("value-host-allocation/80C70B90.bin"))?);
    let mut metadata = BTreeMap::new();
    for directory in ["kind16-metadata", "kind16-native-metadata"] {
        for entry in fs::read_dir(corpus.join(directory))? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "bin") {
                continue;
            }
            let tag = u32::from_str_radix(
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .context("spatial metadata tag")?,
                16,
            )?;
            metadata.insert(tag, Payload(fs::read(path)?));
        }
    }
    let mut names = BTreeSet::new();
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(
        root.join("enigma-resource-dependencies/source-manifest.json"),
    )?)?;
    for (tag, entry) in manifest["tags"]
        .as_object()
        .context("source resource manifest")?
    {
        if entry["reference"].as_u64() != Some(0x80806927) {
            continue;
        }
        let p = Payload(fs::read(
            root.join("enigma-resource-dependencies/raw")
                .join(format!("{tag}.bin")),
        )?);
        for at in p.array(0x108, 4, Some(0x80800070))? {
            names.insert(p.u32(at)?);
        }
    }
    let converted = emit(
        &source,
        &template,
        &allocation,
        &metadata,
        &graph,
        &names,
        0x81FD1FFB,
        0x81FD1FFA,
    )?;
    let sd = source.pointer(24)?;
    let nd = converted.owner.pointer(24)?;
    let ni = converted.owner.pointer(16)?;
    ensure!(
        converted.owner.0.len() == 520 && converted.allocation.0 == allocation.0,
        "spatial owner or allocation shape differs"
    );
    ensure!(
        converted.owner.u32(ni)? == 0x81FD1FFB
            && converted.owner.u32(nd)? == 0x81FD1FFB
            && converted.owner.u32(0x44)? == 0x81FD1FFA,
        "private spatial tags differ"
    );
    ensure!(
        converted.objects.len() == 9,
        "spatial object correspondence differs"
    );
    let mut methods = Vec::new();
    for (so, no, source_method, native_method) in [
        (0x88, 0x78, 2, 2),
        (0xB0, 0x98, 3, 3),
        (0xD8, 0xB8, 4, 4),
        (0x100, 0xD8, 5, 5),
        (0x150, 0xF8, 7, 6),
    ] {
        let name = source.u32(sd + so + 32)?;
        ensure!(
            converted.owner.u32(nd + no + 24)? == name
                && authored.u32(authored.pointer(24)? + no + 24)? == name,
            "native authored getter name differs"
        );
        let mapping = converted
            .objects
            .iter()
            .find(|r| r.source.offset == (sd + so) as u64)
            .context("spatial getter relocation")?;
        let a = Interface::read(
            &source,
            mapping.source,
            metadata
                .get(&source.u32(sd + so + 8)?)
                .context("source metadata")?,
            true,
        )?;
        let b = Interface::read(
            &converted.owner,
            mapping.target,
            metadata
                .get(&converted.owner.u32(nd + no + 8)?)
                .context("native metadata")?,
            false,
        )?;
        ensure!(
            a.methods.len() == 1
                && b.methods.len() == 1
                && a.methods[0].index == source_method
                && b.methods[0].index == native_method
                && b.methods[0].implementation_class == 0x8080375F,
            "spatial getter dispatch differs"
        );
        methods.push(serde_json::json!({"name":format!("{name:08X}"),"source_method":source_method,"native_method":native_method}));
    }
    let active_owner = source.u32(source.pointer(16)?)?;
    let active = graph
        .objects()
        .filter(|o| o.owner == active_owner)
        .collect::<Vec<_>>();
    ensure!(
        active.len() == 3
            && active
                .iter()
                .all(|o| converted.objects.iter().any(|m| m.source == *o)),
        "active spatial graph getters unresolved"
    );
    let mut refusals = 0;
    for (field, word) in [
        (source.pointer(16)? - 4, 0x808032A9u32),
        (sd - 4, 0x808032AA),
        (source.pointer(16)?, 0),
        (source.pointer(16)? + 0x40, 1),
        (source.pointer(16)? + 0x50, 0),
        (sd + 16, 1),
        (sd + 0x48, 0),
        (sd + 0x48 + 12, 1),
        (sd + 0x48 + 16, 1),
        (sd + 0x88 + 24, 1),
        (sd + 0x88 + 32, 0),
        (sd + 0x1A0, 1),
    ] {
        let mut invalid = source.clone();
        invalid.0[field..field + 4].copy_from_slice(&word.to_le_bytes());
        ensure!(
            emit(
                &invalid,
                &template,
                &allocation,
                &metadata,
                &graph,
                &names,
                0x81FD1FFB,
                0x81FD1FFA
            )
            .is_err(),
            "malformed spatial source accepted"
        );
        refusals += 1;
    }
    for offset in [0x128, 0x178] {
        let mut invalid = Graph::read(&entity, true)?;
        let edge = invalid
            .connections
            .iter_mut()
            .find(|r| r.provider.object.is_some_and(|o| o.owner == active_owner))
            .context("spatial graph provider")?;
        edge.provider.object.as_mut().unwrap().offset = (sd + offset) as u64;
        ensure!(
            emit(
                &source,
                &template,
                &allocation,
                &metadata,
                &invalid,
                &names,
                0x81FD1FFB,
                0x81FD1FFA
            )
            .is_err(),
            "active unsupported spatial provider accepted"
        );
        refusals += 1;
        let mut dynamic = names.clone();
        dynamic.insert(source.u32(sd + offset + 32)?);
        ensure!(
            emit(
                &source,
                &template,
                &allocation,
                &metadata,
                &graph,
                &dynamic,
                0x81FD1FFB,
                0x81FD1FFA
            )
            .is_err(),
            "dynamically requested unsupported spatial name accepted"
        );
        refusals += 1;
    }
    let mut missing = metadata.clone();
    missing.remove(&source.u32(sd + 0x88 + 8)?);
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &missing,
            &graph,
            &names,
            0x81FD1FFB,
            0x81FD1FFA
        )
        .is_err(),
        "missing spatial metadata accepted"
    );
    refusals += 1;
    let mut wrong = metadata.clone();
    let value = wrong
        .get_mut(&template.u32(template.pointer(24)? + 0xF8 + 8)?)
        .context("native method metadata")?;
    let at = value.array(16, 24, Some(0x80809C56))?[0];
    value.0[at + 4..at + 8].copy_from_slice(&7u32.to_le_bytes());
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &wrong,
            &graph,
            &names,
            0x81FD1FFB,
            0x81FD1FFA
        )
        .is_err(),
        "wrong native spatial method accepted"
    );
    refusals += 1;
    for (owner, alloc) in [(0, 0x81FD1FFA), (0x81FD1FFB, 0), (0x81FD1FFB, 0x81FD1FFB)] {
        ensure!(
            emit(
                &source,
                &template,
                &allocation,
                &metadata,
                &graph,
                &names,
                owner,
                alloc
            )
            .is_err(),
            "invalid private spatial tags accepted"
        );
        refusals += 1;
    }
    let mut wrong = template.clone();
    wrong.0[template.pointer(16)? + 0x30..template.pointer(16)? + 0x34]
        .copy_from_slice(&1u32.to_le_bytes());
    ensure!(
        emit(
            &source,
            &wrong,
            &allocation,
            &metadata,
            &graph,
            &names,
            0x81FD1FFB,
            0x81FD1FFA
        )
        .is_err(),
        "initialized native spatial template accepted"
    );
    refusals += 1;
    fs::create_dir_all(&output)?;
    fs::write(output.join("81FD1FFB.bin"), &converted.owner.0)?;
    fs::write(output.join("81FD1FFA.bin"), &converted.allocation.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"source_owner_sha256":hex::encode(Sha256::digest(&source.0)),"source_entity_sha256":hex::encode(Sha256::digest(&entity.0)),"owner_sha256":hex::encode(Sha256::digest(&converted.owner.0)),"allocation_sha256":hex::encode(Sha256::digest(&converted.allocation.0)),"objects":converted.objects,"methods":methods,"active_graph_getters":active,"refusals":refusals,"scope":"Private native component with verified initial-state envelope, native allocation, authored getter names and explicit source graph interface correspondence. No whole-owner twin or live execution proof.","installable":false,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}
