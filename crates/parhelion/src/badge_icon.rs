//! Authoring support for the Project Sunrise collection-badge icon.
//!
//! The source mark is Solus's artwork in `assets/parhelion`. This module only
//! performs deterministic raster composition and clones the Lunar badge's seven-tag icon chain;
//! it does not install packages or mutate an existing tag.

use image::{ImageFormat, Rgba, RgbaImage, imageops::FilterType};
use sundial::package_authoring::icon_schema::{ICON_DEFINITION_SIZE, ICON_PRIMARY_LAYER_OFFSET};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    AuthoringResult, NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    appended_tags::AppendedTagAllocator,
    error::{input as invalid, validation},
    payload_guards::{
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE, donor_mutation_is_limited_to,
        is_stock_straight_rgba8_texture_header,
    },
    shared_tag_memory::{
        IconDefinitionCompanion, build_icon_companion_payload, dependency_set,
        read_and_validate_icon_companion,
    },
    tag_payload::{read_u32, write_u32},
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
const LOW_DATA_SIZE: usize = LOW_WIDTH as usize * LOW_HEIGHT as usize * 4;
const HIGH_DATA_SIZE: usize = HIGH_WIDTH as usize * HIGH_HEIGHT as usize * 4;
const LAYER_SIZE: usize = 180;
const CARD_PURPLE_TOP: [u8; 3] = [32, 18, 49];
const CARD_PURPLE_BOTTOM: [u8; 3] = [54, 30, 72];
const CARD_GOLD_GLOW: [u8; 3] = [221, 172, 74];
const CARD_GOLD_GLOW_MAX_MIX: u32 = 5_243;

const LOW_HEADER_TAG_OFFSET: usize = 0x90;
const HIGH_HEADER_TAG_OFFSET: usize = 0xB0;
const TAGS_PER_ICON: usize = 7;

/// Absolute ordinals in the complete appended-tag slice passed to the extended-overlay writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BadgeIconOrdinals {
    pub low_data: usize,
    pub low_header: usize,
    pub high_data: usize,
    pub high_header: usize,
    pub layer: usize,
    pub container: usize,
    pub companion: usize,
}

/// Dynamically assigned destination-package tags for the seven emitted icon resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BadgeIconTags {
    pub low_data: TagHash,
    pub low_header: TagHash,
    pub high_data: TagHash,
    pub high_header: TagHash,
    pub layer: TagHash,
    pub container: TagHash,
    pub companion: TagHash,
}

/// A complete append plan for one self-contained Sunrise badge icon chain.
///
/// `new_tags` is always ordered low data/header, high data/header, layer, then an adjacent
/// icon-definition/companion pair. The reference overrides use absolute appended ordinals, so
/// the two texture data/header pairs remain reciprocal after this plan is inserted after other
/// authored tags.
#[derive(Clone, Debug)]
pub struct BadgeIconPlan {
    pub new_tags: Vec<NewTagSpec>,
    pub reference_overrides: Vec<NewTagReferenceOverride>,
    #[cfg(test)]
    pub ordinals: BadgeIconOrdinals,
    #[cfg(test)]
    pub tags: BadgeIconTags,
    pub container_tag: TagHash,
}

