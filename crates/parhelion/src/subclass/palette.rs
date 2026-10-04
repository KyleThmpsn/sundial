//! An ability's effect colors. Its effects draw their color from palette textures
//! (`sundial::package_authoring::ability_palette`), and an edit can take another stock palette's
//! colors in place of one's own, then turn its hue and scale its saturation and brightness. The build gives the ability a private copy of the palette, and
//! of the materials, particle systems and graphs that lead to it, so the stock ability and every
//! other user of the palette keep its colors.

use serde::{Deserialize, Serialize};

/// The widest hue turn, in degrees each way.
pub const MOST_HUE: i16 = 180;
/// The most saturation or brightness, in percent of the stock palette's.
pub const MOST_PERCENT: u16 = 200;
const FULL: u16 = 100;

/// One palette's change, by the stock palette texture it starts from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteEdit {
    #[serde(with = "super::hex_hash")]
    pub palette: u32,
    /// The stock palette whose colors it starts from, in place of its own.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "super::hex_hash::optional"
    )]
    pub from: Option<u32>,
    /// Degrees the hue turns.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hue: i16,
    /// Percent of the stock saturation.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub saturation: u16,
    /// Percent of the stock brightness.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub brightness: u16,
}

const fn full() -> u16 {
    FULL
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_full(value: &u16) -> bool {
    *value == FULL
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero(value: &i16) -> bool {
    *value == 0
}

impl PaletteEdit {
    /// The stock palette as it is.
    #[must_use]
    pub const fn new(palette: u32) -> Self {
        Self {
            palette,
            from: None,
            hue: 0,
            saturation: FULL,
            brightness: FULL,
        }
    }

    /// Whether it leaves the palette as it is.
    #[must_use]
    pub const fn is_stock(&self) -> bool {
        self.from.is_none() && self.adjusts_nothing()
    }

    /// Whether it leaves the colors it starts from as they are.
    const fn adjusts_nothing(&self) -> bool {
        self.hue == 0 && self.saturation == FULL && self.brightness == FULL
    }

    /// The palette whose pixels it starts from.
    #[must_use]
    pub fn source(&self) -> u32 {
        self.from.unwrap_or(self.palette)
    }

    pub(super) fn validate(&self, context: &str) -> Result<(), String> {
        if self.hue.abs() > MOST_HUE
            || self.saturation > MOST_PERCENT
            || self.brightness > MOST_PERCENT
        {
            return Err(format!(
                "{context} changes palette 0x{:08X} beyond its range",
                self.palette
            ));
        }
        if self.from == Some(self.palette) {
            return Err(format!(
                "{context} takes palette 0x{:08X}'s colors from itself",
                self.palette
            ));
        }
        Ok(())
    }

    /// Recolors sRGB RGBA8 pixels of the palette it starts from in place: each turns its hue and
    /// scales its saturation and brightness, and keeps its alpha.
    pub fn apply(&self, pixels: &mut [u8]) {
        if self.adjusts_nothing() {
            return;
        }
        let saturation = f32::from(self.saturation) / f32::from(FULL);
        let brightness = f32::from(self.brightness) / f32::from(FULL);
        for pixel in pixels.chunks_exact_mut(4) {
            let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(|v| f32::from(v) / 255.0);
            let (h, s, v) = hsv(r, g, b);
            let (r, g, b) = rgb(
                (h + f32::from(self.hue)).rem_euclid(360.0),
                (s * saturation).clamp(0.0, 1.0),
                (v * brightness).clamp(0.0, 1.0),
            );
            for (slot, value) in pixel.iter_mut().zip([r, g, b]) {
                *slot = (value * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Hue in degrees, saturation and value, from RGB in 0 to 1.
fn hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let range = max - r.min(g).min(b);
    let hue = if range <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / range).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / range + 2.0)
    } else {
        60.0 * ((r - g) / range + 4.0)
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        range / max
    };
    (hue, saturation, max)
}

fn rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let chroma = v * s;
    let x = chroma * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - chroma;
    let (r, g, b) = match (h / 60.0) as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    (r + m, g + m, b + m)
}
