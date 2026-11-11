//! Native pose controllers (instance `8080348F`, definition `80803640`) grown by channel inputs,
//! parameters and supplemental layers. The controller's allocation and the layer buffers of its
//! animation implementation (`808036E2`) follow every count.
use crate::{
    presentation::{append, put},
    tiger::{instance, payload::Payload},
};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

fn rows(p: &Payload, fields: &[usize], stride: usize) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|&at| p.0[at..at + stride].iter().copied())
        .collect()
}

fn relative(p: &mut Payload, field: usize, target: usize) -> Result<()> {
    put(
        &mut p.0,
        field,
        &(target as i64 - field as i64).to_le_bytes(),
    )
}

/// Allocation rows below `root`, by key, class and old count, take their new counts. Every
/// requested row must occur exactly `expected` times. A key can also size another array of the
/// same class, as stock first-person controller allocation `8162C3ED` sizes 53 local parameters
/// beside its 121 global ones, so the old count is part of the match.
fn counts(
    allocation: &Payload,
    root: usize,
    changes: &[(u32, u32, usize, usize, usize)],
) -> Result<Payload> {
    let mut p = allocation.clone();
    let mut found = vec![0; changes.len()];
    let mut pending = allocation.array(root, 40, Some(0x80808852))?;
    let mut seen = BTreeSet::new();
    while let Some(row) = pending.pop() {
        ensure!(
            seen.insert(row) && seen.len() < 1024,
            "Pose allocation repeats a node"
        );
        for (index, &(key, class, old, new, _)) in changes.iter().enumerate() {
            if allocation.u32(row)? == key
                && allocation.u32(row + 16)? == class
                && allocation.u32(row + 20)? as usize == old
            {
                put(&mut p.0, row + 20, &u32::try_from(new)?.to_le_bytes())?;
                found[index] += 1;
            }
        }
        if allocation.u64(row + 8)? != 0 {
            pending.push(allocation.pointer(row + 8)?);
        }
        pending.extend(allocation.array(row + 24, 40, Some(0x80808852))?);
    }
    ensure!(
        found
            .iter()
            .zip(changes)
            .all(|(&found, change)| found == change.4),
        "Pose allocation is incomplete"
    );
    Ok(p)
}

/// The counts a pose controller grows by.
pub struct Growth<'a> {
    /// Names of the appended channel inputs, in order.
    pub inputs: &'a [u32],
    /// Parameter count of the controller's library before it was extended.
    pub parameters_before: usize,
    pub parameters: usize,
    pub layers: usize,
}

pub struct Grown {
    pub controller: Payload,
    pub allocation: Payload,
    /// Every input definition offset before and after the inputs moved.
    pub definitions: Vec<(usize, usize)>,
    /// Ordinal and definition offset of each appended input.
    pub added: Vec<(usize, usize)>,
    pub layers_before: usize,
}

