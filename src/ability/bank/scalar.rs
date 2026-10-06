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
    // Base ability initialization and property dispatch both expose exactly seven inputs.
    if input >= 7 || !value.is_finite() || (multiply && value < 0.0) {
        return Err("Invalid base ability input adjustment".into());
    }
    let modifier = Modifier::Scalar {
        input,
        value,
        multiply,
    };
    let before = property_rows(payload)?;
    if before.iter().any(|row| row.key == key) {
        return Err(format!(
            "The bank already has a property row for key {key:08X}"
        ));
    }
    let layout = layout(payload)?;
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
    if runtime.count != definitions.count
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
    let modifier_instance = open_block(&mut out, CLASS);
    out.resize(modifier_instance + 48, 0);
    let child_instances = array(&mut out, 1, CHILD, 32);
    let input_instances = array(&mut out, runtime.count + 1, INPUT, 80);
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
    let modifier_definition = open_block(&mut out, TWIN);
    out.resize(modifier_definition + 40, 0);
    let child_definitions = array(&mut out, 1, CHILD_TWIN, 24);
    let input_definitions = array(&mut out, definitions.count + 1, INPUT_TWIN, 48);
    out.resize(out.len().next_multiple_of(16), 0);

    let placement = Placement {
        instance_first: property_instances + 16,
        definition_first: property_definitions + 16,
        modifier_instance,
        modifier_definition,
    };
    write_rows(
        payload, &mut out, &layout, count, &placement, shift, key, handler,
    )?;
    pair(
        &mut out,
        layout.owner,
        modifier_instance,
        modifier_definition,
        CLASS,
        TWIN,
    );
    descriptor(&mut out, modifier_instance + 32, child_instances, 1)?;
    descriptor(&mut out, modifier_definition + 16, child_definitions, 1)?;
    put_u32(&mut out, modifier_definition + 32, runtime.count as u32);
    put_u32(&mut out, modifier_definition + 36, 1);

    let ci = child_instances + 16;
    let cd = child_definitions + 16;
    pair(&mut out, layout.owner, ci, cd, CHILD, CHILD_TWIN);
    put_pointer(&mut out, ci + 16, modifier_instance)?;
    out[cd + 16] = input;
    out[cd + 17] = u8::from(multiply);
    put_u32(&mut out, cd + 20, value.to_bits());

    for index in 0..=runtime.count {
        let i = input_instances + 16 + index * 80;
        let d = input_definitions + 16 + index * 48;
        if index < runtime.count {
            let old_i = runtime.first + index * 80;
            let old_d = definitions.first + index * 48;
            out[i..i + 80].copy_from_slice(&payload[old_i..old_i + 80]);
            out[d..d + 48].copy_from_slice(&payload[old_d..old_d + 48]);
        } else {
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
        runtime.count + 1,
    )?;
    descriptor(
        &mut out,
        shift.at(layout.instance) + DEFINITION_INPUTS,
        input_definitions,
        runtime.count + 1,
    )?;
    write_descriptors(
        &mut out,
        &layout,
        shift,
        count,
        property_instances,
        property_definitions,
        None,
    )?;
    let size = out.len() as u64;
    put_u64(&mut out, SIZE, size);
    check_read_back(&out, &before, key, handler, CLASS, modifier, None)?;
    // Keep the declaration, flattened value and runtime activation state in agreement.
    let new_i = input_instances + 16 + runtime.count * 80;
    let new_d = input_definitions + 16 + runtime.count * 48;
    if out[cd + 16] != input
        || out[cd + 17] != u8::from(multiply)
        || u32_at(&out, cd + 20)? != value.to_bits()
        || u32_at(&out, new_d + 40)? != value.to_bits()
        || u32_at(&out, new_d + 44)? != u32::from(multiply)
        || u32_at(&out, new_i + 64)? != 1.0f32.to_bits()
    {
        return Err("Numeric property changed during serialization".into());
    }
    Ok(out)
}
