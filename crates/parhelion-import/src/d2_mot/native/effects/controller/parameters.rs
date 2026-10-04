//! Named spawn parameters consumed by the projectile's expression bank.
use super::*;
use crate::d2_mot::entity::links::Interface;
use std::collections::{BTreeMap, BTreeSet};

pub struct Parameters {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
}

fn method(
    payload: &Payload,
    at: usize,
    metadata: &BTreeMap<u32, Payload>,
    modern: bool,
) -> Result<()> {
    let object = Object {
        owner: payload.u32(at)?,
        class: if modern { 0x808098D2 } else { 0x80809AE1 },
        offset: (at + 16) as u64,
    };
    let interface = Interface::read(
        payload,
        object,
        metadata
            .get(&payload.u32(at + 24)?)
            .context("parameter provider metadata absent")?,
        modern,
    )?;
    ensure!(
        interface.definition_offset == at
            && interface.instance_class == if modern { 0x808098D3 } else { 0x80809AE2 }
            && interface.methods.len() == 1,
        "parameter provider interface differs"
    );
    let method = &interface.methods[0];
    ensure!(
        method.implementation_class == if modern { 0x8080875C } else { 0x80808BEB }
            && method.index == 1
            && method.arguments == [0, 0],
        "parameter getter contract differs"
    );
    Ok(())
}

fn paired(
    payload: &Payload,
    instance: usize,
    definition: usize,
    owner: u32,
    ic: u32,
    dc: u32,
) -> Result<()> {
    ensure!(
        payload.u32(instance)? == owner
            && payload.u32(definition)? == owner
            && payload.u32(instance + 4)? == dc
            && payload.u32(definition + 4)? == ic
            && payload.u64(instance + 8)? == definition as u64
            && payload.u64(definition + 8)? == instance as u64,
        "parameter pair differs"
    );
    Ok(())
}

pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Parameters> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid parameter component tags"
    );
    for payload in [source, template, allocation] {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64,
            "parameter component size differs"
        );
    }
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    ensure!(
        si >= 4
            && sd >= 4
            && ni >= 4
            && nd >= 4
            && source.u32(si - 4)? == 0x80808760
            && source.u32(sd - 4)? == 0x80808757
            && template.u32(ni - 4)? == 0x80808BF1
            && template.u32(nd - 4)? == 0x80808BE6,
        "parameter component classes differ"
    );
    let source_tag = source.u32(si)?;
    let old_tag = template.u32(ni)?;
    paired(source, si, sd, source_tag, 0x80808760, 0x80808757)?;
    paired(template, ni, nd, old_tag, 0x80808BF1, 0x80808BE6)?;
    ensure!(
        source.bytes::<48>(si + 16)? == template.bytes::<48>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == template.bytes::<56>(nd + 16)?
            && source.bytes::<32>(sd + 0x98)? == [0; 32]
            && template.bytes::<16>(nd + 0x90)? == [0; 16],
        "parameter component contains unsupported state or extensions"
    );
    let inputs = source.array(si + 0x40, 48, Some(0x8080875C))?;
    let declarations = source.array(sd + 0xB8, 80, Some(0x8080875D))?;
    ensure!(
        inputs.len() == declarations.len() && !inputs.is_empty() && inputs.len() <= 256,
        "parameter count differs or exceeds capacity"
    );
    let state_end = inputs[0]
        .checked_sub(20)
        .context("parameter runtime array header")?;
    ensure!(
        source
            .0
            .get(si + 0x50..state_end)
            .context("parameter runtime defaults")?
            .iter()
            .all(|byte| *byte == 0),
        "parameter component has initialized extension state"
    );
    let target = *template
        .array(nd + 0xA0, 64, Some(0x80808BEC))?
        .first()
        .context("native parameter envelope absent")?;
    let target_instance = usize::try_from(template.u64(target + 8)?)?;
    paired(
        template,
        target_instance,
        target,
        old_tag,
        0x80808BEB,
        0x80808BEC,
    )?;
    method(template, target, metadata, false)?;
    let mut owner = template.clone();
    append_array(
        &mut owner.0,
        ni + 0x40,
        0x80808BEB,
        &vec![0; inputs.len() * 48],
        48,
    )?;
    append_array(
        &mut owner.0,
        nd + 0xA0,
        0x80808BEC,
        &vec![0; inputs.len() * 64],
        64,
    )?;
    let target_inputs = owner.array(ni + 0x40, 48, Some(0x80808BEB))?;
    let target_declarations = owner.array(nd + 0xA0, 64, Some(0x80808BEC))?;
    let mut objects = Vec::new();
    let mut names = BTreeSet::new();
    for (index, (&input, &definition)) in inputs.iter().zip(&declarations).enumerate() {
        paired(
            source, input, definition, source_tag, 0x8080875C, 0x8080875D,
        )?;
        method(source, definition, metadata, true)?;
        let name = source.u32(definition + 48)?;
        ensure!(
            names.insert(name)
                && source.u32(definition + 52)? == 1
                && source.u64(definition + 32)? == 0
                && source.u64(definition + 40)? == 0
                && source.u64(definition + 72)? == 0
                && source.pointer(input + 16)? == si
                && source.u64(input + 24)? == 0
                && source.bytes::<16>(input + 32)? == source.bytes::<16>(definition + 56)?,
            "parameter name, mode or initial state differs"
        );
        for lane in 0..4 {
            source.f32(definition + 56 + lane * 4)?;
        }
        let i = target_inputs[index];
        let d = target_declarations[index];
        put(&mut owner.0, d, &template.bytes::<64>(target)?)?;
        put(
            &mut owner.0,
            i + 16,
            &(ni as i64 - (i + 16) as i64).to_le_bytes(),
        )?;
        put(&mut owner.0, i + 32, &source.bytes::<16>(definition + 56)?)?;
        put(&mut owner.0, d + 40, &source.bytes::<24>(definition + 48)?)?;
        for (at, twin, class) in [(i, d, 0x80808BECu32), (d, i, 0x80808BEB)] {
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
            put(&mut owner.0, at + 4, &class.to_le_bytes())?;
            put(&mut owner.0, at + 8, &(twin as u64).to_le_bytes())?;
        }
        for (from, source_class, to, native_class) in [
            (input, 0x8080875C, i, 0x80808BEB),
            (definition, 0x8080875D, d, 0x80808BEC),
            (definition + 16, 0x808098D2, d + 16, 0x80809AE1),
        ] {
            objects.push(Relocation {
                source: Object {
                    owner: source_tag,
                    class: source_class,
                    offset: from as u64,
                },
                target: Object {
                    owner: owner_tag,
                    class: native_class,
                    offset: to as u64,
                },
            });
        }
    }
    for at in (0..owner.0.len().saturating_sub(15)).step_by(4) {
        if owner.u32(at)? == old_tag {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "untyped parameter owner occurrence"
            );
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
        }
    }
    put(&mut owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    let allocation = allocate(allocation, vec![(0xBD97BB75, 0x80808BEB, inputs.len())])?;
    seal(&mut owner)?;
    Ok(Parameters {
        owner,
        allocation,
        objects,
    })
}