/// Appends inputs, parameter states and layer states to `controller`. New inputs copy the
/// first input's declaration and state with their own names. Inputs move to the end of the
/// owner, so connections that consume them must follow `definitions`.
pub fn grow(controller: &Payload, allocation: &Payload, growth: &Growth<'_>) -> Result<Grown> {
    let mut p = controller.clone();
    let root = controller.pointer(16)?;
    let definition = usize::try_from(controller.u64(root + 8)?)?;
    ensure!(
        controller.u32(root - 4)? == 0x8080348f && controller.u32(definition - 4)? == 0x80803640,
        "Pose controller roots differ"
    );
    let old_inputs = controller.array(root + 0x30, 96, Some(0x80809788))?;
    let old_definitions = controller.array(definition + 0xa8, 40, Some(0x80809789))?;
    ensure!(
        !old_inputs.is_empty() && old_inputs.len() == old_definitions.len(),
        "Pose input counts differ"
    );
    let mut inputs = rows(controller, &old_inputs, 96);
    let mut declarations = rows(controller, &old_definitions, 40);
    for _ in growth.inputs {
        inputs.extend(controller.bytes::<96>(old_inputs[0])?);
        declarations.extend(controller.bytes::<40>(old_definitions[0])?);
    }
    append(&mut p.0, root + 0x30, 0x80809788, &inputs, 96)?;
    append(&mut p.0, definition + 0xa8, 0x80809789, &declarations, 40)?;
    let inputs = p.array(root + 0x30, 96, Some(0x80809788))?;
    let declarations = p.array(definition + 0xa8, 40, Some(0x80809789))?;
    for (index, (&input, &declaration)) in inputs.iter().zip(&declarations).enumerate() {
        put(&mut p.0, input + 8, &(declaration as u64).to_le_bytes())?;
        relative(&mut p, input + 16, root + 0x30)?;
        put(&mut p.0, declaration + 8, &(input as u64).to_le_bytes())?;
        if let Some(name) = index
            .checked_sub(old_inputs.len())
            .map(|added| growth.inputs[added])
        {
            put(&mut p.0, declaration + 32, &name.to_le_bytes())?;
        }
    }
    let before = growth.parameters_before;
    let after = before + growth.parameters;
    ensure!(
        controller.u32(definition + 0x144)? as usize == before
            && controller.u32(definition + 0x148)? as usize == before,
        "Pose parameter definition counts differ"
    );
    // A dirty mask holds one bit per parameter in 32-bit words.
    ensure!(
        before.div_ceil(32) == after.div_ceil(32),
        "Pose parameters require a larger dirty mask"
    );
    for (field, stride, class) in [(0x1f0, 4, 0x8080000f), (0x200, 16, 0x80800090)] {
        let old = controller.array(root + field, stride, Some(class))?;
        ensure!(old.len() == before, "Pose parameter state count differs");
        let mut data = rows(controller, &old, stride);
        data.resize(data.len() + growth.parameters * stride, 0);
        append(&mut p.0, root + field, u64::from(class), &data, stride)?;
    }
    for field in [0x144, 0x148] {
        put(
            &mut p.0,
            definition + field,
            &u32::try_from(after)?.to_le_bytes(),
        )?;
    }
    let states = controller.array(root + 0x500, 120, Some(0x8080371a))?;
    ensure!(
        controller.u32(definition + 0x180)? as usize == states.len(),
        "Pose layer state count differs"
    );
    let mut data = rows(controller, &states, 120);
    data.resize(data.len() + growth.layers * 120, 0);
    append(&mut p.0, root + 0x500, 0x8080371a, &data, 120)?;
    put(
        &mut p.0,
        definition + 0x180,
        &u32::try_from(states.len() + growth.layers)?.to_le_bytes(),
    )?;
    let end = p.0.len();
    instance::seal_span(&mut p, &mut Vec::new(), end)?;
    let inputs_before = old_inputs.len();
    let allocation = counts(
        allocation,
        0x20,
        &[
            (
                0x86374c3c,
                0x80809788,
                inputs_before,
                inputs_before + growth.inputs.len(),
                1,
            ),
            (0xcd06e535, 0x8080000f, before, after, 1),
            (0x6b6ba194, 0x80800090, before, after, 1),
            (
                0xe5b438c2,
                0x8080371a,
                states.len(),
                states.len() + growth.layers,
                1,
            ),
        ],
    )?;
    // Sealing can move the definition, so offsets are read again.
    let declarations = p.array(
        usize::try_from(p.u64(p.pointer(16)? + 8)?)? + 0xa8,
        40,
        Some(0x80809789),
    )?;
    Ok(Grown {
        definitions: old_definitions
            .iter()
            .copied()
            .zip(declarations.iter().copied())
            .collect(),
        added: (inputs_before..declarations.len())
            .map(|ordinal| (ordinal, declarations[ordinal]))
            .collect(),
        controller: p,
        allocation,
        layers_before: states.len(),
    })
}

/// Grows the five layer buffers of animation implementation `consumer`, owner tag `owner`, from
/// `before` layers by `added`, with its allocation. Each buffer's records name their owner.
pub fn outputs(
    consumer: &Payload,
    owner: u32,
    allocation: &Payload,
    before: usize,
    added: usize,
) -> Result<(Payload, Payload)> {
    let mut p = consumer.clone();
    let root = consumer.pointer(16)?;
    ensure!(
        consumer.u32(root - 4)? == 0x808036e2,
        "Animation implementation differs"
    );
    for (offset, implementation, definition_class, row_class, stride) in [
        (0x560, 0x80803708, 0x80803709, 0x8080371a, 120),
        (0x5a0, 0x80803708, 0x80803709, 0x8080371a, 120),
        (0x620, 0x8080370c, 0x8080370d, 0x8080370f, 56),
        (0x660, 0x8080370c, 0x8080370d, 0x8080370f, 56),
        (0x6a0, 0x8080370c, 0x8080370d, 0x8080370f, 56),
    ] {
        let instance = root + offset;
        let definition = usize::try_from(consumer.u64(instance + 8)?)?;
        ensure!(
            consumer.u32(instance)? == owner
                && consumer.u32(instance + 4)? == definition_class
                && consumer.u32(definition)? == owner
                && consumer.u32(definition + 4)? == implementation
                && consumer.u64(definition + 8)? == instance as u64
                && consumer.u32(definition + 16)? as usize == before,
            "Animation layer buffer definition differs"
        );
        let old = consumer.array(instance + 32, stride, Some(row_class))?;
        ensure!(old.len() == before, "Animation layer buffer count differs");
        let mut data = rows(consumer, &old, stride);
        data.resize(data.len() + added * stride, 0);
        append(&mut p.0, instance + 32, u64::from(row_class), &data, stride)?;
        put(
            &mut p.0,
            definition + 16,
            &u32::try_from(before + added)?.to_le_bytes(),
        )?;
    }
    let end = p.0.len();
    instance::seal_span(&mut p, &mut Vec::new(), end)?;
    let allocation = counts(
        allocation,
        32,
        &[
            (0xe5b438c2, 0x8080371a, before, before + added, 2),
            (0x87e5340d, 0x8080370f, before, before + added, 3),
        ],
    )?;
    Ok((p, allocation))
}

