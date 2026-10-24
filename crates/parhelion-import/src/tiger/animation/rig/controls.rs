//! The rig control resource's seven row arrays in either build, checked for consistent control
//! groups, parents, bone and slot lookups and default transforms.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

/// Per row array: its modern and Shadowkeep descriptor offsets, row stride, and modern and
/// Shadowkeep row classes.
pub(crate) const FIELDS: [(usize, usize, usize, u32, u32); 7] = [
    (0xA8, 0x90, 8, 0x80808C5F, 0x8080907F),
    (0xB8, 0xA0, 52, 0x80808C56, 0x80809076),
    (0xC8, 0xB0, 32, 0x80809F4F, 0x80809F75),
    (0xD8, 0xC0, 2, 0x80800006, 0x80800006),
    (0xF0, 0xD8, 2, 0x80800006, 0x80800006),
    (0x100, 0xE8, 2, 0x80800006, 0x80800006),
    (0x128, 0x110, 8, 0x80808BDA, 0x80808FFE),
];

pub(crate) fn read(p: &Payload, modern: bool) -> Result<Vec<Vec<Vec<u8>>>> {
    let resource = p.pointer(24)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && resource >= 4
            && p.u32(resource - 4)? == if modern { 0x80808B5F } else { 0x80808F8F },
        "rig control resource layout differs"
    );
    FIELDS
        .iter()
        .map(|&(s, n, stride, sc, nc)| {
            p.array(
                resource + if modern { s } else { n },
                stride,
                Some(if modern { sc } else { nc }),
            )?
            .into_iter()
            .map(|at| Ok(p.0[at..at + stride].to_vec()))
            .collect()
        })
        .collect()
}

pub(crate) fn short(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

pub(crate) fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

pub(crate) fn check(rows: &[Vec<Vec<u8>>]) -> Result<()> {
    let controls = rows[1].len();
    let bones = rows[4].len();
    ensure!(
        controls > 0
            && controls < 0x800
            && bones > 0
            && bones < 0x800
            && rows[2].len() == controls
            && rows[5].len() == controls,
        "rig control counts disagree"
    );
    ensure!(
        rows[0].iter().map(|r| u64::from(word(r, 4))).sum::<u64>() == controls as u64,
        "rig control groups do not cover their slots"
    );
    for (slot, row) in rows[1].iter().enumerate() {
        let bone = usize::from(short(row, 44));
        ensure!(
            bone < bones
                && short(&rows[4][bone], 0) as usize == slot
                && short(&rows[5][slot], 0) as usize == bone,
            "rig control bone lookup is not bidirectional"
        );
        for at in [4, 10, 24, 30] {
            let parent = short(row, at);
            ensure!(
                parent == u16::MAX || usize::from(parent) < bones,
                "rig control parent exceeds skeleton"
            );
        }
    }
    for (bone, row) in rows[4].iter().enumerate() {
        let slot = short(row, 0);
        ensure!(
            slot == u16::MAX
                || (usize::from(slot) < controls
                    && short(&rows[5][usize::from(slot)], 0) as usize == bone),
            "rig control forward lookup has an invalid slot"
        );
    }
    for row in &rows[2] {
        let f = |at| f32::from_le_bytes(row[at..at + 4].try_into().unwrap());
        let norm = (0..4).map(|i| f(i * 4).powi(2)).sum::<f32>();
        ensure!(
            (0..8).all(|i| f(i * 4).is_finite())
                && (norm - 1.0).abs() < 1e-3
                && (f(28) - 1.0).abs() < 1e-3,
            "rig control default transform is invalid"
        );
    }
    for row in &rows[6] {
        ensure!(
            (word(row, 4) as usize) < bones,
            "named rig control exceeds skeleton"
        );
    }
    Ok(())
}

/// Validate the native resource and the allocation-local state carried with it.
pub fn validate(bytes: &[u8]) -> Result<()> {
    let p = Payload(bytes.to_vec());
    check(&read(&p, false)?)?;
    let instance = p.pointer(16)?;
    let end = instance
        .checked_add(usize::try_from(p.u64(0x48)?)?)
        .context("rig control instance extent")?;
    ensure!(
        instance >= 4 && p.u32(instance - 4)? == 0x80808F96 && end <= p.pointer(24)?,
        "rig control instance layout differs"
    );
    let state = p.array_range(instance + 0x30, 48, Some(0x80808F92))?;
    ensure!(
        state.is_empty() || (state.start >= instance && state.end <= end),
        "rig control state escapes its instance"
    );
    Ok(())
}
