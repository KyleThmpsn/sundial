use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

/// The model header's draw index capacity.
///
/// Every draw record carries a model-wide draw index, and the client sizes its per-view record
/// sets from this field. Across 20,412 stock models it is the record count rounded up to a
/// multiple of 32, with records numbered from 0, whatever the count (up to 446 records). The
/// other 829 leave both it and the layout flags at 0x38 zero. A header copied from a smaller
/// model under-declares it, and per-view extraction then writes past its set into the next
/// view job, which hangs the world as soon as the model is drawn.
pub const DRAW_INDEX_CAPACITY: usize = 0x30;

/// The model-wide draw index of a native draw record.
pub const DRAW_INDEX: usize = 22;

/// The visibility group of the selected carrier mesh's main opaque body.
///
/// Native bitmap production tests each record's group against the render
/// instance's enabled groups before enabling its model-wide draw index. Group
/// zero is not a universal base group. Sum the highest-detail opaque primitive counts
/// so a small accessory or a repeated lower-detail body cannot select the group.
pub fn body_group(model: &Payload, mesh: usize) -> Result<u16> {
    let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
    let start = usize::from(model.u16(mesh + 40)?);
    let end = usize::from(model.u16(mesh + 42)?);
    let mut groups = std::collections::BTreeMap::<u16, u64>::new();
    for &part in parts.get(start..end).context("Invalid opaque draw range")? {
        // Native LOD categories are coverage sets. These include level zero,
        // including carriers whose geometry spans several detail levels.
        if !matches!(model.u8(part + 27)?, 0..=3 | 10) {
            continue;
        }
        let group = model.u16(part + 20)?;
        ensure!(
            group <= i16::MAX as u16,
            "Invalid native body visibility group"
        );
        *groups.entry(group).or_default() += u64::from(model.u32(part + 16)?);
    }
    let largest = groups
        .values()
        .copied()
        .max()
        .context("No native opaque body group")?;
    ensure!(largest > 0, "Empty native opaque body group");
    let mut candidates = groups.into_iter().filter(|(_, count)| *count == largest);
    let group = candidates.next().context("Missing native body group")?.0;
    ensure!(
        candidates.next().is_none(),
        "Ambiguous native body visibility group"
    );
    Ok(group)
}

/// The draw index capacity stock models declare for `records` draw records.
pub fn draw_index_capacity(records: usize) -> Result<u32> {
    Ok(u32::try_from(records.div_ceil(32) * 32)?)
}

fn zero_layout(model: &Payload) -> Result<bool> {
    Ok(model.u32(DRAW_INDEX_CAPACITY)? == 0 && model.u64(0x38)? == 0)
}

/// Declare `records` draw indices in a model header.
///
/// Every writer that changes a header's record count calls this, because the header it starts
/// from belongs to another model. A header in the zero layout keeps it.
pub fn declare_draw_indices(header: &mut [u8], records: usize) -> Result<()> {
    let model = Payload(header.get(..0x40).context("short model header")?.to_vec());
    if !zero_layout(&model)? {
        header[DRAW_INDEX_CAPACITY..DRAW_INDEX_CAPACITY + 4]
            .copy_from_slice(&draw_index_capacity(records)?.to_le_bytes());
    }
    Ok(())
}

/// Declare every draw record of a finished model payload.
///
/// Graphs imported before the importer declared draw indices still carry their host's
/// capacity, so installs run this over the model they read back.
pub fn declare_model_draw_indices(model: &mut [u8]) -> Result<()> {
    let payload = Payload(model.to_vec());
    let mut records = 0;
    for mesh in payload.array(16, 136, Some(0x8080_7378))? {
        records += payload.array(mesh + 24, 32, Some(0x8080_737E))?.len();
    }
    declare_draw_indices(model, records)
}

