//! Optical placement of the existing glyph, shared by package output and editor previews.
//! The native corner plate retains its logical geometry. Output can use a denser pixel grid.

use super::{AUTHORED_TEXTURE_PNGS, AuthoringResult, invalid};

fn glyph_scale(index: usize) -> f64 {
    if matches!(index, 1 | 5) { 1.0 } else { 1.15 }
}

fn glyph_down_shift(index: usize) -> f64 {
    if matches!(index, 1 | 5) { 3.0 } else { 1.0 }
}

fn glyph_height_scale(index: usize) -> f64 {
    if matches!(index, 1 | 5) { 1.12 } else { 1.15 }
}

fn glyph_left_shift(index: usize) -> f64 {
    // Stock right-corner marks have a 4px visible right inset. The approved Sunrise
    // source has an 8px inset, so translate it directly without the old 15% enlargement.
    if matches!(index, 1 | 5) { -4.0 } else { 2.0 }
}

pub(super) fn adjust_corner_glyph(
    index: usize,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> AuthoringResult<Vec<u8>> {
    place_glyph(index, width, height, pixels, 1)
}

pub(super) fn render_output(
    index: usize,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> AuthoringResult<Vec<u8>> {
    place_glyph(index, width, height, pixels, super::OUTPUT_TEXTURE_SCALE)
}

fn place_glyph(
    index: usize,
    width: u32,
    height: u32,
    mut pixels: Vec<u8>,
    output_scale: u32,
) -> AuthoringResult<Vec<u8>> {
    let (source_index, center_x, center_y, bounds) = match index {
        0 | 4 => (0, 18.0, 15.0, (5, 4, 31, 26)),
        1 | 5 => (1, 36.0, 14.0, (23, 3, 49, 25)),
        2 | 3 => {
            return if output_scale == 1 {
                Ok(pixels)
            } else {
                Ok(super::upscale_texture(width, height, pixels)?.into_raw())
            };
        }
        _ => return Err(invalid("Unknown watermark placement lane")),
    };
    // The paired light lane identifies only glyph pixels, not the dark plate's diagonal border.
    let guide = image::load_from_memory(AUTHORED_TEXTURE_PNGS[source_index])
        .map_err(|error| invalid(format!("Could not read watermark glyph mask: {error}")))?
        .into_rgba8();
    if guide.dimensions() != (width, height) || pixels.len() != (width * height * 4) as usize {
        return Err(invalid(
            "Watermark placement dimensions do not match its source lane",
        ));
    }
    let background = if index < 2 {
        [0u8, 0, 0, 102]
    } else {
        [185u8, 185, 185, 255]
    };
    let base = premultiplied(background);
    let mut glyph = vec![[0.0; 4]; (width * height) as usize];
    for y in bounds.1..=bounds.3 {
        for x in bounds.0..=bounds.2 {
            let mask = guide.get_pixel(x, y);
            if mask[3] == 0 || mask[0] == 0 {
                continue;
            }
            let offset = (y * width + x) as usize;
            let color = premultiplied(
                pixels[offset * 4..offset * 4 + 4]
                    .try_into()
                    .expect("RGBA pixel"),
            );
            for channel in 0..4 {
                glyph[offset][channel] = color[channel] - base[channel];
            }
            pixels[offset * 4..offset * 4 + 4].copy_from_slice(&background);
        }
    }
    // Resize the empty plate, then sample the original glyph directly onto the final grid.
    // Do not first quantize the placed glyph to 54/96px and enlarge that blurred result.
    let plate = pixels;
    let mut pixels = if output_scale == 1 {
        plate.clone()
    } else {
        super::upscale_texture(width, height, plate.clone())?.into_raw()
    };
    let output_width = width * output_scale;
    for y in 0..height * output_scale {
        for x in 0..output_width {
            let logical_x = (f64::from(x) + 0.5) / f64::from(output_scale) - 0.5;
            let logical_y = (f64::from(y) + 0.5) / f64::from(output_scale) - 0.5;
            let sx =
                (logical_x - (center_x - glyph_left_shift(index))) / glyph_scale(index) + center_x;
            let sy = (logical_y - (center_y + glyph_down_shift(index))) / glyph_height_scale(index)
                + center_y;
            let delta = sample(&glyph, width, height, sx, sy);
            if delta.iter().all(|value| value.abs() < f64::EPSILON) {
                continue;
            }
            let offset = ((y * output_width + x) * 4) as usize;
            // Preserve the resized plate's antialiasing instead of treating its filter
            // fringe as a new border or replacing it with a flat background color.
            let output_base =
                premultiplied(pixels[offset..offset + 4].try_into().expect("RGBA pixel"));
            let alpha = (output_base[3] + delta[3]).clamp(0.0, 255.0);
            let mut output_pixel = [0; 4];
            for channel in 0..3 {
                output_pixel[channel] = ((output_base[channel] + delta[channel]) * 255.0 / alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            output_pixel[3] = alpha.round() as u8;
            if output_pixel == pixels[offset..offset + 4] {
                continue;
            }
            let plate_offset = (((y / output_scale) * width + x / output_scale) * 4) as usize;
            if plate[plate_offset..plate_offset + 4] != background {
                return Err(invalid(format!(
                    "Adjusted watermark lane {index} at scale {output_scale} would overlap the native corner edge at ({x}, {y})",
                )));
            }
            pixels[offset..offset + 4].copy_from_slice(&output_pixel);
        }
    }
    Ok(pixels)
}

fn premultiplied(pixel: [u8; 4]) -> [f64; 4] {
    let alpha = f64::from(pixel[3]);
    [
        f64::from(pixel[0]) * alpha / 255.0,
        f64::from(pixel[1]) * alpha / 255.0,
        f64::from(pixel[2]) * alpha / 255.0,
        alpha,
    ]
}

fn sample(pixels: &[[f64; 4]], width: u32, height: u32, x: f64, y: f64) -> [f64; 4] {
    let left = x.floor() as i32;
    let top = y.floor() as i32;
    let mut result = [0.0; 4];
    for dy in 0..2 {
        for dx in 0..2 {
            let sx = left + dx;
            let sy = top + dy;
            if sx < 0 || sy < 0 || sx >= width as i32 || sy >= height as i32 {
                continue;
            }
            let weight = (1.0 - (x - f64::from(sx)).abs()) * (1.0 - (y - f64::from(sy)).abs());
            let value = pixels[(sy as u32 * width + sx as u32) as usize];
            for channel in 0..4 {
                result[channel] += value[channel] * weight;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optical_adjustment_preserves_plate_edges_and_standalone_badges() {
        for (index, png) in AUTHORED_TEXTURE_PNGS.iter().enumerate() {
            let image = image::load_from_memory(png).unwrap().into_rgba8();
            let (width, height) = image.dimensions();
            let source = image.into_raw();
            let adjusted = adjust_corner_glyph(index, width, height, source.clone()).unwrap();
            if let Some(directory) = std::env::var_os("SUNDIAL_TEST_ICON_PREVIEW_DIR") {
                let path = std::path::Path::new(&directory).join(format!("watermark-{index}.png"));
                image::save_buffer(path, &adjusted, width, height, image::ColorType::Rgba8)
                    .unwrap();
            }
            if matches!(index, 2 | 3) {
                assert_eq!(source, adjusted);
                continue;
            }
            assert_ne!(source, adjusted);
            let bounds = if width == 96 {
                (1, 2, 31, 29)
            } else {
                (19, 1, 53, 32)
            };
            for y in 0..height {
                for x in 0..width {
                    if x < bounds.0 || x > bounds.2 || y < bounds.1 || y > bounds.3 {
                        let offset = ((y * width + x) * 4) as usize;
                        assert_eq!(
                            source[offset..offset + 4],
                            adjusted[offset..offset + 4],
                            "lane {index}, ({x},{y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn optical_adjustment_enlarges_glyph_and_applies_lane_placement() {
        for (index, center_x) in [(0, 18.0), (1, 36.0)] {
            let source = image::load_from_memory(AUTHORED_TEXTURE_PNGS[index])
                .unwrap()
                .into_rgba8();
            let adjusted = adjust_corner_glyph(
                index,
                source.width(),
                source.height(),
                source.as_raw().clone(),
            )
            .unwrap();
            let (before_mass, before_x) = glyph_mass_and_center(source.as_raw(), source.width());
            let (after_mass, after_x) = glyph_mass_and_center(&adjusted, source.width());
            assert!(
                (after_mass / before_mass - glyph_scale(index) * glyph_height_scale(index)).abs()
                    < 0.02
            );
            let expected_x =
                (before_x - center_x) * glyph_scale(index) + center_x - glyph_left_shift(index);
            assert!((after_x - expected_x).abs() < 0.1);
            assert!(after_x < before_x - glyph_left_shift(index) + 0.2);
        }
    }

    fn glyph_mass_and_center(pixels: &[u8], width: u32) -> (f64, f64) {
        let mut mass = 0.0;
        let mut weighted_x = 0.0;
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            let weight = f64::from(pixel[0]) * f64::from(pixel[3]) / 255.0;
            mass += weight;
            weighted_x += weight * f64::from(index as u32 % width);
        }
        (mass, weighted_x / mass)
    }
}
