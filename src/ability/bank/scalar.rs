//! Native numeric properties retain both their declaration and flattened runtime inputs.
use super::*;

pub(super) const CLASS: u32 = 0x8080_451B;
pub(super) const TWIN: u32 = 0x8080_451C;
const CHILD: u32 = 0x8080_451E;
const CHILD_TWIN: u32 = 0x8080_451F;
const INPUT: u32 = 0x8080_45BE;
const INPUT_TWIN: u32 = 0x8080_45B7;
const INSTANCE_INPUTS: usize = 0x180;
const DEFINITION_INPUTS: usize = 0xA8;

fn array(out: &mut Vec<u8>, count: usize, class: u32, stride: usize) -> usize {
    let header = open_block(out, ARRAY_MARKER);
    out.resize(header + 16 + count * stride, 0);
    put_u64(out, header, count as u64);
    put_u32(out, header + 8, class);
    header
}

fn pair(out: &mut [u8], owner: u32, instance: usize, definition: usize, class: u32, twin: u32) {
    for (at, other, kind) in [(instance, definition, twin), (definition, instance, class)] {
        put_u32(out, at, owner);
        put_u32(out, at + 4, kind);
        put_u64(out, at + 8, other as u64);
    }
}

fn descriptor(out: &mut [u8], at: usize, header: usize, count: usize) -> Result<(), String> {
    put_u64(out, at, count as u64);
    put_pointer(out, at + 8, header)
}

pub(super) fn with_row(
    payload: &[u8],
    key: u32,
    input: u8,
    value: f32,
    multiply: bool,
) -> Result<Vec<u8>, String> {
    if property_rows(payload)?.iter().any(|row| row.key == key) {
        return Err(format!(
            "The bank already has a property row for key {key:08X}"
        ));
    }
    with_row_under(payload, key, input, value, multiply)
}

/// Whether the bank shows the handler slot of numeric property rows.
pub(super) fn has_handler(payload: &[u8]) -> Result<bool, String> {
    let modifier = Modifier::Scalar {
        input: 0,
        value: 1.0,
        multiply: true,
    };
    Ok(handler_slot_in(
        &property_rows(payload)?,
        read::bank_class(payload)?,
        modifier,
    )?
    .is_some())
}

/// The bank with one more numeric property row under `key`, which other rows may already use.
pub(super) fn with_row_under(
    payload: &[u8],
    key: u32,
    input: u8,
    value: f32,
    multiply: bool,
) -> Result<Vec<u8>, String> {
    // Base ability initialization and property dispatch both expose exactly seven inputs.
    if input >= 7 || !value.is_finite() || (multiply && value < 0.0) {
        return Err("Invalid base ability input adjustment".into());
    }
    let modifier = Modifier::Scalar {
        input,
        value,
        multiply,
    };
    with_inputs(payload, key, modifier, &[(input, value, multiply)])
}

pub(super) fn with_group(payload: &[u8], key: u32, body: GroupModifier) -> Result<Vec<u8>, String> {
    let inputs = body.entries[..usize::from(body.count)]
        .iter()
        .map(|entry| (entry.input as u8, f32::from_bits(entry.value), false))
        .collect::<Vec<_>>();
    with_inputs(payload, key, Modifier::Group(body), &inputs)
}

