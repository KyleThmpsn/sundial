//! Health's native dispatch remains owned by native interface metadata.
use super::*;
use crate::d2_mot::entity::links::Interface;
use std::collections::BTreeMap;

type Mapping = (usize, usize, u32, u32, &'static [u32], &'static [u32]);

const INTERFACES: &[Mapping] = &[
    (0x260, 0x1F8, 0x80803FF8, 0x80804B86, &[5, 6], &[5, 6]),
    (
        0x280,
        0x210,
        0x8080409A,
        0x80804BE5,
        &[
            0, 1, 2, 3, 4, 13, 14, 15, 16, 17, 18, 19, 24, 25, 26, 27, 28, 29, 30, 20, 22, 21, 32,
            31, 33, 34, 35, 36, 37, 23, 38, 39, 40, 41, 42, 43, 44, 45, 50, 51, 52, 46, 47, 48, 49,
        ],
        &[
            0, 1, 2, 3, 4, 13, 14, 15, 16, 17, 18, 19, 24, 25, 26, 27, 20, 22, 21, 28, 29, 30, 31,
            32, 23, 33, 34, 35, 36, 37, 38, 39, 40,
        ],
    ),
    (0x2A0, 0x228, 0x808098CD, 0x80809ADE, &[10], &[10]),
    (0x2C0, 0x240, 0x808098C9, 0x80809ADA, &[53], &[41]),
    (0x2E0, 0x258, 0x808095C6, 0x808097BB, &[11], &[11]),
    (0x300, 0x270, 0x808095C4, 0x808097B9, &[12], &[12]),
    (
        0x320,
        0x288,
        0x8080979E,
        0x80809684,
        &[54, 55, 56],
        &[42, 43, 44],
    ),
    (0x340, 0x2A0, 0x8080979B, 0x80809682, &[57], &[45]),
    (0x360, 0x2D0, 0x808091A8, 0x80809494, &[59], &[48]),
    (0x380, 0x2E8, 0x8080955E, 0x8080974B, &[58], &[47]),
    (0x3A8, 0x308, 0x8080955E, 0x8080974B, &[60], &[49]),
];

pub(super) fn emit(
    source: &Payload,
    output: &Payload,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<Vec<Relocation>> {
    let sd = source.pointer(24)?;
    let nd = output.pointer(24)?;
    let mut objects = Vec::new();
    for &(from, to, sc, nc, sm, nm) in INTERFACES {
        let relocation = object(source, output, sd + from, sc, nd + to, nc)?;
        for (payload, object, root, implementation, methods, modern) in [
            (source, relocation.source, sd, 0x80804020, sm, true),
            (output, relocation.target, nd, 0x80804BEE, nm, false),
        ] {
            let at = usize::try_from(object.offset)?;
            let contract = Interface::read(
                payload,
                object,
                metadata
                    .get(&payload.u32(at + 8)?)
                    .context("health interface metadata absent")?,
                modern,
            )?;
            ensure!(
                contract.definition_offset == root
                    && contract.instance_class == object.class - 1
                    && contract.methods.len() == methods.len()
                    && contract
                        .methods
                        .iter()
                        .zip(methods)
                        .all(
                            |(method, index)| method.implementation_class == implementation
                                && method.index == *index
                                && method.arguments == [0, 0]
                        ),
                "health interface dispatch differs"
            );
            let tail = if modern { 16 } else { 8 };
            ensure!(
                payload
                    .0
                    .get(at + 16..at + 16 + tail)
                    .context("health interface state extent")?
                    .iter()
                    .all(|&v| v == 0),
                "health interface state is initialized"
            );
        }
        objects.push(relocation);
    }
    ensure!(
        source.u64(sd + 0x3A0)? == 0x808040D1
            && source.u64(sd + 0x3C8)? == 0x808040CD
            && output.u64(nd + 0x300)? == 0x80804C4F
            && output.u64(nd + 0x320)? == 0x80804C4B,
        "health named interface classes differ"
    );
    Ok(objects)
}
