//! Admission of a source round-count curve through its actual magazine destination.
use super::*;

fn column(p: &Payload, operand: usize) -> Result<Option<u32>> {
    (p.bytes::<12>(operand)? == [0; 12]
        && p.u32(operand + 12)? == 0x811C9DC5
        && p.u32(operand + 20)? == 0x7F7FFFFF)
        .then(|| p.u32(operand + 16))
        .transpose()
}

fn unit(p: &Payload, group: usize, operand: usize) -> Result<bool> {
    if p.bytes::<12>(operand)? != [0; 12] || p.u32(operand + 12)? != 0x811C9DC5 {
        return Ok(false);
    }
    if p.u32(operand + 16)? == 0 {
        return Ok(p.f32(operand + 20)? == 1.);
    }
    let Some(output) = column(p, operand)?.and_then(|v| v.checked_sub(0x3C00)) else {
        return Ok(false);
    };
    let curves = p.array(group, 56, Some(0x80803F8E))?;
    let curve = *curves
        .get(output as usize / 32)
        .context("Source magazine factor curve is absent")?;
    let tiers = p.array(curve + 16, 16, Some(0x8080BC4D))?;
    if tiers.is_empty() {
        return Ok(false);
    }
    for tier in tiers {
        let values = p.array(tier, 4, Some(0x8080000F))?;
        if values
            .get(output as usize % 32)
            .map(|&at| p.f32(at))
            .transpose()?
            != Some(1.)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The first column must be the sole writer of Magazine input zero, copied or rounded as an
/// integer, or multiplied by a factor that is one throughout its source domain. A curve named
/// Magazine may instead feed a barrel's reservoir cost. Its shape alone does not establish rounds.
pub(super) fn conventional(p: &Payload, group: usize, curve: usize) -> Result<bool> {
    let rows = p
        .array(group + 16, 96, Some(0x80803F96))?
        .into_iter()
        .filter(|&row| p.u32(row).ok() == Some(1) && p.u32(row + 4).ok() == Some(0))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return Ok(false);
    };
    if p.u32(row + 8)? != u32::MAX || p.u32(row + 12)? != 0x811C9DC5 {
        return Ok(false);
    }
    let output = 0x3C00 + u32::try_from(curve)? * 32;
    let a = column(p, row + 20)? == Some(output);
    Ok(match p.u32(row + 16)? {
        4 | 7 => a,
        2 => {
            (a && unit(p, group, row + 44)?)
                || (column(p, row + 44)? == Some(output) && unit(p, group, row + 20)?)
        }
        _ => false,
    })
}
