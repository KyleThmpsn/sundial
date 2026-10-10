//! A subclass's inventory icon drawn as the stock ones are: a diamond from corner to corner in the
//! subclass's HUD color, under a light border, with artwork of its own at the middle. The stock
//! icons are one 160-pixel image each, so the diamond is drawn at that size and takes the place of
//! the icon's image, as an imported image does. The build and the page both draw it through
//! `Parts::draw`. The build finds the Super through its sources, and the page, its icon editors
//! and library rows through the catalog's subclasses, as a `Preview`.
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use sundial::investment::SubclassSummary;
use sundial::package_authoring::{PackageManager, ability_hud};
use tiger_pkg::TagHash;

use crate::icon_edit::ImportedIcon;
use crate::perk::Icon;

/// A subclass's Generated Icon: its icon drawn as a diamond in the HUD color, with `glyph` at its
/// middle at `size`. With no glyph the diamond is plain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratedIcon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glyph: Option<Icon>,
    /// The glyph's size, in percent of the 96 pixels a stock icon's glyph takes.
    #[serde(skip_serializing_if = "is_full_size")]
    pub size: u16,
}

/// The sizes a glyph can take, in percent: from a small mark to the diamond's full width.
pub const GLYPH_SIZES: std::ops::RangeInclusive<u16> = 25..=165;
/// The size of a stock icon's glyph.
pub const FULL_SIZE: u16 = 100;

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_full_size(size: &u16) -> bool {
    *size == FULL_SIZE
}

impl Default for GeneratedIcon {
    fn default() -> Self {
        Self {
            glyph: None,
            size: FULL_SIZE,
        }
    }
}

impl GeneratedIcon {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !GLYPH_SIZES.contains(&self.size) {
            return Err(format!(
                "A Generated Icon's symbol size must be {}% to {}%",
                GLYPH_SIZES.start(),
                GLYPH_SIZES.end()
            ));
        }
        self.glyph.as_ref().map_or(Ok(()), Icon::validate)
    }
}

/// How a generated icon draws what it reads: in its own HUD color, or in the Super's without one,
/// with its glyph at its size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Look {
    pub(crate) color: Option<[u8; 3]>,
    pub(crate) size: u16,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            color: None,
            size: FULL_SIZE,
        }
    }
}

/// The stock subclass icons' edge, the width and shade of their border, and the share of the edge
/// their glyph takes, so a 96-pixel perk icon sits at its own size.
const EDGE: u32 = 160;
const BORDER: f32 = 3.0;
const BORDER_SHADE: f32 = 180.0;
const GLYPH: u32 = 96;
/// The faint inner diamond that stands for the stock icons' line work, its distance inside the
/// edge and how far it lightens the fill.
const INNER_LINE: f32 = 13.5;
const INNER_LINE_STRENGTH: f32 = 0.16;
/// The color of a Super whose HUD row has none: the stock neutral row's.
const NEUTRAL: [u8; 3] = [145, 151, 158];

/// A recipe's generated icon as the recipe gives it: the Supers whose glyph row may color it, the
/// first with a glyph, its symbol, and how it draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Generated {
    /// Each a stock subclass and entry: the ability the Super's icon comes from, then the Super.
    supers: Vec<(u32, u8)>,
    glyph: Option<Icon>,
    look: Look,
}

impl Generated {
    /// The generated icon `recipe` draws in place of its icon's image, if it draws one.
    pub(crate) fn of(recipe: &crate::WeaponRecipe) -> Option<Self> {
        use super::layout::SUPER;
        let overrides = &recipe.overrides;
        let generated = overrides.subclass_icon.as_ref()?;
        if recipe.kind != crate::ItemKind::Subclass || overrides.icon_edit.imported_image.is_some()
        {
            return None;
        }
        let base = recipe.donor.item_hash.parse_u32().ok()?;
        let abilities = overrides.subclass_abilities.as_ref();
        let choice = abilities.map_or_else(
            || super::SubclassChoice::stock(SUPER, base, SUPER),
            |abilities| abilities.ability(base, SUPER),
        );
        let icon = match &choice.edits.icon {
            Some(super::EntryIcon::Ability { subclass, entry }) => Some((*subclass, *entry)),
            _ => None,
        };
        Some(Self {
            supers: icon
                .into_iter()
                .chain([(choice.source, choice.source_entry)])
                .collect(),
            glyph: generated.glyph.clone(),
            look: Look {
                color: abilities.and_then(|abilities| abilities.hud_color),
                size: generated.size,
            },
        })
    }

    /// The icon as a preview reads it, with each Super's entity as `subclasses` name it.
    pub(crate) fn preview(&self, subclasses: &[SubclassSummary]) -> Preview {
        Preview {
            super_entities: self
                .supers
                .iter()
                .filter_map(|&(subclass, entry)| {
                    subclasses
                        .iter()
                        .find(|summary| summary.hash == subclass)?
                        .entry_entities
                        .get(&entry)
                        .copied()
                })
                .collect(),
            glyph: self.glyph.clone(),
            look: self.look,
        }
    }
}

