use std::ops::Range;

pub(crate) const STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE: usize = 40;

/// Checks the exact straight-RGBA8 texture header used by the audited stock icon chains.
pub(crate) fn is_stock_straight_rgba8_texture_header(
    header: &[u8],
    width: u32,
    height: u32,
    data_size: usize,
) -> bool {
    header.len() == STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE
        && read_u32(header, 0).is_some_and(|value| value as usize == data_size)
        && read_u32(header, 4) == Some(0x1C)
        && read_u32(header, 8) == Some(0)
        && read_u16(header, 12) == Some(0xCAFE)
        && read_u16(header, 14).is_some_and(|value| u32::from(value) == width)
        && read_u16(header, 16).is_some_and(|value| u32::from(value) == height)
        && read_u16(header, 18) == Some(1)
        && read_u16(header, 20) == Some(1)
        && read_u16(header, 22) == Some(0x0120)
        && read_u32(header, 24) == Some(0x0000_0100)
        && read_u32(header, 28) == Some(0x0100_0000)
        && read_u32(header, 32) == Some(0x0001_0300)
        && read_u32(header, 36) == Some(u32::MAX)
}

/// Returns true when authored bytes differ from the donor only inside explicitly allowed ranges.
pub(crate) fn donor_mutation_is_limited_to(
    donor: &[u8],
    authored: &[u8],
    allowed: &[Range<usize>],
) -> bool {
    donor.len() == authored.len()
        && donor
            .iter()
            .zip(authored)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after || allowed.iter().any(|range| range.contains(&offset))
            })
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_header(width: u16, height: u16) -> [u8; STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE] {
        let mut header = [0_u8; STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE];
        let data_size = u32::from(width) * u32::from(height) * 4;
        header[0..4].copy_from_slice(&data_size.to_le_bytes());
        header[4..8].copy_from_slice(&0x1C_u32.to_le_bytes());
        header[12..14].copy_from_slice(&0xCAFE_u16.to_le_bytes());
        header[14..16].copy_from_slice(&width.to_le_bytes());
        header[16..18].copy_from_slice(&height.to_le_bytes());
        header[18..20].copy_from_slice(&1_u16.to_le_bytes());
        header[20..22].copy_from_slice(&1_u16.to_le_bytes());
        header[22..24].copy_from_slice(&0x0120_u16.to_le_bytes());
        header[24..28].copy_from_slice(&0x0000_0100_u32.to_le_bytes());
        header[28..32].copy_from_slice(&0x0100_0000_u32.to_le_bytes());
        header[32..36].copy_from_slice(&0x0001_0300_u32.to_le_bytes());
        header[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        header
    }

    #[test]
    fn stock_texture_header_requires_the_complete_audited_shape() {
        let mut header = stock_header(54, 45);
        assert!(is_stock_straight_rgba8_texture_header(
            &header,
            54,
            45,
            54 * 45 * 4
        ));

        header[22] ^= 1;
        assert!(!is_stock_straight_rgba8_texture_header(
            &header,
            54,
            45,
            54 * 45 * 4
        ));
    }

    #[test]
    fn donor_guard_accepts_only_declared_byte_ranges() {
        let donor = [0_u8; 12];
        let mut authored = donor;
        authored[5] = 1;
        assert!(donor_mutation_is_limited_to(
            &donor,
            &authored,
            std::slice::from_ref(&(4..8))
        ));

        authored[9] = 1;
        assert!(!donor_mutation_is_limited_to(
            &donor,
            &authored,
            std::slice::from_ref(&(4..8))
        ));
        assert!(!donor_mutation_is_limited_to(
            &donor,
            &authored[..11],
            std::slice::from_ref(&(0..12))
        ));
    }
}
