//! Preserve source solvers and validate native state, skeleton and display bindings.
use super::*;
use serde_json::{Value, json};

pub(super) fn native_bindings(
    owner: &Payload,
    resource: usize,
    solver: &[u8],
    bones: u32,
) -> Result<Value> {
    let report = crate::tiger::physics::cloth::inspect_native(solver)?;
    let bindings = &report["bindings"];
    for count in bindings["transforms"]
        .as_array()
        .context("Cloth transform sets")?
    {
        ensure!(
            count.as_u64().is_some_and(|n| n <= u64::from(bones)),
            "Cloth transform set exceeds the validated body skeleton"
        );
    }
    let states = bindings["states"].as_array().context("Cloth states")?;
    for role in 0..11 {
        let at = resource + 0x338 + role * 12;
        let state = states
            .get(owner.u32(at)? as usize)
            .context("Cloth role selects an absent state")?;
        let constraint = owner.u32(at + 4)?;
        ensure!(
            owner.u32(at + 8)? < 4,
            "Cloth role selects an absent display group"
        );
        if constraint != u32::MAX {
            let transitions = state["transition_constraints"]
                .as_array()
                .context("Cloth state transitions")?;
            ensure!(
                !transitions.is_empty()
                    && transitions
                        .iter()
                        .all(|v| v.as_u64() == Some(u64::from(constraint))),
                "Cloth role selects a different transition constraint"
            );
        }
    }
    let buffers = bindings["buffers"].as_array().context("Cloth buffers")?;
    let mut selected = std::collections::BTreeSet::new();
    for group in 0..4 {
        for row in owner.array(resource + 0x3C0 + group * 0x140, 12, Some(0x80801AE0))? {
            let index = owner.u32(row + 4)? as usize;
            let buffer = buffers
                .get(index)
                .context("Display binding selects an absent cloth buffer")?;
            ensure!(
                buffer["type"] == 4
                    && buffer["vertices"].as_u64() == Some(u64::from(owner.u32(row + 8)?))
                    && selected.insert(index),
                "Cloth display buffer type, size or ownership differs"
            );
        }
    }
    ensure!(
        buffers
            .iter()
            .enumerate()
            .filter(|(_, b)| b["type"] == 4)
            .all(|(i, _)| selected.contains(&i)),
        "A cloth display buffer has no model binding"
    );
    Ok(report)
}

/// Resolve simulated draws from the owner's display bindings. Source draw flags also
/// describe other render roles, so they cannot identify a cloth buffer by themselves.
/// Each simulated draw must address vertices written by its own group's display buffers.
pub(super) fn simulated_draws(
    owner: &Payload,
    resource: usize,
    model: &Payload,
    mesh: usize,
    indices: &[u32],
    restart: u32,
    vertices: usize,
) -> Result<std::collections::BTreeSet<usize>> {
    let parts = model.array(mesh + 0x48, 36, Some(0x80801AEF))?;
    let mut covered = std::collections::BTreeSet::new();
    let mut simulated = std::collections::BTreeSet::new();
    for group in 0..4 {
        let from = resource + 0x3C0 + group * 0x140;
        let bindings = owner.array(from, 12, Some(0x80801AE0))?;
        let ranges = bindings
            .iter()
            .map(|&row| -> Result<std::ops::Range<usize>> {
                let offset = owner.u32(row)? as usize;
                let count = owner.u32(row + 8)? as usize;
                let first = offset / 48;
                let end = first
                    .checked_add(count)
                    .context("Source cloth vertex range overflow")?;
                ensure!(
                    offset.is_multiple_of(48) && count > 0 && end <= vertices,
                    "Source cloth display binding exceeds the float stream"
                );
                Ok(first..end)
            })
            .collect::<Result<Vec<_>>>()?;
        for stage in 0..19 {
            let first = usize::from(model.u16(mesh + 0x58 + stage * 2)?);
            let end = usize::from(model.u16(mesh + 0x5A + stage * 2)?);
            let draws = parts
                .get(first..end)
                .context("Source cloth stage exceeds its draw array")?;
            for row in owner.array(from + 16 + stage * 16, 4, Some(0x80800007))? {
                let index = owner.u32(row)? as usize;
                let draw = *draws
                    .get(index)
                    .context("Source cloth group exceeds its stage")?;
                let ordinal = first + index;
                ensure!(
                    covered.insert(ordinal),
                    "A source cloth draw belongs to multiple groups"
                );
                if ranges.is_empty() {
                    continue;
                }
                let start = model.u32(draw + 8)? as usize;
                let count = model.u32(draw + 12)? as usize;
                let end = start
                    .checked_add(count)
                    .context("Source cloth draw range overflow")?;
                let draw_indices = indices
                    .get(start..end)
                    .context("Source cloth draw exceeds the index stream")?;
                ensure!(
                    draw_indices.iter().all(|&vertex| vertex == restart
                        || ranges
                            .iter()
                            .any(|range| range.contains(&(vertex as usize)))),
                    "Source cloth draw addresses vertices outside its display buffers"
                );
                simulated.insert(ordinal);
            }
        }
    }
    ensure!(
        covered.len() == parts.len(),
        "Source cloth display groups do not cover every draw"
    );
    Ok(simulated)
}