/// A generated icon as a preview reads and draws it: the entities whose glyph row may color it,
/// the first with a glyph, its symbol, and how it draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Preview {
    super_entities: Vec<u32>,
    glyph: Option<Icon>,
    pub(crate) look: Look,
}

impl Preview {
    /// Whether both read the same from the packages. The color and size are drawn, not read.
    pub(crate) fn same_reads(&self, other: &Self) -> bool {
        self.super_entities == other.super_entities && self.glyph == other.glyph
    }

    /// Reads the row color of the first Super entity that names a glyph, and the symbol's pixels.
    pub(crate) fn read(&self, manager: &PackageManager) -> Result<Parts, String> {
        let mut super_glyph = None;
        for &entity in &self.super_entities {
            super_glyph = entity_glyph(manager, entity)?;
            if super_glyph.is_some() {
                break;
            }
        }
        Parts::read(manager, super_glyph, self.glyph.as_ref())
    }
}

/// What a generated icon needs from the packages: the Super's HUD color, which it takes without a
/// HUD color of its own, and the glyph's pixels.
#[derive(Clone, Debug, Default)]
pub(crate) struct Parts {
    pub(crate) super_color: Option<[u8; 3]>,
    pub(crate) glyph: Option<RgbaImage>,
}

impl Parts {
    /// Reads the color of `super_glyph`'s row and the pixels of `glyph`.
    pub(crate) fn read(
        manager: &PackageManager,
        super_glyph: Option<u32>,
        glyph: Option<&Icon>,
    ) -> Result<Self, String> {
        Ok(Self {
            super_color: super_glyph
                .map(|key| super::hud::row_color(manager, key))
                .transpose()
                .map_err(|error| error.to_string())?
                .flatten(),
            glyph: glyph.map(|icon| glyph_pixels(manager, icon)).transpose()?,
        })
    }

    /// The icon as `look` draws it, in the Super's HUD color without a color of its own, as the
    /// icon's image.
    pub(crate) fn draw(&self, look: Look) -> Result<ImportedIcon, String> {
        let color = look.color.or(self.super_color).unwrap_or(NEUTRAL);
        ImportedIcon::from_drawn_at_size(draw(color, self.glyph.as_ref(), look.size))
    }
}

/// The HUD glyph the entity `entity` names, whose row colors a Super's tiles.
fn entity_glyph(manager: &PackageManager, entity: u32) -> Result<Option<u32>, String> {
    let payload = manager
        .read_tag(TagHash(entity))
        .map_err(|error| format!("Ability entity 0x{entity:08X}: {error}"))?;
    ability_hud::glyph_site(manager, &payload).map(|site| site.map(|site| site.key))
}

/// A glyph's pixels: an installed perk icon decoded, or the artwork's own image.
fn glyph_pixels(manager: &PackageManager, icon: &Icon) -> Result<RgbaImage, String> {
    match icon {
        Icon::Texture { tag } => {
            let tag = TagHash(tag.parse_u32().map_err(|error| error.to_string())?);
            let image = crate::icon_edit::package_icons::load(manager, tag)?;
            let [width, height] = image.size.map(|side| u32::try_from(side).unwrap_or(0));
            Ok(RgbaImage::from_fn(width, height, |x, y| {
                Rgba(image[(x as usize, y as usize)].to_srgba_unmultiplied())
            }))
        }
        Icon::Image { image, .. } => Ok(crate::icon_edit::glyph::fit(&image.fit_to(GLYPH, GLYPH))),
    }
}

/// The diamond in `color`, lit a little from above as the stock icons are, with `glyph` fitted to
/// its middle at `size`, in percent of the stock glyph's.
fn draw(color: [u8; 3], glyph: Option<&RgbaImage>, size: u16) -> RgbaImage {
    let edge = EDGE as f32;
    let center = edge / 2.0;
    let mut canvas = RgbaImage::from_fn(EDGE, EDGE, |x, y| {
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        // How far inside the edge the pixel's center sits, across the edge.
        let inside = (center - (x - center).abs() - (y - center).abs()) / std::f32::consts::SQRT_2;
        let coverage = (inside + 0.5).clamp(0.0, 1.0);
        if coverage <= 0.0 {
            return Rgba([0; 4]);
        }
        let shade = 1.06 - 0.12 * y / edge;
        let line = (1.0 - (inside - INNER_LINE).abs() / 0.75).clamp(0.0, 1.0) * INNER_LINE_STRENGTH;
        let border = (coverage - (inside - BORDER + 0.5).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let channel = |value: u8| {
            let fill = (f32::from(value) * shade).min(255.0);
            let lined = fill + (255.0 - fill) * line;
            (lined + (BORDER_SHADE - lined) * border).round() as u8
        };
        Rgba([
            channel(color[0]),
            channel(color[1]),
            channel(color[2]),
            (coverage * 255.0).round() as u8,
        ])
    });
    if let Some(glyph) = glyph {
        let side = (GLYPH * u32::from(size) / u32::from(FULL_SIZE)).clamp(1, EDGE);
        let glyph = crate::image_import::fit(glyph, side, side);
        let offset = i64::from((EDGE - side) / 2);
        image::imageops::overlay(&mut canvas, &glyph, offset, offset);
    }
    canvas
}