fn with_inputs(
    payload: &[u8],
    key: u32,
    modifier: Modifier,
    values: &[(u8, f32, bool)],
) -> Result<Vec<u8>, String> {
    let before = property_rows(payload)?;
    if before.iter().any(|row| row.key == key) && matches!(modifier, Modifier::Group(_)) {
        return Err(format!(
            "The bank already has a property row for key {key:08X}"
        ));
    }
    let layout = layout(payload)?;
    let classes = modifier_classes(modifier)?;
    let handler = handler_slot_in(&before, read::bank_class(payload)?, modifier)?
        .ok_or("The bank has no native numeric property handler")?;
    let runtime = rows(
        payload,
        layout.definition + INSTANCE_INPUTS,
        INPUT,
        "numeric instance",
    )?;
    let definitions = rows(
        payload,
        layout.instance + DEFINITION_INPUTS,
        INPUT_TWIN,
        "numeric definition",
    )?;
    if runtime.count == 0
        || runtime.count != definitions.count
        || runtime.first + runtime.count * 80 > layout.instance
        || definitions.first < layout.instance
        || definitions.first + definitions.count * 48 > payload.len()
    {
        return Err("Numeric input arrays differ in count or clone region".into());
    }
    for index in 0..runtime.count {
        let i = runtime.first + index * 80;
        let d = definitions.first + index * 48;
        if u32_at(payload, i)? != layout.owner
            || u32_at(payload, d)? != layout.owner
            || u32_at(payload, i + 4)? != INPUT_TWIN
            || u32_at(payload, d + 4)? != INPUT
            || u64_at(payload, i + 8)? as usize != d
            || u64_at(payload, d + 8)? as usize != i
            || pointer(payload, i + 16)? != layout.definition
            || pointer(payload, d + 16)? != d
        {
            return Err("Numeric input pair has an unsupported binding".into());
        }
    }
    // Use this bank's native scalar implementation envelope. The definition's provider
    // and its reserved fields must agree across the table before it can supply a new row.
    let template = &payload[definitions.first..definitions.first + 48];
    for index in 1..definitions.count {
        let d = definitions.first + index * 48;
        if payload[d + 24..d + 40] != template[24..40] {
            return Err("Numeric input providers differ within the bank".into());
        }
    }

    let count = before.len();
    let marker = layout.instance - 4;
    let mut out = payload[..marker].to_vec();
    let property_instances = array(&mut out, count + 1, DEFINITION_ROW_CLASS, INSTANCE_ROW_SIZE);
    let modifier_instance = open_block(&mut out, classes.definition);
    let child_instances = if matches!(modifier, Modifier::Scalar { .. }) {
        out.resize(modifier_instance + 48, 0);
        Some(array(&mut out, 1, CHILD, 32))
    } else {
        out.resize(modifier_instance + 32, 0);
        None
    };
    let input_instances = array(&mut out, runtime.count + values.len(), INPUT, 80);
    while (out.len() + 4) % 16 != layout.instance % 16 {
        out.push(0);
    }
    let shift = Shift {
        instance: layout.instance,
        delta: out.len() + 4 - layout.instance,
    };
    out.extend_from_slice(&payload[marker..]);
    follow_shift(payload, &mut out, &blocks(payload, layout.owner), shift)?;

    let property_definitions = array(&mut out, count + 1, INSTANCE_ROW_CLASS, DEFINITION_ROW_SIZE);
    let modifier_definition = open_block(&mut out, classes.instance);
    out.resize(modifier_definition + 40, 0);
    let child_definitions = match modifier {
        Modifier::Scalar { .. } => array(&mut out, 1, CHILD_TWIN, 24),
        Modifier::Group(body) => {
            let header = array(&mut out, values.len(), 0x80804485, 8);
            for (index, entry) in body.entries[..values.len()].iter().enumerate() {
                let at = open_block(&mut out, entry.class);
                out.resize(at + 8, 0);
                put_u32(&mut out, at, u32::from(entry.input));
                put_u32(&mut out, at + 4, entry.value);
                put_pointer(&mut out, header + 16 + index * 8, at)?;
            }
            header
        }
        _ => return Err("Unsupported numeric declaration".into()),
    };
    let input_definitions = array(&mut out, definitions.count + values.len(), INPUT_TWIN, 48);
    out.resize(out.len().next_multiple_of(16), 0);

    let placement = Placement {
        instance_first: property_instances + 16,
        definition_first: property_definitions + 16,
    };
    let added = [Added {
        key,
        before: count,
        gate: None,
        handler,
        classes,
        modifier_instance,
        modifier_definition,
        body: Body::Own,
    }];
    write_rows(payload, &mut out, &layout, count, &placement, shift, &added)?;
    pair(
        &mut out,
        layout.owner,
        modifier_instance,
        modifier_definition,
        classes.definition,
        classes.instance,
    );
    descriptor(
        &mut out,
        modifier_definition + 16,
        child_definitions,
        values.len(),
    )?;
    put_u32(&mut out, modifier_definition + 32, runtime.count as u32);
    put_u32(&mut out, modifier_definition + 36, values.len() as u32);

    if let Some(child_instances) = child_instances {
        let (input, value, multiply) = values[0];
        descriptor(&mut out, modifier_instance + 32, child_instances, 1)?;
        let ci = child_instances + 16;
        let cd = child_definitions + 16;
        pair(&mut out, layout.owner, ci, cd, CHILD, CHILD_TWIN);
        put_pointer(&mut out, ci + 16, modifier_instance)?;
        out[cd + 16] = input;
        out[cd + 17] = u8::from(multiply);
        put_u32(&mut out, cd + 20, value.to_bits());
    }

    for index in 0..runtime.count + values.len() {
        let i = input_instances + 16 + index * 80;
        let d = input_definitions + 16 + index * 48;
        if index < runtime.count {
            let old_i = runtime.first + index * 80;
            let old_d = definitions.first + index * 48;
            out[i..i + 80].copy_from_slice(&payload[old_i..old_i + 80]);
            out[d..d + 48].copy_from_slice(&payload[old_d..old_d + 48]);
        } else {
            let (_, value, multiply) = values[index - runtime.count];
            out[d..d + 48].copy_from_slice(template);
            for at in [i + 40, i + 48, i + 52] {
                put_u32(&mut out, at, u32::MAX);
            }
            put_u32(&mut out, i + 64, 1.0f32.to_bits());
            out[i + 68] = 1;
            put_u32(&mut out, d + 40, value.to_bits());
            put_u32(&mut out, d + 44, u32::from(multiply));
        }
        pair(&mut out, layout.owner, i, d, INPUT, INPUT_TWIN);
        put_pointer(&mut out, i + 16, layout.definition)?;
        put_pointer(&mut out, d + 16, d)?;
    }
    descriptor(
        &mut out,
        layout.definition + INSTANCE_INPUTS,
        input_instances,
        runtime.count + values.len(),
    )?;
    descriptor(
        &mut out,
        shift.at(layout.instance) + DEFINITION_INPUTS,
        input_definitions,
        runtime.count + values.len(),
    )?;
    write_descriptors(
        &mut out,
        &layout,
        shift,
        count + 1,
        property_instances,
        property_definitions,
        None,
    )?;
    let size = out.len() as u64;
    put_u64(&mut out, SIZE, size);
    check_read_back(&out, &before, &added, None)?;
    // Keep the declaration, flattened value and runtime activation state in agreement.
    for (index, &(input, value, multiply)) in values.iter().enumerate() {
        let new_i = input_instances + 16 + (runtime.count + index) * 80;
        let new_d = input_definitions + 16 + (runtime.count + index) * 48;
        let child = if child_instances.is_some() {
            child_definitions + 32
        } else {
            pointer(&out, child_definitions + 16 + index * 8)?
        };
        if out[child] != input
            || out[child + 1] != u8::from(multiply)
            || u32_at(&out, child + 4)? != value.to_bits()
            || u32_at(&out, new_d + 40)? != value.to_bits()
            || u32_at(&out, new_d + 44)? != u32::from(multiply)
            || u32_at(&out, new_i + 64)? != 1.0f32.to_bits()
        {
            return Err("Numeric property changed during serialization".into());
        }
    }
    Ok(out)
}
