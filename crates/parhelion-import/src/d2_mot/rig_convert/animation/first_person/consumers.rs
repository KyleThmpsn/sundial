//! Every first-person component that directly consumes the clip bank.
use super::*;
pub use crate::tiger::animation::bank_field;

pub(super) fn prepare(
    reader: &mut Reader,
    rig: &Value,
    entity: u32,
    bank: u32,
    graph: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for component in rig["components"]
        .as_array()
        .context("animation components")?
    {
        if hex(&component["entity"])? != entity
            || !matches!(component["class"].as_str(), Some("80803640" | "808036CF"))
        {
            continue;
        }
        let owner = hex(&component["owner"])?;
        let payload = reader.tag(owner, Some(0x80809C36))?;
        let field = bank_field(&payload)?;
        ensure!(
            payload.u32(field)? == bank,
            "animation consumer names another bank"
        );
        ensure!(
            word_occurrences(&payload.0, bank) == 1,
            "animation consumer bank reference is ambiguous"
        );
        let key = format!("bank_consumer_{}", result.len());
        let file = format!("animation/bank-consumer-{owner:08X}.bin");
        fs::write(graph.join(&file), &payload.0)?;
        files.insert(key.clone(), file);
        result.push(json!({"owner":owner,"file_key":key}));
    }
    ensure!(
        result.len() == 2,
        "first-person bank consumers missing or ambiguous"
    );
    Ok(result)
}
