//! Native particle compute passes linked by the system record.
use super::*;
use std::collections::BTreeSet;

pub(crate) struct ComputePass {
    pub phase: &'static str,
    pub material: u32,
    pub shader: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelKind {
    DualMaskRamp,
}

/// Identify a decoded native pixel shader, rather than choosing a render path
/// from an effect or weapon tag. The checksum belongs to the compiled shader.
pub(super) fn pixel_kind(manager: &PackageManager, material: u32) -> Option<PixelKind> {
    const DUAL_MASK_RAMP: [u8; 16] = [
        0x52, 0x9D, 0x44, 0xF3, 0x87, 0xCF, 0x7D, 0x07, 0x82, 0x80, 0x7C, 0x84, 0xE4, 0xF5, 0xD5,
        0xAB,
    ];
    let entry = manager.get_entry(material)?;
    if entry.file_type != 8 || entry.reference != 0x8080_71E8 || entry.file_size > 1024 * 1024 {
        return None;
    }
    let bytes = manager.read_tag(material).ok()?;
    let mut seen = BTreeSet::new();
    for row in bytes.chunks_exact(4) {
        let header = u32::from_le_bytes(row.try_into().ok()?);
        if !seen.insert(header) {
            continue;
        }
        let Some(shader) = manager
            .get_entry(header)
            .filter(|entry| entry.file_type == 33)
        else {
            continue;
        };
        let Some(payload) = manager.get_entry(shader.reference) else {
            continue;
        };
        if payload.file_type != 41 || payload.reference != header || payload.file_size > 1024 * 1024
        {
            continue;
        }
        let Ok(dxbc) = manager.read_tag(shader.reference) else {
            continue;
        };
        if dxbc_stage(&dxbc) == Some(0) && dxbc.get(4..20) == Some(&DUAL_MASK_RAMP[..]) {
            return Some(PixelKind::DualMaskRamp);
        }
    }
    None
}

pub(super) fn passes(manager: &PackageManager, system: &[u8]) -> Vec<ComputePass> {
    let mut found = Vec::new();
    if system.len() != 52 {
        return found;
    }
    for (offset, phase) in [(4, "Spawn"), (8, "Motion"), (0x10, "Appearance")] {
        let Ok(material) = u32_at(system, offset) else {
            continue;
        };
        let Some(entry) = manager.get_entry(material) else {
            continue;
        };
        if entry.file_type != 8 || entry.reference != 0x8080_71E8 || entry.file_size > 1024 * 1024 {
            continue;
        }
        let Ok(bytes) = manager.read_tag(material) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for row in bytes.chunks_exact(4) {
            let header = u32::from_le_bytes(row.try_into().unwrap());
            if !seen.insert(header) {
                continue;
            }
            let Some(header_entry) = manager.get_entry(header) else {
                continue;
            };
            if header_entry.file_type != 33 || header_entry.file_size != 40 {
                continue;
            }
            let shader = header_entry.reference;
            let Some(shader_entry) = manager.get_entry(shader) else {
                continue;
            };
            if shader_entry.file_type != 41
                || shader_entry.reference != header
                || shader_entry.file_size > 1024 * 1024
            {
                continue;
            }
            let Ok(shader_bytes) = manager.read_tag(shader) else {
                continue;
            };
            if dxbc_stage(&shader_bytes) == Some(5) {
                found.push(ComputePass {
                    phase,
                    material,
                    shader,
                    size: shader_entry.file_size,
                });
            }
        }
    }
    found
}

fn dxbc_stage(bytes: &[u8]) -> Option<u16> {
    if bytes.get(..4)? != b"DXBC" {
        return None;
    }
    let count = usize::try_from(u32_at(bytes, 28).ok()?).ok()?;
    for index in 0..count.min(32) {
        let offset = usize::try_from(u32_at(bytes, 32 + index * 4).ok()?).ok()?;
        if matches!(bytes.get(offset..offset + 4)?, b"SHDR" | b"SHEX") {
            return Some((u32_at(bytes, offset + 8).ok()? >> 16) as u16);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::dxbc_stage;

    #[test]
    fn compute_shader_stage_requires_bounded_dxbc_chunks() {
        let mut bytes = vec![0u8; 52];
        bytes[..4].copy_from_slice(b"DXBC");
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&40u32.to_le_bytes());
        bytes[40..44].copy_from_slice(b"SHEX");
        bytes[48..52].copy_from_slice(&0x0005_0050u32.to_le_bytes());
        assert_eq!(dxbc_stage(&bytes), Some(5));
        bytes[32..36].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(dxbc_stage(&bytes), None);
    }
}
