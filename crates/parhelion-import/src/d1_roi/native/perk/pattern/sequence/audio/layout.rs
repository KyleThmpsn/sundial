//! Complete connections, locator rows and lifecycle on the retained sequence envelope.
use super::*;

pub(super) fn status(p: &Payload, native: bool) -> Result<()> {
    let live = p.pointer(16)?;
    let rows = p.array(0x30, 16, Some(if native { 0x808091A4 } else { 0x80800930 }))?;
    let expected = if native {
        vec![
            (0x50, 0x80803B32, 0x00020000),
            (0x1D0, 0x80803A60, 0x0003FFFF),
            (0, 0x80804209, 0x0003FFFF),
        ]
    } else {
        vec![(0x50, 0x8080255D, 0x00030001), (0, 0x80802A75, 0x0004FFFF)]
    };
    ensure!(
        rows.len() == expected.len(),
        "Status audio lifecycle count differs"
    );
    for (row, (offset, class, methods)) in rows.into_iter().zip(expected) {
        ensure!(
            p.pointer(row)? == live + offset
                && p.u32(row + 8)? == class
                && p.u32(row + 12)? == methods,
            "Status audio lifecycle target or methods differ"
        );
    }
    Ok(())
}

pub(super) fn graph(p: &Payload, native: bool) -> Result<Vec<u32>> {
    let shift = if native { 0 } else { 16 };
    let owners = p.array(
        16 + shift,
        12,
        Some(if native { 0x80809C04 } else { 0x80800715 }),
    )?;
    let exports = p.array(
        0x58 + shift,
        40,
        Some(if native { 0x80809C22 } else { 0x80800370 }),
    )?;
    let refs = p.array(
        0x68 + shift,
        24,
        Some(if native { 0x80809C20 } else { 0x80800790 }),
    )?;
    let masks = p.array(0x78 + shift, 2, Some(0x80800006))?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && owners.len() == 3
            && exports.len() == if native { 39 } else { 36 }
            && refs.len() == exports.len()
            && p.bytes::<16>(0x30 + shift)? == [0; 16]
            && masks.len() == 2
            && masks.iter().all(|&at| p.u16(at).ok() == Some(0)),
        "Status sound graph has additional owners, providers, inputs or conditions"
    );
    let mut tags = Vec::new();
    for at in owners {
        ensure!(
            p.u64(at + 4)? == 0,
            "Status sound owner has additional metadata"
        );
        tags.push(p.u32(at)?);
    }
    ensure!(
        tags.iter().copied().collect::<BTreeSet<_>>().len() == 3,
        "Status sound repeats an owner"
    );
    Ok(tags)
}

pub(super) fn connections(
    p: &Payload,
    status: (u32, usize),
    sequence: (u32, usize),
    native: bool,
) -> Result<()> {
    let rows = p.array(
        if native { 32 } else { 48 },
        if native { 72 } else { 88 },
        Some(if native { 0x80809BC9 } else { 0x8080054E }),
    )?;
    ensure!(
        rows.len() == if native { 2 } else { 1 },
        "Status sound connection count differs"
    );
    for (index, row) in rows.into_iter().enumerate() {
        let bound = index == 0;
        let fields = if native {
            vec![
                (0, 0x811C9DC5),
                (8, status.0 as u64 | (0x808092F4u64 << 32)),
                (16, (status.1 + if bound { 0x60 } else { 0x3B0 }) as u64),
                (24, 0),
                (32, 0x811C9DC5),
                (
                    40,
                    if bound {
                        sequence.0 as u64 | (0x808092F8u64 << 32)
                    } else {
                        u64::MAX
                    },
                ),
                (48, if bound { (sequence.1 + 0x90) as u64 } else { 0 }),
                (56, if bound { 1 } else { u16::MAX as u64 }),
                (64, 0),
            ]
        } else {
            vec![
                (0, 0x811C9DC5),
                (8, status.0 as u64),
                (16, (status.1 + 0xB8) as u64),
                (24, 0x80800456),
                (32, 0),
                (40, 0x811C9DC5),
                (48, sequence.0 as u64),
                (56, (sequence.1 + 0xA8) as u64),
                (64, 0x80800988),
                (72, 1),
                (80, 0),
            ]
        };
        for (offset, value) in fields {
            ensure!(
                p.u64(row + offset)? == value,
                "Status sound connection endpoint differs"
            );
        }
    }
    Ok(())
}

pub(super) fn native_pair(
    p: &Payload,
    owner: u32,
    live: usize,
    definition: usize,
    ic: u32,
    dc: u32,
) -> Result<()> {
    ensure!(
        p.u32(live)? == owner
            && p.u32(live + 4)? == dc
            && p.u64(live + 8)? == definition as u64
            && p.u32(definition)? == owner
            && p.u32(definition + 4)? == ic
            && p.u64(definition + 8)? == live as u64,
        "Native sound sequence embedded pair differs"
    );
    Ok(())
}

/// The owner's instance and definition offsets, after checking its extent and allocation.
fn extent(p: &Payload, native: bool) -> Result<(usize, usize)> {
    let (i, d, prefix) = (p.pointer(16)?, p.pointer(24)?, p.pointer(8)?);
    ensure!(
        prefix >= 4
            && prefix < i
            && i < d
            && d < p.0.len()
            && p.u64(0)? == p.0.len() as u64
            && p.u64(0x48)? == (d - i) as u64
            && p.u64(0x50)? == (i - prefix) as u64
            && p.bytes::<16>(0x20)? == [0; 16],
        "Sound sequence owner extent or dependencies differ"
    );
    ensure!(
        i - prefix == 40
            && d - i == if native { 1200 } else { 1024 }
            && p.u32(prefix - 4)? == if native { 0x808084E2 } else { 0x8080043F }
            && p.bytes::<20>(prefix)? == [0; 20]
            && p.u32(prefix + 20)? == if native { 0x10000 } else { 0 }
            && p.bytes::<12>(prefix + 24)? == [0; 12],
        "Sound sequence constructor defaults or runtime allocation differ"
    );
    Ok((i, d))
}

