//! Add explicit numeric providers to the entity's one native input scheduler.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Input {
    pub name: u32,
    pub namespace: u32,
    pub property: u32,
    pub value_type: u16,
}

/// Find a declared numeric interface by its native implementation and method.
pub(crate) fn provider(
    native: &mut crate::tiger::reader::Reader,
    owner: u32,
    payload: &Payload,
    implementation: u32,
    method: u32,
) -> Result<crate::tiger::entity::links::Object> {
    use crate::tiger::entity::links;
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    let mut found = Vec::new();
    for at in (definition + 0x48..payload.0.len().saturating_sub(23)).step_by(8) {
        if payload.pointer(at).ok() != Some(definition)
            || payload.u32(at + 12)? != 0
            || native.reference(payload.u32(at + 8)?).ok() != Some(0x80809c54)
        {
            continue;
        }
        let metadata = native.tag(payload.u32(at + 8)?, Some(0x80809c54))?;
        if metadata.u32(8)? != 0x80809ae1 {
            continue;
        }
        let object = links::Object {
            owner,
            class: 0x80809ae1,
            offset: at as u64,
        };
        let interface = links::Interface::read(payload, object, &metadata, false)?;
        if interface.instance_class == 0x80809ae2
            && interface.methods.len() == 1
            && interface.methods[0].implementation_class == implementation
            && interface.methods[0].index == method
        {
            found.push(object);
        }
    }
    ensure!(
        found.len() == 1,
        "Numeric provider {implementation:08X} method {method} is absent or ambiguous"
    );
    Ok(found[0])
}

pub(crate) struct Extension {
    pub payload: Payload,
    pub allocation: Payload,
    pub declarations: Vec<usize>,
    pub relocated: BTreeMap<usize, usize>,
    pub first: usize,
    pub count: usize,
    pub old_outputs: usize,
}

impl Extension {
    pub fn channel(&self, index: u32) -> Result<u32> {
        if index == u32::MAX {
            return Ok(index);
        }
        ensure!(
            (index as usize) < self.old_outputs,
            "Bank consumer channel exceeds declarations"
        );
        Ok(index
            + if index as usize >= self.first {
                self.count as u32
            } else {
                0
            })
    }
}

fn bytes(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(field, stride, Some(class))?
        .into_iter()
        .flat_map(|at| p.0[at..at + stride].iter().copied())
        .collect())
}

fn mask(
    p: &Payload,
    field: usize,
    first: usize,
    extra: usize,
    before: usize,
    initial: bool,
) -> Result<Vec<u8>> {
    let old = p.array(field, 4, Some(0x8080000b))?;
    if old.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        old.len() == before.div_ceil(32),
        "Bank dependency mask width differs"
    );
    let mut words = vec![0u32; (before + extra).div_ceil(32)];
    for index in 0..before {
        if p.u32(old[index / 32])? & (1 << (index % 32)) != 0 {
            let target = index + usize::from(index >= first) * extra;
            words[target / 32] |= 1 << (target % 32);
        }
    }
    if initial {
        for index in first..first + extra {
            words[index / 32] |= 1 << (index % 32);
        }
    }
    Ok(words.into_iter().flat_map(u32::to_le_bytes).collect())
}

fn grow_allocation(
    original: &Payload,
    first: usize,
    count: usize,
    old_outputs: usize,
) -> Result<Payload> {
    let outputs = old_outputs + count;
    let mut allocation = original.clone();
    let allocation_rows = allocation.array(0x20, 40, Some(0x80808852))?;
    for (name, class, before, after) in [
        (0xa8333ec9, 0x808097a7, first, first + count),
        (0xfc2f3d6f, 0x80800090, old_outputs, outputs),
        (
            0x4a423762,
            0x8080000b,
            old_outputs.div_ceil(32),
            outputs.div_ceil(32),
        ),
    ] {
        let matches = allocation_rows
            .iter()
            .copied()
            .filter(|&row| allocation.u32(row).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Bank allocation field is absent or ambiguous"
        );
        let row = matches[0];
        ensure!(
            allocation.u32(row + 16)? == class
                && allocation.u32(row + 20)? as usize == before
                && allocation.u64(row + 24)? == 0,
            "Bank allocation shape differs"
        );
        put(
            &mut allocation.0,
            row + 20,
            &u32::try_from(after)?.to_le_bytes(),
        )?;
    }
    Ok(allocation)
}

