//! Status reporting wraps Duration and retains its two scalar programs.
use super::*;

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
    pair(source, si, sd, 0x8080356C, 0x8080B7D1)?;
    pair(output, ni, nd, 0x80804211, 0x80804212)?;
    ensure!(
        source.u64(si + 24)? == 0
            && source.u64(sd + 0xD8)? == 0
            && source.u64(sd + 0x130)? == 0
            && source.u64(sd + 0x138)? <= 1
            && source.u64(sd + 0x140)? == 0,
        "status has initialized labels or a modern reporting extension"
    );
    let mut state = [0; 88];
    for at in [0, 24, 48, 72] {
        state[at..at + 8].fill(255);
    }
    for at in [16, 40, 64] {
        state[at..at + 4].fill(255);
    }
    ensure!(
        source.bytes::<88>(si + 0x200)? == state && source.u64(si + 0x258)? == 0,
        "status reporting state is initialized"
    );
    put(&mut output.0, nd + 0x80, &source.bytes::<16>(sd + 0xC8)?)?;
    put(&mut output.0, nd + 0x90, &[0; 8])?;
    put(&mut output.0, nd + 0xE8, &[0; 16])?;
    // A null modern reporting extension selects the empty grouping name.
    // The following flag controls the scalar adjustment of remaining time.
    put(&mut output.0, nd + 0xE8, &0x811C9DC5u32.to_le_bytes())?;
    put(&mut output.0, nd + 0xEC, &source.bytes::<4>(sd + 0x138)?)?;
    let mut nested = duration::emit(
        source,
        output,
        envelope,
        metadata,
        (si + 0x20, sd + 0x198),
        (ni + 0x20, nd + 0x148),
    )?;
    nested.allocation = if nested.allocation.is_empty() {
        Vec::new()
    } else {
        vec![allocation::Row::named(
            0xCEB29590,
            u32::MAX,
            0,
            nested.allocation,
        )]
    };
    for (from, to, name) in [(0xE0, 0x98, 0xADF81894), (0x148, 0xF8, 0x409BE998)] {
        let converted = scalar::emit_nested(
            source,
            output,
            envelope,
            sd + from,
            nd + to,
            scalar::Contract {
                source_parent: si,
                native_parent: ni,
                builtins: 1,
            },
        )?;
        nested.objects.extend(converted.objects);
        if converted.extra_inputs != 0 {
            nested.allocation.push(allocation::Row::indirect(
                name,
                allocation::Row::inputs(converted.extra_inputs),
            ));
        }
    }
    super::super::refresh_size(output)?;
    for (from, to, sc, nc, method) in [
        (0x10, 0x10, 0x8080955E, 0x8080974B, 2),
        (0x38, 0x30, 0x8080955E, 0x8080974B, 3),
        (0x88, 0x50, 0x808098CD, 0x80809ADE, 0),
        (0xA8, 0x68, 0x808095C6, 0x808097BB, 1),
    ] {
        provider(
            source,
            sd + from,
            sd,
            sc,
            (0x8080356C, &[method]),
            true,
            metadata,
        )?;
        provider(
            output,
            nd + to,
            nd,
            nc,
            (0x80804211, &[method]),
            false,
            metadata,
        )?;
        nested
            .objects
            .push(object(source, output, sd + from, sc, nd + to, nc)?);
    }
    provider(
        source,
        sd + 0x60,
        sd,
        0x8080955E,
        (0x8080356C, &[4]),
        true,
        metadata,
    )?;
    nested.unbound.push(Object {
        owner: source.u32(si)?,
        class: 0x8080955E,
        offset: (sd + 0x60) as u64,
    });
    nested.unbound.push(Object {
        owner: source.u32(si)?,
        class: 0x8080B7D0,
        offset: (si + 0x258) as u64,
    });
    nested.unbound.push(Object {
        owner: source.u32(si)?,
        class: 0x8080B7D2,
        offset: (sd + 0x130) as u64,
    });
    nested
        .objects
        .push(object(source, output, si, 0x8080356C, ni, 0x80804211)?);
    nested
        .objects
        .push(object(source, output, sd, 0x8080B7D1, nd, 0x80804212)?);
    Ok(nested)
}
