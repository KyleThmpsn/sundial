//! Deterministic editing and private authoring of a weapon icon's primary image layer.
//!
//! The editor intentionally operates only on the primary layer selected by an icon definition at
//! `+0x14`. The donor definition, layer, texture headers, and texture data remain untouched.

mod authoring;
mod color_selection;
mod editor;
mod import_ui;
mod imported;
mod preview;
#[cfg(test)]
mod tests;
mod transforms;

pub(crate) use authoring::build_weapon_icon_edit_plan;
pub use color_selection::IconColorReplacement;
pub(crate) use editor::{WeaponIconEditor, WeaponIconEditorAction};
pub use imported::ImportedIcon;
pub(crate) use imported::decode as decode_image;
pub(crate) use imported::fit as fit_rgba_image;
pub(crate) use preview::{
    render_texture_preview, render_weapon_icon_preview, render_weapon_icon_preview_from_manager,
};

use serde::{Deserialize, Serialize};

const ICON_PREVIEW_SIZE: usize = 96;

/// Reproducible adjustments for a weapon's private primary icon image.
///
/// Imported artwork replaces the primary image first, followed by hue, saturation, brightness,
/// contrast, channel balance, inversion, selected color replacements, opacity, then orientation.
/// Replacements match and preserve shading from the original artwork, before global adjustments.
/// The context uses the authored rarity background, Sunrise watermark, and donor foreground.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponIconEdit {
    /// Independent source-color replacements, blended after global color edits, before orientation.
    #[serde(
        skip_serializing_if = "color_selection::is_identity",
        serialize_with = "color_selection::serialize_changes"
    )]
    pub color_replacements: Vec<IconColorReplacement>,
    /// Optional embedded replacement, applied before color and orientation adjustments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_image: Option<ImportedIcon>,
    /// Clockwise quarter-turns applied before flips.
    #[serde(skip_serializing_if = "is_default_value")]
    pub rotation_quarter_turns: u8,
    #[serde(skip_serializing_if = "is_default_value")]
    pub flip_horizontal: bool,
    #[serde(skip_serializing_if = "is_default_value")]
    pub flip_vertical: bool,
    #[serde(skip_serializing_if = "is_default_value")]
    pub hue_shift_degrees: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub saturation: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub brightness: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub contrast: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub red_balance: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub green_balance: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub blue_balance: i16,
    #[serde(skip_serializing_if = "is_default_value")]
    pub invert: bool,
    #[serde(skip_serializing_if = "is_full_opacity")]
    pub opacity_percent: u8,
}

fn is_default_value<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
fn is_full_opacity(value: &u8) -> bool {
    *value == 100
}

impl Default for WeaponIconEdit {
    fn default() -> Self {
        Self {
            color_replacements: Vec::new(),
            imported_image: None,
            rotation_quarter_turns: 0,
            flip_horizontal: false,
            flip_vertical: false,
            hue_shift_degrees: 0,
            saturation: 0,
            brightness: 0,
            contrast: 0,
            red_balance: 0,
            green_balance: 0,
            blue_balance: 0,
            invert: false,
            opacity_percent: 100,
        }
    }
}
