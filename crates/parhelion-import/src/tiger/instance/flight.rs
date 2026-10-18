//! Restore physical array envelopes around authored flight input state.
use super::{Payload, put};
use anyhow::{Context, Result, ensure};

pub(super) fn restore_input_envelopes(p: &mut Payload) -> Result<usize> {
    let instance = p.pointer(16)?;
    let schema = p.pointer(24)?;
    let definition = usize::try_from(p.u64(instance + 8)?)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && instance < definition
            && definition <= schema
            && p.u32(instance - 4)? == 0x80803CF2
            && p.u32(instance + 4)? == 0x80803CF1
            && p.u32(definition - 4)? == 0x80803CF1
            && p.u32(definition + 4)? == 0x80803CF2
            && p.u64(definition + 8)? == instance as u64,
        "Flight component root pair differs"
    );
    let mut restored = 0;
    // Pitch and thrust have native scalar expressions with mutable input arrays.
    for field in [0x130, 0x210] {
        let expression = instance + field;
        ensure!(
            p.u32(expression + 4)? == 0x808089F6,
            "Flight scalar expression differs"
        );
        let descriptor = expression + 0x20;
        let rows = p.array(descriptor, 96, Some(0x80809788))?;
        let Some(&last) = rows.last() else {
            continue;
        };
        let header = p.pointer(descriptor + 8)?;
        let prefix = header.checked_sub(4).context("Flight input envelope")?;
        ensure!(
            header.is_multiple_of(16) && prefix >= instance + 0x260 && last + 96 <= schema - 4,
            "Flight input envelope exceeds its mutable span"
        );
        for row in rows {
            let link = usize::try_from(p.u64(row + 8)?)?;
            ensure!(
                p.u32(row + 4)? == 0x80809789
                    && p.u32(link + 4)? == 0x80809788
                    && p.u64(link + 8)? == row as u64
                    && p.u32(row)? == p.u32(link)?,
                "Flight input pair differs"
            );
        }
        match p.u32(prefix)? {
            0 => {
                put(&mut p.0, prefix, &0x80809FBDu32.to_le_bytes())?;
                restored += 1;
            }
            0x80809FBD => {}
            _ => anyhow::bail!("Flight input has an unexpected physical envelope"),
        }
    }
    Ok(restored)
}
