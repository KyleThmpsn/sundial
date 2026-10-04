//! Source channel banks hosted by the native bank implementation.
//!
//! Connections to other controllers remain explicit object relocations. Native
//! interface metadata supplies native methods, never shifted source ordinals.
use super::{append_array, array_bytes, procedural, put};
use crate::d2_mot::{entity::links::Object, payload::Payload};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
mod allocation;
pub(super) mod constants;
pub mod damage;
pub mod movement;
pub mod network;
pub mod parameters;
pub mod response;
pub mod scalar;
#[cfg(test)]
mod tests;

#[derive(Serialize)]
pub struct Relocation {
    pub source: Object,
    pub target: Object,
}

pub struct Bank {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    pub channels: usize,
    pub inputs: usize,
    /// Serialized translation does not establish incoming provider connections
    /// or application of allocation metadata during native construction.
    pub gates: Vec<String>,
}

/// Finish a controller whose nested runtime records were appended during
/// assembly. Conservatively include the entire assembled stream in its copied
/// instance span. Definitions remain addressable at their original tag offsets.
pub fn seal(owner: &mut Payload) -> Result<()> {
    let end = owner.0.len();
    super::layout::seal_span(owner, &mut Vec::new(), end)?;
    Ok(())
}

fn copy_array(
    source: &Payload,
    destination: &mut Payload,
    from: usize,
    to: usize,
    source_class: u32,
    target_class: u32,
    stride: usize,
) -> Result<usize> {
    let rows = source.array(from, stride, Some(source_class))?;
    let bytes = array_bytes(source, from, stride)?;
    append_array(
        &mut destination.0,
        to,
        u64::from(target_class),
        &bytes,
        stride,
    )?;
    Ok(rows.len())
}

