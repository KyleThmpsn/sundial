//! Verify the invisible native parts that retain attachment markers.
use super::*;
use std::collections::BTreeMap;

pub(super) fn audit(
    graph: &Value,
    symbols: &Value,
    assignments: &Payload,
    base: &PackageManager,
    read: &impl Fn(u32) -> Result<Payload>,
) -> Result<BTreeMap<u32, u32>> {
    let mut kept = BTreeMap::new();
    let mut keys = BTreeSet::new();
    let rows = assignments.array(8, 8, None)?;
    let nodes = graph["nodes"].as_array().context("graph nodes")?;
    let tag = |name: &str| -> Result<u32> {
        Ok(u32::try_from(
            symbols[name].as_u64().context("kept part symbol")?,
        )?)
    };
    for part in graph["kept_parts"].as_array().into_iter().flatten() {
        let assignment = u32::try_from(part["assignment"].as_u64().context("kept assignment")?)?;
        let key = u32::try_from(part["key"].as_u64().context("kept key")?)?;
        ensure!(
            kept.insert(assignment, key).is_none() && keys.insert(key),
            "duplicate kept part assignment or private key"
        );
        let parent = part["parent"].as_str().context("kept parent")?;
        let prefix = parent.strip_suffix("-parent").context("kept parent role")?;
        let row = rows
            .iter()
            .find(|&&row| assignments.u32(row).ok() == Some(key))
            .context("kept part assignment missing")?;
        ensure!(
            assignments.u32(row + 4)? == tag(parent)?,
            "kept part parent not linked"
        );
        ensure!(
            read(tag(parent)?)?.u32(16)? == tag(&format!("{prefix}-entity"))?,
            "kept part entity not linked"
        );
        let model_name = format!("{prefix}-model");
        if let Some(node) = nodes.iter().find(|node| node["symbol"] == model_name) {
            let model = read(tag(&model_name)?)?;
            ensure!(
                model.f32(0x6C)? == 0.0,
                "kept part {model_name} still has visible geometry"
            );
            let template =
                u32::try_from(node["template"].as_u64().context("kept model template")?)?;
            let mut original = base.read_tag(TagHash(template))?;
            original
                .get_mut(0x6C..0x70)
                .context("kept model position scale")?
                .fill(0);
            ensure!(
                model.0 == original,
                "kept part {model_name} changed native data outside its position scale"
            );
        }
    }
    Ok(kept)
}
