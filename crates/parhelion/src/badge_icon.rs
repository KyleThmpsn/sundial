//! Authoring support for the Project Sunrise collection-badge icon.
//!
//! The source mark is Solus's artwork in `assets/parhelion`. This module only performs
//! deterministic raster composition for a copy of the Lunar badge's icon chain
//! ([`crate::icon_chain`]). It does not install packages or mutate an existing tag.

use image::{ImageFormat, Rgba, RgbaImage, imageops::FilterType};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::{input as invalid, validation},
    icon_chain::{Chain, Donor, Plan, Texture},
};

const SOURCE_PNG: &[u8] = include_bytes!("../../../assets/parhelion/sunrise-badge-source.png");

const DONOR_LOW_DATA: TagHash = TagHash(0x8132_E45A);
const DONOR_LOW_HEADER: TagHash = TagHash(0x8132_E45B);
const DONOR_HIGH_DATA: TagHash = TagHash(0x8132_E45C);
const DONOR_HIGH_HEADER: TagHash = TagHash(0x8132_E45D);
const DONOR_LAYER: TagHash = TagHash(0x8132_E45E);
const DONOR_CONTAINER: TagHash = TagHash(0x8132_E45F);
const DONOR_COMPANION: TagHash = TagHash(0x8132_E460);

const LOW_WIDTH: u32 = 208;
const LOW_HEIGHT: u32 = 126;
const HIGH_WIDTH: u32 = 440;
const HIGH_HEIGHT: u32 = 268;
const HIGH_DATA_SIZE: usize = HIGH_WIDTH as usize * HIGH_HEIGHT as usize * 4;
const LAYER_SIZE: usize = 180;
const CARD_PURPLE_TOP: [u8; 3] = [32, 18, 49];
const CARD_PURPLE_BOTTOM: [u8; 3] = [54, 30, 72];
const CARD_GOLD_GLOW: [u8; 3] = [221, 172, 74];
const CARD_GOLD_GLOW_MAX_MIX: u32 = 5_243;

const LOW_HEADER_TAG_OFFSET: usize = 0x90;
const HIGH_HEADER_TAG_OFFSET: usize = 0xB0;

/// The Lunar badge's chain: a layer of two lanes, one texture each.
fn lunar() -> Donor {
    Donor {
        label: "Lunar badge icon".into(),
        textures: [
            Texture {
                data: DONOR_LOW_DATA,
                header: DONOR_LOW_HEADER,
                width: LOW_WIDTH,
                height: LOW_HEIGHT,
                layer_field: LOW_HEADER_TAG_OFFSET,
            },
            Texture {
                data: DONOR_HIGH_DATA,
                header: DONOR_HIGH_HEADER,
                width: HIGH_WIDTH,
                height: HIGH_HEIGHT,
                layer_field: HIGH_HEADER_TAG_OFFSET,
            },
        ],
        layer: DONOR_LAYER,
        layer_size: LAYER_SIZE,
        layer_words: vec![
            (0x00, LAYER_SIZE as u32),
            (0x10, 0x10),
            (0x1C, 0x8080_4A67),
            (0x20, 2),
            (0x28, 0x18),
            (0x3C, 0x8080_9FBD),
            (0x40, 2),
            (0x48, 0x8080_4A6C),
            (0x50, 1),
            (0x58, 0x28),
            (0x60, 1),
            (0x68, 0x38),
            (0x7C, 0x8080_9FBD),
            (0x80, 1),
            (0x88, 0x8080_4A6F),
            (0x9C, 0x8080_9FBD),
            (0xA0, 1),
            (0xA8, 0x8080_4A6F),
        ],
        container: DONOR_CONTAINER,
        companion: DONOR_COMPANION,
    }
}

/// Builds a seven-tag texture chain using the configured Shadowkeep package manager for donor
/// reads and Solus's bundled Sunrise artwork for the badge mark.
///
/// `current_entry_count` is the destination package's entry count before any append operation.
/// `appended_ordinal_base` is the number of tags the caller will place before this plan in the
/// same `new_tags` slice. No assigned destination tag is hardcoded.
#[cfg(test)]
pub fn build_badge_icon_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
) -> AuthoringResult<Plan> {
    build_icon_plan(
        manager,
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
        None,
    )
}

pub(crate) fn build_icon_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    artwork: Option<&crate::presentation::Artwork>,
) -> AuthoringResult<Plan> {
    build_icon_plan_with_branding(
        manager,
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
        artwork,
        crate::branding::Branding::Sunrise,
    )
}

