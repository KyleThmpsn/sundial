//! Source skinning records addressed by the packed position selector.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

pub(crate) fn selector(vertex: &[u8]) -> Result<i16> {
    Ok(i16::from_le_bytes(
        vertex
            .get(6..8)
            .context("missing skinning selector")?
            .try_into()?,
    ))
}

pub(crate) fn weighted(selector: i16) -> bool {
    selector.unsigned_abs() >= 0x800
}

fn records(selector: i16, vertex: usize) -> Result<std::ops::Range<usize>> {
    ensure!(
        weighted(selector) && selector != i16::MIN,
        "invalid weighted selector"
    );
    let count = if selector < 0 { 2 } else { 1 };
    let start = ((usize::from(selector.unsigned_abs()) - 0x800) * 8
        + ((vertex
            .checked_mul(count)
            .context("skinning index overflow")?)
            & 7))
        * 4;
    Ok(start..start + count * 4)
}

fn influences(selector: i16, vertex: usize, auxiliary: &[u8]) -> Result<Vec<(usize, u8)>> {
    if !weighted(selector) {
        ensure!(selector >= 0, "negative rigid bone selector");
        return Ok(vec![(selector as usize, 255)]);
    }
    let bytes = auxiliary
        .get(records(selector, vertex)?)
        .context("weighted vertex exceeds auxiliary buffer")?;
    ensure!(
        bytes
            .chunks_exact(4)
            .map(|r| u16::from(r[2]) + u16::from(r[3]))
            .sum::<u16>()
            == 255,
        "source skinning weights do not sum to 255"
    );
    Ok(bytes
        .chunks_exact(4)
        .flat_map(|r| [(usize::from(r[0]), r[2]), (usize::from(r[1]), r[3])])
        .collect())
}

pub(crate) fn used(positions: &[u8], auxiliary: &[u8]) -> Result<BTreeSet<usize>> {
    ensure!(
        positions.len().is_multiple_of(24),
        "invalid skinning position stride"
    );
    let mut bones = BTreeSet::new();
    for (index, vertex) in positions.chunks_exact(24).enumerate() {
        // The shader reads even zero-weight matrices, so they need valid indices too.
        bones.extend(
            influences(selector(vertex)?, index, auxiliary)?
                .into_iter()
                .map(|(bone, _)| bone),
        );
    }
    Ok(bones)
}

pub(crate) fn carrier_positions(positions: &[u8], auxiliary: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        positions.len().is_multiple_of(24),
        "invalid skinning position stride"
    );
    let mut result = positions.to_vec();
    for (index, vertex) in result.chunks_exact_mut(24).enumerate() {
        let (bone, _) = influences(selector(vertex)?, index, auxiliary)?
            .into_iter()
            .max_by_key(|(_, weight)| *weight)
            .context("empty skinning influences")?;
        vertex[6..8].copy_from_slice(&u16::try_from(bone)?.to_le_bytes());
    }
    // This stream satisfies the native carrier's rigid layout. Actual deformation
    // still uses the original selector and every weight in the adapted source VS.
    Ok(result)
}

pub(crate) fn remap(positions: &[u8], auxiliary: &[u8], bones: &[u16]) -> Result<Vec<u8>> {
    used(positions, auxiliary)?;
    let mut result = auxiliary.to_vec();
    for (index, vertex) in positions.chunks_exact(24).enumerate() {
        let selector = selector(vertex)?;
        if !weighted(selector) {
            continue;
        }
        for offset in records(selector, index)?.step_by(4) {
            for component in 0..2 {
                let source = usize::from(auxiliary[offset + component]);
                let target = *bones.get(source).context("weighted bone has no mapping")?;
                result[offset + component] =
                    u8::try_from(target).context("weighted bone exceeds byte palette")?;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(selectors: &[i16]) -> Vec<u8> {
        selectors
            .iter()
            .flat_map(|s| {
                let mut v = [0; 24];
                v[6..8].copy_from_slice(&s.to_le_bytes());
                v
            })
            .collect()
    }

    #[test]
    fn mixed_rigid_two_and_four_bone_records_preserve_weights() {
        let positions = positions(&[1, 0x800, -0x801]);
        let mut aux = vec![0; 64];
        aux[4..8].copy_from_slice(&[2, 3, 100, 155]);
        aux[48..56].copy_from_slice(&[0, 2, 10, 20, 3, 4, 30, 195]);
        assert_eq!(
            used(&positions, &aux).unwrap(),
            BTreeSet::from([0, 1, 2, 3, 4])
        );
        let mapped = remap(&positions, &aux, &[4, 3, 2, 1, 0]).unwrap();
        assert_eq!(&mapped[4..8], &[2, 1, 100, 155]);
        assert_eq!(&mapped[48..56], &[4, 2, 10, 20, 1, 0, 30, 195]);
        let carrier = carrier_positions(&positions, &aux).unwrap();
        assert_eq!(selector(&carrier[24..]).unwrap(), 3);
        assert_eq!(selector(&carrier[48..]).unwrap(), 4);
    }

    #[test]
    fn rejects_truncated_weights_invalid_selectors_and_unmapped_bones() {
        let p = positions(&[0x800]);
        assert!(used(&p, &[0, 1, 128]).is_err());
        assert!(used(&p, &[0, 1, 128, 128]).is_err());
        assert!(used(&positions(&[i16::MIN]), &[0; 4]).is_err());
        assert!(used(&positions(&[-1]), &[]).is_err());
        assert!(remap(&p, &[0, 1, 255, 0], &[0]).is_err());
        assert!(remap(&p, &[0, 1, 255, 0], &[0, 256]).is_err());
    }

    #[test]
    fn shared_record_is_remapped_from_source_only_once() {
        let p = positions(&[0x800; 9]);
        let aux = [0, 1, 127, 128].repeat(8);
        assert_eq!(
            remap(&p, &aux, &[1, 0]).unwrap(),
            [1, 0, 127, 128].repeat(8)
        );
    }
}
