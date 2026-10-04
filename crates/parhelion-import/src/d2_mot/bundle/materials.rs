use super::{add, patch, put};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fs, path::Path};

pub(super) fn append(
    out: &Path,
    nodes: &mut Vec<Value>,
    mapped: &Path,
    mapping: &Value,
) -> Result<()> {
    for (symbol, material) in mapping["materials"]
        .as_object()
        .context("mapped materials")?
    {
        let tag = u32::from_str_radix(
            material["native_donor"]
                .as_str()
                .context("material donor")?,
            16,
        )?;
        let mut payload = Payload(fs::read(
            mapped.join(material["payload"].as_str().context("material payload")?),
        )?);
        let mut fixups = vec![];
        // Audited Hook alpha-clipped G-buffer shader: t3 albedo, t4 normal,
        // t5 gstack. Modern plated materials have no fixed texture rows.
        // Retain the alpha-capable shader, but point it at this weapon's plates.
        if tag == 0x80EC2704
            && material["stage"] == "GenerateGbuffer"
            && material["source_texture_slots"]
                .as_array()
                .context("source texture slots")?
                .is_empty()
        {
            let rows = payload.array(0x2D0, 8, None)?;
            for (slot, symbol) in [
                (3, "texture-albedo-header"),
                (4, "texture-normal-header"),
                (5, "texture-gstack-header"),
            ] {
                let matches = rows
                    .iter()
                    .copied()
                    .filter(|&o| payload.u32(o).ok() == Some(slot))
                    .collect::<Vec<_>>();
                ensure!(
                    matches.len() == 1,
                    "native plated shader slot {slot} missing or ambiguous"
                );
                let offset = matches[0] + 4;
                put(&mut payload.0, offset, &u32::MAX.to_le_bytes())?;
                fixups.push(patch(offset, symbol));
            }
        }
        add(out, nodes, symbol, tag, &payload.0, None, fixups)?;
    }
    Ok(())
}