pub(crate) fn build_icon_plan_with_branding(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    artwork: Option<&crate::presentation::Artwork>,
    branding: crate::branding::Branding,
) -> AuthoringResult<Plan> {
    let donor = lunar();
    let chain = donor.read(manager)?;
    let [low_mask, high_mask] = &chain.data;
    let low_data = render_artwork(artwork, low_mask, LOW_WIDTH, LOW_HEIGHT, branding)?;
    let high_data = render_artwork(artwork, high_mask, HIGH_WIDTH, HIGH_HEIGHT, branding)?;
    validate_pixel_buffer(&low_data, low_mask, LOW_WIDTH, LOW_HEIGHT, "low-resolution")?;
    validate_pixel_buffer(
        &high_data,
        high_mask,
        HIGH_WIDTH,
        HIGH_HEIGHT,
        "high-resolution",
    )?;
    let mut plan = donor.plan(
        &chain,
        [low_data, high_data],
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
    )?;
    if appended_ordinal_base != 0 || artwork.is_some() {
        plan.fingerprint()?;
    }
    Ok(plan)
}

/// The Lunar badge's chain, read and checked.
fn read_and_validate_donor(manager: &PackageManager) -> AuthoringResult<Chain> {
    lunar().read(manager)
}

fn decode_source() -> AuthoringResult<RgbaImage> {
    let image = image::load_from_memory_with_format(SOURCE_PNG, ImageFormat::Png)
        .map_err(|error| {
            invalid(format!(
                "Could not decode canonical Sunrise badge PNG: {error}"
            ))
        })?
        .into_rgba8();
    if image.width() != 1024 || image.height() != 1024 {
        return Err(invalid(format!(
            "Canonical Sunrise badge PNG is {}x{}; expected 1024x1024",
            image.width(),
            image.height()
        )));
    }
    Ok(image)
}

fn render_card(
    source: &RgbaImage,
    donor: &[u8],
    width: u32,
    height: u32,
    branding: crate::branding::Branding,
) -> AuthoringResult<Vec<u8>> {
    let expected_size = width as usize * height as usize * 4;
    if donor.len() != expected_size {
        return Err(invalid(format!(
            "Lunar badge donor pixel buffer is {} bytes. Expected {expected_size}",
            donor.len()
        )));
    }
    let geometry = card_geometry(source, width, height)?;
    let resized =
        image::imageops::resize(source, geometry.side, geometry.side, FilterType::Lanczos3);
    let mut card = RgbaImage::from_fn(width, height, |x, y| match branding {
        crate::branding::Branding::Dawn => crate::branding::dawn_background(y, height),
        crate::branding::Branding::Sunrise => card_background(x, y, width, height),
    });
    for (source_x, source_y, pixel) in resized.enumerate_pixels() {
        let target = card.get_pixel_mut(geometry.x + source_x, geometry.y + source_y);
        *target = composite_onto_opaque(*pixel, *target);
    }

    Ok(finish_card(card, donor, width, height))
}

fn render_artwork(
    artwork: Option<&crate::presentation::Artwork>,
    donor: &[u8],
    width: u32,
    height: u32,
    branding: crate::branding::Branding,
) -> AuthoringResult<Vec<u8>> {
    if let Some(artwork) = artwork.filter(|a| a.composition().is_some()) {
        if donor.len() != (width * height * 4) as usize {
            return Err(invalid("Badge mask has unexpected dimensions"));
        }
        return Ok(finish_card(
            artwork.render(width, height),
            donor,
            width,
            height,
        ));
    }
    let source = artwork.map_or_else(decode_source, |a| Ok(a.pixels().clone()))?;
    render_card(&source, donor, width, height, branding)
}

pub(crate) fn preview_mask(manager: &PackageManager) -> AuthoringResult<Vec<u8>> {
    let [_, high] = read_and_validate_donor(manager)?.data;
    Ok(high)
}

pub(crate) fn preview(
    artwork: Option<&crate::presentation::Artwork>,
    mask: Option<&[u8]>,
) -> AuthoringResult<RgbaImage> {
    preview_with_branding(artwork, mask, crate::branding::Branding::Sunrise)
}

pub(crate) fn preview_with_branding(
    artwork: Option<&crate::presentation::Artwork>,
    mask: Option<&[u8]>,
    branding: crate::branding::Branding,
) -> AuthoringResult<RgbaImage> {
    let fallback;
    let mask = if let Some(mask) = mask {
        mask
    } else {
        fallback = vec![255; HIGH_DATA_SIZE];
        &fallback
    };
    let pixels = render_artwork(artwork, mask, HIGH_WIDTH, HIGH_HEIGHT, branding)?;
    RgbaImage::from_raw(HIGH_WIDTH, HIGH_HEIGHT, pixels)
        .ok_or_else(|| invalid("Badge preview has unexpected dimensions"))
}

