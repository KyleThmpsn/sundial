//! Reading a bank's layout, rows, parameter table and blocks, and the references an edit moves.
use super::*;

/// The four bytes before every block hold its twin's class, and a block starts 8 byte aligned.
const BLOCK_ALIGNMENT: usize = 8;
/// The most rows a bank is read with. The largest stock bank, the Hunter melee, has 99.
const MOST_ROWS: usize = 4096;

pub(super) fn pointer(payload: &[u8], at: usize) -> Result<usize, String> {
    let relative = i64_at(payload, at)?;
    let base = i64::try_from(at).map_err(|_| "Bank offset does not fit i64".to_owned())?;
    let target = base
        .checked_add(relative)
        .ok_or_else(|| format!("Bank pointer at {at:#x} overflowed"))?;
    usize::try_from(target)
        .ok()
        .filter(|target| *target < payload.len())
        .ok_or_else(|| format!("Bank pointer at {at:#x} leaves the payload"))
}

pub(super) fn rows(
    payload: &[u8],
    descriptor: usize,
    header_class: u32,
    label: &str,
) -> Result<Rows, String> {
    let count = usize::try_from(u64_at(payload, descriptor)?)
        .map_err(|_| format!("Bank {label} count does not fit this platform"))?;
    if count == 0 || count > MOST_ROWS {
        return Err(format!("Bank {label} count {count} is not an array"));
    }
    let header = pointer(payload, descriptor + 8)?;
    if u64_at(payload, header)? as usize != count {
        return Err(format!("Bank {label} header repeats a different count"));
    }
    if u32_at(payload, header + 8)? != header_class {
        return Err(format!(
            "Bank {label} header names class {:08X}",
            u32_at(payload, header + 8)?
        ));
    }
    if header < 4 || u32_at(payload, header - 4)? != ARRAY_MARKER {
        return Err(format!("Bank {label} header has no array marker"));
    }
    Ok(Rows {
        count,
        first: header + 16,
    })
}

pub(super) fn layout(payload: &[u8]) -> Result<Layout, String> {
    let size = usize::try_from(u64_at(payload, SIZE)?)
        .map_err(|_| "Bank size does not fit this platform".to_owned())?;
    if size != payload.len() {
        return Err(format!(
            "Bank size {size} differs from its {} bytes",
            payload.len()
        ));
    }
    let definition = pointer(payload, DEFINITION_POINTER)?;
    let instance = pointer(payload, INSTANCE_POINTER)?;
    let owner = u32_at(payload, definition)?;
    if u32_at(payload, instance)? != owner {
        return Err("Bank definition and instance blocks name different owners".into());
    }
    if u64_at(payload, definition + BLOCK_TWIN)? as usize != instance
        || u64_at(payload, instance + BLOCK_TWIN)? as usize != definition
    {
        return Err("Bank definition and instance blocks are not twins".into());
    }
    let instance_rows = rows(
        payload,
        definition + INSTANCE_ROWS_DESCRIPTOR,
        DEFINITION_ROW_CLASS,
        "instance row",
    )?;
    let definition_rows = rows(
        payload,
        instance + DEFINITION_ROWS_DESCRIPTOR,
        INSTANCE_ROW_CLASS,
        "definition row",
    )?;
    if instance_rows.count != definition_rows.count {
        return Err("Bank instance and definition rows differ in count".into());
    }
    // Three stock jump banks list no parameters at all.
    let parameters = if u64_at(payload, instance + PARAMETERS_DESCRIPTOR)? == 0 {
        None
    } else {
        Some(rows(
            payload,
            instance + PARAMETERS_DESCRIPTOR,
            PARAMETER_ROW_CLASS,
            "parameter",
        )?)
    };
    if instance_rows.first >= instance
        || definition_rows.first < instance
        || parameters
            .as_ref()
            .is_some_and(|table| table.first < instance)
    {
        return Err(
            "Bank arrays sit outside their blocks' regions: the client clones the \
                    definition block through to the instance block, and the instance \
                    block through to the end"
                .into(),
        );
    }
    for index in 0..instance_rows.count {
        let instance_row = instance_rows.first + index * INSTANCE_ROW_SIZE;
        let definition_row = definition_rows.first + index * DEFINITION_ROW_SIZE;
        for (row, class) in [
            (instance_row, INSTANCE_ROW_CLASS),
            (definition_row, DEFINITION_ROW_CLASS),
        ] {
            if u32_at(payload, row)? != owner || u32_at(payload, row + BLOCK_CLASS)? != class {
                return Err(format!(
                    "Bank row {index} is not a {class:08X} row of the bank"
                ));
            }
        }
        if u64_at(payload, instance_row + BLOCK_TWIN)? as usize != definition_row
            || u64_at(payload, definition_row + BLOCK_TWIN)? as usize != instance_row
        {
            return Err(format!(
                "Bank row {index} and its twin do not point at each other"
            ));
        }
        if pointer(payload, instance_row + INSTANCE_ROW_BANK)? != definition {
            return Err(format!(
                "Bank instance row {index} does not point at the bank"
            ));
        }
        let modifier = pointer(payload, definition_row + ROW_MODIFIER)?;
        let twin = pointer(payload, instance_row + INSTANCE_ROW_MODIFIER)?;
        if twin >= instance || modifier < instance {
            return Err(format!(
                "Bank row {index}'s modifiers sit outside their blocks' regions"
            ));
        }
        if u64_at(payload, modifier + BLOCK_TWIN)? as usize != twin
            || u64_at(payload, twin + BLOCK_TWIN)? as usize != modifier
        {
            return Err(format!("Bank row {index}'s modifier pair are not twins"));
        }
        if pointer(payload, twin + INSTANCE_MODIFIER_ROW)? != instance_row {
            return Err(format!(
                "Bank row {index}'s instance modifier does not point back at the row"
            ));
        }
        if i64_at(payload, definition_row + ROW_ATTACK_KEY)? != 0 {
            let reference = pointer(payload, definition_row + ROW_ATTACK_KEY)?;
            if reference < instance + 4 || u32_at(payload, reference - 4)? != ATTACK_KEY_CLASS {
                return Err(format!(
                    "Bank row {index}'s attack key reference is not a {ATTACK_KEY_CLASS:08X} \
                     block in the instance region"
                ));
            }
        }
    }
    Ok(Layout {
        owner,
        definition,
        instance,
        instance_rows,
        definition_rows,
        parameters,
    })
}

