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

/// Exact match required to seed a cleared region, allowing only for a filtered edge.
pub const CLEARED_SEED_TOLERANCE: i32 = 4;
/// How far a pixel may sit off the cleared color's brightness ramp and still be part of its halo.
///
/// Measured across every installed ornament icon: this clears each plate completely while leaving
/// gold, cream and green weapon art intact. Widening it to 30 starts eating a weapon's own metal.
pub const CLEARED_HALO_TOLERANCE: i32 = 8;
/// Hue within which a pixel bordering a cleared region counts as part of its edge, in the
/// integer hue space where a full turn is 1536 units, so this is 20 degrees.
pub const CLEARED_EDGE_HUE_UNITS: i32 = 20 * 1536 / 360;
/// Chroma a pixel needs before its hue is meaningful enough to match an edge.
pub const CLEARED_EDGE_MINIMUM_CHROMA: i32 = 12;
/// How many pixels out from a cleared region that edge is followed.
///
/// The bound is what keeps the pass safe: where the color ran behind the art its edge tints the
/// art and leaves the color's own ramp, so only pixels within this many steps of the region are
/// eligible, and only if they still carry its hue.
pub const CLEARED_EDGE_DEPTH: usize = 2;

/// Clears one flat color, and the halo where it fades into the art behind it, to transparency.
///
/// The color is whatever the caller names, including one a person picked out of the artwork,
/// so a black or near-black choice has to behave as well as a bright one.
///
/// A flat color painted behind artwork fades through its own brightness rather than toward a
/// neighbouring hue, so its antialiased edge is the same color darkened, not a nearby color. This
/// seeds on the color itself and floods along that ramp, which removes the edge while leaving art
/// that merely looks similar alone: a pixel is only eligible if it touches the region.
///
/// Where the color ran behind the art instead of behind nothing, its edge blends with the art and
/// leaves that ramp, so a bounded pass afterwards takes the few pixels next to the region that
/// still carry the color's hue. Clearing them rather than neutralizing them keeps the edge clean.
///
/// `pixels` is unpremultiplied RGBA8.
pub fn clear_color_region(pixels: &mut [u8], width: usize, height: usize, cleared: [u8; 3]) {
    if width == 0 || height == 0 || pixels.len() != width * height * 4 {
        return;
    }
    let brightest = i32::from(cleared.iter().copied().max().unwrap_or(0));
    let brightest_channel = cleared
        .iter()
        .position(|value| i32::from(*value) == brightest)
        .unwrap_or(0);
    // Compare against the color scaled to this pixel's brightness without leaving integers:
    // pixel[c] * brightest is the same test as pixel[c] vs cleared[c] * scale, scaled up.
    let on_ramp = |pixel: &[u8]| {
        // Black has no brightness ramp to darken along, and the scaled comparison below
        // degenerates to a test of one channel when it is the reference. Its antialiased
        // edge is still black, so near-black is the whole of its halo.
        if brightest == 0 {
            return (0..3).all(|channel| i32::from(pixel[channel]) <= CLEARED_HALO_TOLERANCE);
        }
        let level = i32::from(pixel[brightest_channel]);
        if level * 100 < brightest * 4 || level * 100 > brightest * 106 {
            return false;
        }
        (0..3).all(|channel| {
            let expected = i32::from(cleared[channel]) * level;
            let actual = i32::from(pixel[channel]) * brightest;
            actual.abs_diff(expected) <= (CLEARED_HALO_TOLERANCE * brightest) as u32
        })
    };

    let mut region = vec![false; width * height];
    let mut pending = Vec::new();
    for index in 0..width * height {
        let pixel = &pixels[index * 4..index * 4 + 4];
        if pixel[3] == 0 {
            continue;
        }
        if (0..3).all(|channel| {
            i32::from(pixel[channel]).abs_diff(i32::from(cleared[channel]))
                <= CLEARED_SEED_TOLERANCE as u32
        }) {
            region[index] = true;
            pending.push(index);
        }
    }
    let neighbours = |index: usize| {
        let x = (index % width) as i64;
        let y = (index / width) as i64;
        [
            (-1_i64, 0_i64),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            let nx = x + dx;
            let ny = y + dy;
            (nx >= 0 && ny >= 0 && nx < width as i64 && ny < height as i64)
                .then(|| ny as usize * width + nx as usize)
        })
    };
    while let Some(index) = pending.pop() {
        for neighbour in neighbours(index) {
            if region[neighbour] || pixels[neighbour * 4 + 3] == 0 {
                continue;
            }
            if on_ramp(&pixels[neighbour * 4..neighbour * 4 + 3]) {
                region[neighbour] = true;
                pending.push(neighbour);
            }
        }
    }

    let cleared_hue = hue_units(&cleared);
    for _ in 0..CLEARED_EDGE_DEPTH {
        let edge = (0..width * height)
            .filter(|index| {
                !region[*index]
                    && pixels[index * 4 + 3] > 0
                    && neighbours(*index).any(|neighbour| region[neighbour])
                    && carries_hue(&pixels[index * 4..index * 4 + 3], cleared_hue)
            })
            .collect::<Vec<_>>();
        for index in edge {
            region[index] = true;
        }
    }

    for (index, inside) in region.into_iter().enumerate() {
        if inside {
            pixels[index * 4..index * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}

/// One full turn of hue in the integer hue space, six sectors of 256 units each.
const HUE_TURN: i32 = 1536;

/// Returns a color's hue in that integer space, or `None` for a neutral color with no hue.
fn hue_units(color: &[u8]) -> Option<i32> {
    let red = i32::from(*color.first()?);
    let green = i32::from(*color.get(1)?);
    let blue = i32::from(*color.get(2)?);
    let maximum = red.max(green).max(blue);
    let chroma = maximum - red.min(green).min(blue);
    if chroma == 0 {
        return None;
    }
    let hue = if maximum == red {
        (green - blue) * 256 / chroma
    } else if maximum == green {
        512 + (blue - red) * 256 / chroma
    } else {
        1024 + (red - green) * 256 / chroma
    };
    Some(hue.rem_euclid(HUE_TURN))
}

/// Returns whether a pixel carries enough of a hue to be treated as part of that color's edge.
fn carries_hue(pixel: &[u8], cleared_hue: Option<i32>) -> bool {
    let (Some(cleared_hue), Some(hue)) = (cleared_hue, hue_units(pixel)) else {
        return false;
    };
    let chroma = i32::from(*pixel[..3].iter().max().unwrap_or(&0))
        - i32::from(*pixel[..3].iter().min().unwrap_or(&0));
    let delta = (hue - cleared_hue).abs();
    delta.min(HUE_TURN - delta) <= CLEARED_EDGE_HUE_UNITS && chroma >= CLEARED_EDGE_MINIMUM_CHROMA
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

    const PLATE: [u8; 3] = [0xF2, 0xE3, 0x70];

    fn filled(width: usize, height: usize, pixel: [u8; 4]) -> Vec<u8> {
        pixel
            .iter()
            .copied()
            .cycle()
            .take(width * height * 4)
            .collect()
    }

    fn put(pixels: &mut [u8], width: usize, x: usize, y: usize, value: [u8; 4]) {
        pixels[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&value);
    }

    fn at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        pixels[(y * width + x) * 4..(y * width + x) * 4 + 4]
            .try_into()
            .expect("four channels")
    }

    fn transparent(pixels: &[u8]) -> usize {
        pixels.chunks_exact(4).filter(|pixel| pixel[3] == 0).count()
    }

    #[test]
    fn clearing_a_plate_leaves_the_art_drawn_over_it() {
        let mut pixels = filled(8, 8, [PLATE[0], PLATE[1], PLATE[2], 255]);
        put(&mut pixels, 8, 3, 3, [0x20, 0x40, 0xFF, 255]);
        clear_color_region(&mut pixels, 8, 8, PLATE);
        assert_eq!(transparent(&pixels), 63);
        assert_eq!(at(&pixels, 8, 3, 3), [0x20, 0x40, 0xFF, 255]);
    }

    /// The plate is painted at a range of alpha values, and the color is what identifies it,
    /// so a translucent plate pixel is cleared like an opaque one.
    #[test]
    fn a_translucent_plate_pixel_is_cleared_like_an_opaque_one() {
        for alpha in [255, 192, 128, 64, 16, 1] {
            let mut pixels = filled(4, 4, [PLATE[0], PLATE[1], PLATE[2], 255]);
            put(&mut pixels, 4, 2, 2, [PLATE[0], PLATE[1], PLATE[2], alpha]);
            clear_color_region(&mut pixels, 4, 4, PLATE);
            assert_eq!(at(&pixels, 4, 2, 2), [0, 0, 0, 0], "alpha {alpha}");
        }
    }

    /// The flood only reaches pixels that touch the region, so art that merely resembles the
    /// cleared color survives when nothing joins it to a seed.
    #[test]
    fn art_near_the_cleared_color_survives_when_it_touches_no_seed() {
        let similar = [0xF2, 0xE3, 0x60, 255];
        let mut pixels = filled(9, 9, [0, 0, 0, 0]);
        for x in 0..9 {
            put(&mut pixels, 9, x, 0, [PLATE[0], PLATE[1], PLATE[2], 255]);
        }
        put(&mut pixels, 9, 4, 8, similar);
        clear_color_region(&mut pixels, 9, 9, PLATE);
        // The art pixel is the only one still drawn: the plate row went and the rest of the
        // image was already transparent.
        assert_eq!(transparent(&pixels), 9 * 9 - 1);
        assert_eq!(at(&pixels, 9, 4, 8), similar);
        for x in 0..9 {
            assert_eq!(at(&pixels, 9, x, 0), [0, 0, 0, 0], "plate pixel {x} stayed");
        }
    }

    /// The color can be picked out of the artwork, so black has to work. It has no brightness
    /// ramp and no hue, so its region is the near-black pixels and nothing else.
    #[test]
    fn a_black_cleared_color_clears_black_and_leaves_the_rest() {
        let mut pixels = filled(8, 8, [90, 70, 40, 255]);
        put(&mut pixels, 8, 0, 0, [0, 0, 0, 255]);
        put(&mut pixels, 8, 1, 0, [2, 1, 2, 255]);
        put(&mut pixels, 8, 7, 7, [0, 0, 0, 255]);
        clear_color_region(&mut pixels, 8, 8, [0, 0, 0]);
        assert_eq!(at(&pixels, 8, 0, 0), [0, 0, 0, 0]);
        assert_eq!(
            at(&pixels, 8, 1, 0),
            [0, 0, 0, 0],
            "its antialiased edge goes too"
        );
        assert_eq!(at(&pixels, 8, 7, 7), [0, 0, 0, 0]);
        assert_eq!(transparent(&pixels), 3, "the art around it stays");
    }

    #[test]
    fn malformed_or_empty_images_are_left_alone() {
        let mut short = vec![1, 2, 3];
        clear_color_region(&mut short, 4, 4, PLATE);
        assert_eq!(short, vec![1, 2, 3]);
        let mut empty: Vec<u8> = Vec::new();
        clear_color_region(&mut empty, 0, 0, PLATE);
        assert!(empty.is_empty());
    }
}