/// Builds a seven-tag texture chain using the configured Shadowkeep package manager for donor
/// reads and Solus's bundled Sunrise artwork for the badge mark.
///
/// `current_entry_count` is the destination package's entry count before any append operation.
/// `appended_ordinal_base` is the number of tags the caller will place before this plan in the
/// same `new_tags` slice. No assigned destination tag is hardcoded.
pub fn build_badge_icon_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
) -> AuthoringResult<BadgeIconPlan> {
    let donor = read_and_validate_donor(manager)?;
    let source = decode_source()?;
    let low_data = render_card(&source, &donor.low_data, LOW_WIDTH, LOW_HEIGHT)?;
    let high_data = render_card(&source, &donor.high_data, HIGH_WIDTH, HIGH_HEIGHT)?;
    validate_pixel_buffer(
        &low_data,
        &donor.low_data,
        LOW_WIDTH,
        LOW_HEIGHT,
        "low-resolution",
    )?;
    validate_pixel_buffer(
        &high_data,
        &donor.high_data,
        HIGH_WIDTH,
        HIGH_HEIGHT,
        "high-resolution",
    )?;

    let ordinals = ordinals(appended_ordinal_base)?;
    let tags = assigned_tags(destination_package_id, current_entry_count, ordinals)?;

    let mut layer = donor.layer.clone();
    write_tag(&mut layer, LOW_HEADER_TAG_OFFSET, tags.low_header)?;
    write_tag(&mut layer, HIGH_HEADER_TAG_OFFSET, tags.high_header)?;
    let mut container = donor.container.clone();
    write_tag(&mut container, ICON_PRIMARY_LAYER_OFFSET, tags.layer)?;
    let dependencies = dependency_set([
        tags.low_data,
        tags.low_header,
        tags.high_data,
        tags.high_header,
        tags.layer,
        tags.container,
        tags.companion,
    ]);
    let companion = build_icon_companion_payload(
        &donor.companion.template_payload,
        tags.companion,
        tags.container,
        &dependencies,
    )?;

    validate_emitted_chain(
        &donor.low_header,
        &donor.high_header,
        &donor.layer,
        &layer,
        &donor.container,
        &container,
        tags,
    )?;

    let new_tags = vec![
        NewTagSpec {
            template_tag: DONOR_LOW_DATA,
            payload: low_data,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_LOW_HEADER,
            payload: donor.low_header,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_HIGH_DATA,
            payload: high_data,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_HIGH_HEADER,
            payload: donor.high_header,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_LAYER,
            payload: layer,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_CONTAINER,
            payload: container,
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: DONOR_COMPANION,
            payload: companion,
            storage: NewTagStorageMode::InheritTemplate,
        },
    ];
    let reference_overrides = reciprocal_reference_overrides(ordinals);
    validate_append_plan(&new_tags, &reference_overrides, ordinals, tags)?;

    Ok(BadgeIconPlan {
        new_tags,
        reference_overrides,
        #[cfg(test)]
        ordinals,
        #[cfg(test)]
        tags,
        container_tag: tags.container,
    })
}

struct DonorChain {
    low_data: Vec<u8>,
    low_header: Vec<u8>,
    high_data: Vec<u8>,
    high_header: Vec<u8>,
    layer: Vec<u8>,
    container: Vec<u8>,
    companion: IconDefinitionCompanion,
}

fn read_and_validate_donor(manager: &PackageManager) -> AuthoringResult<DonorChain> {
    let low_data = read_donor_tag(manager, DONOR_LOW_DATA, LOW_DATA_SIZE, 0x28, 0x01)?;
    let low_header = read_donor_tag(
        manager,
        DONOR_LOW_HEADER,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
    )?;
    let high_data = read_donor_tag(manager, DONOR_HIGH_DATA, HIGH_DATA_SIZE, 0x28, 0x01)?;
    let high_header = read_donor_tag(
        manager,
        DONOR_HIGH_HEADER,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
    )?;
    let layer = read_donor_tag(manager, DONOR_LAYER, LAYER_SIZE, 0x08, 0x00)?;
    let container = read_donor_tag(manager, DONOR_CONTAINER, ICON_DEFINITION_SIZE, 0x10, 0x00)?;
    let companion = read_and_validate_icon_companion(manager, DONOR_CONTAINER)?;

    validate_entry_reference(manager, DONOR_LOW_DATA, DONOR_LOW_HEADER)?;
    validate_entry_reference(manager, DONOR_LOW_HEADER, DONOR_LOW_DATA)?;
    validate_entry_reference(manager, DONOR_HIGH_DATA, DONOR_HIGH_HEADER)?;
    validate_entry_reference(manager, DONOR_HIGH_HEADER, DONOR_HIGH_DATA)?;
    validate_entry_reference(manager, DONOR_LAYER, TagHash(0x8080_4A69))?;
    validate_entry_reference(manager, DONOR_CONTAINER, TagHash(0x8080_4A53))?;
    validate_texture_header(&low_header, LOW_WIDTH, LOW_HEIGHT, LOW_DATA_SIZE, "low")?;
    validate_texture_header(
        &high_header,
        HIGH_WIDTH,
        HIGH_HEIGHT,
        HIGH_DATA_SIZE,
        "high",
    )?;
    validate_layer(&layer)?;
    validate_container(&container)?;
    let expected_dependencies = dependency_set([
        DONOR_LOW_DATA,
        DONOR_LOW_HEADER,
        DONOR_HIGH_DATA,
        DONOR_HIGH_HEADER,
        DONOR_LAYER,
        DONOR_CONTAINER,
        DONOR_COMPANION,
    ]);
    if companion.tag != DONOR_COMPANION || companion.dependencies != expected_dependencies {
        return Err(invalid(format!(
            "Lunar badge icon companion {} no longer contains exactly its seven-tag stock chain",
            companion.tag
        )));
    }

    // Reading both pixel tags is intentional: it validates that the donor chain is complete and
    // its declared straight-RGBA payload sizes still agree with the headers before we clone it.
    if low_data.len() != LOW_DATA_SIZE || high_data.len() != HIGH_DATA_SIZE {
        return Err(validation("Lunar icon donor pixel payload sizes changed"));
    }

    Ok(DonorChain {
        low_data,
        low_header,
        high_data,
        high_header,
        layer,
        container,
        companion,
    })
}

