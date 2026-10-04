//! External-package control-array oracle, specified before additional flows.
use super::super::NATIVE_RANGE_CLASS;
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn snapshot(payload: &Payload) -> Result<serde_json::Value> {
    let mut nodes = Vec::new();
    let definition = payload.pointer(24)?;
    for row in payload.array(definition + 0x158, 24, Some(0x808093E6))? {
        let at = payload.pointer(row + 16)?;
        let class = payload.u32(at - 4)?;
        let size = match class {
            0x808093D7 | 0x808093D9 => 0x58,
            0x808093CD | 0x808093D5 => 0x68,
            _ => anyhow::bail!("unsupported native oracle control {class:08X}"),
        };
        let instance = usize::try_from(payload.u64(at + 8)?)?;
        anyhow::ensure!(
            payload.u64(instance + 8)? == at as u64,
            "control reciprocal offset differs"
        );
        anyhow::ensure!(
            payload.u32(instance + 4)? == class,
            "control reciprocal class differs"
        );
        anyhow::ensure!(
            payload.u32(instance - 4)? == payload.u32(at + 4)?,
            "control runtime class differs"
        );
        let mut fixed = payload
            .0
            .get(at..at + size)
            .context("control fixed extent")?
            .to_vec();
        fixed[0..4].fill(0);
        fixed[8..16].fill(0);
        for field in [0x40, 0x48, 0x50] {
            fixed[field..field + 8].fill(0);
        }
        let mut runtime = payload
            .0
            .get(instance..instance + 0x60)
            .context("control runtime extent")?
            .to_vec();
        runtime[0..4].fill(0);
        runtime[8..24].fill(0);
        let children = payload
            .array(at + 0x38, 4, Some(0x808093FB))?
            .into_iter()
            .map(|row| payload.bytes::<4>(row))
            .collect::<Result<Vec<_>>>()?;
        let mut conditions = Vec::new();
        for field in [at + 0x48, at + 0x50] {
            if payload.u64(field)? == 0 {
                conditions.push(None);
            } else {
                let condition = payload.pointer(field)?;
                anyhow::ensure!(
                    payload.u32(condition - 4)? == NATIVE_RANGE_CLASS,
                    "oracle condition class differs"
                );
                conditions.push(Some(payload.bytes::<16>(condition)?));
            }
        }
        let weights = if class == 0x808093D5 {
            fixed[0x60..0x68].fill(0);
            payload
                .array(at + 0x58, 4, Some(0x8080000F))?
                .into_iter()
                .map(|row| payload.bytes::<4>(row))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        nodes.push(serde_json::json!({"class": class, "fixed": fixed, "runtime": runtime,
                                    "children": children, "conditions": conditions, "weights": weights}));
    }
    Ok(serde_json::Value::Array(nodes))
}

#[test]
#[ignore = "requires explicitly configured source and native sequence exports"]
fn additional_flow_package_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_FLOW_OUTPUT")?);
    anyhow::ensure!(!output.exists(), "flow oracle output already exists");
    let pairs: serde_json::Value = serde_json::from_slice(&fs::read(corpus.join("pairs.json"))?)?;
    let goldens: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("flow-golden-pairs.json"))?)?;
    let goldens = goldens.as_array().context("complete flow golden pairs")?;
    let mut artifacts = Vec::new();
    let mut refusals = 0;
    let mut whole_owners = 0;
    let mut named_controls = 0;
    for pair in pairs["pairs"].as_array().context("flow pair list")? {
        let whole_owner = goldens.iter().any(|golden| {
            golden["source"] == pair["source"]["tag"] && golden["native"] == pair["native"]["tag"]
        });
        let source_nodes = pair["source"]["nodes"].as_array().context("flow nodes")?;
        if !source_nodes.iter().any(|node| {
            matches!(
                node["cls"].as_str(),
                Some("808091D9" | "808091E1" | "808091E5")
            )
        }) {
            continue;
        }
        let source_tag = pair["source"]["tag"].as_str().context("source tag")?;
        let native_tag = pair["native"]["tag"].as_str().context("native tag")?;
        let source = Payload(fs::read(
            corpus.join("modern").join(format!("{source_tag}.bin")),
        )?);
        let native = Payload(fs::read(
            corpus.join("native").join(format!("{native_tag}.bin")),
        )?);
        let sequence = Sequence::read(&source)?;
        let names = native
            .array(native.pointer(24)? + 0x188, 40, Some(0x80809789))?
            .into_iter()
            .map(|row| native.u32(row + 32))
            .collect::<Result<Vec<_>>>()?;
        let mut inputs = BTreeMap::new();
        for (index, input) in sequence.inputs.iter().enumerate() {
            let found: Vec<_> = names
                .iter()
                .enumerate()
                .filter(|(_, name)| **name == input.name)
                .collect();
            anyhow::ensure!(found.len() == 1, "flow input has no unique native name");
            inputs.insert(u32::try_from(index)?, u32::try_from(found[0].0)?);
        }
        let mut emitted = native.clone();
        let allocation = write(&source, &sequence, &mut emitted, &inputs, names.len())?;
        anyhow::ensure!(
            allocation.children.len() == sequence.controls.len(),
            "flow allocation count differs"
        );
        let actual = snapshot(&emitted)?;
        let expected = snapshot(&native)?;
        if whole_owner {
            anyhow::ensure!(
                actual == expected,
                "complete control arrays differ for {source_tag} to {native_tag}"
            );
            whole_owners += 1;
        }
        let named = |snapshot: &serde_json::Value| -> Result<Vec<serde_json::Value>> {
            Ok(snapshot
                .as_array()
                .context("control snapshot rows")?
                .iter()
                .filter(|row| row["class"].as_u64() == Some(0x808093CD))
                .cloned()
                .collect())
        };
        let actual_named = named(&actual)?;
        anyhow::ensure!(
            actual_named == named(&expected)?,
            "named-input control records differ for {source_tag} to {native_tag}"
        );
        named_controls += actual_named.len();
        for control in sequence
            .controls
            .iter()
            .filter(|control| control.class == 0x808091D9)
        {
            let at = control.offset;
            for field in [0x48, 0x4C, 0x50] {
                let mut invalid = source.clone();
                invalid.0[at + field..at + field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                let invalid_sequence = Sequence::read(&invalid)?;
                anyhow::ensure!(
                    write(
                        &invalid,
                        &invalid_sequence,
                        &mut native.clone(),
                        &inputs,
                        names.len()
                    )
                    .is_err(),
                    "malformed named-input control accepted"
                );
                refusals += 1;
            }
            for bounds in [[f32::INFINITY, 1.0], [2.0, 1.0]] {
                let mut invalid = source.clone();
                invalid.0[at + 0x40..at + 0x44].copy_from_slice(&bounds[0].to_le_bytes());
                invalid.0[at + 0x44..at + 0x48].copy_from_slice(&bounds[1].to_le_bytes());
                anyhow::ensure!(
                    write(
                        &invalid,
                        &sequence,
                        &mut native.clone(),
                        &inputs,
                        names.len()
                    )
                    .is_err(),
                    "invalid named-input control bounds accepted"
                );
                refusals += 1;
            }
            let mut unmapped = inputs.clone();
            unmapped.remove(&source.u32(at + 0x48)?);
            anyhow::ensure!(
                write(
                    &source,
                    &sequence,
                    &mut native.clone(),
                    &unmapped,
                    names.len()
                )
                .is_err(),
                "unmapped named-input control accepted"
            );
            refusals += 1;
            let mut outside = inputs.clone();
            outside.insert(source.u32(at + 0x48)?, u32::try_from(names.len())?);
            anyhow::ensure!(
                write(
                    &source,
                    &sequence,
                    &mut native.clone(),
                    &outside,
                    names.len()
                )
                .is_err(),
                "out-of-range named-input control accepted"
            );
            refusals += 1;
        }
        for control in sequence
            .controls
            .iter()
            .filter(|control| control.class == 0x808091E1)
        {
            let weights = source.array(control.offset + 0x40, 4, Some(0x8080000F))?;
            anyhow::ensure!(!weights.is_empty(), "weighted oracle fixture is empty");
            for value in [f32::NAN, -1.0] {
                let mut invalid = source.clone();
                invalid.0[weights[0]..weights[0] + 4].copy_from_slice(&value.to_le_bytes());
                let invalid_sequence = Sequence::read(&invalid)?;
                anyhow::ensure!(
                    write(
                        &invalid,
                        &invalid_sequence,
                        &mut native.clone(),
                        &inputs,
                        names.len()
                    )
                    .is_err(),
                    "malformed flow weight accepted"
                );
                refusals += 1;
            }
            let mut invalid = source.clone();
            for row in &weights {
                invalid.0[*row..*row + 4].fill(0);
            }
            let invalid_sequence = Sequence::read(&invalid)?;
            anyhow::ensure!(
                write(
                    &invalid,
                    &invalid_sequence,
                    &mut native.clone(),
                    &inputs,
                    names.len()
                )
                .is_err(),
                "zero flow weight total accepted"
            );
            refusals += 1;
        }
        let key = format!("{source_tag}-{native_tag}");
        artifacts.push((key, emitted.0, whole_owner));
    }
    anyhow::ensure!(
        whole_owners > 0 && whole_owners == goldens.len() && named_controls > 0 && refusals > 0,
        "flow package coverage differs"
    );
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (key, payload, whole_owner) in artifacts {
        fs::write(output.join(format!("{key}.bin")), &payload)?;
        rows.push(serde_json::json!({"key": key, "bytes": payload.len(),
            "whole_control_array_verified": whole_owner,
            "sha256": format!("{:x}", Sha256::digest(&payload))}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "owners": rows, "whole_control_arrays": whole_owners, "named_controls": named_controls,
            "malformed_refusals": refusals,
            "scope": "Complete control array and runtime comparison against exported native records. Other arrays in these diagnostic owner buffers remain the supplied native fixture. Whole sequence assembly and scheduling are unverified.",
            "installable": false, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