/// Checks the structure the edits rely on: the header, both blocks, both row arrays, the
/// parameter table and every row's twin, modifier pair and attack key reference.
pub fn validate(payload: &[u8]) -> Result<(), String> {
    layout(payload).map(|_| ())
}

fn parameter_at(payload: &[u8], row: usize) -> Result<Parameter, String> {
    Ok(Parameter {
        name: u32_at(payload, row)?,
        reset: f32::from_bits(u32_at(payload, row + 4)?),
        applied: f32::from_bits(u32_at(payload, row + 8)?),
        add: u32_at(payload, row + 12)? != 0,
    })
}

pub(super) fn table_parameters(
    payload: &[u8],
    table: Option<&Rows>,
) -> Result<Vec<Parameter>, String> {
    let Some(table) = table else {
        return Ok(Vec::new());
    };
    (0..table.count)
        .map(|index| parameter_at(payload, table.first + index * PARAMETER_ROW_SIZE))
        .collect()
}

/// The bank's script parameter table, in its order.
pub fn parameters(payload: &[u8]) -> Result<Vec<Parameter>, String> {
    let layout = layout(payload)?;
    table_parameters(payload, layout.parameters.as_ref())
}

/// The handler slot the bank hands `modifier` to, read from its own rows with the same
/// modifier class: for a parameter, the rows reading that parameter first, then any script
/// row. None when no stock row shows the slot, in which case no row can be added, since a
/// modifier handed to the wrong slot faults the client at world load.
pub fn handler_slot(payload: &[u8], modifier: Modifier) -> Result<Option<HandlerIndex>, String> {
    handler_slot_in(&property_rows(payload)?, modifier)
}

pub(super) fn handler_slot_in(
    rows: &[PropertyRow],
    modifier: Modifier,
) -> Result<Option<HandlerIndex>, String> {
    let (class, kind) = match modifier {
        Modifier::Charges(_) => (CHARGE_DEFINITION_CLASS, "charge"),
        Modifier::Parameter { .. } => (SCRIPT_DEFINITION_CLASS, "script parameter"),
        Modifier::Melee { .. } => (ATTACK_DEFINITION_CLASS, "melee attack"),
        Modifier::Scalar { .. } => (scalar::CLASS, "base ability input"),
    };
    let same_class: Vec<&PropertyRow> = rows
        .iter()
        .filter(|row| row.modifier_class == class)
        .collect();
    let reading: BTreeSet<HandlerIndex> = match modifier {
        Modifier::Parameter { name, .. } => same_class
            .iter()
            .filter(|row| {
                row.parameters
                    .iter()
                    .any(|parameter| parameter.name == name)
            })
            .map(|row| row.handler)
            .collect(),
        Modifier::Charges(_) | Modifier::Melee { .. } | Modifier::Scalar { .. } => BTreeSet::new(),
    };
    let slots = if reading.is_empty() {
        same_class.iter().map(|row| row.handler).collect()
    } else {
        reading
    };
    match slots.len() {
        0 => Ok(None),
        1 => Ok(slots.into_iter().next()),
        _ => Err(format!(
            "The bank hands {kind} modifiers to several handler slots {slots:?}"
        )),
    }
}

