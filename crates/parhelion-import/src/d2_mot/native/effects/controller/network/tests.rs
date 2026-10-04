//! Explicitly configured package-to-owner verification, specified before emission.
use super::*;
use serde_json::json;
use std::{fs, path::PathBuf};

#[test]
#[ignore = "Requires PARHELION_NETWORK_CORPUS and PARHELION_NETWORK_OUTPUT"]
fn native_network_owner_and_dispatch() -> Result<()> {
    let directory =
        PathBuf::from(std::env::var_os("PARHELION_NETWORK_CORPUS").context("network corpus")?);
    let output =
        PathBuf::from(std::env::var_os("PARHELION_NETWORK_OUTPUT").context("network output")?);
    let read = |path| -> Result<Payload> { Ok(Payload(fs::read(path)?)) };
    let source = read(directory.join("modern/80CEA2EF.bin"))?;
    let template = read(directory.join("shadowkeep/80C709E1.bin"))?;
    let allocation = read(directory.join("shadowkeep/80C70B90.bin"))?;
    let mut metadata = BTreeMap::new();
    for side in ["modern", "shadowkeep"] {
        for row in fs::read_dir(directory.join(side))? {
            let path = row?.path();
            if path.extension().is_some_and(|v| v == "bin") {
                let tag = u32::from_str_radix(
                    path.file_stem()
                        .context("metadata name")?
                        .to_str()
                        .context("metadata name")?,
                    16,
                )?;
                metadata.insert(tag, read(path)?);
            }
        }
    }
    let native = emit(
        &source,
        &template,
        &allocation,
        &metadata,
        0x81FE0108,
        0x81FE0109,
    )?;
    ensure!(
        native.objects.len() == 9 && native.omitted.len() == 1,
        "network interface coverage"
    );
    for row in &native.objects[2..] {
        let source_at = usize::try_from(row.source.offset)?;
        let native_at = usize::try_from(row.target.offset)?;
        let m = Interface::read(
            &source,
            row.source,
            &metadata[&source.u32(source_at + 8)?],
            true,
        )?;
        let n = Interface::read(
            &native.owner,
            row.target,
            &metadata[&native.owner.u32(native_at + 8)?],
            false,
        )?;
        ensure!(
            m.methods.len() == n.methods.len() && m.methods[0].arguments == n.methods[0].arguments,
            "network dispatch arguments changed"
        );
    }
    let source_tag = source.u32(source.pointer(16)?)?;
    let native_tag = native.owner.u32(native.owner.pointer(16)?)?;
    ensure!(
        source_tag != native_tag && native_tag == 0x81FE0108,
        "private owner was not allocated"
    );
    let mut rejected = 0;
    for at in [0, source.pointer(16)? + 16, source.pointer(24)? + 0x1A8] {
        let mut bad = source.clone();
        bad.0[at] ^= 1;
        ensure!(
            emit(
                &bad,
                &template,
                &allocation,
                &metadata,
                0x81FE0108,
                0x81FE0109
            )
            .is_err(),
            "invalid source network form accepted"
        );
        rejected += 1;
    }
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &BTreeMap::new(),
            0x81FE0108,
            0x81FE0109
        )
        .is_err(),
        "network metadata omission accepted"
    );
    rejected += 1;
    fs::create_dir_all(&output)?;
    fs::write(output.join("owner.bin"), &native.owner.0)?;
    fs::write(output.join("allocation.bin"), &native.allocation.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "objects":native.objects,"omitted":native.omitted,"rejection_checks":rejected,
            "installable":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
