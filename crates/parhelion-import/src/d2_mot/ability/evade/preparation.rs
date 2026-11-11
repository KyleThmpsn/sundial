//! Translate the seven movement preparation programs onto their native consumer fields.
use super::pair;
use crate::{
    d2_mot::native::effects::{self, controller},
    presentation::{append, put},
    tiger::{entity::links::Object, payload::Payload},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// The containing graph still owns profile binding, native provider registration, allocation
/// and sealing. Replacing its math alone does not construct an equippable ability.
pub struct Motion {
    pub owner: Payload,
    pub objects: Vec<controller::Relocation>,
    pub evidence: Value,
}

fn relocation(
    source: &Payload,
    native: &Payload,
    from: usize,
    to: usize,
    classes: [u32; 2],
) -> Result<controller::Relocation> {
    Ok(controller::Relocation {
        source: Object {
            owner: source.u32(source.pointer(16)?)?,
            class: classes[0],
            offset: from as u64,
        },
        target: Object {
            owner: native.u32(native.pointer(16)?)?,
            class: classes[1],
            offset: to as u64,
        },
    })
}

fn plain(
    source: &Payload,
    native: &mut Payload,
    from: usize,
    to: usize,
    parents: [usize; 2],
) -> Result<Value> {
    for (p, definition, parent, classes) in [
        (source, from, parents[0], [0x8080862D, 0x8080862E]),
        (&*native, to, parents[1], [0x808089F3, 0x808089F4]),
    ] {
        let instance = usize::try_from(p.u64(definition + 8)?)?;
        pair(p, definition, instance, classes)?;
        ensure!(
            p.pointer(instance + 16)? == parent
                && p.u64(instance + 24)? == 0
                && p.u64(definition + 48)? == 1
                && p.u64(definition + 56)? == 1
                && p.u64(definition + 64)? == 0,
            "Movement preparation expression has another parent or input contract"
        );
    }
    let source_code = source
        .array(from + 16, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| source.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let mut constants = source
        .array(from + 32, 16, Some(0x80800090))?
        .into_iter()
        .flat_map(|at| source.0[at..at + 16].iter().copied())
        .collect::<Vec<_>>();
    ensure!(
        constants.chunks_exact(4).all(|lane| {
            f32::from_le_bytes(lane.try_into().expect("Four-byte constant lane")).is_finite()
        }),
        "Movement preparation has non-finite constants"
    );
    let code = effects::lower_program(&source_code, constants.len() / 16, 1)?;
    let source_constants = hex::encode(&constants);
    effects::lower_constants(&source_code, &mut constants);
    append(&mut native.0, to + 16, 0x80800009, &code, 1)?;
    append(&mut native.0, to + 32, 0x80800090, &constants, 16)?;
    Ok(json!({
        "source_code":hex::encode(source_code),
        "native_code":hex::encode(code),
        "source_constants":source_constants,"constants":hex::encode(constants),
        "inputs":1,"outputs":1
    }))
}

/// Replace an already bound native preparation's math from a modern preparation. Both consume
/// one scalar input in each of the seven corresponding named fields. The native provider tail
/// stays bound to its native implementation. Its registration and the parent profile are the
/// containing graph author's obligations.
pub fn replace_motion(
    source: &Payload,
    native: &Payload,
    from: usize,
    to: usize,
) -> Result<Motion> {
    let si = usize::try_from(source.u64(from + 8)?)?;
    let ni = usize::try_from(native.u64(to + 8)?)?;
    pair(source, from, si, [0x808037FC, 0x808037FD])?;
    pair(native, to, ni, [0x808044BE, 0x808044BF])?;
    ensure!(
        source.u32(from + 0x20)? <= 1
            && source.u32(from + 0x24)? == 0
            && native.u32(to + 0x20)? <= 1
            && native.u32(to + 0x24)? == 0,
        "Movement preparation has an unsupported mode"
    );
    let mut owner = native.clone();
    for (start, end) in [(0x10, 0x20), (0x148, 0x160), (0x250, 0x270)] {
        ensure!(
            (start..end)
                .step_by(4)
                .all(|offset| source.f32(from + offset).is_ok_and(f32::is_finite)),
            "Movement preparation has non-finite numeric settings"
        );
        put(
            &mut owner.0,
            to + start,
            &source.0[from + start..from + end],
        )?;
    }
    put(&mut owner.0, to + 0x20, &source.bytes::<4>(from + 0x20)?)?;
    let mut objects = vec![
        relocation(source, &owner, from, to, [0x808037FD, 0x808044BF])?,
        relocation(source, &owner, si, ni, [0x808037FC, 0x808044BE])?,
    ];
    let mut programs = Vec::new();
    for offset in [0x28, 0x70, 0xB8, 0x100, 0x160, 0x1A8] {
        let evidence = plain(source, &mut owner, from + offset, to + offset, [si, ni])?;
        programs.push(json!({"field":offset,"program":evidence}));
        objects.push(relocation(
            source,
            &owner,
            from + offset,
            to + offset,
            [0x8080862E, 0x808089F4],
        )?);
        objects.push(relocation(
            source,
            &owner,
            usize::try_from(source.u64(from + offset + 8)?)?,
            usize::try_from(owner.u64(to + offset + 8)?)?,
            [0x8080862D, 0x808089F3],
        )?);
    }
    let len = owner.0.len() as u64;
    put(&mut owner.0, 0, &len.to_le_bytes())?;
    ensure!(
        source.u64(from + 0x230)? == 0 && owner.u64(to + 0x230)? == 0,
        "Movement preparation has external scalar inputs"
    );
    let converted = controller::scalar::emit_nested(
        source,
        &mut owner,
        native,
        from + 0x1F0,
        to + 0x1F0,
        controller::scalar::Contract {
            source_parent: si,
            native_parent: ni,
            builtins: 1,
        },
    )?;
    ensure!(
        converted.extra_inputs == 0,
        "Movement preparation added external inputs"
    );
    programs.push(json!({"field":0x1F0,"program":{
        "source_code":hex::encode(converted.source_code),
        "native_code":hex::encode(converted.native_code),
        "constants":hex::encode(converted.constants),"inputs":converted.inputs,"outputs":1
    }}));
    objects.extend(converted.objects);
    Ok(Motion {
        owner,
        objects,
        evidence: json!({"programs":programs,"source_definition":from,
            "native_definition":to,"provider_tail":"native", "gameplay_verified":false}),
    })
}
