//! Translate game state and render-group bindings separately from Havok storage.
use super::*;

pub(super) fn build(
    source: &Payload,
    bindings: &Value,
    model: &Payload,
    mesh: usize,
    vertices: usize,
    template: &Payload,
) -> Result<Vec<u8>> {
    let states = bindings["states"].as_array().context("Cloth states")?;
    let buffers = bindings["buffers"].as_array().context("Cloth buffers")?;
    ensure!(
        states.len() == 11 && source.u32(0x6A4)? == 0,
        "Source cloth game definition differs"
    );
    let mut bytes = template
        .0
        .get(..0x698)
        .context("Native cloth definition prefix")?
        .to_vec();
    bytes[8..0x690].fill(0);
    put(&mut bytes, 0x690, &u32::MAX.to_le_bytes())?;
    for role in 0..11 {
        let index = source.u32(8 + role * 8)? as usize;
        let flags = source.u32(12 + role * 8)?;
        ensure!(
            index < states.len() && flags & !0x301 == 0,
            "Unsupported cloth state or flags"
        );
        let transition = if flags & 1 != 0 {
            let constraints = states[index]["transition_constraints"]
                .as_array()
                .context("Cloth transition constraints")?;
            ensure!(
                constraints.len() == 1,
                "Cloth state has ambiguous transition constraints"
            );
            u32::try_from(number(&constraints[0])?)?
        } else {
            u32::MAX
        };
        put(&mut bytes, 8 + role * 12, &(index as u32).to_le_bytes())?;
        put(&mut bytes, 12 + role * 12, &transition.to_le_bytes())?;
        put(&mut bytes, 16 + role * 12, &(flags >> 8).to_le_bytes())?;
    }
    let parts = model.array(mesh + 32, 36, None)?;
    let mut used_buffers = std::collections::BTreeSet::new();
    let mut vertex_ranges = Vec::new();
    let mut stage_parts = vec![std::collections::BTreeSet::new(); 24];
    for group in 0..4 {
        let from = 0x60 + group * 0x190;
        let to = 0x90 + group * 0x180;
        let mut rows = Vec::new();
        for row in source.array(from, 12, Some(0x80806D66))? {
            let offset = source.u32(row)? as usize;
            let buffer = source.u32(row + 4)? as usize;
            let count = source.u32(row + 8)? as usize;
            let definition = buffers
                .get(buffer)
                .context("Cloth display buffer is absent")?;
            ensure!(
                definition["type"] == 4
                    && number(&definition["vertices"])? == count
                    && used_buffers.insert(buffer),
                "Cloth display buffer is duplicated or has a different length"
            );
            ensure!(
                offset.is_multiple_of(48)
                    && offset
                        .checked_add(count * 48)
                        .is_some_and(|n| n <= vertices * 48),
                "Cloth output exceeds its float vertex buffer"
            );
            let layout = &definition["layout"];
            ensure!(
                layout["numSlots"] == 1
                    && layout["slots"][0]["stride"] == 48
                    && layout["triangleFormat"] == 1,
                "Cloth display layout differs from native float PNT"
            );
            for (i, start) in [0, 16, 32].into_iter().enumerate() {
                let e = &layout["elementsLayout"][i];
                ensure!(
                    e["vectorConversion"] == 0
                        && e["vectorSize"] == 16
                        && e["slotId"] == 0
                        && e["slotStart"] == start,
                    "Cloth display element layout differs"
                );
            }
            vertex_ranges.push(offset / 48..offset / 48 + count);
            rows.extend_from_slice(&source.0[row..row + 12]);
        }
        append_array(&mut bytes, to, 0x80807280, &rows, 12)?;
        for (stage, selected) in stage_parts.iter_mut().enumerate() {
            let local = source.array(from + 16 + stage * 16, 4, Some(0x80800007))?;
            let start = model.u16(mesh + 48 + stage * 2)? as usize;
            let end = model.u16(mesh + 50 + stage * 2)? as usize;
            let range = parts
                .get(start..end)
                .context("Cloth render stage exceeds parts")?;
            let mut indices = Vec::new();
            for at in local {
                let index = source.u32(at)? as usize;
                let &draw = range
                    .get(index)
                    .context("Cloth render group exceeds its stage")?;
                if stage < 23 {
                    ensure!(
                        selected.insert(index),
                        "Cloth draw belongs to multiple render groups"
                    );
                    let simulated = model.u8(draw + 29)? == 3 && model.u8(draw + 30)? == 127;
                    ensure!(
                        simulated != rows.is_empty(),
                        "Cloth render group and simulation stream disagree"
                    );
                    indices.extend((index as u32).to_le_bytes());
                }
            }
            if stage < 23 {
                append_array(&mut bytes, to + 16 + stage * 16, 0x80800007, &indices, 4)?;
            }
        }
    }
    vertex_ranges.sort_by_key(|range| range.start);
    ensure!(
        vertex_ranges.first().is_some_and(|r| r.start == 0)
            && vertex_ranges.last().is_some_and(|r| r.end == vertices)
            && vertex_ranges.windows(2).all(|w| w[0].end == w[1].start),
        "Cloth display buffers do not partition the model vertices"
    );
    ensure!(
        used_buffers.len() == buffers.iter().filter(|b| b["type"] == 4).count(),
        "Cloth display buffer lacks a game binding"
    );
    for (stage, selected) in stage_parts.iter().take(23).enumerate() {
        let count =
            usize::from(model.u16(mesh + 50 + stage * 2)? - model.u16(mesh + 48 + stage * 2)?);
        ensure!(
            selected.len() == count,
            "Cloth render groups do not cover their stage"
        );
    }
    Ok(bytes)
}