/// Translate a complete channel bank into a caller-allocated owner and allocation
/// tag. The native template supplies only the implementation/dispatch envelope.
pub fn bank(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Bank> {
    for payload in [source, template, allocation] {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64,
            "bank payload size differs"
        );
    }
    ensure!(
        owner_tag != u32::MAX && allocation_tag != u32::MAX && owner_tag != allocation_tag,
        "invalid translated bank tags"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    ensure!(
        source.u32(si - 4)? == 0x808095B1 && source.u32(sd - 4)? == 0x80809597,
        "unsupported source channel bank"
    );
    ensure!(
        template.u32(ni - 4)? == 0x8080979F && template.u32(nd - 4)? == 0x80809790,
        "unsupported native channel bank envelope"
    );
    let source_tag = source.u32(si)?;
    let old_tag = template.u32(ni)?;
    ensure!(
        source.u32(si + 4)? == 0x80809597
            && source.u32(sd + 4)? == 0x808095B1
            && source.u32(sd)? == source_tag
            && source.u64(si + 8)? == sd as u64
            && source.u64(sd + 8)? == si as u64,
        "source bank pair differs"
    );
    ensure!(
        template.u32(ni + 4)? == 0x80809790
            && template.u32(nd + 4)? == 0x8080979F
            && template.u32(nd)? == old_tag
            && template.u64(ni + 8)? == nd as u64
            && template.u64(nd + 8)? == ni as u64,
        "native bank pair differs"
    );
    let declarations = source.array(sd + 0x148, 112, Some(0x808095A9))?;
    ensure!(
        !declarations.is_empty() && declarations.len() <= 256,
        "unsupported bank dependency capacity"
    );
    for (start, end) in [(0x10, 0x30), (0xB0, 0xB8), (0xC8, 0xD0)] {
        ensure!(
            source.0[si + start..si + end] == template.0[ni + start..ni + end],
            "unsupported bank runtime defaults"
        );
    }
    let mask_words = declarations.len().div_ceil(32);
    let numeric = source.array(si + 0x90, 16, Some(0x808095A5))?;
    ensure!(
        source.array(si + 0x40, 4, Some(0x8080000B))?.len() == mask_words
            && source.array(si + 0x50, 16, Some(0x80800090))?.len() == declarations.len()
            && source.array(si + 0xA0, 4, Some(0x8080000B))?.len() == numeric.len().div_ceil(32),
        "bank channel state dimensions differ"
    );
    for &row in &declarations {
        let storage = source.u16(row + 72)?;
        ensure!(
            storage == u16::MAX || usize::from(storage) < numeric.len(),
            "bank stored slot outside numeric allocation"
        );
        let deps = source.array(row + 80, 4, Some(0x8080000B))?;
        ensure!(
            deps.is_empty() || deps.len() == mask_words,
            "bank dependency mask width differs"
        );
        if let Some(last) = deps.last() {
            let remainder = declarations.len() % 32;
            ensure!(
                remainder == 0 || source.u32(*last)? >> remainder == 0,
                "bank dependency outside declarations"
            );
        }
    }
    let order = declarations
        .iter()
        .map(|at| Ok(format!("{:08X}", source.u32(*at)?)))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        order
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == order.len(),
        "ambiguous bank channel names"
    );
    let source_inputs = source.array(si + 0x30, 80, Some(0x808095AF))?;
    let source_defs = source.array(sd + 0x138, 32, Some(0x808095B0))?;
    ensure!(
        source_inputs.len() == source_defs.len(),
        "bank input pair count differs"
    );
    let native_inputs = template.array(ni + 0x30, 80, Some(0x808097A7))?;
    let native_defs = template.array(nd + 0xC8, 32, Some(0x808097A8))?;
    let runtime_template = *native_inputs
        .first()
        .context("native bank has no input envelope")?;
    let definition_template = *native_defs
        .first()
        .context("native bank has no input definition")?;
    ensure!(
        template.u32(runtime_template)? == old_tag
            && template.u32(runtime_template + 4)? == 0x808097A8
            && template.u64(runtime_template + 8)? == definition_template as u64
            && template.u32(definition_template)? == old_tag
            && template.u32(definition_template + 4)? == 0x808097A7
            && template.u64(definition_template + 8)? == runtime_template as u64
            && template.u64(definition_template + 16)? == 0
            && template.u64(definition_template + 24)? == 0x80809AE2,
        "native bank input envelope differs"
    );
    let mut owner = template.clone();
    let mut objects = vec![];
    // Native interfaces expose lifecycle and channel access through native
    // metadata. Source-only interfaces are deliberately absent from this map.
    for (source_class, source_offset, target_class, target_offset) in [
        (0x808095B1, si, 0x8080979F, ni),
        (0x80809597, sd, 0x80809790, nd),
        (0x808095B3, sd + 0x48, 0x80807C75, nd + 0x48),
        (0x808095CF, sd + 0x68, 0x808097C2, nd + 0x60),
        (0x808095C6, sd + 0x88, 0x808097BB, nd + 0x78),
        (0x808098CD, sd + 0xA8, 0x80809ADE, nd + 0x90),
        (0x808098C9, sd + 0xC8, 0x80809ADA, nd + 0xA8),
    ] {
        objects.push(Relocation {
            source: Object {
                owner: source_tag,
                class: source_class,
                offset: source_offset as u64,
            },
            target: Object {
                owner: owner_tag,
                class: target_class,
                offset: target_offset as u64,
            },
        });
    }
    let mut input_data = Vec::new();
    let mut definition_data = Vec::new();
    for (&input, &definition) in source_inputs.iter().zip(&source_defs) {
        ensure!(
            source.u32(input)? == source_tag
                && source.u32(definition)? == source_tag
                && source.u32(input + 4)? == 0x808095B0
                && source.u32(definition + 4)? == 0x808095AF
                && source.u64(input + 8)? == definition as u64
                && source.u64(definition + 8)? == input as u64,
            "source provider input pair differs"
        );
        ensure!(
            source.pointer(input + 16)? == si
                && template.pointer(runtime_template + 16)? == ni
                && source.0[input + 24..input + 80]
                    == template.0[runtime_template + 24..runtime_template + 80]
                && source.u64(definition + 16)? == 0
                && source.u64(definition + 24)? == 0x808098D3,
            "source provider input state differs from native contract"
        );
        input_data.extend_from_slice(&template.0[runtime_template..runtime_template + 80]);
        definition_data
            .extend_from_slice(&template.0[definition_template..definition_template + 32]);
    }
    append_array(&mut owner.0, ni + 0x30, 0x808097A7, &input_data, 80)?;
    append_array(&mut owner.0, nd + 0xC8, 0x808097A8, &definition_data, 32)?;
    let inputs = owner.array(ni + 0x30, 80, Some(0x808097A7))?;
    let definitions = owner.array(nd + 0xC8, 32, Some(0x808097A8))?;
    for (i, (&input, &definition)) in inputs.iter().zip(&definitions).enumerate() {
        put(
            &mut owner.0,
            input + 16,
            &(i64::try_from(ni)? - i64::try_from(input + 16)?).to_le_bytes(),
        )?;
        for (at, class, paired) in [
            (input, 0x808097A8u32, definition),
            (definition, 0x808097A7, input),
        ] {
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
            put(&mut owner.0, at + 4, &class.to_le_bytes())?;
            put(&mut owner.0, at + 8, &(paired as u64).to_le_bytes())?;
        }
        for (at, class, target, target_class) in [
            (source_inputs[i], 0x808095AF, input, 0x808097A7),
            (source_defs[i], 0x808095B0, definition, 0x808097A8),
        ] {
            objects.push(Relocation {
                source: Object {
                    owner: source_tag,
                    class,
                    offset: at as u64,
                },
                target: Object {
                    owner: owner_tag,
                    class: target_class,
                    offset: target as u64,
                },
            });
        }
    }
    let mut allocation_counts = vec![(0xA8333EC9u32, 0x808097A7u32, inputs.len())];
    for (field, stride, source_class, target_class, name) in [
        (0x40, 4, 0x8080000B, 0x8080000B, 0x4A423762),
        (0x50, 16, 0x80800090, 0x80800090, 0xFC2F3D6F),
        (0x60, 16, 0x80800090, 0x80800090, 0xB6515162),
        (0x80, 8, 0x808095A6, 0x8080979D, 0xBAFA25AF),
        (0x90, 16, 0x808095A5, 0x8080979C, 0x3A55A801),
        (0xA0, 4, 0x8080000B, 0x8080000B, 0x083C8E12),
        (0xB8, 24, 0x808095A3, 0x8080979A, 0x6D1C25F8),
    ] {
        let count = copy_array(
            source,
            &mut owner,
            si + field,
            ni + field,
            source_class,
            target_class,
            stride,
        )?;
        allocation_counts.push((name, target_class, count));
    }
    ensure!(
        source.u64(si + 0x70)? == 0 && source.u64(si + 0x78)? == 0,
        "unsupported source bank auxiliary runtime state"
    );
    put(&mut owner.0, ni + 0x70, &[0; 16])?;
    let source_tail = source.bytes::<40>(sd + 0x198)?;
    put(&mut owner.0, nd + 0x128, &source_tail)?;
    for (from, to) in [(0x168, 0xF8), (0x178, 0x108), (0x188, 0x118)] {
        copy_array(
            source,
            &mut owner,
            sd + from,
            nd + to,
            0x808095A8,
            0x808097A0,
            12,
        )?;
    }
    ensure!(
        source.u64(sd + 0x158)? == 0 && source.u64(sd + 0x160)? == 0,
        "unsupported source channel bank extension"
    );
    put(&mut owner.0, nd + 0xE8, &[0; 16])?;
    channels(source, &mut owner, &declarations, &order)?;
    // Replace self references left in the native envelope and detached template
    // arrays too. Every occurrence must be a checked typed owner reference.
    for at in (0..owner.0.len().saturating_sub(15)).step_by(4) {
        if owner.u32(at)? == old_tag {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "untyped native bank owner occurrence"
            );
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
        }
    }
    put(&mut owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    let allocation = allocate(allocation, allocation_counts)?;
    seal(&mut owner)?;
    Ok(Bank {
        owner,
        allocation,
        objects,
        channels: declarations.len(),
        inputs: inputs.len(),
        gates: vec![
            "Incoming provider connections require validated native producers".into(),
            "Provider controller state banks and input semantics require separate translation"
                .into(),
            "Native construction and allocation metadata application remain unverified".into(),
        ],
    })
}

