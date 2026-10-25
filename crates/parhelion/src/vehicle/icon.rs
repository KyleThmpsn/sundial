//! A summoned vehicle's HUD silhouette as an authored Sparrow's inventory art. The silhouette is
//! the ammunition HUD icon the vehicle's weapon content names, white on transparency. It takes the
//! place of the icon's primary image, under the rarity plate's watermark, as an imported image
//! does. The build and the page both compose the icon through
//! [`WeaponIconEdit::with_art`](crate::WeaponIconEdit::with_art).
use super::{Sparrow, Summon};
use crate::WeaponIconEdit;
use crate::icon_edit::ImportedIcon;
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

/// The icon's edge, and the margin the silhouette keeps from it.
const EDGE: u32 = 96;
const MARGIN: u32 = 4;

/// The vehicle whose silhouette an item's icon shows: one it summons in place of the Sparrow, with
/// Vehicle Icon on, when the icon has no image of its own.
pub(crate) fn shown<'a>(sparrow: Option<&'a Sparrow>, icon: &WeaponIconEdit) -> Option<&'a Summon> {
    sparrow
        .filter(|sparrow| sparrow.vehicle_icon && sparrow.summon != Summon::Sparrow)
        .filter(|_| icon.imported_image.is_none())
        .map(|sparrow| &sparrow.summon)
}

/// `vehicle`'s silhouette as icon art, fitted inside a small margin and centered. `None` for a
/// vehicle whose graph names none, such as an unarmed one.
pub(crate) fn silhouette(
    manager: &PackageManager,
    vehicle: &Summon,
) -> Result<Option<ImportedIcon>, String> {
    let Some(entity) = vehicle.entity()? else {
        return Ok(None);
    };
    let payload = manager
        .read_tag(TagHash(entity))
        .map_err(|error| format!("Vehicle entity 0x{entity:08X}: {error}"))?;
    let Some(hud) = crate::hud_icon::silhouette(manager, &payload)? else {
        return Ok(None);
    };
    let [width, height] = hud.size.map(|edge| u32::try_from(edge).unwrap_or(u32::MAX));
    let pixels = hud
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_srgba_unmultiplied())
        .collect();
    let source = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or("The vehicle silhouette has the wrong size")?;
    let fitted = crate::image_import::fit(&source, EDGE - 2 * MARGIN, EDGE - 2 * MARGIN);
    let mut canvas = image::RgbaImage::new(EDGE, EDGE);
    image::imageops::replace(&mut canvas, &fitted, i64::from(MARGIN), i64::from(MARGIN));
    ImportedIcon::from_drawn(canvas).map(Some)
}