/// The property rows as the client's lookup walks them, from the instance block.
pub fn property_rows(payload: &[u8]) -> Result<Vec<PropertyRow>, String> {
    let layout = layout(payload)?;
    let table = table_parameters(payload, layout.parameters.as_ref())?;
    (0..layout.definition_rows.count)
        .map(|index| {
            let row = layout.definition_rows.first + index * DEFINITION_ROW_SIZE;
            let modifier = pointer(payload, row + ROW_MODIFIER)?;
            let modifier_class = u32_at(payload, modifier + BLOCK_CLASS)?;
            let charge = (modifier_class == CHARGE_DEFINITION_CLASS)
                .then(|| u64_at(payload, modifier + CHARGE_VALUE).map(|value| value as i64))
                .transpose()?;
            let parameters = if modifier_class == SCRIPT_DEFINITION_CLASS {
                let first = u32_at(payload, modifier + SCRIPT_FIRST)? as usize;
                let count = u32_at(payload, modifier + SCRIPT_COUNT)? as usize;
                table
                    .get(first..first.saturating_add(count))
                    .ok_or_else(|| {
                        format!(
                            "Bank row {index} selects parameters {first}+{count} the table lacks"
                        )
                    })?
                    .to_vec()
            } else {
                Vec::new()
            };
            Ok(PropertyRow {
                key: u32_at(payload, row + ROW_KEY)?,
                name: u32_at(payload, row + ROW_NAME)?,
                handler: HandlerIndex::new(u32_at(payload, row + ROW_HANDLER)?),
                modifier_class,
                charge,
                parameters,
            })
        })
        .collect()
}

/// Every typed block by offset, with its twin: the owner tag, a class and a twin offset whose
/// own link points back, found at every 8 byte step. Rows and modifiers are blocks too.
pub(super) fn blocks(payload: &[u8], owner: u32) -> BTreeMap<usize, usize> {
    let mut found = BTreeMap::new();
    let mut at = 0x80;
    while at + 16 <= payload.len() {
        let class = u32_at(payload, at + BLOCK_CLASS).unwrap_or(0);
        if u32_at(payload, at).unwrap_or(0) == owner && (0x8080_0000..=0x8080_FFFF).contains(&class)
        {
            let twin = u64_at(payload, at + BLOCK_TWIN).unwrap_or(u64::MAX) as usize;
            if twin + 16 <= payload.len()
                && twin % BLOCK_ALIGNMENT == 0
                && u32_at(payload, twin).unwrap_or(0) == owner
                && u64_at(payload, twin + BLOCK_TWIN).unwrap_or(0) as usize == at
            {
                found.insert(at, twin);
            }
        }
        at += BLOCK_ALIGNMENT;
    }
    found
}

/// Where the instance block was before an edit and how far the edit moved it, for the tags
/// that name the bank's blocks by absolute offset.
pub fn instance_shift(before: &[u8], after: &[u8]) -> Result<(usize, usize), String> {
    let was = layout(before)?.instance;
    let now = layout(after)?.instance;
    now.checked_sub(was)
        .map(|delta| (was, delta))
        .ok_or_else(|| "The edited bank's instance block moved backwards".into())
}

/// A tag's references into the bank by absolute offset, the ability entity's way of naming a
/// bank block or field: the bank tag, the class it expects there and the offset, sixteen bytes
/// at eight byte alignment. Every offset at or past the instance block moves with it. None
/// when the tag has no reference past the instance block.
pub fn retarget_references(
    referrer: &[u8],
    bank: u32,
    before: &[u8],
    after: &[u8],
) -> Result<Option<Vec<u8>>, String> {
    let (instance, delta) = instance_shift(before, after)?;
    if delta == 0 {
        return Ok(None);
    }
    let mut out = referrer.to_vec();
    let mut moved = false;
    let mut at = 0;
    while at + 16 <= referrer.len() {
        if u32_at(referrer, at)? == bank
            && (0x8080_0000..=0x8080_FFFF).contains(&u32_at(referrer, at + 4)?)
            && let Ok(offset) = usize::try_from(u64_at(referrer, at + 8)?)
            && (instance..before.len()).contains(&offset)
            && offset % BLOCK_ALIGNMENT == 0
        {
            put_u64(&mut out, at + 8, (offset + delta) as u64);
            moved = true;
        }
        at += BLOCK_ALIGNMENT;
    }
    Ok(moved.then_some(out))
}

/// How many typed blocks link to their twin, the census an edit must grow by exactly the rows
/// and modifiers it adds.
pub fn block_count(payload: &[u8]) -> Result<usize, String> {
    let layout = layout(payload)?;
    Ok(blocks(payload, layout.owner).len())
}
