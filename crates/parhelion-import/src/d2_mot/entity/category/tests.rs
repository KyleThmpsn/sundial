//! Package, dispatch and actual native lifecycle fixture oracle, written first.
use super::*;
use crate::d2_mot::native::categories;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::PathBuf};

#[test]
#[ignore = "requires explicitly configured category and source entity exports"]
fn category_owner_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_CATEGORY_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_CATEGORY_OWNER_OUTPUT")?);
    ensure!(
        !output.exists(),
        "category owner oracle output already exists"
    );
    let load = |name: &str| -> Result<Payload> { Ok(Payload(fs::read(corpus.join(name))?)) };
    let source = load("category-owners-modern/80CED924.bin")?;
    let source_allocation = load("category-owners-modern/80CEA637.bin")?;
    let template = load("category-owners-native/815282E5.bin")?;
    let allocation = Payload(fs::read(
        corpus
            .parent()
            .context("category corpus parent")?
            .join("sequence-corpus/value-host-allocation/80C70B90.bin"),
    )?);
    let entity = Payload(fs::read(
        corpus
            .parent()
            .context("category corpus parent")?
            .join("enigma-source-entity/80CEE9B0.bin"),
    )?);
    let graph = Graph::read(&entity, true)?;
    let namespace = categories::emit_static(
        &load("source-category/raw/80CEA5F8.bin")?,
        &load("native-category/raw/80C70CA1.bin")?,
        &BTreeSet::from([0xD1945C50, 0x000A69A1, 0x9C6C4BA0]),
    )?;
    let mut metadata = BTreeMap::new();
    for dir in ["category-metadata-modern", "category-metadata-native"] {
        for entry in fs::read_dir(corpus.join(dir))? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "bin") {
                continue;
            }
            let tag = u32::from_str_radix(
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .context("category metadata tag")?,
                16,
            )?;
            metadata.insert(tag, Payload(fs::read(path)?));
        }
    }
    let native = emit(
        &source,
        &source_allocation,
        &template,
        &allocation,
        &metadata,
        &graph,
        &namespace,
        0x81FD1FFF,
        0x81FD1FFE,
    )?;
    let mut expected = load("private-category-model/owner.bin")?;
    expected.0[0x44..0x48].copy_from_slice(&0x81FD1FFEu32.to_le_bytes());
    ensure!(
        native.owner.0 == expected.0 && native.allocation.0 == allocation.0,
        "category owner differs from native lifecycle fixture"
    );
    let provider = Object {
        owner: 0x80CED924,
        class: 0x8080977F,
        offset: (source.pointer(24)? + 0xD0) as u64,
    };
    ensure!(
        native.channels.get(&(provider, 0)) == Some(&0)
            && native.channels.get(&(provider, 9)) == Some(&6),
        "category channel correspondence differs"
    );
    let mut refusals = Vec::new();
    for channel in [6, 7, 8, 10] {
        let mut changed = Graph::read(&entity, true)?;
        let edge = changed
            .connections
            .iter_mut()
            .find(|edge| edge.provider.object == Some(provider))
            .context("source category edge")?;
        edge.channel = channel;
        ensure!(
            emit(
                &source,
                &source_allocation,
                &template,
                &allocation,
                &metadata,
                &changed,
                &namespace,
                0x81FD1FFF,
                0x81FD1FFE
            )
            .is_err(),
            "unsupported category channel accepted"
        );
        refusals.push(format!("Unsupported Channel {channel}"));
    }
    for (name, at, byte) in [
        ("Initialized Runtime Mask", source.pointer(16)? + 0x30, 1),
        ("Definition Input", source.pointer(24)? + 0xA0, 1),
        ("Lifecycle Parameter", source.pointer(24)? + 0x100, 12),
    ] {
        let mut changed = source.clone();
        changed.0[at] = byte;
        ensure!(
            emit(
                &changed,
                &source_allocation,
                &template,
                &allocation,
                &metadata,
                &graph,
                &namespace,
                0x81FD1FFF,
                0x81FD1FFE
            )
            .is_err(),
            "unsupported category state accepted: {name}"
        );
        refusals.push(name.to_owned());
    }
    let mut changed = metadata.clone();
    let meta = changed
        .get_mut(&0x80CEA5EC)
        .context("source category dispatch")?;
    let first = meta.pointer(24)? + 16;
    meta.0[first + 4..first + 8].copy_from_slice(&12u32.to_le_bytes());
    ensure!(
        emit(
            &source,
            &source_allocation,
            &template,
            &allocation,
            &changed,
            &graph,
            &namespace,
            0x81FD1FFF,
            0x81FD1FFE
        )
        .is_err(),
        "changed category dispatch accepted"
    );
    refusals.push("Dispatch Contract".to_owned());
    fs::create_dir_all(&output)?;
    fs::write(output.join("owner.bin"), &native.owner.0)?;
    fs::write(output.join("allocation.bin"), &native.allocation.0)?;
    let report = serde_json::json!({
        "source_sha256":hex::encode(Sha256::digest(&source.0)),
        "native_sha256":hex::encode(Sha256::digest(&native.owner.0)),
        "objects":native.objects.iter().map(|mapping| serde_json::json!({
            "source":mapping.source,"target":mapping.target})).collect::<Vec<_>>(),
        "channels":native.channels.iter().map(|((object,source),target)| serde_json::json!({
            "object":object,"source":source,"native":target})).collect::<Vec<_>>(),
        "refusals":refusals,"native_lifecycle_fixture_exact":true,
        "installable":false,"gameplay_verified":false
    });
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