fn read_donor_tag(
    manager: &PackageManager,
    tag: TagHash,
    expected_size: usize,
    expected_type: u8,
    expected_subtype: u8,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("Lunar icon donor tag {tag} has no package entry")))?;
    if entry.file_type != expected_type || entry.file_subtype != expected_subtype {
        return Err(invalid(format!(
            "Lunar icon donor tag {tag} has type {:02X}/{:02X}; expected {expected_type:02X}/{expected_subtype:02X}",
            entry.file_type, entry.file_subtype
        )));
    }
    let payload = manager.read_tag(tag).map_err(|error| {
        invalid(format!(
            "Could not read Lunar icon donor tag {tag}: {error}"
        ))
    })?;
    if payload.len() != expected_size || entry.file_size as usize != expected_size {
        return Err(invalid(format!(
            "Lunar icon donor tag {tag} is {} bytes (entry declares {}); expected {expected_size}",
            payload.len(),
            entry.file_size
        )));
    }
    Ok(payload)
}

fn validate_entry_reference(
    manager: &PackageManager,
    tag: TagHash,
    expected: TagHash,
) -> AuthoringResult<()> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("Lunar icon donor tag {tag} has no package entry")))?;
    if entry.reference != u32::from(expected) {
        return Err(invalid(format!(
            "Lunar icon donor tag {tag} references {:08X}; expected {expected}",
            entry.reference
        )));
    }
    Ok(())
}

fn validate_texture_header(
    header: &[u8],
    width: u32,
    height: u32,
    data_size: usize,
    lane: &str,
) -> AuthoringResult<()> {
    if !is_stock_straight_rgba8_texture_header(header, width, height, data_size) {
        return Err(invalid(format!(
            "Lunar {lane}-resolution icon header no longer describes the expected straight-RGBA texture"
        )));
    }
    Ok(())
}

fn validate_layer(layer: &[u8]) -> AuthoringResult<()> {
    let structure = [
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
        (LOW_HEADER_TAG_OFFSET, u32::from(DONOR_LOW_HEADER)),
        (0x9C, 0x8080_9FBD),
        (0xA0, 1),
        (0xA8, 0x8080_4A6F),
        (HIGH_HEADER_TAG_OFFSET, u32::from(DONOR_HIGH_HEADER)),
    ];
    if layer.len() != LAYER_SIZE
        || structure
            .into_iter()
            .any(|(offset, value)| read_u32(layer, offset).ok() != Some(value))
    {
        return Err(invalid(
            "Lunar icon donor layer no longer has exactly two expected texture lanes",
        ));
    }
    Ok(())
}

fn validate_container(container: &[u8]) -> AuthoringResult<()> {
    if container.len() != ICON_DEFINITION_SIZE
        || read_u32(container, 0)? != ICON_DEFINITION_SIZE as u32
        || read_u32(container, ICON_PRIMARY_LAYER_OFFSET)? != u32::from(DONOR_LAYER)
        || (0x18..=0x28)
            .step_by(4)
            .any(|offset| read_u32(container, offset).ok() != Some(u32::MAX))
    {
        return Err(invalid(
            "Lunar icon donor container no longer has one required layer and no optional layers",
        ));
    }
    Ok(())
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
) -> AuthoringResult<Vec<u8>> {
    let expected_size = width as usize * height as usize * 4;
    if donor.len() != expected_size {
        return Err(invalid(format!(
            "Lunar badge donor pixel buffer is {} bytes; expected {expected_size}",
            donor.len()
        )));
    }
    let geometry = card_geometry(source, width, height)?;
    let resized =
        image::imageops::resize(source, geometry.side, geometry.side, FilterType::Lanczos3);
    let mut card = RgbaImage::from_fn(width, height, |x, y| card_background(x, y, width, height));
    for (source_x, source_y, pixel) in resized.enumerate_pixels() {
        let target = card.get_pixel_mut(geometry.x + source_x, geometry.y + source_y);
        *target = composite_onto_opaque(*pixel, *target);
    }

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
    Ok(card.into_raw())
}

