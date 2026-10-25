//! Artwork colors use the inventory icon's deterministic pixel transforms.
use crate::WeaponIconEdit;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Adjustments {
    pub hue: i16,
    pub saturation: i16,
    pub brightness: i16,
    pub contrast: i16,
    pub opacity: u8,
    pub invert: bool,
}

impl Default for Adjustments {
    fn default() -> Self {
        Self {
            hue: 0,
            saturation: 0,
            brightness: 0,
            contrast: 0,
            opacity: 100,
            invert: false,
        }
    }
}

impl Adjustments {
    pub(crate) fn is_identity(&self) -> bool {
        self == &Self::default()
    }

    fn icon_edit(&self) -> WeaponIconEdit {
        WeaponIconEdit {
            hue_shift_degrees: self.hue,
            saturation: self.saturation,
            brightness: self.brightness,
            contrast: self.contrast,
            opacity_percent: self.opacity,
            invert: self.invert,
            ..Default::default()
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        self.icon_edit()
            .validate()
            .map_err(|_| "Color adjustments are outside the supported range.".to_owned())
    }

    pub(super) fn apply(&self, image: &mut image::RgbaImage) {
        if self.is_identity() {
            return;
        }
        let (width, height) = image.dimensions();
        self.icon_edit()
            .apply_to_rgba8_sized(image.as_mut(), width as usize, height as usize)
            .expect("validated artwork colors and RGBA image dimensions");
    }
}