fn allocate(allocation: &Payload, counts: Vec<(u32, u32, usize)>) -> Result<Payload> {
    let mut allocation = allocation.clone();
    let allocation_rows = allocation.array(0x20, 40, Some(0x80808852))?;
    let mut allocation_data = Vec::new();
    for (name, class, count) in counts {
        if count == 0 {
            continue;
        }
        let candidates = allocation_rows
            .iter()
            .copied()
            .filter(|r| allocation.u32(*r).ok() == Some(name))
            .collect::<Vec<_>>();
        let row = if let [row] = candidates.as_slice() {
            *row
        } else if name == 0xB6515162 {
            *allocation_rows
                .iter()
                .find(|r| allocation.u32(**r).ok() == Some(0xFC2F3D6F))
                .context("vector allocation template")?
        } else {
            anyhow::bail!("native allocation template missing {name:08X}");
        };
        ensure!(
            allocation.u32(row + 16)? == class && allocation.u64(row + 24)? == 0,
            "native allocation shape differs"
        );
        let mut bytes = allocation.bytes::<40>(row)?;
        bytes[..4].copy_from_slice(&name.to_le_bytes());
        bytes[20..24].copy_from_slice(&u32::try_from(count)?.to_le_bytes());
        allocation_data.extend(bytes);
    }
    append_array(&mut allocation.0, 0x20, 0x80808852, &allocation_data, 40)?;
    Ok(allocation)
}

fn channels(
    source: &Payload,
    owner: &mut Payload,
    declarations: &[usize],
    order: &[String],
) -> Result<()> {
    let sd = source.pointer(24)?;
    let nd = owner.pointer(24)?;
    let mut declaration_data = array_bytes(source, sd + 0x148, 112)?;
    for row in declaration_data.chunks_exact_mut(112) {
        for pointer in [16, 32, 64, 88, 96] {
            row[pointer..pointer + 8].fill(0);
        }
    }
    append_array(&mut owner.0, nd + 0xD8, 0x808097A1, &declaration_data, 112)?;
    let target_rows = owner.array(nd + 0xD8, 112, Some(0x808097A1))?;
    for (&from, &to) in declarations.iter().zip(&target_rows) {
        copy_array(source, owner, from + 80, to + 80, 0x8080000B, 0x8080000B, 4)?;
        if source.u64(from + 8)? != 0 {
            let procedure = procedural::Declaration::read(source, from, declarations)?;
            let state = if procedure.interpolation_state.is_some() {
                Some(source.u32(source.pointer(from + 96)? + 4)?)
            } else {
                None
            };
            procedure.write(&mut owner.0, to, order, state)?;
        } else {
            ensure!(
                source.0[from + 16..from + 72].iter().all(|v| *v == 0)
                    && source.u64(from + 96)? == 0,
                "unsupported bank declaration default"
            );
        }
    }
    Ok(())
}
