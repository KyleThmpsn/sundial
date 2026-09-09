//! Small image primitives shared by Sundial's catalog and package-authoring UI.

/// Alpha-composites one unpremultiplied RGBA pixel over another.
pub fn blend_rgba_pixel(destination: &mut [u8], source: [u8; 4]) {
    debug_assert!(destination.len() >= 4);
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    let destination_alpha = u32::from(destination[3]);
    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha = source_alpha + (destination_alpha * inverse_source_alpha + 127) / 255;
    for channel in 0..3 {
        let premultiplied = u32::from(source[channel]) * source_alpha
            + (u32::from(destination[channel]) * destination_alpha * inverse_source_alpha + 127)
                / 255;
        destination[channel] = ((premultiplied + output_alpha / 2) / output_alpha) as u8;
    }
    destination[3] = output_alpha as u8;
}

/// Decodes one complete BC1/DXT1 mip into unpremultiplied RGBA8 pixels.
pub fn decode_bc1(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let block_width = width.div_ceil(4);
    let block_height = height.div_ceil(4);
    let required = block_width
        .checked_mul(block_height)
        .and_then(|blocks| blocks.checked_mul(8))
        .ok_or("BC1 texture size overflowed")?;
    if data.len() < required {
        return Err("BC1 texture data is truncated".into());
    }
    let pixel_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("BC1 pixel buffer size overflowed")?;
    let mut rgba = vec![0; pixel_count];
    for block_y in 0..block_height {
        for block_x in 0..block_width {
            let offset = (block_y * block_width + block_x) * 8;
            let color_0 = u16::from_le_bytes([data[offset], data[offset + 1]]);
            let color_1 = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let mut colors = [[0_u8; 4]; 4];
            colors[0] = rgb565(color_0);
            colors[1] = rgb565(color_1);
            if color_0 > color_1 {
                for channel in 0..3 {
                    colors[2][channel] = ((2 * u16::from(colors[0][channel])
                        + u16::from(colors[1][channel]))
                        / 3) as u8;
                    colors[3][channel] = ((u16::from(colors[0][channel])
                        + 2 * u16::from(colors[1][channel]))
                        / 3) as u8;
                }
                colors[2][3] = 255;
                colors[3][3] = 255;
            } else {
                for channel in 0..3 {
                    colors[2][channel] =
                        ((u16::from(colors[0][channel]) + u16::from(colors[1][channel])) / 2) as u8;
                }
                colors[2][3] = 255;
            }
            let indices = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            for pixel_y in 0..4 {
                for pixel_x in 0..4 {
                    let x = block_x * 4 + pixel_x;
                    let y = block_y * 4 + pixel_y;
                    if x >= width || y >= height {
                        continue;
                    }
                    let pixel = pixel_y * 4 + pixel_x;
                    let color = colors[((indices >> (pixel * 2)) & 3) as usize];
                    rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
                }
            }
        }
    }
    Ok(rgba)
}

fn rgb565(color: u16) -> [u8; 4] {
    let red = ((color >> 11) & 0x1F) as u8;
    let green = ((color >> 5) & 0x3F) as u8;
    let blue = (color & 0x1F) as u8;
    [
        (u16::from(red) * 255 / 31) as u8,
        (u16::from(green) * 255 / 63) as u8,
        (u16::from(blue) * 255 / 31) as u8,
        255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_decodes_opaque_and_translucent_modes() {
        let opaque = decode_bc1(&[0x00, 0xF8, 0xE0, 0x07, 0, 0, 0, 0], 4, 4).unwrap();
        assert_eq!(&opaque[..4], &[255, 0, 0, 255]);

        let transparent = decode_bc1(&[0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], 4, 4).unwrap();
        assert_eq!(&transparent[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn alpha_composition_uses_unpremultiplied_channels() {
        let mut destination = [0, 0, 255, 255];
        blend_rgba_pixel(&mut destination, [255, 0, 0, 128]);
        assert_eq!(destination, [128, 0, 127, 255]);
    }
}
