//! Entity-visible damage providers, checked against both dispatch contracts.
use super::{Omitted, Payload, Relocation};
use crate::d2_mot::entity::links::{Interface, Object};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
pub(super) const PROVIDERS: [(usize, usize, u32, u32); 9] = [
    (0x20, 0x20, 0x8080295E, 0x8080377A),
    (0x40, 0x38, 0x808098CD, 0x80809ADE),
    (0x60, 0x50, 0x808095C4, 0x808097B9),
    (0x80, 0x68, 0x808098C9, 0x80809ADA),
    (0xA0, 0x80, 0x80802ADF, 0x808038D0),
    (0xC0, 0x98, 0x808091A8, 0x80809494),
    (0xE0, 0xB0, 0x808098D2, 0x80809AE1),
    (0x108, 0xD0, 0x808098D2, 0x80809AE1),
    (0x130, 0xF0, 0x808098D2, 0x80809AE1),
];
const METHODS: &[(&[u32], &[u32])] = &[
    (&[0, 1], &[0, 1]),
    (&[4], &[4]),
    (&[7], &[7]),
    (&[8], &[8]),
    (&[12, 13, 9, 10, 11], &[9, 10, 11]),
    (&[6], &[6]),
    (&[14], &[12]),
    (&[15], &[13]),
    (&[16], &[14]),
];

pub(super) fn emit(
    source: &Payload,
    owner: &Payload,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<(Vec<Relocation>, Vec<Omitted>)> {
    let sd = source.pointer(24)?;
    let native_d = owner.pointer(24)?;
    let settings = sd + 0x1F8;
    let native_settings = native_d + 0x1F8;
    let source_tag = source.u32(source.pointer(16)?)?;
    let owner_tag = owner.u32(owner.pointer(16)?)?;
    let mut objects = Vec::new();
    let mut omitted_methods = Vec::new();
    for (index, (sm, sn, sc, nc)) in PROVIDERS.into_iter().enumerate() {
        let from = settings + sm;
        let to = native_settings + sn;
        let source_object = Object {
            owner: source_tag,
            class: sc,
            offset: from as u64,
        };
        let native_object = Object {
            owner: owner_tag,
            class: nc,
            offset: to as u64,
        };
        let s = Interface::read(
            source,
            source_object,
            metadata
                .get(&source.u32(from + 8)?)
                .context("source damage provider metadata missing")?,
            true,
        )?;
        let n = Interface::read(
            owner,
            native_object,
            metadata
                .get(&owner.u32(to + 8)?)
                .context("native damage provider metadata missing")?,
            false,
        )?;
        ensure!(
            s.instance_class == if sc == 0x808098D2 { sc + 1 } else { sc - 1 }
                && n.instance_class == if nc == 0x80809AE1 { nc + 1 } else { nc - 1 },
            "damage provider instance class differs"
        );
        ensure!(
            s.definition_offset == sd
                && n.definition_offset == native_d
                && source.bytes::<8>(from + 16)? == owner.bytes::<8>(to + 16)?,
            "damage provider parent or state differs"
        );
        for (interface, implementation, indices) in [
            (&s, 0x80802D47, METHODS[index].0),
            (&n, 0x80803778, METHODS[index].1),
        ] {
            ensure!(
                interface.methods.len() == indices.len()
                    && interface
                        .methods
                        .iter()
                        .zip(indices)
                        .all(
                            |(method, index)| method.implementation_class == implementation
                                && method.index == *index
                                && method.arguments == [0, 0]
                        ),
                "damage provider dispatch differs"
            );
        }
        if index == 4 {
            omitted_methods.push(Omitted {
                object: source_object,
                methods: vec![12, 13],
            });
        }
        objects.push(Relocation {
            source: source_object,
            target: native_object,
        });
    }
    Ok((objects, omitted_methods))
}
