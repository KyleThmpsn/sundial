//! A map's own picture in the private match list. The list draws each map with the image its
//! private match display names by row of the item icon table (investment globals slot 75). The
//! row names an icon container whose layer holds two straight RGBA8 textures, 648 by 857 and 510
//! by 287 for Distant Shore, with the picture the screen shows. A map with a Reach `source` takes
//! a copy of its donor's chain holding the level's picture from MCC
//! ([`parhelion_import::halo_reach::level::picture`]), cropped to each texture's shape about its
//! middle and resized.
use image::{RgbaImage, imageops::FilterType};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::icon_schema::ICON_PRIMARY_LAYER_OFFSET;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::invalid,
    icon_chain::{Donor, Texture},
    shared_tag_memory::read_and_validate_icon_companion,
    tag_payload::{array_at, read_tag, read_u16, read_u32},
};

use super::{MapSource, Picture};

/// The item icon table's rows at `+0x08`: 24 bytes, the container at `+0x10`.
const ICON_ROWS: usize = 0x08;
const ICON_ROW_SIZE: usize = 0x18;
const ICON_ROW_CLASS: u32 = 0x8080_2957;
const ICON_ROW_CONTAINER: usize = 0x10;
/// A private match picture's layer: one lane array at `+0x50` holding both texture headers, at
/// `+0x80` and `+0x84`, behind these words.
const LAYER_SIZE: usize = 0x88;
const LAYER_TEXTURES: [usize; 2] = [0x80, 0x84];
const LAYER_WORDS: [(usize, u32); 13] = [
    (0x00, LAYER_SIZE as u32),
    (0x10, 0x10),
    (0x1C, 0x8080_4A67),
    (0x20, 1),
    (0x28, 0x18),
    (0x3C, 0x8080_9FBD),
    (0x40, 1),
    (0x48, 0x8080_4A6C),
    (0x50, 2),
    (0x58, 0x18),
    (0x6C, 0x8080_9FBD),
    (0x70, 2),
    (0x78, 0x8080_4A6F),
];
/// A straight RGBA8 texture header's width and height, `u16` each.
const HEADER_WIDTH: usize = 0x0E;
const HEADER_HEIGHT: usize = 0x10;

/// The picture of map `map`, named `name`, whose donor's private match display names
/// `donor_row` of the item icon table `icons`, or `None` when its source level has no picture.
pub(super) fn load(
    manager: &PackageManager,
    icons: &[u8],
    donor_row: u32,
    map: usize,
    name: &str,
    source: &MapSource,
) -> AuthoringResult<Option<Picture>> {
    let Some(level) = parhelion_import::halo_reach::level::picture::read(&source.maps, &source.map)
        .map_err(|error| invalid(format!("Map {name} picture: {error:#}")))?
    else {
        return Ok(None);
    };
    let (count, _, rows, class) = array_at(icons, ICON_ROWS)?;
    let donor_row = u16::try_from(donor_row)
        .ok()
        .filter(|&row| usize::from(row) < count && class == ICON_ROW_CLASS)
        .ok_or_else(|| {
            invalid(format!(
                "Map {name}'s donor picture row {donor_row} is not an item icon row"
            ))
        })?;
    let container = TagHash(read_u32(
        icons,
        rows + usize::from(donor_row) * ICON_ROW_SIZE + ICON_ROW_CONTAINER,
    )?);
    let layer = TagHash(read_u32(
        &read_tag(manager, container, "private match picture")?,
        ICON_PRIMARY_LAYER_OFFSET,
    )?);
    let layer_payload = read_tag(manager, layer, "private match picture layer")?;
    let textures = LAYER_TEXTURES
        .map(|field| texture(manager, &layer_payload, field))
        .into_iter()
        .collect::<AuthoringResult<Vec<_>>>()?;
    let donor = Donor {
        label: format!("Map {name} picture"),
        textures: [textures[0], textures[1]],
        layer,
        layer_size: LAYER_SIZE,
        layer_words: LAYER_WORDS.to_vec(),
        container,
        companion: read_and_validate_icon_companion(manager, container)?.tag,
    };
    let source = RgbaImage::from_raw(level.width, level.height, level.rgba)
        .ok_or_else(|| invalid(format!("Map {name}'s picture {} has no pixels", level.name)))?;
    let pixels = donor
        .textures
        .map(|texture| fit(&source, texture.width, texture.height));
    Ok(Some(Picture {
        map,
        hash: crate::presentation::text_hash(name, "private-match-picture"),
        donor_row,
        donor,
        pixels,
    }))
}

/// The texture whose header the layer names at `field`, with its size from the header and the
/// data entry the header names.
fn texture(manager: &PackageManager, layer: &[u8], field: usize) -> AuthoringResult<Texture> {
    let header = TagHash(read_u32(layer, field)?);
    let header_payload = read_tag(manager, header, "private match picture texture")?;
    let data = manager
        .get_entry(header)
        .map(|entry| TagHash(entry.reference))
        .ok_or_else(|| invalid(format!("Picture texture {header} has no entry")))?;
    Ok(Texture {
        data,
        header,
        width: u32::from(read_u16(&header_payload, HEADER_WIDTH)?),
        height: u32::from(read_u16(&header_payload, HEADER_HEIGHT)?),
        layer_field: field,
    })
}

/// `source` cropped about its middle to the shape of `width` by `height`, resized to it, and
/// made opaque.
fn fit(source: &RgbaImage, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (source.width(), source.height());
    // Keep the whole height when the source is wider than the target, else the whole width.
    let (crop_w, crop_h) = if u64::from(w) * u64::from(height) > u64::from(h) * u64::from(width) {
        (
            (u64::from(h) * u64::from(width) / u64::from(height)).max(1) as u32,
            h,
        )
    } else {
        (
            w,
            (u64::from(w) * u64::from(height) / u64::from(width)).max(1) as u32,
        )
    };
    let cropped =
        image::imageops::crop_imm(source, (w - crop_w) / 2, (h - crop_h) / 2, crop_w, crop_h)
            .to_image();
    let mut fitted = image::imageops::resize(&cropped, width, height, FilterType::Lanczos3);
    for pixel in fitted.pixels_mut() {
        pixel[3] = 255;
    }
    fitted.into_raw()
}
