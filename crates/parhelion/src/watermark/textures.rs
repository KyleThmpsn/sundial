//! Decoding, rendering and validating the authored watermark textures.
use super::*;

pub(crate) fn decode_authored_texture(
    texture_index: usize,
    width: u32,
    height: u32,
) -> AuthoringResult<Vec<u8>> {
    let pixels = decode_source_texture(texture_index, width, height)?;
    placement::adjust_corner_glyph(texture_index, width, height, pixels)
}

/// One audited PNG, decoded and checked against its hash.
struct DecodedTexture {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// The PNGs are compiled in, so the hash check and the decode give the same answer every
/// time. Each artwork group used to repeat both for all six textures, twice over: once to
/// validate the authored texture and once to render the output. They now run once.
fn decoded_textures() -> &'static [Result<DecodedTexture, String>] {
    static DECODED: std::sync::OnceLock<Vec<Result<DecodedTexture, String>>> =
        std::sync::OnceLock::new();
    DECODED.get_or_init(|| {
        (0..AUTHORED_TEXTURE_PNGS.len())
            .map(decode_audited_png)
            .collect()
    })
}

fn decode_audited_png(texture_index: usize) -> Result<DecodedTexture, String> {
    let png = AUTHORED_TEXTURE_PNGS
        .get(texture_index)
        .ok_or_else(|| format!("Unknown Sunrise watermark texture {texture_index}"))?;
    let expected_hash = AUTHORED_TEXTURE_PNG_SHA1
        .get(texture_index)
        .ok_or_else(|| format!("Missing hash for watermark texture {texture_index}"))?;
    if Sha1::digest(png).as_slice() != expected_hash {
        return Err(format!(
            "Pre-rendered Sunrise watermark texture {texture_index} no longer matches its audited asset"
        ));
    }
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|error| {
            format!(
                "Could not decode pre-rendered Sunrise watermark texture {texture_index}: {error}"
            )
        })?
        .into_rgba8();
    Ok(DecodedTexture {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}

fn decode_source_texture(
    texture_index: usize,
    width: u32,
    height: u32,
) -> AuthoringResult<Vec<u8>> {
    let decoded = decoded_textures()
        .get(texture_index)
        .ok_or_else(|| invalid(format!("Unknown Sunrise watermark texture {texture_index}")))?
        .as_ref()
        .map_err(|message| invalid(message.clone()))?;
    if (decoded.width, decoded.height) != (width, height) {
        return Err(invalid(format!(
            "Pre-rendered Sunrise watermark texture {texture_index} is {}x{}; expected {width}x{height}",
            decoded.width, decoded.height
        )));
    }
    Ok(decoded.pixels.clone())
}

/// Shared high-resolution output for package textures and editor previews. This resamples
/// the approved small-scale design, not the original full-size Sunrise logo.
pub(crate) fn render_output_texture(texture_index: usize) -> AuthoringResult<image::RgbaImage> {
    static RENDERED: std::sync::OnceLock<Vec<Result<image::RgbaImage, String>>> =
        std::sync::OnceLock::new();
    // Every artwork group renders the same six outputs from the same audited pixels, and the
    // editor preview asks for them again, so the resample happens once.
    let rendered = RENDERED
        .get_or_init(|| {
            (0..TEXTURE_DIMENSIONS.len())
                .map(render_audited_output)
                .collect()
        })
        .get(texture_index)
        .ok_or_else(|| invalid("Unknown Sunrise watermark texture"))?;
    rendered
        .as_ref()
        .cloned()
        .map_err(|message| validation(message.clone()))
}

fn render_audited_output(texture_index: usize) -> Result<image::RgbaImage, String> {
    let &(width, height) = TEXTURE_DIMENSIONS
        .get(texture_index)
        .ok_or_else(|| "Unknown Sunrise watermark texture".to_owned())?;
    let pixels = placement::render_output(
        texture_index,
        width,
        height,
        decode_source_texture(texture_index, width, height).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    image::RgbaImage::from_raw(
        width * OUTPUT_TEXTURE_SCALE,
        height * OUTPUT_TEXTURE_SCALE,
        pixels,
    )
    .ok_or_else(|| "Rendered watermark has invalid dimensions".to_owned())
}

pub(super) fn upscale_texture(
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> AuthoringResult<image::RgbaImage> {
    let source = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| validation("Watermark RGBA dimensions do not match its payload"))?;
    Ok(crate::image_import::fit(
        &source,
        width * OUTPUT_TEXTURE_SCALE,
        height * OUTPUT_TEXTURE_SCALE,
    ))
}

pub(super) fn resize_texture_header(
    header: &mut [u8],
    width: u32,
    height: u32,
    data_size: usize,
) -> AuthoringResult<()> {
    let width_field = u16::try_from(width)
        .map_err(|_| validation("Watermark width exceeds its native header field"))?;
    let height_field = u16::try_from(height)
        .map_err(|_| validation("Watermark height exceeds its native header field"))?;
    let size_field = u32::try_from(data_size)
        .map_err(|_| validation("Watermark payload exceeds its native header field"))?;
    if header.len() != STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE {
        return Err(validation("Watermark texture header has an invalid size"));
    }
    write_u32(header, 0, size_field)?;
    header[14..16].copy_from_slice(&width_field.to_le_bytes());
    header[16..18].copy_from_slice(&height_field.to_le_bytes());
    if !is_stock_straight_rgba8_texture_header(header, width, height, data_size) {
        return Err(validation(
            "High-resolution watermark texture header is invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_authored_texture(
    texture_index: usize,
    width: u32,
    height: u32,
    donor: &[u8],
    authored: &[u8],
) -> AuthoringResult<()> {
    let expected_size = width as usize * height as usize * 4;
    if donor.len() != expected_size || authored.len() != expected_size {
        return Err(validation(format!(
            "Sunrise watermark texture {texture_index} has malformed donor/authored RGBA dimensions"
        )));
    }

    if matches!(texture_index, 2 | 3) {
        return validate_standalone_texture(texture_index, width, donor, authored);
    }

    let (min_x, min_y, max_x, max_y) = match texture_index {
        0 | 4 => (1, 2, 31, 29),
        1 | 5 => (19, 1, 53, 32),
        _ => {
            return Err(validation(format!(
                "Sunrise watermark texture {texture_index} has no audited lane semantics"
            )));
        }
    };
    let mut changed = 0usize;
    for (pixel_index, (before, after)) in donor
        .chunks_exact(4)
        .zip(authored.chunks_exact(4))
        .enumerate()
    {
        if before == after {
            continue;
        }
        changed += 1;
        let x = pixel_index as u32 % width;
        let y = pixel_index as u32 / width;
        if x < min_x || x > max_x || y < min_y || y > max_y {
            return Err(validation(format!(
                "Sunrise watermark texture {texture_index} changed native earmark pixel ({x}, {y}) outside the audited glyph bounds"
            )));
        }
    }
    let expected_polarity = authored.chunks_exact(4).any(|pixel| {
        pixel[3] >= 200
            && if texture_index < 2 {
                pixel[0..3].iter().all(|channel| *channel >= 220)
            } else {
                pixel[0..3].iter().all(|channel| *channel <= 32)
            }
    });
    if changed == 0 || !expected_polarity {
        return Err(validation(format!(
            "Sunrise watermark texture {texture_index} does not contain its expected {} glyph",
            if texture_index < 2 { "white" } else { "black" }
        )));
    }
    Ok(())
}

fn validate_standalone_texture(
    texture_index: usize,
    width: u32,
    donor: &[u8],
    authored: &[u8],
) -> AuthoringResult<()> {
    let donor_alpha = donor
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    let authored_alpha = authored
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    if donor
        .chunks_exact(4)
        .zip(authored.chunks_exact(4))
        .any(|(before, after)| before[..3] != after[..3])
    {
        return Err(validation(format!(
            "Sunrise standalone watermark {texture_index} changed its native RGB polarity"
        )));
    }
    if Sha1::digest(&donor_alpha).as_slice() != DONOR_STANDALONE_ALPHA_SHA1 {
        return Err(invalid(format!(
            "Standalone watermark donor {texture_index} no longer has its audited alpha mask"
        )));
    }
    if donor_alpha == authored_alpha
        || Sha1::digest(&authored_alpha).as_slice() != AUTHORED_STANDALONE_ALPHA_SHA1
        || alpha_bounds(&authored_alpha, width) != Some(AUTHORED_STANDALONE_ALPHA_BOUNDS)
    {
        return Err(validation(format!(
            "Sunrise standalone watermark {texture_index} does not contain its audited Sunrise alpha mask"
        )));
    }
    Ok(())
}

fn alpha_bounds(alpha: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (index, value) in alpha.iter().copied().enumerate() {
        if value == 0 {
            continue;
        }
        let x = index as u32 % width;
        let y = index as u32 / width;
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((min_x, min_y, max_x, max_y)) => {
                (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
            }
        });
    }
    bounds
}

pub(crate) fn private_icon_fingerprint(container: &[u8], visual_revision: &[u8]) -> u32 {
    let mut digest = Sha1::new();
    digest.update(b"parhelion.icon-composition.v1");
    digest.update(&container[ICON_CONTENT_FINGERPRINT_OFFSET + 4..]);
    digest.update(visual_revision);
    let bytes = digest.finalize();
    u32::from_le_bytes(bytes[..4].try_into().expect("SHA-1 prefix"))
}