pub(crate) fn validate(model: &Payload, mesh: usize, parts: &[usize]) -> Result<()> {
    if !zero_layout(model)? {
        let capacity = model.u32(DRAW_INDEX_CAPACITY)?;
        ensure!(
            capacity == draw_index_capacity(parts.len())?,
            "model declares {capacity} draw indices for {} draw records",
            parts.len()
        );
    }
    let mut numbered = vec![false; parts.len()];
    for &part in parts {
        let index = usize::from(model.u16(part + DRAW_INDEX)?);
        ensure!(
            numbered.get(index) == Some(&false),
            "draw index {index} repeats or lies outside the {} draw records",
            parts.len()
        );
        numbered[index] = true;
    }
    for stage in 0..23 {
        let start = model.u16(mesh + 40 + stage * 2)? as usize;
        let end = model.u16(mesh + 42 + stage * 2)? as usize;
        ensure!(
            start <= end && end <= parts.len(),
            "draw stage {stage} range is invalid"
        );
        let mut index = start;
        while index < end {
            let count = model.u8(parts[index] + 29)? as usize;
            ensure!(
                count > 0,
                "draw stage {stage}, part {index} has zero group length and stalls the native iterator"
            );
            ensure!(count <= end - index, "draw group exceeds its stage range");
            index += count;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESH: usize = 0xB0;
    const PARTS: [usize; 2] = [0x150, 0x170];

    /// Two single-stage records in one mesh, numbered and declared the way stock models are.
    fn sample() -> Payload {
        let mut bytes = vec![0; 0x190];
        for stage in 1..24 {
            bytes[MESH + 40 + stage * 2..MESH + 42 + stage * 2]
                .copy_from_slice(&2u16.to_le_bytes());
        }
        bytes[DRAW_INDEX_CAPACITY..DRAW_INDEX_CAPACITY + 4].copy_from_slice(&32u32.to_le_bytes());
        bytes[0x38] = 1;
        for (index, part) in PARTS.into_iter().enumerate() {
            bytes[part + DRAW_INDEX] = index as u8;
            bytes[part + 29] = 1;
        }
        Payload(bytes)
    }

    #[test]
    fn native_iterator_requires_progress_within_each_stage() {
        let mut model = sample();
        assert!(validate(&model, MESH, &PARTS).is_ok());
        model.0[PARTS[0] + 29] = 0;
        assert!(validate(&model, MESH, &PARTS).is_err());
        model.0[PARTS[0] + 29] = 2;
        assert!(validate(&model, MESH, &PARTS).is_ok());
        model.0[PARTS[0] + 29] = 3;
        assert!(validate(&model, MESH, &PARTS).is_err());
    }

    #[test]
    fn draw_indices_fit_the_declared_capacity() {
        let mut model = sample();
        model.0[PARTS[1] + DRAW_INDEX] = 0;
        assert!(validate(&model, MESH, &PARTS).is_err(), "repeated index");
        model.0[PARTS[1] + DRAW_INDEX] = 2;
        assert!(
            validate(&model, MESH, &PARTS).is_err(),
            "index past the records"
        );

        let mut model = sample();
        model.0[DRAW_INDEX_CAPACITY] = 0;
        assert!(
            validate(&model, MESH, &PARTS).is_err(),
            "undeclared in the flagged layout"
        );
        model.0[0x38] = 0;
        assert!(validate(&model, MESH, &PARTS).is_ok(), "zero layout");

        let mut model = sample();
        model.0[DRAW_INDEX_CAPACITY] = 64;
        assert!(
            validate(&model, MESH, &PARTS).is_err(),
            "stock declares exactly the rounded count"
        );
    }

    #[test]
    fn declared_capacity_follows_the_record_count() {
        for (records, capacity) in [
            (1, 32),
            (32, 32),
            (33, 64),
            (64, 64),
            (77, 96),
            (235, 256),
            (446, 448),
        ] {
            assert_eq!(draw_index_capacity(records).unwrap(), capacity);
        }
        let mut header = sample().0;
        header[DRAW_INDEX_CAPACITY..DRAW_INDEX_CAPACITY + 4].copy_from_slice(&96u32.to_le_bytes());
        declare_draw_indices(&mut header, 235).unwrap();
        assert_eq!(Payload(header).u32(DRAW_INDEX_CAPACITY).unwrap(), 256);

        let mut zero = vec![0; 0x40];
        declare_draw_indices(&mut zero, 235).unwrap();
        assert_eq!(Payload(zero).u32(DRAW_INDEX_CAPACITY).unwrap(), 0);
    }

    #[test]
    fn a_finished_model_declares_all_of_its_records() {
        // One mesh with 40 records, laid out the way the importer writes models, still
        // carrying a 32-index capacity from the model it was built on.
        let records = 40;
        let mut model = vec![0; 0x150 + records * 32];
        let mut put = |at: usize, bytes: &[u8]| model[at..at + bytes.len()].copy_from_slice(bytes);
        put(16, &1u64.to_le_bytes());
        put(24, &(0xA0i64 - 24).to_le_bytes());
        put(0xA0, &1u64.to_le_bytes());
        put(0xA8, &0x8080_7378u32.to_le_bytes());
        put(MESH + 24, &(records as u64).to_le_bytes());
        put(MESH + 32, &(0x140i64 - (MESH as i64 + 32)).to_le_bytes());
        put(0x140, &(records as u64).to_le_bytes());
        put(0x148, &0x8080_737Eu32.to_le_bytes());
        put(DRAW_INDEX_CAPACITY, &32u32.to_le_bytes());
        put(0x38, &[1]);
        declare_model_draw_indices(&mut model).unwrap();
        assert_eq!(Payload(model).u32(DRAW_INDEX_CAPACITY).unwrap(), 64);
    }
}
