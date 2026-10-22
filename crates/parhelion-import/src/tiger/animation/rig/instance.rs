//! Grow FK pose storage inside the copied instance without changing its range topology.
use super::*;

/// Keep the existing section records and move only the resource-side metadata. Consumers of
/// typed interior handles need the returned relocation when they link the private owner.
pub(crate) fn grow(
    original: &Payload,
    tag: u32,
    counts: &[usize],
) -> Result<(Payload, Option<(usize, usize)>)> {
    let instance = original.pointer(16)?;
    let definition = original.pointer(24)?;
    let sections = original.array(instance + 0x30, 64, Some(0x80808544))?;
    ensure!(
        sections.len() == counts.len(),
        "FK section topology differs"
    );
    let poses = sections
        .iter()
        .map(|row| rows(original, row + 32, 32, 0x80809F75))
        .collect::<Result<Vec<_>>>()?;
    if poses
        .iter()
        .zip(counts)
        .all(|(bytes, &count)| bytes.len() / 32 >= count)
    {
        return Ok((original.clone(), None));
    }
    ranges::Layout::read(original)?;
    ensure!(
        counts.iter().all(|&count| count > 0 && count < 0x800),
        "FK pose count exceeds supported storage"
    );
    let cut = definition
        .checked_sub(8)
        .context("FK resource class boundary")?;
    let schemas = original.array(definition + 0xD8, 24, Some(0x80808549))?;
    let procedural = original.array(instance + 0x40, 8, Some(0x80808A06))?;
    let mut out = original.0[..cut].to_vec();
    for ((&section, bytes), &count) in sections.iter().zip(&poses).zip(counts) {
        if count > bytes.len() / 32 {
            let mut grown = bytes.clone();
            grown.resize(count * 32, 0);
            append(&mut out, section + 32, 0x80809F75, &grown, 32)?;
        }
    }
    let ranges = rows(original, instance + 0x40, 8, 0x80808A06)?;
    ranges::trail_procedural(&mut out, instance, &procedural, &ranges)?;
    // Keep the resource's original alignment and leave only zero padding after the
    // procedural ranges. The native startup reader treats trailing words as tags.
    let delta = (out.len() - cut).div_ceil(32) * 32;
    out.resize(cut + delta, 0);
    out.extend_from_slice(&original.0[cut..]);
    ranges::rebase(&mut out, original, tag, cut, delta)?;
    for ((&schema, bytes), &count) in schemas.iter().zip(&poses).zip(counts) {
        if count > bytes.len() / 32 {
            put(
                &mut out,
                schema + delta + 16,
                &u32::try_from(count)?.to_le_bytes(),
            )?;
        }
    }
    let size = u64::try_from(out.len())?;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok((Payload(out), Some((cut, delta))))
}
