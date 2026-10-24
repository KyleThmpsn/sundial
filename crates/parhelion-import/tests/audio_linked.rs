//! Shared source definitions through conversion, namespace persistence and emitted routing.
use anyhow::{Context, Result, ensure};
use parhelion_import::tiger::audio::bank::{self, Bank, Namespace};
use serde_json::json;
use std::{collections::BTreeMap, fs};

fn source(bank: u32, target: u32, reverse: bool) -> Vec<u8> {
    let event = |id: u32, action: u32| {
        let mut v = id.to_le_bytes().to_vec();
        v.extend(1u32.to_le_bytes());
        v.extend(action.to_le_bytes());
        (4u8, v)
    };
    let mut action = 0x20u32.to_le_bytes().to_vec();
    action.extend(0x1C00u16.to_le_bytes());
    action.extend(0x30u32.to_le_bytes());
    action.extend([0, 0, 0]);
    let mut rows = vec![event(0x10, target), event(bank + 100, 0x20), (3, action)];
    if reverse {
        rows.reverse();
    }
    let mut hierarchy = (rows.len() as u32).to_le_bytes().to_vec();
    for (kind, row) in rows {
        hierarchy.push(kind);
        hierarchy.extend((row.len() as u32).to_le_bytes());
        hierarchy.extend(row);
    }
    let header = [89u32, bank, 0, 0, 7]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    for (name, bytes) in [(b"BKHD", header), (b"HIRC", hierarchy)] {
        output.extend(name);
        output.extend((bytes.len() as u32).to_le_bytes());
        output.extend(bytes);
    }
    output
}

fn read_hierarchy(bytes: &[u8]) -> Result<BTreeMap<u32, (u8, Vec<u8>)>> {
    let word = |a: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes.get(a..a + 4).context("Word")?.try_into()?,
        ))
    };
    let mut at = 0;
    while at < bytes.len() {
        let end = at + 8 + word(at + 4)? as usize;
        ensure!(end <= bytes.len(), "Chunk bounds");
        if &bytes[at..at + 4] == b"HIRC" {
            let count = word(at + 8)?;
            at += 12;
            let mut result = BTreeMap::new();
            for _ in 0..count {
                let kind = bytes[at];
                let n = word(at + 1)? as usize;
                let body = bytes
                    .get(at + 5..at + 5 + n)
                    .context("Object bounds")?
                    .to_vec();
                ensure!(
                    result.insert(word(at + 5)?, (kind, body)).is_none(),
                    "Duplicate emitted ID"
                );
                at += 5 + n;
            }
            ensure!(at == end, "Hierarchy extent");
            return Ok(result);
        }
        at = end;
    }
    anyhow::bail!("No hierarchy")
}

#[test]
fn shared_definitions_keep_one_route_and_conflicts_fail_before_emission() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let first = source(1, 0x20, false);
    let second = source(2, 0x20, true);
    let banks = [bank::upgrade(&first)?, bank::upgrade(&second)?];
    let encoded = serde_json::to_vec(&banks)?;
    fs::write(root.join("banks.json"), encoded)?;
    let banks: Vec<Bank> = serde_json::from_slice(&fs::read(root.join("banks.json"))?)?;
    let namespace = Namespace::new("shared-definition-e2e", &banks)?;
    let namespace: Namespace = serde_json::from_slice(&serde_json::to_vec(&namespace)?)?;
    let mut outputs = Vec::new();
    for (i, bank) in banks.iter().enumerate() {
        let bytes = namespace.instantiate(bank, &BTreeMap::new(), &BTreeMap::new())?;
        let path = root.join(format!("bank-{i}.bnk"));
        fs::write(&path, &bytes)?;
        outputs.push(read_hierarchy(&fs::read(path)?)?);
    }
    let common = outputs[0]
        .keys()
        .filter(|id| outputs[1].contains_key(id))
        .copied()
        .collect::<Vec<_>>();
    ensure!(
        common.len() == 2 && !common.contains(&0x10) && !common.contains(&0x20),
        "Shared objects lost private identity"
    );
    let event = *common
        .iter()
        .find(|id| outputs[0][id].0 == 4)
        .context("Shared event")?;
    let action = *common
        .iter()
        .find(|id| outputs[0][id].0 == 3)
        .context("Shared action")?;
    for objects in &outputs {
        let body = &objects[&event].1;
        ensure!(
            u32::from_le_bytes(body[4..8].try_into()?) == 1
                && u32::from_le_bytes(body[8..12].try_into()?) == action,
            "Persisted event does not target the shared action"
        );
        ensure!(
            u32::from_le_bytes(objects[&action].1[6..10].try_into()?) == 0x30,
            "External target was rewritten"
        );
    }
    let conflict = bank::upgrade(&source(2, 0x21, true))?;
    let error = Namespace::new("shared-definition-e2e", &[banks[0].clone(), conflict])
        .expect_err("Conflicting event definitions must be rejected");
    ensure!(
        format!("{error:#}").contains("conflicting linked audio object 00000010"),
        "Conflict lost its object identity: {error:#}"
    );
    let mut changed = serde_json::to_value(&banks[1])?;
    let reference = changed["refs"]
        .as_array_mut()
        .context("Relocations")?
        .iter_mut()
        .find(|r| r["value"] == 0x20 && r["offset"].as_u64().is_some_and(|offset| offset > 40))
        .context("Object relocation")?;
    reference["kind"] = json!("Media");
    let changed: Bank = serde_json::from_value(changed)?;
    ensure!(
        Namespace::new("shared-definition-e2e", &[banks[0].clone(), changed]).is_err(),
        "Equal bytes with incompatible relocation roles were accepted"
    );
    fs::write(
        root.join("verification.json"),
        serde_json::to_vec_pretty(
            &json!({"source_version":89,"target_version":113,"shared_event":event,"shared_action":action,"conflict":format!("{error:#}"),"source_banks":[first,second],"limits":"Conversion and emitted event routing. No live audio playback."}),
        )?,
    )?;
    println!("Audio definition artifact: {}", temporary.keep().display());
    Ok(())
}
