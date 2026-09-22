//! Export the top mip without decompressing BC blocks or changing color space.
//! Modern large-buffer selection follows Charm's Texture.GetRawBytes(false).
use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::fs;

/// Describe one complete native texture payload without the donor's streaming state.
pub(crate) fn resident(header: &mut [u8], bytes: usize) -> Result<()> {
    ensure!(header.len() == 40, "native texture header size differs");
    let p = crate::d2_mot::payload::Payload(header.to_vec());
    ensure!(
        p.u16(12)? == 0xcafe
            && p.u32(0)? as usize == bytes
            && p.u16(14)? > 0
            && p.u16(16)? > 0
            && p.u16(18)? == 1
            && matches!(p.u16(20)?, 1 | 6)
            && (1..=15).contains(&p.u8(23)?)
            && matches!(p.u32(36)?, 0 | u32::MAX | 0x811C9DC5),
        "resident texture requires a complete unsplit payload"
    );
    // BC1 and BC7 donors can be exchanged when composing source dye maps.
    // Keep the upload pitch metadata consistent with the new block format.
    match p.u32(4)? {
        71 | 72 => header[22] = 4,
        98 | 99 => header[22] = 8,
        _ => {}
    }
    // Native resident BC textures and the single-mip RGBA control use this
    // storage state. Retaining a streaming donor's 2/1 pair reads unrelated
    // bytes even when the resource dimensions and package references are valid.
    header[24] = 0;
    header[28] = 0;
    header[29] = u8::from(header[23] > 1);
    Ok(())
}

fn surface_size(width: u32, height: u32, format: u32) -> Result<usize> {
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384,
        "unsupported texture dimensions"
    );
    let block = match format {
        71 | 72 => 8,
        98 | 99 => 16,
        _ => bail!("unsupported texture format {format}"),
    };
    Ok(width.div_ceil(4) as usize * height.div_ceil(4) as usize * block)
}
fn dds(width: u32, height: u32, format: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let size = surface_size(width, height, format)?;
    ensure!(pixels.len() == size, "top mip size mismatch");
    let mut words = [0u32; 37];
    words[0] = u32::from_le_bytes(*b"DDS ");
    words[1] = 124;
    words[2] = 0x81007;
    words[3] = height;
    words[4] = width;
    words[5] = u32::try_from(size)?;
    words[7] = 1;
    words[19] = 32;
    words[20] = 4;
    words[21] = u32::from_le_bytes(*b"DX10");
    words[27] = 0x1000;
    words[32] = format;
    words[33] = 3;
    words[35] = 1;
    let mut result = words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<_>>();
    result.extend_from_slice(pixels);
    Ok(result)
}
pub fn export(r: &mut Reader, tag: u32) -> Result<Value> {
    let header = r.tag(tag, None)?;
    let format = header.u32(4)?;
    let w = header.u16(34)? as u32;
    let h = header.u16(36)? as u32;
    ensure!(
        header.u16(38)? == 1 && header.u16(40)? == 1,
        "only 2D single-layer textures are supported"
    );
    let size = surface_size(w, h, format)?;
    let large = header.u32(60)?;
    let data_tag = if [0, u32::MAX, 0x811C9DC5].contains(&large) {
        r.reference(tag)?
    } else {
        large
    };
    let data = r.tag(data_tag, None)?;
    let pixels = data.0.get(..size).context("truncated texture top mip")?;
    let name = format!("{tag:08X}.dds");
    fs::write(r.output.join(&name), dds(w, h, format, pixels)?)?;
    Ok(
        json!({"dds":name,"source_buffer":format!("{data_tag:08X}"),"top_mip_bytes":size,"exported_mips":1,"native_material_ready":false}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resident_block_pitch_follows_format_instead_of_donor() {
        let mut header = hex::decode(
            "705501006200000000000000feca00010001010001000809020100000101000100030100ffffffff",
        )
        .unwrap();
        for (format, donor_bits, expected) in [(98u32, 4, 8), (99, 4, 8), (71, 8, 4), (72, 8, 4)] {
            header[4..8].copy_from_slice(&format.to_le_bytes());
            header[22] = donor_bits;
            resident(&mut header, 87408).unwrap();
            assert_eq!(header[22], expected);
        }
    }
    #[test]
    fn resident_storage_matches_native_headers() {
        // Native BC7 resident 80BEA293 and the verified RGBA8 control retain
        // different mip state while both omit the streaming donor's flags.
        for (mips, expected) in [
            (9, "000100000001000100030100"),
            (1, "000100000000000100030100"),
        ] {
            let mut h = hex::decode(
                "705501006200000000000000feca00010001010001000809020100000101000100030100ffffffff",
            )
            .unwrap();
            h[23] = mips;
            let prefix = h[..24].to_vec();
            resident(&mut h, 87408).unwrap();
            assert_eq!(&h[..24], prefix);
            assert_eq!(hex::encode(&h[24..36]), expected);
            assert_eq!(&h[36..], &[255; 4]);
            assert!(resident(&mut h, 87407).is_err());
            h[36..40].copy_from_slice(&0x81A62000u32.to_le_bytes());
            assert!(resident(&mut h, 87408).is_err());
        }
    }
    #[test]
    fn block_dimensions_and_dds_header() {
        assert_eq!(surface_size(5, 7, 72).unwrap(), 32);
        assert_eq!(surface_size(5, 7, 98).unwrap(), 64);
        let out = dds(5, 7, 98, &[42; 64]).unwrap();
        assert_eq!(&out[..4], b"DDS ");
        assert_eq!(&out[84..88], b"DX10");
        assert_eq!(&out[128..132], &98u32.to_le_bytes());
        assert_eq!(&out[148..], &[42; 64]);
        assert!(dds(5, 7, 98, &[0; 63]).is_err());
        assert!(surface_size(4, 4, 0).is_err());
    }
}