fn lookups(
    original: &Payload,
    p: &mut Payload,
    definition: usize,
    first: usize,
    inputs: &[Input],
    shifted: impl Fn(u32) -> Result<u32>,
) -> Result<()> {
    for field in [0xf8, 0x108, 0x118] {
        let mut lookup = original
            .array(definition + field, 12, Some(0x808097a0))?
            .into_iter()
            .map(|row| {
                Ok((
                    original.u32(row)?,
                    original.u32(row + 4)?,
                    shifted(original.u32(row + 8)?)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        if field == 0x118 {
            lookup.extend(
                inputs
                    .iter()
                    .enumerate()
                    .map(|(i, input)| (input.namespace, input.property, (first + i) as u32)),
            );
        }
        lookup.sort_unstable();
        append(
            &mut p.0,
            definition + field,
            0x808097a0,
            &lookup
                .into_iter()
                .flat_map(|(a, b, c)| [a, b, c])
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            12,
        )?;
    }
    Ok(())
}

fn write_outputs(
    original: &Payload,
    p: &mut Payload,
    definition: usize,
    rows: &[usize],
    first: usize,
    inputs: &[Input],
    shifted: impl Fn(u32) -> Result<u32>,
) -> Result<()> {
    let old_outputs = rows.len();
    let count = inputs.len();
    let outputs = old_outputs + count;
    let mut data = Vec::new();
    for (index, &row) in rows.iter().enumerate() {
        if index == first {
            data.resize(data.len() + count * 112, 0);
        }
        data.extend(original.bytes::<112>(row)?);
    }
    if first == rows.len() {
        data.resize(data.len() + count * 112, 0);
    }
    append(&mut p.0, definition + 0xd8, 0x808097a1, &data, 112)?;
    let written = p.array(definition + 0xd8, 112, Some(0x808097a1))?;
    for (index, &old) in rows.iter().enumerate() {
        let row = written[shifted(index as u32)? as usize];
        // Programs address a separate index array. Their local bytecode, constants
        // and interpolation parameters remain unchanged.
        for field in [16, 32, 64, 88, 96] {
            if original.u64(old + field)? != 0 {
                let target = original.pointer(old + field)?;
                put(
                    &mut p.0,
                    row + field,
                    &(target as i64 - row as i64 - field as i64).to_le_bytes(),
                )?;
            }
        }
        let indices = original
            .array(old + 56, 4, Some(0x80800007))?
            .into_iter()
            .map(|at| shifted(original.u32(at)?))
            .collect::<Result<Vec<_>>>()?;
        append(
            &mut p.0,
            row + 56,
            0x80800007,
            &indices
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            4,
        )?;
        let dependencies = mask(original, old + 80, first, count, old_outputs, false)?;
        append(&mut p.0, row + 80, 0x8080000b, &dependencies, 4)?;
    }
    for (index, input) in inputs.iter().enumerate() {
        let ordinal = first + index;
        let row = written[ordinal];
        put(&mut p.0, row, &input.name.to_le_bytes())?;
        put(&mut p.0, row + 72, &u16::MAX.to_le_bytes())?;
        put(&mut p.0, row + 74, &input.value_type.to_le_bytes())?;
        put(&mut p.0, row + 104, &[0xff, 0xfe, 0xff, 0xff])?;
        let mut dependencies = vec![0u32; outputs.div_ceil(32)];
        dependencies[ordinal / 32] = 1 << (ordinal % 32);
        append(
            &mut p.0,
            row + 80,
            0x8080000b,
            &dependencies
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            4,
        )?;
    }
    Ok(())
}

pub(crate) fn extend(
    original: &Payload,
    allocation: &Payload,
    inputs: &[Input],
) -> Result<Extension> {
    let instance = original.pointer(16)?;
    let definition = usize::try_from(original.u64(instance + 8)?)?;
    ensure!(
        original.u32(instance - 4)? == 0x8080979f
            && original.u32(definition - 4)? == 0x80809790
            && !inputs.is_empty(),
        "Expected a native bank and new numeric inputs"
    );
    let owner = original.u32(instance)?;
    let old_states = original.array(instance + 0x30, 80, Some(0x808097a7))?;
    let old_inputs = original.array(definition + 0xc8, 32, Some(0x808097a8))?;
    let rows = original.array(definition + 0xd8, 112, Some(0x808097a1))?;
    let first = old_inputs.len();
    let count = inputs.len();
    let old_outputs = rows.len();
    let outputs = old_outputs + count;
    ensure!(
        first > 0
            && first == old_states.len()
            && first <= old_outputs
            && outputs <= u16::MAX as usize,
        "Native input bank dimensions differ"
    );
    let mut names = rows
        .iter()
        .map(|&row| original.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    for input in inputs {
        ensure!(
            names.insert(input.name) && matches!(input.value_type, 0..=5),
            "New raw channel name or value type differs"
        );
    }
    for (&state, &declaration) in old_states.iter().zip(&old_inputs) {
        ensure!(
            original.u32(state)? == owner
                && original.u32(state + 4)? == 0x808097a8
                && original.u64(state + 8)? == declaration as u64
                && original.pointer(state + 16)? == instance
                && original.u32(declaration)? == owner
                && original.u32(declaration + 4)? == 0x808097a7
                && original.u64(declaration + 8)? == state as u64
                && original.u64(declaration + 16)? == 0
                && original.u64(declaration + 24)? == 0x80809ae2,
            "Native imported channel is not reciprocal"
        );
    }
    let mut p = original.clone();
    let mut states = bytes(original, instance + 0x30, 80, 0x808097a7)?;
    let mut declarations = bytes(original, definition + 0xc8, 32, 0x808097a8)?;
    for _ in inputs {
        states.extend(original.bytes::<80>(old_states[0])?);
        declarations.extend(original.bytes::<32>(old_inputs[0])?);
    }
    append(&mut p.0, instance + 0x30, 0x808097a7, &states, 80)?;
    append(&mut p.0, definition + 0xc8, 0x808097a8, &declarations, 32)?;
    let states = p.array(instance + 0x30, 80, Some(0x808097a7))?;
    let declarations = p.array(definition + 0xc8, 32, Some(0x808097a8))?;
    for (&state, &declaration) in states.iter().zip(&declarations) {
        put(&mut p.0, state + 8, &(declaration as u64).to_le_bytes())?;
        put(
            &mut p.0,
            state + 16,
            &(instance as i64 - state as i64 - 16).to_le_bytes(),
        )?;
        put(&mut p.0, declaration + 8, &(state as u64).to_le_bytes())?;
    }
    let relocated = old_inputs
        .iter()
        .copied()
        .zip(declarations.iter().copied())
        .collect();
    let shifted = |index: u32| -> Result<u32> {
        ensure!(
            (index as usize) < old_outputs,
            "Bank index exceeds its original declarations"
        );
        Ok(index
            + if index as usize >= first {
                count as u32
            } else {
                0
            })
    };
    write_outputs(original, &mut p, definition, &rows, first, inputs, shifted)?;
    lookups(original, &mut p, definition, first, inputs, shifted)?;
    let mut cached = bytes(original, instance + 0x50, 16, 0x80800090)?;
    ensure!(
        cached.len() == old_outputs * 16,
        "Bank cached value count differs"
    );
    cached.splice(first * 16..first * 16, vec![0; count * 16]);
    append(&mut p.0, instance + 0x50, 0x80800090, &cached, 16)?;
    let pending = mask(original, instance + 0x40, first, count, old_outputs, true)?;
    append(&mut p.0, instance + 0x40, 0x8080000b, &pending, 4)?;
    for field in [0xc0, 0xc4] {
        let ordinal = original.u32(definition + field)?;
        if ordinal != u32::MAX {
            put(
                &mut p.0,
                definition + field,
                &shifted(ordinal)?.to_le_bytes(),
            )?;
        }
    }
    for (field, before, after) in [
        (0x130, old_outputs.div_ceil(32), outputs.div_ceil(32)),
        (0x134, old_outputs, outputs),
    ] {
        ensure!(
            original.u32(definition + field)? as usize == before,
            "Bank schema count differs"
        );
        put(
            &mut p.0,
            definition + field,
            &u32::try_from(after)?.to_le_bytes(),
        )?;
    }
    let allocation = grow_allocation(allocation, first, count, old_outputs)?;
    Ok(Extension {
        payload: p,
        allocation,
        declarations: declarations[first..].to_vec(),
        relocated,
        first,
        count,
        old_outputs,
    })
}
