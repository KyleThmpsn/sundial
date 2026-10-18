//! Matching identity, count, signed priority and bounded refresh policy.
use super::*;

fn inactive_limit(source: &Payload, i: usize, d: usize, parent: usize) -> Result<Vec<Object>> {
    pair(source, i, d, 0x8080862F, 0x80808630)?;
    ensure!(
        source.pointer(i + 16)? == parent
            && source.u64(i + 24)? == 0
            && source.array(i + 32, 48, Some(0x80809590))?.is_empty()
            && source.array(d + 64, 40, Some(0x80809591))?.is_empty()
            && source.u64(d + 48)? == 1
            && source.u64(d + 56)? == 1
            && source.u64(d + 80)? == 0
            && source.u64(d + 88)? == 0x811C9DC5,
        "modern Duration limit has external inputs or a reference"
    );
    let code = array_bytes(source, d + 16, 1)?;
    let constants = source.array(d + 32, 16, Some(0x80800090))?;
    ensure!(
        code == [0x42, 0, 0x4C, 0]
            && constants.len() == 1
            && (0..4)
                .all(|lane| source.u32(constants[0] + lane * 4).ok() == Some((-1.0f32).to_bits())),
        "modern Duration limit is not the inactive literal"
    );
    let owner = source.u32(i)?;
    Ok(vec![
        Object {
            owner,
            class: 0x8080862F,
            offset: i as u64,
        },
        Object {
            owner,
            class: 0x80808630,
            offset: d as u64,
        },
    ])
}

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    envelope: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    from: (usize, usize),
    to: (usize, usize),
) -> Result<Embedded> {
    let (si, sd) = from;
    let (ni, nd) = to;
    pair(source, si, sd, 0x80802DBC, 0x80802DBD)?;
    pair(output, ni, nd, 0x80803B32, 0x80803B33)?;
    ensure!(
        source.u64(si + 24)? == 0
            && source.u64(si + 32)? == 0
            && source.u64(si + 40)? == 0
            && source.u64(si + 48)? == u64::MAX
            && source.u64(si + 56)? == 0,
        "Duration runtime state is initialized"
    );
    let flags = source.bytes::<4>(sd + 20)?;
    ensure!(
        flags[0] & !5 == 0
            && flags[1] <= 1
            && flags[2] <= 1
            && flags[3] == 0
            && source.u32(sd + 24)? <= i32::MAX as u32,
        "unsupported modern Duration matching or refresh policy"
    );
    // The modern matching helper always applies signed priority filtering.
    // Native 08 selects that filtered path without permitting equal priority.
    let native_flags = 8 | (flags[0] & 1) | (flags[1] << 2) | (flags[2] << 4);
    put(
        &mut output.0,
        nd + 0x98,
        &u32::from(native_flags).to_le_bytes(),
    )?;
    put(&mut output.0, nd + 0x9C, &source.bytes::<4>(sd + 0x10)?)?;
    put(&mut output.0, nd + 0xA0, &source.bytes::<8>(sd + 0x18)?)?;
    put(
        &mut output.0,
        nd + 0xA8,
        &u64::from((flags[0] & 4) != 0).to_le_bytes(),
    )?;
    let mut unbound = inactive_limit(source, si + 0x40, sd + 0x20, si)?;
    unbound.extend(inactive_limit(source, si + 0x140, sd + 0x1D8, si)?);
    // Native lifecycle and competition interfaces keep their original method
    // metadata. Modern-only callbacks have no graph endpoint mapping.
    let owner = source.u32(si)?;
    for (offset, class, methods) in [
        (0x238, 0x808098D2, vec![22]),
        (0x260, 0x8080955E, vec![20]),
        (0x288, 0x8080955E, vec![21]),
        (0x2B0, 0x808098CD, vec![1]),
        (0x2D0, 0x808091A8, vec![4]),
        (0x2F0, 0x808091AB, vec![5]),
        (0x310, 0x80802DC5, (6..=19).collect()),
        (0x330, 0x808095C6, vec![3]),
    ] {
        provider(
            source,
            sd + offset,
            sd,
            class,
            (0x80802DBC, &methods),
            true,
            metadata,
        )?;
        unbound.push(Object {
            owner,
            class,
            offset: (sd + offset) as u64,
        });
    }
    for (offset, class, methods) in [
        (0x10, 0x8080974B, vec![13]),
        (0x30, 0x8080974B, vec![14]),
        (0x50, 0x80809ADE, vec![1]),
        (0x68, 0x80809494, vec![3]),
        (0x80, 0x80803B38, (4..=12).collect()),
    ] {
        provider(
            output,
            nd + offset,
            nd,
            class,
            (0x80803B32, &methods),
            false,
            metadata,
        )?;
    }
    let mut timer = timer::emit(
        source,
        output,
        envelope,
        metadata,
        (si + 0x80, sd + 0x80),
        (ni + 0x30, nd + 0xB0),
    )?;
    if flags[2] == 0 {
        timer.stable_inputs.clear();
    }
    timer
        .objects
        .push(object(source, output, si, 0x80802DBC, ni, 0x80803B32)?);
    timer
        .objects
        .push(object(source, output, sd, 0x80802DBD, nd, 0x80803B33)?);
    timer.unbound.extend(unbound);
    timer.allocation = if timer.allocation.is_empty() {
        Vec::new()
    } else {
        vec![allocation::Row::named(
            0xD73E8B17,
            u32::MAX,
            0,
            timer.allocation,
        )]
    };
    Ok(timer)
}
