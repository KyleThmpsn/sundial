//! Serialized initial health state, region fractions and optional hit storage.
use super::*;

pub(super) struct State {
    pub objects: Vec<Relocation>,
    pub allocation: Vec<allocation::Row>,
    pub unbound: Vec<Object>,
}

pub(super) fn emit(source: &Payload, output: &mut Payload, regions: usize) -> Result<State> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let from = si + 0x340;
    let definition = sd + 0x6E0;
    let to = ni + 0x240;
    let target_definition = nd + 0x590;
    check_pair(source, from, definition, 0x80804013, 0x80804014, si)?;
    check_pair(
        source,
        from + 0x100,
        definition + 16,
        0x80802125,
        0x80802126,
        from,
    )?;
    ensure!(
        source.u64(definition + 32)? == regions as u64
            && source.u64(definition + 40)? == regions as u64,
        "health state region dimensions differ"
    );
    let mut defaults = [0u8; 216];
    for (at, bytes) in [
        (0x24, u32::MAX.to_le_bytes()),
        (0x28, u32::MAX.to_le_bytes()),
        (0x5C, 1f32.to_le_bytes()),
        (0x8C, 1f32.to_le_bytes()),
        (0xA0, 0x811C9DC5u32.to_le_bytes()),
        (0xA4, 0x811C9DC5u32.to_le_bytes()),
        (0xD4, u32::MAX.to_le_bytes()),
    ] {
        defaults[at - 24..at - 20].copy_from_slice(&bytes);
    }
    defaults[0x4D - 24..0x51 - 24].fill(255);
    ensure!(
        source.bytes::<216>(from + 24)? == defaults,
        "health state is already initialized"
    );
    ensure!(
        source.u64(from + 0x118)? == 0
            && source.u64(from + 0x138)? == 0
            && source.u64(from + 0x140)? == 0x811C9DC5
            && source.bytes::<16>(from + 0x148)? == [0; 16]
            && source.u32(from + 0x158)? == 0x811C9DC5
            && source.u32(from + 0x15C)? == 0x811C9DC5
            && source.u32(from + 0x160)? == 0
            && source.u32(from + 0x164)? == 0xFF00
            && source.u64(from + 0x168)? == 0
            && source.u32(from + 0x134)? == 0,
        "health capacity state extension differs"
    );
    let capacity = source.f32(from + 0x130)?;
    ensure!(
        capacity.is_finite() && capacity >= 0.0,
        "invalid initial health capacity"
    );
    let extra = source.array(from + 0x120, 40, Some(0x80802128))?;
    ensure!(
        extra.len() == regions,
        "health capacity input count differs"
    );
    for at in extra {
        ensure!(
            source.bytes::<12>(at)? == [0, 0, 128, 191, 0, 0, 128, 191, 0, 0, 128, 191]
                && source.u32(at + 12)? == 0
                && source.u64(at + 16)? == u64::MAX
                && source.bytes::<16>(at + 24)? == [0; 16],
            "health capacity input contains active state"
        );
    }
    let fractions = source.array(from + 0xF0, 4, Some(0x808040DE))?;
    ensure!(
        fractions.len() == regions,
        "health initial fraction count differs"
    );
    for &at in &fractions {
        ensure!(
            (0.0..=1.0).contains(&source.f32(at)?),
            "invalid initial health fraction"
        );
    }
    pair(output, to, target_definition, 0x80804BA1, 0x80804BA2, ni)?;
    put(
        &mut output.0,
        target_definition + 16,
        &(regions as u64).to_le_bytes(),
    )?;
    put(&mut output.0, ni + 0x324, &capacity.to_le_bytes())?;
    append_array(
        &mut output.0,
        to + 0xD0,
        0x80804C5C,
        &array_bytes(source, from + 0xF0, 4)?,
        4,
    )?;
    let mut rows = vec![allocation::Row::named(
        0xA53CEAA0,
        u32::MAX,
        0,
        vec![allocation::Row::named(
            0x30EA7E62,
            0x80804C5C,
            regions,
            Vec::new(),
        )],
    )];
    let storage = source.u64(si + 0x8D8)? != 0;
    ensure!(
        source.u32(sd + 0x718)? == if storage { 0x80804000 } else { u32::MAX },
        "health hit-storage class differs"
    );
    put(&mut output.0, ni + 0x6E8, &0u64.to_le_bytes())?;
    put(
        &mut output.0,
        nd + 0x5B0,
        &(if storage { 0x80804B8Eu32 } else { u32::MAX }).to_le_bytes(),
    )?;
    if storage {
        let at = source.pointer(si + 0x8D8)?;
        let mut empty = [0u8; 176];
        for index in [0, 16, 40, 56, 80, 128] {
            empty[index..index + 8].fill(255);
        }
        for index in [32, 72, 96] {
            empty[index..index + 4].fill(255);
        }
        empty[124..128].copy_from_slice(&1f32.to_le_bytes());
        ensure!(
            source.bytes::<176>(at)? == empty,
            "health hit storage contains active state"
        );
        let target = typed(output, 0x80804B8E, 176)?;
        put(&mut output.0, target, &empty)?;
        relative(output, ni + 0x6E8, target)?;
        rows.push(allocation::Row::named(
            0x4C8BDAB4,
            0x80804B8E,
            1,
            Vec::new(),
        ));
    }
    Ok(State {
        objects: vec![
            object(source, output, from, 0x80804013, to, 0x80804BA1)?,
            object(
                source,
                output,
                definition,
                0x80804014,
                target_definition,
                0x80804BA2,
            )?,
        ],
        allocation: rows,
        unbound: vec![
            Object {
                owner: source.u32(si)?,
                class: 0x80802125,
                offset: (from + 0x100) as u64,
            },
            Object {
                owner: source.u32(si)?,
                class: 0x80802126,
                offset: (definition + 16) as u64,
            },
        ],
    })
}
