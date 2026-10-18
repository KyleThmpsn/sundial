//! Numeric event outputs are private paired rows with native provider dispatch.
use super::{Bindings, controls::*};
use crate::d2_mot::{
    entity::links::{Interface, Object},
    native::effects::controller::Relocation,
    payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

fn provider(p: &Payload, row: usize, modern: bool, bindings: &Bindings) -> Result<()> {
    let class = if modern { 0x808098D2 } else { 0x80809AE1 };
    let owner = p.u32(p.pointer(16)?)?;
    let metadata = bindings
        .metadata
        .get(&p.u32(row + 24)?)
        .context("sequence output provider metadata absent")?;
    let interface = Interface::read(
        p,
        Object {
            owner,
            class,
            offset: (row + 16) as u64,
        },
        metadata,
        modern,
    )?;
    ensure!(
        interface.definition_offset == row
            && interface.instance_class == class + 1
            && interface.methods.len() == 1
            && interface.methods[0].implementation_class
                == if modern { 0x8080816E } else { 0x808084DE }
            && interface.methods[0].index == 0
            && interface.methods[0].arguments == [0, 0],
        "sequence output provider dispatch differs"
    );
    Ok(())
}

pub(super) fn emit(
    source: &Payload,
    template: &Payload,
    output: &mut Payload,
    bindings: &Bindings,
    objects: &mut Vec<Relocation>,
) -> Result<Option<Allocation>> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let instances = source.array(si + 0xF0, 64, Some(0x8080816E))?;
    let definitions = source.array(sd + 0x1F8, 56, Some(0x8080816F))?;
    ensure!(
        instances.len() == definitions.len() && definitions.len() <= 256,
        "sequence output count differs or exceeds capacity"
    );
    let count = definitions.len();
    let ir = array(output, ni + 0xC0, 0x808084DE, &vec![0; count * 64], 64)?;
    let dr = array(output, nd + 0x178, 0x808084DF, &vec![0; count * 48], 48)?;
    if count == 0 {
        return Ok(None);
    }
    let mut witness = None;
    for p in std::iter::once(template).chain(bindings.event_templates.iter()) {
        let d = p.pointer(24)?;
        if let Some(&row) = p.array(d + 0x178, 48, Some(0x808084DF))?.first() {
            witness = Some((p, row));
            break;
        }
    }
    let (envelope, tr) = witness.context("native sequence output envelope absent")?;
    provider(envelope, tr, false, bindings)?;
    let ti = usize::try_from(envelope.u64(tr + 8)?)?;
    ensure!(
        envelope.u32(ti + 4)? == 0x808084DF
            && envelope.u32(tr + 4)? == 0x808084DE
            && envelope.u64(ti + 8)? == tr as u64
            && envelope.pointer(ti + 16)? == envelope.pointer(16)?
            && envelope.bytes::<40>(ti + 24)? == [0; 40]
            && envelope.u64(tr + 32)? == 0,
        "native sequence output envelope has initialized state"
    );
    let source_owner = source.u32(si)?;
    let native_owner = output.u32(ni)?;
    let mut names = BTreeSet::new();
    for (index, (&i, &d)) in instances.iter().zip(&definitions).enumerate() {
        provider(source, d, true, bindings)?;
        ensure!(
            source.u32(i)? == source_owner
                && source.u32(d)? == source_owner
                && source.u32(i + 4)? == 0x8080816F
                && source.u32(d + 4)? == 0x8080816E
                && source.u64(i + 8)? == d as u64
                && source.u64(d + 8)? == i as u64
                && source.pointer(i + 16)? == si
                && source.bytes::<40>(i + 24)? == [0; 40]
                && source.bytes::<16>(d + 32)? == [0; 16]
                && source.u32(d + 52)? == 1
                && names.insert(source.u32(d + 48)?),
            "sequence output pairing, initial state or publication differs"
        );
        let target_i = ir + index * 64;
        let target_d = dr + index * 48;
        put(output, target_d, &envelope.bytes::<48>(tr)?)?;
        pair(
            output,
            native_owner,
            target_i,
            target_d,
            0x808084DE,
            0x808084DF,
        )?;
        relative(output, target_i + 16, ni)?;
        relative(output, target_d + 16, target_d)?;
        put(output, target_d + 40, &source.bytes::<8>(d + 48)?)?;
        for (from, sc, to, nc) in [
            (i, 0x8080816E, target_i, 0x808084DE),
            (d, 0x8080816F, target_d, 0x808084DF),
            (d + 16, 0x808098D2, target_d + 16, 0x80809AE1),
        ] {
            objects.push(Relocation {
                source: Object {
                    owner: source_owner,
                    class: sc,
                    offset: from as u64,
                },
                target: Object {
                    owner: native_owner,
                    class: nc,
                    offset: to as u64,
                },
            });
        }
    }
    Ok(Some(Allocation::array(771384421, 0x808084DE, count)))
}
