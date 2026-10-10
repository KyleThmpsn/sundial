//! Private ability glyph rows carry an authored tile color in the third lane, separate from
//! native donor colors in the first. A Super carries the inherited subclass theme in the second
//! lane, independent of its own color. A row can also draw an icon that no glyph row holds,
//! from the icon's own primary layer.
use sundial::package_authoring::{PackageManager, ability_hud};
use tiger_pkg::TagHash;

use super::{AttunementPath, Place, SubclassAbilities, layout};
use crate::error::invalid;
use crate::{AuthoringResult, ReplacementSpec};

#[derive(Clone, Copy)]
pub(crate) struct Row {
    pub key: u32,
    pub source: u32,
    /// The ability's own color. `None` keeps the source row's colors.
    pub rgb: Option<[u8; 3]>,
    pub theme: Option<[u8; 3]>,
    /// What the tile draws in place of the source row's art.
    pub art: Option<Art>,
}

/// An icon the tile draws that no glyph row holds: a node's icon or the entry's own artwork.
#[derive(Clone, Copy)]
pub(crate) enum Art {
    /// All art layers of another glyph, without borrowing its colors.
    Glyph(u32),
    /// A primary layer an icon container holds, class `80804A69` as a glyph's layers are.
    Layer(u32),
    /// The primary layer of the entry's own icon container, known once the build has made it.
    OwnIcon,
}

/// Where a glyph row names its layers: the primary at +4, then the further slots.
const PRIMARY_LAYER: usize = 4;
const LAYER_SLOTS: std::ops::Range<usize> = 8..0x1C;
/// The class of an icon layer, which glyph rows and icon containers share.
const LAYER_CLASS: u32 = 0x8080_4A69;
/// Where an icon container names its primary layer.
const CONTAINER_PRIMARY_LAYER: usize = 0x14;

/// The primary layer of icon container `container`, as a glyph row can draw it: a stock one
/// checked against the layer class, or one the build made.
pub(crate) fn primary_layer(
    manager: &PackageManager,
    container: &[u8],
    authored: bool,
) -> AuthoringResult<u32> {
    let layer = crate::tag_payload::read_u32(container, CONTAINER_PRIMARY_LAYER)?;
    if matches!(layer, 0 | u32::MAX)
        || !authored
            && manager
                .get_entry(TagHash(layer))
                .is_none_or(|entry| entry.reference != LAYER_CLASS)
    {
        return Err(invalid(format!(
            "Icon layer {layer:08X} cannot draw an ability's HUD tile"
        )));
    }
    Ok(layer)
}

pub(crate) fn is_super(place: Place) -> bool {
    matches!(
        place,
        Place::Ability(layout::SUPER) | Place::Node(AttunementPath::Middle, 0)
    )
}

/// Include every tree node when the subclass theme is authored. These build-local choices
/// preserve recipe inheritance. Only the two Supers need copied entities for a shared theme.
pub(crate) fn include_entries(abilities: &mut SubclassAbilities, base: u32) {
    if abilities.hud_color.is_none() {
        return;
    }
    for place in Place::all() {
        match place {
            Place::Ability(entry) if abilities.choice(entry).is_none() => {
                abilities.choices.push(abilities.ability(base, entry));
                abilities.choices.sort_by_key(|choice| choice.entry);
            }
            Place::Node(path, position) => {
                let node = abilities.node(base, path, position);
                let attunement = abilities.attunement_entry(path, base);
                if attunement.node(position).is_none() {
                    attunement.nodes.push(node);
                    attunement.nodes.sort_by_key(|node| node.position);
                }
            }
            _ => {}
        }
    }
}

/// The ability HUD glyph table, once its class is the one this reads.
fn glyph_table(manager: &PackageManager) -> AuthoringResult<Vec<u8>> {
    let tag = TagHash(ability_hud::GLYPH_TABLE);
    if manager
        .get_entry(tag)
        .is_none_or(|entry| entry.reference != 0x80804A55)
    {
        return Err(invalid(
            "The ability HUD glyph table has an unsupported class",
        ));
    }
    manager
        .read_tag(tag)
        .map_err(|error| invalid(error.to_string()))
}

