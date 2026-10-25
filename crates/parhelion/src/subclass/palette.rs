//! An ability's effect colors. Its effects draw their color from palette textures
//! (`sundial::package_authoring::ability_palette`) and from color constants of their materials
//! (`ability_tint`). A palette edit can take another stock palette's colors in place of its own,
//! and both kinds of edit turn hue and scale saturation and brightness, or colorize: give every
//! color one hue and saturation, grays and whites too, which a hue turn leaves as they are. The
//! build gives the ability a private copy of the palette, and of the materials, particle systems
//! and graphs that lead to it, so the stock ability and every other user of the palette keep its
//! colors. A grade over every effect reaches the colors palettes and tints do not hold.

use crate::dye::DyeValue;
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
    /// Degrees the hue turns, or with `colorize` the hue every color takes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hue: i16,
    /// Percent of the stock saturation, or with `colorize` of full saturation.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub saturation: u16,
    /// Percent of the stock brightness.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub brightness: u16,
    /// Whether every color takes the hue and saturation in place of turning its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colorize: bool,
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
            colorize: false,
        }
    }

    /// Whether it leaves the palette as it is.
    #[must_use]
    pub const fn is_stock(&self) -> bool {
        self.from.is_none() && self.adjusts_nothing()
    }

    /// Whether it leaves the colors it starts from as they are.
    const fn adjusts_nothing(&self) -> bool {
        !self.colorize && self.hue == 0 && self.saturation == FULL && self.brightness == FULL
    }

    const fn change(&self) -> Change {
        Change {
            hue: self.hue,
            saturation: self.saturation,
            brightness: self.brightness,
            colorize: self.colorize,
        }
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

    /// Recolors sRGB RGBA8 pixels of the palette it starts from in place: each turns its hue, or
    /// takes the hue when colorizing, scales its saturation and brightness, and keeps its alpha.
    pub fn apply(&self, pixels: &mut [u8]) {
        if self.adjusts_nothing() {
            return;
        }
        for pixel in pixels.chunks_exact_mut(4) {
            let color = [pixel[0], pixel[1], pixel[2]].map(|v| f32::from(v) / 255.0);
            let color = self.change().apply(color, true);
            for (slot, value) in pixel.iter_mut().zip(color) {
                *slot = (value * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// One stock color of an ability's materials, by the color it starts as, and its change. Every
/// color constant of the ability's effects with that exact color changes with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TintEdit {
    pub color: [DyeValue; 3],
    /// Degrees the hue turns, or with `colorize` the hue the color takes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hue: i16,
    /// Percent of the stock saturation, or with `colorize` of full saturation.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub saturation: u16,
    /// Percent of the stock brightness.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub brightness: u16,
    /// Whether the color takes the hue and saturation in place of turning its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colorize: bool,
}

impl TintEdit {
    /// The stock color as it is. `None` for a color that is not finite.
    #[must_use]
    pub fn new(rgb: [f32; 3]) -> Option<Self> {
        Some(Self {
            color: [
                DyeValue::new(rgb[0])?,
                DyeValue::new(rgb[1])?,
                DyeValue::new(rgb[2])?,
            ],
            hue: 0,
            saturation: FULL,
            brightness: FULL,
            colorize: false,
        })
    }

    /// Whether it starts from `rgb`, compared exactly.
    #[must_use]
    pub fn starts_from(&self, rgb: [f32; 3]) -> bool {
        self.color
            .iter()
            .zip(rgb)
            .all(|(own, stock)| own.get().to_bits() == (stock + 0.0).to_bits())
    }

    /// Whether it leaves the color as it is.
    #[must_use]
    pub const fn is_stock(&self) -> bool {
        !self.colorize && self.hue == 0 && self.saturation == FULL && self.brightness == FULL
    }

    pub(super) fn validate(&self, context: &str) -> Result<(), String> {
        if self.hue.abs() > MOST_HUE
            || self.saturation > MOST_PERCENT
            || self.brightness > MOST_PERCENT
        {
            return Err(format!("{context} changes a tint beyond its range"));
        }
        Ok(())
    }

    /// The changed color. Shaders read these as plain numbers, often brighter than 1, so
    /// brightness is not capped.
    #[must_use]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_stock() {
            return rgb;
        }
        Change {
            hue: self.hue,
            saturation: self.saturation,
            brightness: self.brightness,
            colorize: self.colorize,
        }
        .apply(rgb, false)
    }
}

/// A grade over every effect of an ability, after any palette and tint changes: the color each
/// particle effect draws turns its hue, or with `colorize` takes one, with its saturation and
/// brightness scaled. The build writes it at the end of a private copy of each effect's pixel
/// program, so it reaches colors no palette or tint holds, such as those of particle curves and
/// shader literals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectGrade {
    /// Degrees the hue turns, or with `colorize` the hue every color takes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hue: i16,
    /// Percent of each color's saturation, or with `colorize` of full saturation.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub saturation: u16,
    /// Percent of each color's brightness.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub brightness: u16,
    /// Whether every color takes the hue and saturation in place of turning its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colorize: bool,
}

impl EffectGrade {
    /// The grade that leaves every color as it is.
    pub const STOCK: Self = Self {
        hue: 0,
        saturation: FULL,
        brightness: FULL,
        colorize: false,
    };

    /// Whether it leaves every color as it is.
    #[must_use]
    pub const fn is_stock(&self) -> bool {
        !self.colorize && self.hue == 0 && self.saturation == FULL && self.brightness == FULL
    }

    /// The color the build's graded program writes for `rgb`, a linear color an effect adds or
    /// overlays.
    #[must_use]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.program().apply(rgb)
    }

    pub(super) fn validate(&self, context: &str) -> Result<(), String> {
        if self.hue.abs() > MOST_HUE
            || self.saturation > MOST_PERCENT
            || self.brightness > MOST_PERCENT
        {
            return Err(format!("{context} grades its effects beyond their range"));
        }
        Ok(())
    }

    /// `bytecode`, a pixel program's DXBC container, graded as the build grades it for a material
    /// whose blend adds or overlays its color, or with `keeps_neutral` one that multiplies it.
    /// `None` for a program the grade leaves as it is.
    pub fn grade_program(
        &self,
        bytecode: &[u8],
        keeps_neutral: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        let program = if keeps_neutral {
            self.neutral_program()
        } else {
            self.program()
        };
        crate::dxbc::grade::grade(bytecode, program).map_err(|error| error.to_string())
    }

    /// What the build writes into a pixel program whose material multiplies, subtracts or takes
    /// the smaller of its color and what is drawn behind it, where white and gray mean no change.
    /// A colorize then sets only the hue, keeping each color's saturation and brightness, so
    /// grays and white stay as they are. A turn already keeps them.
    #[must_use]
    pub(crate) fn neutral_program(&self) -> crate::dxbc::grade::Grade {
        if !self.colorize {
            return self.program();
        }
        let percent = |value: u16| f32::from(value) / f32::from(FULL);
        let (r, g, b) = rgb_of(f32::from(self.hue).rem_euclid(360.0), 1.0, 1.0);
        crate::dxbc::grade::Grade::Hue {
            hue: [r, g, b],
            saturation: percent(self.saturation),
            brightness: percent(self.brightness),
        }
    }

    /// What the build writes into each pixel program. A colorize keeps each color's largest
    /// channel and gives it the hue and saturation, as `TintEdit::apply` does. A turn uses the
    /// luminance-preserving hue rotation and saturation matrices of the W3C Filter Effects
    /// `feColorMatrix`, scaled by the brightness.
    #[must_use]
    pub(crate) fn program(&self) -> crate::dxbc::grade::Grade {
        use crate::dxbc::grade::Grade;
        let percent = |value: u16| f32::from(value) / f32::from(FULL);
        let brightness = percent(self.brightness);
        if self.colorize {
            let hue = f32::from(self.hue).rem_euclid(360.0);
            let (r, g, b) = rgb_of(hue, percent(self.saturation).clamp(0.0, 1.0), 1.0);
            return Grade::Colorize([r, g, b].map(|channel| channel * brightness));
        }
        let (sin, cos) = f32::from(self.hue).to_radians().sin_cos();
        let rotate = [
            [
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
            ],
            [
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
            ],
            [
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
            ],
        ];
        let s = percent(self.saturation);
        let saturate = [
            [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
            [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
            [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
        ];
        let rows = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                brightness
                    * (0..3)
                        .map(|k| saturate[row][k] * rotate[k][column])
                        .sum::<f32>()
            })
        });
        Grade::Matrix(rows)
    }
}

/// What an edit does to each color.
#[derive(Clone, Copy)]
struct Change {
    hue: i16,
    saturation: u16,
    brightness: u16,
    colorize: bool,
}

impl Change {
    /// `[r, g, b]` with its hue turned, or set when colorizing, its saturation scaled, or set to a
    /// percent of full, and its brightness scaled. Brightness is capped at 1 when `cap` is set. A
    /// colorized gray or white takes the hue at that saturation, and black stays black.
    fn apply(self, [r, g, b]: [f32; 3], cap: bool) -> [f32; 3] {
        let (h, s, v) = hsv(r, g, b);
        let percent = |value: u16| f32::from(value) / f32::from(FULL);
        let (hue, saturation) = if self.colorize {
            (
                f32::from(self.hue).rem_euclid(360.0),
                percent(self.saturation),
            )
        } else {
            (
                (h + f32::from(self.hue)).rem_euclid(360.0),
                s * percent(self.saturation),
            )
        };
        let value = v * percent(self.brightness);
        let value = if cap {
            value.clamp(0.0, 1.0)
        } else {
            value.max(0.0)
        };
        let (r, g, b) = rgb_of(hue, saturation.clamp(0.0, 1.0), value);
        [r, g, b]
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

fn rgb_of(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
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
