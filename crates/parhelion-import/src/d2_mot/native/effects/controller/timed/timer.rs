//! Timer programs, event bindings and allocation beneath Duration.
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
    pair(source, si, sd, 0x80802EA3, 0x80802EA4)?;
    pair(output, ni, nd, 0x80803C50, 0x80803C51)?;
    ensure!(
        source.u64(si + 24)? == 0
            && output.u64(ni + 24)? == 0
            && source.bytes::<32>(si + 0xA0)? == output.bytes::<32>(ni + 0xA0)?
            && source.u64(sd + 0x90)? <= 1,
        "timer has initialized or unsupported state"
    );
    // Modern 352860 reads +90 for its nonpositive-duration route. Native
    // EDF2E0 reads +70 for the corresponding branch. This is source tuning.
    put(&mut output.0, nd + 0x70, &source.bytes::<8>(sd + 0x90)?)?;
    let mut objects = input(
        source,
        output,
        (si + 0x50, sd + 0xF8, si),
        (ni + 0x50, nd + 0xD8, ni),
    )?;
    for (from, to, sc, nc, method) in [
        (0x10, 0x10, 0x808098CD, 0x80809ADE, 1),
        (0x30, 0x28, 0x808095C6, 0x808097BB, 2),
        (0x50, 0x40, 0x808091A8, 0x80809494, 3),
        (0x70, 0x58, 0x808091AB, 0x80809497, 4),
        (0x130, 0x108, 0x808098D2, 0x80809AE1, 10),
    ] {
        provider(
            source,
            sd + from,
            sd,
            sc,
            (0x80802EA3, &[method]),
            true,
            metadata,
        )?;
        provider(
            output,
            nd + to,
            nd,
            nc,
            (0x80803C50, &[method]),
            false,
            metadata,
        )?;
        objects.push(object(source, output, sd + from, sc, nd + to, nc)?);
    }
    ensure!(
        source.u64(sd + 0x150)? == output.u64(nd + 0x120)?,
        "timer output identity differs"
    );
    let converted = scalar::emit_nested(
        source,
        output,
        envelope,
        sd + 0x98,
        nd + 0x78,
        scalar::Contract {
            source_parent: si,
            native_parent: ni,
            builtins: 1,
        },
    )?;
    let stable_inputs = converted
        .objects
        .iter()
        .filter_map(|mapping| (mapping.source.class == 0x80809591).then_some(mapping.source))
        .collect();
    // Arithmetic has already been decoded by scalar::emit_nested. Inspect only
    // instruction boundaries so an operand byte cannot masquerade as an input.
    let mut at = 0;
    while at < converted.source_code.len() {
        let op = converted.source_code[at];
        if op == 0x4A {
            ensure!(
                converted.source_code[at + 1] != 0,
                "timer refresh depends on an unqualified built-in input"
            );
        }
        at += if matches!(op, 0x29 | 0x42..=0x4A | 0x4C) {
            2
        } else {
            1
        };
    }
    let allocation = if converted.extra_inputs == 0 {
        Vec::new()
    } else {
        vec![allocation::Row::indirect(
            0xCD03C6B1,
            allocation::Row::inputs(converted.extra_inputs),
        )]
    };
    objects.extend(converted.objects);
    objects.push(object(source, output, si, 0x80802EA3, ni, 0x80803C50)?);
    objects.push(object(source, output, sd, 0x80802EA4, nd, 0x80803C51)?);
    Ok(Embedded {
        objects,
        unbound: Vec::new(),
        allocation,
        stable_inputs,
    })
}