/// The sRGB color the HUD tints the tiles of glyph `key` with, from its stock row, as the native
/// loader publishes it. The existing theme binding reads the first vector directly, even
/// when its presence bit is clear. A transparent vector supplies no visible theme.
pub(crate) fn row_color(manager: &PackageManager, key: u32) -> AuthoringResult<Option<[u8; 3]>> {
    let rows = crate::hud_icon::assets::table_rows(&glyph_table(manager)?)?;
    let Some(row) = rows.get(&key) else {
        return Ok(None);
    };
    let channel = |index: usize| -> AuthoringResult<f32> {
        let start = 0x20 + index * 4;
        row.get(start..start + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(f32::from_le_bytes)
            .ok_or_else(|| invalid("An ability HUD glyph row is too short"))
    };
    if channel(3)? <= 0.0 {
        return Ok(None);
    }
    let mut rgb = [0; 3];
    for (index, value) in rgb.iter_mut().enumerate() {
        *value = (channel(index)?.max(0.0).powf(1.0 / 2.2) * 255.0)
            .round()
            .min(255.0) as u8;
    }
    Ok(Some(rgb))
}

pub(crate) fn build(
    manager: &PackageManager,
    added: impl Iterator<Item = Row>,
) -> AuthoringResult<Option<ReplacementSpec>> {
    let added: Vec<_> = added.collect();
    if added.is_empty() {
        return Ok(None);
    }
    let tag = TagHash(ability_hud::GLYPH_TABLE);
    let table = glyph_table(manager)?;
    let stock = crate::hud_icon::assets::table_rows(&table)?;
    let rows = added
        .into_iter()
        .map(|added| {
            let mut row = stock
                .get(&added.source)
                .ok_or_else(|| {
                    invalid(format!("Ability HUD glyph {:08X} is missing", added.source))
                })?
                .clone();
            row[..4].copy_from_slice(&added.key.to_le_bytes());
            // An icon with no glyph of its own is drawn alone, from its primary layer.
            match added.art {
                Some(Art::Glyph(glyph)) => {
                    let donor = stock.get(&glyph).ok_or_else(|| {
                        invalid(format!("Ability HUD glyph {glyph:08X} is missing"))
                    })?;
                    row[PRIMARY_LAYER..LAYER_SLOTS.end]
                        .copy_from_slice(&donor[PRIMARY_LAYER..LAYER_SLOTS.end]);
                }
                Some(Art::Layer(layer)) => {
                    row[PRIMARY_LAYER..PRIMARY_LAYER + 4].copy_from_slice(&layer.to_le_bytes());
                    for at in LAYER_SLOTS.step_by(4) {
                        row[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                    }
                }
                Some(Art::OwnIcon) => {
                    return Err(invalid(
                        "An ability's HUD art names its own icon before the icon was made",
                    ));
                }
                None => {}
            }
            // The first lane remains the native color fallback. Only an authored color sets
            // the third lane, so a donor's native color cannot override the inherited theme.
            // The native icon-row loader raises RGB to 1/2.2 before publishing the widget color.
            for (lane, rgb) in [added.rgb, added.theme, added.rgb].into_iter().enumerate() {
                let Some(rgb) = rgb else {
                    continue;
                };
                let rgb = rgb.map(|channel| (f32::from(channel) / 255.0).powf(2.2));
                for (channel, value) in rgb.into_iter().chain([1.0]).enumerate() {
                    let start = 0x20 + lane * 16 + channel * 4;
                    row[start..start + 4].copy_from_slice(&value.to_le_bytes());
                }
                row[0x60] |= 1 << lane;
            }
            Ok((added.key, row))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(Some(ReplacementSpec {
        tag,
        payload: crate::hud_icon::assets::insert_rows(table, stock, rows)?,
    }))
}
