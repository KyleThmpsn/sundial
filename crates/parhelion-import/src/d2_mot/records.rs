//! Read-only validation matching Sunrise's record-objective array contract.
use crate::d2_mot::reader::{Reader, write_json};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn audit(r: &mut Reader) -> Result<Value> {
    let global_tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .context("investment globals")?
        .hash
        .0;
    let globals = r.tag(global_tag, None)?;
    let root = r.tag(globals.u32(16)?, None)?;
    let tag = root.u32(8 + 72 * 16)?;
    let table = r.tag(tag, None)?;
    let rows = table.array(8, 0xD8, Some(0x80807452))?;
    let mut failures = vec![];
    let mut authored = vec![];
    for (index, &row) in rows.iter().enumerate() {
        let hash = table.u32(row + 0x28)?;
        let descriptor = row + 0x30;
        let result = (|| -> Result<Value> {
            let entries = table.array(descriptor, 2, Some(0x80807455))?;
            let marker = if entries.is_empty() {
                None
            } else {
                let header = table.pointer(descriptor + 8)?;
                ensure!(header >= 4, "objective array header underflow");
                let marker = table.u32(header - 4)?;
                ensure!(
                    marker >> 16 == 0x8080,
                    "objective array marker {marker:08X}"
                );
                Some(format!("{marker:08X}"))
            };
            let objectives = entries
                .iter()
                .map(|&at| table.u16(at))
                .collect::<Result<Vec<_>>>()?;
            Ok(
                json!({"row":index,"hash":format!("{hash:08X}"),"marker":marker,"objectives":objectives}),
            )
        })();
        match result {
            Ok(value) if index >= 2242 => authored.push(value),
            Ok(_) => (),
            Err(error) => failures
                .push(json!({"row":index,"hash":format!("{hash:08X}"),"error":error.to_string()})),
        }
    }
    let report = json!({"table":format!("{tag:08X}"),"record_count":rows.len(),"authored":authored,"failures":failures,"passed":failures.is_empty()});
    write_json(&r.output.join("records.json"), &report)?;
    r.finish()?;
    ensure!(
        failures.is_empty(),
        "{} malformed record objective arrays; see records.json",
        failures.len()
    );
    Ok(report)
}
