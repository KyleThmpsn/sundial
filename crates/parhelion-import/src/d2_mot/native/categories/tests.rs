//! Configured native package and captured lifecycle artifact oracle, written first.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

mod aliases;

#[test]
#[ignore = "requires explicitly configured source, native and lifecycle exports"]
fn dictionary_package_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_CATEGORY_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_CATEGORY_OUTPUT")?);
    ensure!(!output.exists(), "category oracle output already exists");
    let source = Payload(fs::read(corpus.join("source-category/raw/80CEA5F8.bin"))?);
    let stock = Payload(fs::read(corpus.join("native-category/raw/80C70CA1.bin"))?);
    let expected = fs::read(corpus.join("private-category-model/dictionary.bin"))?;
    let baseline = emit_static(&source, &stock, &BTreeSet::new())?;
    ensure!(
        baseline.payload.0 == stock.0,
        "native dictionary does not round trip"
    );
    let requested = BTreeSet::from([0xD1945C50, 0x000A69A1, 0x9C6C4BA0]);
    let private = emit_static(&source, &stock, &requested)?;
    ensure!(
        private.payload.0 == expected,
        "private dictionary differs from native-call fixture"
    );
    ensure!(
        private.names[..baseline.names.len()] == baseline.names,
        "stock name indices changed"
    );
    ensure!(
        private.added_static == [0xD1945C50, 0x000A69A1, 0x9C6C4BA0],
        "imported name order differs"
    );
    ensure!(
        private.source_indices[85] == Some(306)
            && private.source_indices[86] == Some(307)
            && private.source_indices[354] == Some(308),
        "private correspondence differs"
    );
    let source_owner = Payload(fs::read(
        corpus.join("category-owners-modern/80CED924.bin"),
    )?);
    let definition = source_owner.pointer(24)?;
    let source_mask = source_owner.bytes::<56>(definition + 0x68)?;
    let native_mask = private.definition_mask(&source_mask)?;
    let expected_owner = Payload(fs::read(corpus.join("private-category-model/owner.bin"))?);
    ensure!(
        native_mask == expected_owner.bytes::<40>(expected_owner.pointer(24)? + 0x60)?,
        "projectile definition mask differs from captured native fixture"
    );
    let mut refusals = Vec::new();
    let mut refused = |label: &str, result: Result<Namespace>| -> Result<()> {
        ensure!(result.is_err(), "malformed dictionary accepted: {label}");
        refusals.push(label.to_owned());
        Ok(())
    };
    let mut malformed = source.clone();
    malformed.0[0] ^= 1;
    refused("Payload Size", emit_static(&malformed, &stock, &requested))?;
    let mut malformed = source.clone();
    let names = malformed.pointer(16)? + 16;
    let first = malformed.bytes::<4>(names)?;
    malformed.0[names + 4..names + 8].copy_from_slice(&first);
    refused(
        "Duplicate Name",
        emit_static(&malformed, &stock, &requested),
    )?;
    let mut malformed = source.clone();
    let header = malformed.pointer(32)?;
    malformed.0[header - 4..header].fill(0);
    refused("Typed Marker", emit_static(&malformed, &stock, &requested))?;
    let mut malformed = source.clone();
    let header = malformed.pointer(32)?;
    malformed.0[header + 8..header + 12].copy_from_slice(&0x808094BEu32.to_le_bytes());
    refused("Element Class", emit_static(&malformed, &stock, &requested))?;
    let mut malformed = source.clone();
    let header = malformed.pointer(32)?;
    malformed.0[header + 16 + 4 + 55] |= 0x80;
    refused(
        "Unnamed Group Bit",
        emit_static(&malformed, &stock, &requested),
    )?;
    let mut malformed = source.clone();
    malformed.0.push(0);
    let size = malformed.0.len() as u64;
    malformed.0[..8].copy_from_slice(&size.to_le_bytes());
    refused(
        "Trailing Record",
        emit_static(&malformed, &stock, &requested),
    )?;
    refused(
        "Missing Name",
        emit_static(&source, &stock, &BTreeSet::from([0x01020304])),
    )?;
    let all = Dictionary::read(&source, true)?.names.into_iter().collect();
    refused("Native Capacity", emit_static(&source, &stock, &all))?;
    let mut malformed = stock.clone();
    let parts = malformed.pointer(48)? + 16;
    malformed.0[parts + 4..parts + 6].copy_from_slice(&12u16.to_le_bytes());
    refused(
        "Overlapping Partitions",
        emit_static(&source, &malformed, &requested),
    )?;
    let mut malformed = stock.clone();
    let parts = malformed.pointer(48)? + 16;
    malformed.0[parts + 3] = 1;
    refused(
        "Partition Padding",
        emit_static(&source, &malformed, &requested),
    )?;
    ensure!(
        private.dynamic_mask(&source_mask).is_err(),
        "static imported bit accepted on wire"
    );
    refusals.push("Unencodable Dynamic Bit".to_owned());
    ensure!(
        baseline.definition_mask(&source_mask).is_err(),
        "unmapped definition bit accepted"
    );
    refusals.push("Unmapped Definition Bit".to_owned());
    let mut mask = [0u8; 56];
    mask[55] = 0x80;
    ensure!(
        private.definition_mask(&mask).is_err(),
        "unnamed source mask bit accepted"
    );
    refusals.push("Unnamed Mask Bit".to_owned());
    fs::create_dir_all(&output)?;
    fs::write(output.join("dictionary.bin"), &private.payload.0)?;
    fs::write(output.join("definition-mask.bin"), native_mask)?;
    let report = serde_json::json!({
        "source_sha256": format!("{:x}", Sha256::digest(&source.0)),
        "stock_sha256": format!("{:x}", Sha256::digest(&stock.0)),
        "private_sha256": format!("{:x}", Sha256::digest(&private.payload.0)),
        "names": private.names.len(), "added_static": private.added_static,
        "refusals": refusals, "stock_round_trip": true,
        "native_call_fixture_exact": true, "installable": false,
        "gameplay_verified": false
    });
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