/// Appends two parameters to library `library`: one reading controller input `input`, which
/// carries channel `channel`, and one mapping it linearly from `range` to phase `[0, 1]`.
/// Returns the library and the index of the phase parameter.
pub fn ramp(
    library: &Payload,
    names: [u32; 2],
    channel: u32,
    input: usize,
    range: [f32; 2],
) -> Result<(Payload, usize)> {
    ensure!(
        range.iter().all(|v| v.is_finite()) && range[0] < range[1],
        "Invalid pose parameter range"
    );
    let old = library.array(8, 24, Some(0x80803650))?;
    let first = old.len();
    ensure!(
        first + 2 < i16::MAX as usize,
        "Pose parameter index exceeds its native domain"
    );
    let mut p = library.clone();
    let mut records = rows(library, &old, 24);
    records.resize(records.len() + 2 * 24, 0);
    append(&mut p.0, 8, 0x80803650, &records, 24)?;
    let written = p.array(8, 24, Some(0x80803650))?;
    for (&from, &to) in old.iter().zip(&written) {
        for offset in [8, 16] {
            if library.u64(from + offset)? != 0 {
                relative(&mut p, to + offset, library.pointer(from + offset)?)?;
            }
        }
    }
    let node = |p: &mut Payload, class: u32, size: usize| -> Result<usize> {
        let at = (p.0.len() + 19) & !15;
        p.0.resize(at + size, 0);
        put(&mut p.0, at - 4, &class.to_le_bytes())?;
        Ok(at)
    };
    // Kind 20 reads one root input, named by its channel, at its controller ordinal.
    let row = written[first];
    put(&mut p.0, row, &names[0].to_le_bytes())?;
    put(&mut p.0, row + 4, &[20, 2, 0, 0])?;
    let source = node(&mut p, 0x80803655, 8)?;
    relative(&mut p, row + 8, source)?;
    put(&mut p.0, source, &channel.to_le_bytes())?;
    put(&mut p.0, source + 4, &u32::try_from(input)?.to_le_bytes())?;
    // Kind 21 evaluates a four-lane program over the parameters it references.
    let row = written[first + 1];
    put(&mut p.0, row, &names[1].to_le_bytes())?;
    put(&mut p.0, row + 4, &[21, 2, 0, 0])?;
    let expression = node(&mut p, 0x80803653, 80)?;
    relative(&mut p, row + 8, expression)?;
    put(&mut p.0, expression + 64, &u32::MAX.to_le_bytes())?;
    append(
        &mut p.0,
        expression,
        0x8080373d,
        &(0x10000u32 | u32::try_from(first)?).to_le_bytes(),
        4,
    )?;
    // (input - low) / (high - low), the program the vehicle steering phase compiles. The
    // direct-phase consumer clamps the result to [0, 1].
    let constants = [range[0], range[1] - range[0], 1., 0.];
    let code = [
        0x3c, 0, 0x34, 0, 0x02, 0x34, 1, 0x04, 0x23, 0x34, 2, 0x03, 0x34, 3, 0x01, 0x3e, 0,
    ];
    let at = expression + 16;
    append(&mut p.0, at, 0x80800009, &code, 1)?;
    let lanes = constants
        .iter()
        .flat_map(|&value| [value; 4])
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    append(&mut p.0, at + 16, 0x80800090, &lanes, 16)?;
    put(&mut p.0, at + 32, &1u64.to_le_bytes())?;
    put(&mut p.0, at + 40, &1u64.to_le_bytes())?;
    let size = p.0.len() as u64;
    put(&mut p.0, 0, &size.to_le_bytes())?;
    Ok((p, first + 1))
}

/// The global (`+08`) and local (`+18`) parameters of library `library` (`8080364C`), as name and
/// the byte after the kind at row `+05`, by index. A packed reference with bit 16 set indexes the
/// global array, otherwise the local one.
pub fn parameter_names(library: &Payload) -> Result<[Vec<(u32, u8)>; 2]> {
    let mut names = [Vec::new(), Vec::new()];
    for (array, field) in names.iter_mut().zip([8, 0x18]) {
        for row in library.array(field, 24, Some(0x80803650))? {
            array.push((library.u32(row)?, library.u8(row + 5)?));
        }
    }
    Ok(names)
}

/// The input ordinal of `name` in `controller`, when it already has one.
pub fn input(controller: &Payload, name: u32) -> Result<Option<usize>> {
    let root = controller.pointer(16)?;
    let definition = usize::try_from(controller.u64(root + 8)?)?;
    let found = controller
        .array(definition + 0xa8, 40, Some(0x80809789))?
        .into_iter()
        .enumerate()
        .filter(|&(_, at)| controller.u32(at + 32).ok() == Some(name))
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    ensure!(found.len() <= 1, "Pose input name repeats");
    Ok(found.first().copied())
}

/// The library and layer table tags a pose controller names.
pub fn resources(controller: &Payload) -> Result<(u32, u32)> {
    let root = controller.pointer(16)?;
    let definition = usize::try_from(controller.u64(root + 8)?)?;
    Ok((
        controller.u32(definition + 0x110)?,
        controller.u32(definition + 0x11c)?,
    ))
}