/// The definition's fixed policy fields, empty inputs and outputs, callback and locator.
fn policy(p: &Payload, d: usize, native: bool) -> Result<()> {
    let fields = if native {
        vec![
            (0x10, 0),
            (0x18, u64::MAX),
            (0x20, 0),
            (0x28, u64::MAX),
            (0x30, 0),
            (0x38, 0),
            (0x40, 0),
            (0x140, 0x80809723),
            (0x198, 0x100000001),
            (0x1E0, 0x100000000),
            (0x1E8, 0x100000001),
            (0x260, 0x8000),
            (0x268, 0xFFFFFFFF00000000),
            (0x2D0, 0),
            (0x2D8, 0),
            (0x2E0, 0x252008),
        ]
    } else {
        vec![
            (0x18, 0),
            (0x20, u32::MAX as u64),
            (0x28, 0),
            (0x30, u32::MAX as u64),
            (0x38, u32::MAX as u64),
            (0x40, 0),
            (0x48, u32::MAX as u64),
            (0x50, 0),
            (0x58, 0),
            (0x158, 0x808006C0),
            (0x258, 0x8000),
            (0x2C0, 0),
            (0x2C8, 0),
            (0x2D0, 0x100252000),
        ]
    };
    for (offset, value) in fields {
        ensure!(
            p.u64(d + offset)? == value,
            "Sound sequence root policy differs at {offset:X}"
        );
    }
    let empty = if native {
        [
            (0x148, 0x158),
            (0x178, 0x198),
            (0x200, 0x250),
            (0x2B0, 0x2D0),
        ]
    } else {
        [
            (0x170, 0x180),
            (0x190, 0x1B0),
            (0x1F8, 0x248),
            (0x2A8, 0x2C0),
        ]
    };
    for (start, end) in empty {
        ensure!(
            (start..end)
                .step_by(8)
                .all(|at| p.u64(d + at).ok() == Some(0)),
            "Sound sequence has additional inputs, outputs or callback state"
        );
    }
    let descriptor = d + if native { 0x250 } else { 0x248 };
    ensure!(
        p.pointer(descriptor)? == d + if native { 0x1F0 } else { 0x1E0 }
            && p.u64(descriptor + 8)? == if native { 0x80C70B85 } else { 0x80AAD53C },
        "Sound sequence callback parameter differs"
    );
    let rows = if native {
        vec![
            (0x1B0, 0x80808617, vec![0, 1, 0, 1, 0x3F, 0]),
            (0x1C0, 0x80808615, vec![0, 0, 1, 0, 0x3F, 0, 0, 0]),
            (0x1D0, 0x80808614, vec![0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0]),
        ]
    } else {
        vec![
            (0x1B0, 0x80800429, vec![0, 1, 0, 1, 0x1F]),
            (0x1C0, 0x80800819, vec![0, 0, 1, 0x1F]),
            (0x1D0, 0x80800696, vec![0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0]),
        ]
    };
    for (offset, class, expected) in rows {
        let rows = p.array(d + offset, expected.len(), Some(class))?;
        ensure!(
            rows.len() == 1
                && p.0.get(rows[0]..rows[0] + expected.len()) == Some(expected.as_slice()),
            "Sound sequence owning-entity locator differs"
        );
    }
    Ok(())
}

/// The embedded instance and definition pairs and the lifecycle registration.
fn pairs(p: &Payload, owner: u32, (i, d): (usize, usize), native: bool) -> Result<()> {
    if native {
        ensure!(
            p.u32(i - 4)? == 0x808084D7 && p.u32(d - 4)? == 0x808084E9,
            "Native sound sequence root class differs"
        );
        for (live, definition, ic, dc) in [
            (i, d, 0x808084D7, 0x808084E9),
            (i + 0x150, d + 0x1A0, 0x8080860C, 0x8080860D),
            (i + 0x1D0, d + 0x1F0, 0x808072C0, 0x808072C1),
            (i + 0x200, d + 0x270, 0x808072CB, 0x808072C7),
        ] {
            native_pair(p, owner, live, definition, ic, dc)?;
        }
        use crate::tiger::entity::lifecycle::{self, Method};
        ensure!(
            lifecycle::read(p)?
                == [Method {
                    target: i,
                    implementation: 0x808084D7,
                    constructor: 0,
                    cleanup: 2
                }],
            "Native sound sequence lifecycle differs"
        );
    } else {
        for (live, definition, ic, dc) in [
            (i, d, 0x8080079A, 0x80800610),
            (i + 0x1C8, d + 0x1E0, 0x80801A67, 0x80801BEF),
            (i + 0x1F8, d + 0x260, 0x80801C3B, 0x80801AF6),
        ] {
            status::pair(p, owner, live, definition, ic, dc)?;
        }
        ensure!(
            lifecycle::Lifecycle::read(p)?.methods
                == [lifecycle::Method {
                    target: i,
                    class: 0x8080079A,
                    implementation: 0x8080079A,
                    constructor: 1,
                    cleanup: 3
                }],
            "Source sound sequence lifecycle differs"
        );
    }
    Ok(())
}

pub(super) fn root(p: &Payload, owner: u32, native: bool) -> Result<()> {
    let (i, d) = extent(p, native)?;
    policy(p, d, native)?;
    pairs(p, owner, (i, d), native)
}