fn donor_pixel(data: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
    let offset = ((y * width + x) * 4) as usize;
    &data[offset..offset + 4]
}

fn card_background(_x: u32, y: u32, _width: u32, height: u32) -> Rgba<u8> {
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

fn ordinals(base: usize) -> AuthoringResult<BadgeIconOrdinals> {
    let ordinal =
        |local: usize| AppendedTagAllocator::checked_ordinal(base, local, "Sunrise badge");
    Ok(BadgeIconOrdinals {
        low_data: ordinal(0)?,
        low_header: ordinal(1)?,
        high_data: ordinal(2)?,
        high_header: ordinal(3)?,
        layer: ordinal(4)?,
        container: ordinal(5)?,
        companion: ordinal(6)?,
    })
}

fn assigned_tags(
    package_id: u16,
    current_entry_count: usize,
    ordinals: BadgeIconOrdinals,
) -> AuthoringResult<BadgeIconTags> {
    let allocator = AppendedTagAllocator::new(package_id, current_entry_count);
    let assigned = |ordinal: usize| {
        allocator.assigned_tag(
            ordinal,
            "Sunrise badge destination entry",
            "Sunrise badge tag",
        )
    };
    Ok(BadgeIconTags {
        low_data: assigned(ordinals.low_data)?,
        low_header: assigned(ordinals.low_header)?,
        high_data: assigned(ordinals.high_data)?,
        high_header: assigned(ordinals.high_header)?,
        layer: assigned(ordinals.layer)?,
        container: assigned(ordinals.container)?,
        companion: assigned(ordinals.companion)?,
    })
}

fn reciprocal_reference_overrides(ordinals: BadgeIconOrdinals) -> Vec<NewTagReferenceOverride> {
    vec![
        NewTagReferenceOverride {
            new_tag_ordinal: ordinals.low_data,
            reference: NewTagReference::Appended(ordinals.low_header),
        },
        NewTagReferenceOverride {
            new_tag_ordinal: ordinals.low_header,
            reference: NewTagReference::Appended(ordinals.low_data),
        },
        NewTagReferenceOverride {
            new_tag_ordinal: ordinals.high_data,
            reference: NewTagReference::Appended(ordinals.high_header),
        },
        NewTagReferenceOverride {
            new_tag_ordinal: ordinals.high_header,
            reference: NewTagReference::Appended(ordinals.high_data),
        },
    ]
}

fn validate_emitted_chain(
    low_header: &[u8],
    high_header: &[u8],
    donor_layer: &[u8],
    layer: &[u8],
    donor_container: &[u8],
    container: &[u8],
    tags: BadgeIconTags,
) -> AuthoringResult<()> {
    if low_header.len() != STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE
        || high_header.len() != STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE
        || layer.len() != LAYER_SIZE
        || container.len() != ICON_DEFINITION_SIZE
        || read_u32(layer, LOW_HEADER_TAG_OFFSET)? != u32::from(tags.low_header)
        || read_u32(layer, HIGH_HEADER_TAG_OFFSET)? != u32::from(tags.high_header)
        || read_u32(container, ICON_PRIMARY_LAYER_OFFSET)? != u32::from(tags.layer)
    {
        return Err(validation(
            "Emitted Sunrise badge texture chain failed its payload-link validation",
        ));
    }
    validate_only_patched_ranges(
        donor_layer,
        layer,
        &[
            LOW_HEADER_TAG_OFFSET..LOW_HEADER_TAG_OFFSET + 4,
            HIGH_HEADER_TAG_OFFSET..HIGH_HEADER_TAG_OFFSET + 4,
        ],
        "layer",
    )?;
    let container_layer_tag_range = ICON_PRIMARY_LAYER_OFFSET..ICON_PRIMARY_LAYER_OFFSET + 4;
    validate_only_patched_ranges(
        donor_container,
        container,
        std::slice::from_ref(&container_layer_tag_range),
        "container",
    )?;
    Ok(())
}

fn validate_only_patched_ranges(
    donor: &[u8],
    emitted: &[u8],
    allowed: &[std::ops::Range<usize>],
    resource: &str,
) -> AuthoringResult<()> {
    if !donor_mutation_is_limited_to(donor, emitted, allowed) {
        return Err(validation(format!(
            "Emitted Sunrise badge {resource} changed donor bytes outside its tag-link fields"
        )));
    }
    Ok(())
}

fn validate_append_plan(
    new_tags: &[NewTagSpec],
    overrides: &[NewTagReferenceOverride],
    ordinals: BadgeIconOrdinals,
    tags: BadgeIconTags,
) -> AuthoringResult<()> {
    let sizes = [
        LOW_DATA_SIZE,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        HIGH_DATA_SIZE,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        LAYER_SIZE,
        ICON_DEFINITION_SIZE,
    ];
    let templates = [
        DONOR_LOW_DATA,
        DONOR_LOW_HEADER,
        DONOR_HIGH_DATA,
        DONOR_HIGH_HEADER,
        DONOR_LAYER,
        DONOR_CONTAINER,
    ];
    if new_tags.len() != TAGS_PER_ICON
        || new_tags[..TAGS_PER_ICON - 1]
            .iter()
            .zip(sizes)
            .zip(templates)
            .any(|((spec, size), template)| {
                spec.payload.len() != size
                    || spec.template_tag != template
                    || spec.storage != NewTagStorageMode::InheritTemplate
            })
        || new_tags[TAGS_PER_ICON - 1].template_tag != DONOR_COMPANION
        || new_tags[TAGS_PER_ICON - 1].storage != NewTagStorageMode::InheritTemplate
        || read_u32(&new_tags[TAGS_PER_ICON - 1].payload, 0)? as usize
            != new_tags[TAGS_PER_ICON - 1].payload.len()
        || read_u32(&new_tags[TAGS_PER_ICON - 1].payload, 0x08)? != u32::from(tags.companion)
        || read_u32(&new_tags[TAGS_PER_ICON - 1].payload, 0x0C)? != u32::from(tags.container)
    {
        return Err(validation(
            "Sunrise badge append plan changed tag order, templates, or payload sizes",
        ));
    }
    let expected = reciprocal_reference_overrides(ordinals);
    if overrides != expected
        || ordinals.companion != ordinals.container + 1
        || tags.companion.pkg_id() != tags.container.pkg_id()
        || tags.companion.entry_index() != tags.container.entry_index().saturating_add(1)
        || (tags.container.entry_index() as usize) < ordinals.container
    {
        return Err(validation(
            "Sunrise badge append plan has an invalid reciprocal-reference layout",
        ));
    }
    Ok(())
}

fn write_tag(bytes: &mut [u8], offset: usize, tag: TagHash) -> AuthoringResult<()> {
    write_u32(bytes, offset, u32::from(tag))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tiger_pkg::{DestinyVersion, GameVersion};

    use super::*;

    #[test]
    fn canonical_source_and_card_geometry_preserve_the_complete_square() {
        let source = decode_source().expect("canonical source should decode");
        assert_eq!(source.dimensions(), (1024, 1024));
        assert_eq!(
            card_geometry(&source, LOW_WIDTH, LOW_HEIGHT).expect("low card geometry"),
            CardGeometry {
                x: 41,
                y: 0,
                side: 126,
            }
        );
        assert_eq!(
            card_geometry(&source, HIGH_WIDTH, HIGH_HEIGHT).expect("high card geometry"),
            CardGeometry {
                x: 86,
                y: 0,
                side: 268,
            }
        );
    }

    #[test]
    fn renderer_preserves_an_opaque_donor_mask_and_untouched_gradient_side_margins() {
        let source = decode_source().expect("canonical source should decode");
        for (width, height, expected_size, left_margin) in [
            (LOW_WIDTH, LOW_HEIGHT, LOW_DATA_SIZE, 41usize),
            (HIGH_WIDTH, HIGH_HEIGHT, HIGH_DATA_SIZE, 86usize),
        ] {
            let donor = vec![255; expected_size];
            let data = render_card(&source, &donor, width, height).expect("card should render");
            assert_eq!(data.len(), expected_size);
            assert!(data.chunks_exact(4).all(|pixel| pixel[3] == 255));
            assert!(data.chunks_exact(4).enumerate().any(|(index, pixel)| {
                let x = index as u32 % width;
                let y = index as u32 / width;
                pixel != card_background(x, y, width, height).0
            }));
            for y in 0..height as usize {
                let row = &data[y * width as usize * 4..(y + 1) * width as usize * 4];
                let background = card_background(0, y as u32, width, height).0;
                assert!(
                    row[..left_margin * 4]
                        .chunks_exact(4)
                        .all(|pixel| pixel == background)
                );
                assert!(
                    row[(width as usize - left_margin) * 4..]
                        .chunks_exact(4)
                        .all(|pixel| pixel == background)
                );
            }
        }
    }

    #[test]
    fn renderer_copies_rounded_alpha_and_wraps_highlight_around_top_edge() {
        let source = decode_source().expect("canonical source should decode");
        let width = 12;
        let height = 8;
        let mut donor = vec![0; width as usize * height as usize * 4];
        for x in 0..width {
            let edge_y = if matches!(x, 0 | 11) {
                2
            } else if matches!(x, 1 | 10) {
                1
            } else {
                0
            };
            for y in edge_y..height {
                let offset = ((y * width + x) * 4) as usize;
                donor[offset..offset + 3].fill(if y < edge_y + 2 { 80 } else { 20 });
                donor[offset + 3] = if y == edge_y && matches!(x, 0 | 11) {
                    128
                } else {
                    255
                };
            }
        }

        let data = render_card(&source, &donor, width, height).expect("card should render");
        assert!(
            data.chunks_exact(4)
                .zip(donor.chunks_exact(4))
                .all(|(pixel, donor_pixel)| pixel[3] == donor_pixel[3])
        );
        for &(x, y) in &[(0, 2), (1, 1), (10, 1), (11, 2)] {
            let pixel = donor_pixel(&data, width, x, y);
            let plain = card_background(x, y, width, height);
            assert!(pixel[0] > plain[0]);
            assert!(pixel[1] > plain[1]);
            assert!(pixel[2] > plain[2]);
        }
    }

    #[test]
    fn card_background_stays_purple_with_only_a_faint_warm_lower_edge() {
        let top = card_background(0, 0, HIGH_WIDTH, HIGH_HEIGHT);
        let bottom = card_background(0, HIGH_HEIGHT - 1, HIGH_WIDTH, HIGH_HEIGHT);
        assert_eq!(&top.0[..3], &CARD_PURPLE_TOP);
        assert!(bottom[0] > CARD_PURPLE_BOTTOM[0]);
        assert!(bottom[1] > CARD_PURPLE_BOTTOM[1]);
        assert!(bottom[2] >= CARD_PURPLE_BOTTOM[2]);
        assert!(bottom[0] - CARD_PURPLE_BOTTOM[0] <= 16);
        assert!(bottom[1] - CARD_PURPLE_BOTTOM[1] <= 16);
        assert!(bottom[2] > bottom[0]);
    }

    #[test]
    fn alpha_compositing_is_straight_and_fully_opaque() {
        let background = card_background(0, 0, LOW_WIDTH, LOW_HEIGHT);
        assert_eq!(
            composite_onto_opaque(Rgba([200, 100, 50, 0]), background),
            background
        );
        assert_eq!(
            composite_onto_opaque(Rgba([200, 100, 50, 255]), background),
            Rgba([200, 100, 50, 255])
        );
        assert_eq!(
            composite_onto_opaque(Rgba([200, 100, 50, 128]), Rgba([0, 0, 0, 255])),
            Rgba([100, 50, 25, 255])
        );
    }

    #[test]
    fn rejects_unencodable_destination_indices() {
        let ordinals = ordinals(2).expect("ordinals should be assigned");
        let error = assigned_tags(0x0197, 8184, ordinals)
            .expect_err("seven icon tags cannot overflow the package entry table");
        assert!(error.to_string().contains("package-table limit"));
    }

    fn assert_real_plan_shape(
        plan: &BadgeIconPlan,
        donor_low_header: &[u8],
        donor_high_header: &[u8],
    ) {
        assert_eq!(plan.new_tags.len(), TAGS_PER_ICON);
        assert_eq!(plan.ordinals.low_data, 2);
        assert_eq!(plan.ordinals.container, 7);
        assert_eq!(plan.ordinals.companion, 8);
        assert_eq!(plan.container_tag, TagHash::new(0x0197, 1007));
        assert_eq!(plan.tags.companion, TagHash::new(0x0197, 1008));
        assert_eq!(plan.new_tags[1].payload, donor_low_header);
        assert_eq!(plan.new_tags[3].payload, donor_high_header);
        assert_eq!(plan.new_tags[6].template_tag, DONOR_COMPANION);
        assert!(
            plan.new_tags
                .iter()
                .all(|tag| tag.storage == NewTagStorageMode::InheritTemplate)
        );
    }

    fn assert_real_plan_links(plan: &BadgeIconPlan, donor_layer: &[u8], donor_container: &[u8]) {
        validate_only_patched_ranges(
            donor_layer,
            &plan.new_tags[4].payload,
            &[
                LOW_HEADER_TAG_OFFSET..LOW_HEADER_TAG_OFFSET + 4,
                HIGH_HEADER_TAG_OFFSET..HIGH_HEADER_TAG_OFFSET + 4,
            ],
            "test layer",
        )
        .expect("only the layer's two host tags should change");
        let container_layer_tag_range = ICON_PRIMARY_LAYER_OFFSET..ICON_PRIMARY_LAYER_OFFSET + 4;
        validate_only_patched_ranges(
            donor_container,
            &plan.new_tags[5].payload,
            std::slice::from_ref(&container_layer_tag_range),
            "test container",
        )
        .expect("only the container's layer host tag should change");
        assert_eq!(
            read_u32(&plan.new_tags[4].payload, LOW_HEADER_TAG_OFFSET)
                .expect("low layer link should decode"),
            u32::from(TagHash::new(0x0197, 1003))
        );
        assert_eq!(
            read_u32(&plan.new_tags[4].payload, HIGH_HEADER_TAG_OFFSET)
                .expect("high layer link should decode"),
            u32::from(TagHash::new(0x0197, 1005))
        );
        assert_eq!(
            read_u32(&plan.new_tags[5].payload, ICON_PRIMARY_LAYER_OFFSET)
                .expect("container layer link should decode"),
            u32::from(TagHash::new(0x0197, 1006))
        );
        assert_eq!(
            read_u32(&plan.new_tags[6].payload, 0x08).expect("companion self tag should decode"),
            u32::from(TagHash::new(0x0197, 1008))
        );
        assert_eq!(
            read_u32(&plan.new_tags[6].payload, 0x0C).expect("companion owner tag should decode"),
            u32::from(TagHash::new(0x0197, 1007))
        );
        assert_eq!(
            crate::shared_tag_memory::validate_icon_companion_payload(
                &plan.new_tags[6].payload,
                plan.tags.companion,
                plan.tags.container,
            )
            .expect("authored badge companion should be canonical"),
            dependency_set((1002..=1008).map(|index| TagHash::new(0x0197, index)))
        );
        assert_eq!(
            plan.reference_overrides,
            reciprocal_reference_overrides(plan.ordinals)
        );
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
    fn real_package_donor_chain_round_trips_when_configured() {
        let package_directory = std::env::var_os("SUNDIAL_TEST_PACKAGES")
            .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
        let manager = PackageManager::new(
            Path::new(&package_directory),
            GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
            None,
        )
        .expect("configured Shadowkeep packages should open");
        let plan = build_badge_icon_plan(&manager, 0x0197, 1000, 2)
            .expect("real Lunar icon chain should author");
        let donor_low_header = manager
            .read_tag(DONOR_LOW_HEADER)
            .expect("low donor header should read");
        let donor_high_header = manager
            .read_tag(DONOR_HIGH_HEADER)
            .expect("high donor header should read");
        let donor_layer = manager
            .read_tag(DONOR_LAYER)
            .expect("donor layer should read");
        let donor_container = manager
            .read_tag(DONOR_CONTAINER)
            .expect("donor container should read");
        assert_real_plan_shape(&plan, &donor_low_header, &donor_high_header);
        assert_real_plan_links(&plan, &donor_layer, &donor_container);
    }
}