/// Move the inline D1 state and display bindings into the native definition.
/// Vertex order and stage-local draw order are retained by the model converter.
pub(super) fn native_definition(
    owner: &Payload,
    resource: usize,
    model: &Payload,
    mesh: usize,
    vertices: usize,
    template: &Payload,
) -> Result<Vec<u8>> {
    use crate::presentation::{append, put};
    let mut output = template
        .0
        .get(..0x698)
        .context("Native cloth definition prefix")?
        .to_vec();
    output[8..0x690].fill(0);
    put(&mut output, 0x690, &u32::MAX.to_le_bytes())?;
    for role in 0..11 {
        let at = resource + 0x338 + role * 12;
        ensure!(
            owner.u32(at + 8)? < 4,
            "Source cloth state selects an absent display group"
        );
        put(&mut output, 8 + role * 12, &owner.bytes::<12>(at)?)?;
    }
    let parts = model.array(mesh + 0x48, 36, Some(0x80801AEF))?;
    let mut coverage: [std::collections::BTreeSet<u32>; 19] =
        std::array::from_fn(|_| std::collections::BTreeSet::new());
    let mut buffers = std::collections::BTreeSet::new();
    let mut ranges = Vec::new();
    for group in 0..4 {
        let from = resource + 0x3C0 + group * 0x140;
        let to = 0x90 + group * 0x180;
        let bindings = owner.array(from, 12, Some(0x80801AE0))?;
        let mut records = Vec::new();
        for row in &bindings {
            let row = *row;
            let offset = owner.u32(row)? as usize;
            let count = owner.u32(row + 8)? as usize;
            ensure!(
                offset.is_multiple_of(48)
                    && count > 0
                    && offset
                        .checked_add(count * 48)
                        .is_some_and(|end| end <= vertices * 48),
                "Source cloth display binding exceeds the float stream"
            );
            ensure!(
                buffers.insert(owner.u32(row + 4)?),
                "Source cloth display buffer is bound twice"
            );
            ranges.push(offset / 48..offset / 48 + count);
            records.extend(owner.bytes::<12>(row)?);
        }
        append(&mut output, to, 0x80807280, &records, 12)?;
        for (stage, selected) in coverage.iter_mut().enumerate() {
            let start = usize::from(model.u16(mesh + 0x58 + stage * 2)?);
            let end = usize::from(model.u16(mesh + 0x5A + stage * 2)?);
            let draws = parts
                .get(start..end)
                .context("Source cloth stage exceeds its draw array")?;
            let mut indices = Vec::new();
            for row in owner.array(from + 16 + stage * 16, 4, Some(0x80800007))? {
                let index = owner.u32(row)?;
                draws
                    .get(index as usize)
                    .context("Source cloth group exceeds its stage")?;
                ensure!(
                    selected.insert(index),
                    "A source cloth draw belongs to multiple groups"
                );
                indices.extend(index.to_le_bytes());
            }
            append(&mut output, to + 16 + stage * 16, 0x80800007, &indices, 4)?;
        }
    }
    ranges.sort_by_key(|range| range.start);
    ensure!(
        ranges.first().is_some_and(|r| r.start == 0)
            && ranges.last().is_some_and(|r| r.end == vertices)
            && ranges.windows(2).all(|w| w[0].end == w[1].start),
        "Source cloth display buffers do not partition its float stream"
    );
    for (stage, selected) in coverage.iter().enumerate() {
        let count = model
            .u16(mesh + 0x5A + stage * 2)?
            .checked_sub(model.u16(mesh + 0x58 + stage * 2)?)
            .context("Source cloth stage ranges reverse")?;
        ensure!(
            selected.len() == usize::from(count),
            "Source cloth display groups do not cover every draw"
        );
    }
    let size = output.len() as u64;
    put(&mut output, 0, &size.to_le_bytes())?;
    Ok(output)
}

impl Source {
    pub(super) fn cloth_solver(
        &self,
        owner: &Payload,
        resource: usize,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let tag = owner.u32(resource + 0x8C0)?;
        let data = self.read(tag)?;
        ensure!(
            data.bytes::<8>(0)? == [0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10]
                && data.u32(12)? == 9
                && data.bytes::<4>(16)? == [8, 1, 0, 1]
                && data.bytes::<15>(40)? == *b"hk_2012.2.0-r1\0",
            "Unsupported Rise of Iron cloth packfile {tag:08X}"
        );
        let report = json!({"tag":tag,"sdk":"hk_2012.2.0-r1","pointer_bytes":8,
            "little_endian":true,"bytes":data.0.len(),"native_binding_verified":false});
        assets.insert(tag, data);
        Ok(report)
    }
}
