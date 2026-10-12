//! Keep appended FK poses in a separate global range after a procedural joint.
use super::*;

pub(super) struct Extra {
    pub bytes: Vec<u8>,
    rows: [usize; 2],
    first: usize,
    count: usize,
}

fn relative(bytes: &mut [u8], field: usize, target: usize) -> Result<()> {
    put(bytes, field, &(target as i64 - field as i64).to_le_bytes())
}

pub(super) fn prepare(
    original: &Payload,
    section: usize,
    cut: usize,
    poses: &[u8],
    first: usize,
) -> Result<Extra> {
    let instance = original.pointer(16)?;
    let mut out = original.0[..cut].to_vec();
    let row = original.bytes::<64>(section)?;
    let rows = [row, row].concat();
    append(&mut out, instance + 0x30, 0x80808544, &rows, 64)?;
    let rows: [usize; 2] = Payload(out.clone())
        .array(instance + 0x30, 64, Some(0x80808544))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Appended FK section count differs"))?;
    // The retained section's pose array is still before the insertion point.
    relative(&mut out, rows[0] + 0x28, original.pointer(section + 0x28)?)?;
    for row in rows {
        relative(&mut out, row + 16, instance + 0x30)?;
    }
    append(&mut out, rows[1] + 0x20, 0x80809f75, poses, 32)?;
    put(&mut out, rows[1] + 0x30, &(first as u32).to_le_bytes())?;
    Ok(Extra {
        bytes: out[cut..].to_vec(),
        rows,
        first,
        count: poses.len() / 32,
    })
}

impl Extra {
    pub(super) fn finish(
        self,
        out: &mut Vec<u8>,
        original: &Payload,
        definition: usize,
        schema: usize,
        instance: usize,
    ) -> Result<()> {
        put(out, instance + 0x30, &2u64.to_le_bytes())?;
        relative(out, instance + 0x38, self.rows[0] - 16)?;
        let row = original.bytes::<24>(schema)?;
        append(out, definition + 0xd8, 0x80808549, &[row, row].concat(), 24)?;
        let schemas = Payload(out.clone()).array(definition + 0xd8, 24, Some(0x80808549))?;
        for (&instance, &schema) in self.rows.iter().zip(&schemas) {
            put(out, instance + 8, &(schema as u64).to_le_bytes())?;
            put(out, schema + 8, &(instance as u64).to_le_bytes())?;
        }
        put(out, schemas[1] + 16, &(self.count as u32).to_le_bytes())?;
        put(out, schemas[1] + 20, &(self.first as u32).to_le_bytes())?;
        Ok(())
    }
}
