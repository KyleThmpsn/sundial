//! The Stormtrance callback forwards the shared numeric recipient to the generic rate writer.
use super::*;

pub(in crate::ability::bank) const CLASS: u32 = 0x8080452E;
const BANK_RUNTIME: u32 = 0x8080428A;
const BANK_DEFINITION: u32 = 0x8080428B;
const PRIORITIES: usize = 0x190;
pub(in crate::ability::bank) const PRIORITY_COUNT: usize = 0xB8;
const BYTE_CLASS: u32 = 0x80800009;

/// Public index values select priority bytes. Typed callback selection is separate.
pub(in crate::ability::bank) fn slot(
    rows: &[PropertyRow],
    bank_class: u32,
) -> Result<HandlerIndex, String> {
    if bank_class != BANK_DEFINITION {
        return Err("Active energy rate requires the qualified Stormtrance bank".into());
    }
    let mut classes = BTreeMap::new();
    let mut slots = BTreeMap::new();
    for row in rows {
        if classes
            .insert(row.modifier_class, row.handler)
            .is_some_and(|previous| previous != row.handler)
            || slots
                .insert(row.handler, row.modifier_class)
                .is_some_and(|previous| previous != row.modifier_class)
        {
            return Err("Active energy rate bank has conflicting priority slots".into());
        }
    }
    if classes.is_empty()
        || slots
            .keys()
            .enumerate()
            .any(|(index, slot)| slot.get() as usize != index)
    {
        return Err("Active energy rate bank has an incomplete priority table".into());
    }
    Ok(classes.get(&CLASS).copied().unwrap_or(HandlerIndex::new(
        u32::try_from(classes.len()).map_err(|_| "Too many ability priority slots")?,
    )))
}

fn priorities(payload: &[u8], layout: &Layout) -> Result<Rows, String> {
    let values = rows(
        payload,
        layout.definition + PRIORITIES,
        BYTE_CLASS,
        "priority",
    )?;
    let properties = property_rows(payload)?;
    let distinct = properties
        .iter()
        .map(|r| r.handler)
        .collect::<BTreeSet<_>>();
    slot(&properties, bank_class(payload)?)?;
    if values.count != distinct.len()
        || u64_at(payload, layout.instance + PRIORITY_COUNT)? != values.count as u64
        || values.first < layout.definition
        || values.first + values.count > layout.instance - 4
    {
        return Err("Ability priority bytes leave their runtime region or omit a slot".into());
    }
    payload
        .get(values.first..values.first + values.count)
        .ok_or("Truncated ability priority bytes")?;
    Ok(values)
}

pub(in crate::ability::bank) fn check(
    payload: &[u8],
    layout: &Layout,
    body: FixedModifier,
) -> Result<(), String> {
    let runtime_marker = layout
        .definition
        .checked_sub(4)
        .ok_or("Active energy rate bank has no runtime marker")?;
    if u32_at(payload, runtime_marker)? != BANK_RUNTIME
        || u32_at(payload, layout.definition + BLOCK_CLASS)? != BANK_DEFINITION
        || u32_at(payload, layout.instance - 4)? != BANK_DEFINITION
        || u32_at(payload, layout.instance + BLOCK_CLASS)? != BANK_RUNTIME
    {
        return Err("Active energy rate has no qualified bank class pair".into());
    }
    let callbacks = rows(payload, 0x20, 0x8080919F, "callback")?;
    if callbacks.count != 3 {
        return Err("Active energy rate bank has another callback table".into());
    }
    for (index, (offset, class, method)) in [
        (0x30, BANK_RUNTIME, 0),
        (0x48, BANK_RUNTIME, 1),
        (0x60, 0x808044FC, 12),
    ]
    .into_iter()
    .enumerate()
    {
        let at = layout.definition + offset;
        if pointer(payload, callbacks.first + index * 8)? != at + 8
            || u32_at(payload, at)? != class
            || u32_at(payload, at + 4)? != method
        {
            return Err("Active energy rate bank has another typed callback".into());
        }
    }
    priorities(payload, layout)?;
    let mut witnesses = 0;
    for index in 0..layout.definition_rows.count {
        let row = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
        let modifier = pointer(payload, row + ROW_MODIFIER)?;
        let class = u32_at(payload, modifier + BLOCK_CLASS)?;
        if class == CLASS {
            let twin = usize::try_from(u64_at(payload, modifier + BLOCK_TWIN)?)
                .map_err(|_| "Rate modifier twin overflow")?;
            let value = f32::from_bits(u32_at(payload, modifier + 16)?);
            if u32_at(payload, modifier - 4)? != CLASS + 1
                || u32_at(payload, twin - 4)? != CLASS
                || u32_at(payload, twin + BLOCK_CLASS)? != CLASS + 1
                || u32_at(payload, modifier)? != layout.owner
                || u32_at(payload, twin)? != layout.owner
                || u32_at(payload, row + ROW_FLAGS)? != 255
                || u32_at(payload, row + ROW_MODE)? != 255
                || u64_at(payload, row + ROW_GATE)? != 0
                || !value.is_finite()
                || value < 0.0
                || u32_at(payload, modifier + 20)? != 0
            {
                return Err("Existing active energy rate has unqualified fields".into());
            }
        }
        if u32_at(payload, row + ROW_KEY)? == body.template_key {
            let twin = usize::try_from(u64_at(payload, modifier + BLOCK_TWIN)?)
                .map_err(|_| "Script companion twin overflow")?;
            if class != SCRIPT_DEFINITION_CLASS
                || u32_at(payload, modifier - 4)? != SCRIPT_INSTANCE_CLASS
                || u32_at(payload, twin - 4)? != SCRIPT_DEFINITION_CLASS
                || u32_at(payload, twin + BLOCK_CLASS)? != SCRIPT_INSTANCE_CLASS
                || u32_at(payload, modifier)? != layout.owner
                || u32_at(payload, twin)? != layout.owner
                || u32_at(payload, row + ROW_FLAGS)? != 255
                || u32_at(payload, row + ROW_MODE)? != 255
                || u32_at(payload, row + 36)? != 0
                || u64_at(payload, row + ROW_GATE)? != 0
            {
                return Err(
                    "Active energy rate requires its ungated native script companion".into(),
                );
            }
            witnesses += 1;
        }
    }
    if witnesses != 1 {
        return Err("Active energy rate has no unique native script companion".into());
    }
    Ok(())
}

/// Append a priority byte only when this is the first rate modifier in this bank.
/// The byte and its typed array must remain in the runtime clone region.
pub(in crate::ability::bank) fn extend_priorities(
    payload: &[u8],
    layout: &Layout,
    out: &mut Vec<u8>,
) -> Result<Option<usize>, String> {
    let current = priorities(payload, layout)?;
    let slot = slot(&property_rows(payload)?, bank_class(payload)?)?;
    if slot.get() as usize != current.count {
        return Ok(None);
    }
    let count = current.count + 1;
    let header = open_block(out, ARRAY_MARKER);
    out.resize(header + 16 + count, 0);
    put_u64(out, header, count as u64);
    put_u32(out, header + 8, BYTE_CLASS);
    out[header + 16..header + 16 + current.count]
        .copy_from_slice(&payload[current.first..current.first + current.count]);
    let descriptor = layout.definition + PRIORITIES;
    put_u64(out, descriptor, count as u64);
    put_pointer(out, descriptor + 8, header)?;
    Ok(Some(count))
}
