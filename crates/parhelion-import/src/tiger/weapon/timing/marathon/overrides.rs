//! Remove fixed native inputs only after their complete source stat scope is admitted.
use super::*;

pub(super) fn plan(
    p: &Payload,
    resource: u64,
    key: u32,
    inputs: &BTreeSet<u32>,
) -> Result<Vec<Value>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let definition = p.pointer(24)?;
    let global = definition + 0x228;
    let keys = p.array(definition + 0x238, 40, Some(0x808038C7))?;
    let selected = keys
        .iter()
        .position(|&row| p.u32(row + 16).ok() == Some(key))
        .unwrap_or(0);
    let read = |descriptor| -> Result<Vec<[u8; 8]>> {
        p.array(descriptor, 8, Some(0x808038C9))?
            .into_iter()
            .map(|row| {
                ensure!(
                    p.u32(row)? <= 18,
                    "Native stat override input is unsupported"
                );
                p.f32(row + 4)?;
                p.bytes::<8>(row)
            })
            .collect()
    };
    let owns = |row: &[u8; 8]| inputs.contains(&u32::from_le_bytes(row[..4].try_into().unwrap()));
    let globals = read(global)?;
    let move_globals = globals.iter().any(owns);
    let mut changes = Vec::new();
    let mut add = |descriptor: usize, rows: Vec<[u8; 8]>| -> Result<()> {
        let offset = u32::try_from(
            u64::try_from(descriptor)?
                .checked_sub(resource)
                .context("Native override precedes its resource")?,
        )?;
        changes.push(json!({"offset":offset,"rows":rows}));
        Ok(())
    };
    if move_globals {
        add(global, Vec::new())?;
    }
    for (index, &descriptor) in keys.iter().enumerate() {
        let original = read(descriptor)?;
        // E86EF0 applies global rows before key rows through E84A20. Moving the whole global
        // prefix preserves the exact setter order on every other key, including duplicates.
        let mut rows = if move_globals {
            globals.iter().chain(&original).copied().collect::<Vec<_>>()
        } else {
            original.clone()
        };
        if index == selected {
            rows.retain(|row| !owns(row));
        }
        if rows != original {
            add(descriptor, rows)?;
        }
    }
    Ok(changes)
}
