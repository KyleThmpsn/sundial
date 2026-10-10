//! Emblems. An emblem's square inventory icon is an item icon like any other. Its nameplate is a
//! second icon container, named by its item strings at +0x82, that holds three images: the 474×96
//! banner at +0x14 (again at +0x28), the overlay drawn on it at +0x20 and the wide background at
//! +0x24. The client draws the banner from the item's dense presentation row, field type 2.
//!
//! A recipe keeps its base emblem's images, takes any of them from another emblem, or brings
//! pictures of its own. The build then gives the emblem a nameplate container of its own.
use crate::image_import::EmbeddedImage;
use crate::{HexHash, dye::DyeValue};
use serde::{Deserialize, Serialize};
use sundial::package_authoring::icon_schema::{
    ICON_FOREGROUND_LAYER_OFFSET, ICON_PRIMARY_LAYER_OFFSET, ICON_WATERMARK_LAYER_OFFSET,
};

mod art;
mod trackers;
pub(crate) use art::{ResolvedNameplate, author, layer_pixels, resolve};
pub use trackers::StatTrackers;
pub(crate) use trackers::apply_trackers;

/// The dense presentation field type the client draws an emblem's nameplate from. The inventory
/// icon is type 1.
pub(crate) const NAMEPLATE_FIELD_TYPE: i8 = 2;

/// The three images of an emblem's nameplate.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NameplatePart {
    #[default]
    Banner,
    Overlay,
    Background,
}

impl NameplatePart {
    pub const ALL: [Self; 3] = [Self::Banner, Self::Overlay, Self::Background];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Banner => "Banner",
            Self::Overlay => "Overlay",
            Self::Background => "Background",
        }
    }

    /// The icon container layer that holds this image.
    #[must_use]
    pub const fn layer_offset(self) -> usize {
        match self {
            Self::Banner => ICON_PRIMARY_LAYER_OFFSET,
            Self::Overlay => ICON_WATERMARK_LAYER_OFFSET,
            Self::Background => ICON_FOREGROUND_LAYER_OFFSET,
        }
    }

    /// The size most stock emblems draw this image at. A picture of the recipe's own is shown at
    /// it, and built at whatever size the base's own layer has.
    #[must_use]
    pub const fn size(self) -> (u32, u32) {
        match self {
            Self::Banner => (474, 96),
            Self::Overlay => (96, 96),
            Self::Background => (1958, 146),
        }
    }
}

/// Where one nameplate image comes from, when it is not the base emblem's.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum NameplateImage {
    /// The same image of another emblem.
    Emblem { item_hash: HexHash },
    /// A picture of the recipe's own, scaled to cover the image and cropped at its edges.
    Image { image: EmbeddedImage },
    /// Source artwork with reusable crop, placement and color edits.
    Artwork {
        artwork: crate::presentation::Artwork,
        /// The native source supplies missing layer layouts and its banner's nameplate colors.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_emblem: Option<HexHash>,
    },
}

/// An emblem's nameplate images. Each one left out keeps the base emblem's.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Nameplate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<NameplateImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<NameplateImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<NameplateImage>,
    /// Native nameplate colors, stored as linear RGBA vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<[[DyeValue; 4]; 2]>,
}

impl Nameplate {
    #[must_use]
    pub const fn part(&self, part: NameplatePart) -> Option<&NameplateImage> {
        match part {
            NameplatePart::Banner => self.banner.as_ref(),
            NameplatePart::Overlay => self.overlay.as_ref(),
            NameplatePart::Background => self.background.as_ref(),
        }
    }

    pub fn set(&mut self, part: NameplatePart, image: Option<NameplateImage>) {
        *match part {
            NameplatePart::Banner => &mut self.banner,
            NameplatePart::Overlay => &mut self.overlay,
            NameplatePart::Background => &mut self.background,
        } = image;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_none()
            && NameplatePart::ALL
                .into_iter()
                .all(|part| self.part(part).is_none())
    }

    /// Every emblem an image names must be a hash.
    pub(crate) fn validate(&self) -> Result<(), String> {
        for part in NameplatePart::ALL {
            let hash = match self.part(part) {
                Some(NameplateImage::Emblem { item_hash }) => Some(item_hash),
                Some(NameplateImage::Artwork { source_emblem, .. }) => source_emblem.as_ref(),
                _ => None,
            };
            if let Some(item_hash) = hash {
                item_hash
                    .parse_u32()
                    .map_err(|error| format!("{} emblem: {error}", part.label()))?;
            }
        }
        Ok(())
    }

    /// The emblems the images name, by image.
    pub(crate) fn emblems(&self) -> impl Iterator<Item = (NameplatePart, &HexHash)> {
        NameplatePart::ALL
            .into_iter()
            .filter_map(|part| match self.part(part)? {
                NameplateImage::Emblem { item_hash } => Some((part, item_hash)),
                NameplateImage::Image { .. } => None,
                NameplateImage::Artwork { source_emblem, .. } => {
                    source_emblem.as_ref().map(|hash| (part, hash))
                }
            })
    }
}