fn finish_card(mut card: RgbaImage, donor: &[u8], width: u32, height: u32) -> Vec<u8> {
    // Reproduce the Lunar card's shallow top bevel. Looking a few pixels inward isolates the
    // donor's edge highlight from its artwork, and following each column's first visible pixel
    // naturally carries that depth around both rounded upper corners.
    for x in 0..width {
        let Some(edge_y) = (0..height).find(|&y| donor_pixel(donor, width, x, y)[3] != 0) else {
            continue;
        };
        let reference_y = (edge_y + 3).min(height - 1);
        for y in edge_y..(edge_y + 2).min(height) {
            let edge = donor_pixel(donor, width, x, y);
            let reference = donor_pixel(donor, width, x, reference_y);
            let edge_luma = edge[..3].iter().map(|value| u32::from(*value)).sum::<u32>() / 3;
            let reference_luma = reference[..3]
                .iter()
                .map(|value| u32::from(*value))
                .sum::<u32>()
                / 3;
            let highlight = edge_luma.saturating_sub(reference_luma).min(64) as u8;
            let pixel = card.get_pixel_mut(x, y);
            for channel in 0..3 {
                pixel[channel] = pixel[channel].saturating_add(highlight);
            }
        }
    }

    // The stock alpha is the shape of the badge, including its antialiased rounded corners.
    for (index, pixel) in card.pixels_mut().enumerate() {
        pixel[3] = donor[index * 4 + 3];
    }
    card.into_raw()
}

fn donor_pixel(data: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
    let offset = ((y * width + x) * 4) as usize;
    &data[offset..offset + 4]
}

pub(crate) fn card_background(_x: u32, y: u32, _width: u32, height: u32) -> Rgba<u8> {
    const MIX_ONE: u32 = u16::MAX as u32;
    let last_y = height.saturating_sub(1).max(1);
    let vertical_mix = y.min(last_y) * MIX_ONE / last_y;
    let mut color = [0u8; 3];
    for channel in 0..3 {
        color[channel] = mix_channel(
            CARD_PURPLE_TOP[channel],
            CARD_PURPLE_BOTTOM[channel],
            vertical_mix,
        );
    }

    let glow_start = last_y * 7 / 10;
    let glow_range = last_y.saturating_sub(glow_start).max(1);
    let glow_progress = y.saturating_sub(glow_start).min(glow_range) * MIX_ONE / glow_range;
    let eased_glow =
        u32::try_from(u64::from(glow_progress) * u64::from(glow_progress) / u64::from(MIX_ONE))
            .expect("quadratic badge-gradient mix fits u32");
    let gold_mix = u32::try_from(
        u64::from(eased_glow) * u64::from(CARD_GOLD_GLOW_MAX_MIX) / u64::from(MIX_ONE),
    )
    .expect("gold badge-gradient mix fits u32");
    for channel in 0..3 {
        color[channel] = mix_channel(color[channel], CARD_GOLD_GLOW[channel], gold_mix);
    }
    Rgba([color[0], color[1], color[2], 255])
}

fn mix_channel(from: u8, to: u8, mix: u32) -> u8 {
    const MIX_ONE: u32 = u16::MAX as u32;
    let inverse = MIX_ONE - mix.min(MIX_ONE);
    ((u32::from(from) * inverse + u32::from(to) * mix.min(MIX_ONE) + MIX_ONE / 2) / MIX_ONE) as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CardGeometry {
    x: u32,
    y: u32,
    side: u32,
}

fn card_geometry(source: &RgbaImage, width: u32, height: u32) -> AuthoringResult<CardGeometry> {
    if source.width() != source.height() || width < height || height == 0 {
        return Err(invalid(
            "Sunrise badge card requires a nonempty square source and a landscape destination",
        ));
    }
    Ok(CardGeometry {
        x: (width - height) / 2,
        y: 0,
        side: height,
    })
}

fn composite_onto_opaque(foreground: Rgba<u8>, background: Rgba<u8>) -> Rgba<u8> {
    let alpha = u32::from(foreground[3]);
    let inverse = 255 - alpha;
    let mut output = [0u8; 4];
    for channel in 0..3 {
        output[channel] = ((u32::from(foreground[channel]) * alpha
            + u32::from(background[channel]) * inverse
            + 127)
            / 255) as u8;
    }
    output[3] = 255;
    Rgba(output)
}

fn validate_pixel_buffer(
    data: &[u8],
    donor: &[u8],
    width: u32,
    height: u32,
    lane: &str,
) -> AuthoringResult<()> {
    let expected = width as usize * height as usize * 4;
    if data.len() != expected || donor.len() != expected {
        return Err(validation(format!(
            "Rendered {lane} Sunrise icon or its donor has an unexpected pixel-buffer size"
        )));
    }
    if data
        .chunks_exact(4)
        .zip(donor.chunks_exact(4))
        .any(|(pixel, donor_pixel)| pixel[3] != donor_pixel[3])
    {
        return Err(validation(format!(
            "Rendered {lane} Sunrise icon did not preserve the Lunar donor alpha mask"
        )));
    }
    let contains_source_pixels = data.chunks_exact(4).enumerate().any(|(index, pixel)| {
        let x = index as u32 % width;
        let y = index as u32 / width;
        pixel[3] != 0 && pixel[..3] != card_background(x, y, width, height).0[..3]
    });
    if !contains_source_pixels {
        return Err(validation(format!(
            "Rendered {lane} Sunrise icon contains no visible source pixels"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
