//! Deterministic pixel transforms shared by preview rendering and package emission.
use super::{IconColorReplacement, WeaponIconEdit, color_selection};
use crate::{AuthoringResult, error::input as invalid};

impl WeaponIconEdit {
    pub const MIN_HUE_SHIFT_DEGREES: i16 = -180;
    pub const MAX_HUE_SHIFT_DEGREES: i16 = 180;
    pub const MIN_COLOR_ADJUSTMENT: i16 = -100;
    pub const MAX_COLOR_ADJUSTMENT: i16 = 100;
    pub const MIN_OPACITY: u8 = 0;
    pub const MAX_OPACITY: u8 = 100;

    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.color_replacements
            .iter()
            .all(IconColorReplacement::is_identity)
            && self.imported_image.is_none()
            && self.rotation_quarter_turns == 0
            && !self.flip_horizontal
            && !self.flip_vertical
            && self.color_is_default()
    }

    pub(super) fn color_is_default(&self) -> bool {
        self.hue_shift_degrees == 0
            && self.saturation == 0
            && self.brightness == 0
            && self.contrast == 0
            && self.red_balance == 0
            && self.green_balance == 0
            && self.blue_balance == 0
            && !self.invert
            && self.opacity_percent == 100
    }

    pub(super) fn reset_color(&mut self) {
        self.hue_shift_degrees = 0;
        self.saturation = 0;
        self.brightness = 0;
        self.contrast = 0;
        self.red_balance = 0;
        self.green_balance = 0;
        self.blue_balance = 0;
        self.invert = false;
        self.opacity_percent = 100;
    }

    pub fn validate(&self) -> AuthoringResult<()> {
        if self.color_replacements.len() > color_selection::MAX_REPLACEMENTS
            || self
                .color_replacements
                .iter()
                .any(|rule| rule.range_percent > 100)
        {
            return Err(invalid(
                "Icon color replacements support up to 16 rules, each with a range of 0–100%",
            ));
        }
        if self.rotation_quarter_turns > 3 {
            return Err(invalid(format!(
                "Weapon icon rotation {} is outside the supported quarter-turn range 0..=3",
                self.rotation_quarter_turns
            )));
        }
        if !(Self::MIN_HUE_SHIFT_DEGREES..=Self::MAX_HUE_SHIFT_DEGREES)
            .contains(&self.hue_shift_degrees)
        {
            return Err(invalid(format!(
                "Weapon icon hue shift {} is outside the supported range {}..={}",
                self.hue_shift_degrees,
                Self::MIN_HUE_SHIFT_DEGREES,
                Self::MAX_HUE_SHIFT_DEGREES
            )));
        }
        for (label, value) in [
            ("saturation", self.saturation),
            ("brightness", self.brightness),
            ("contrast", self.contrast),
            ("red balance", self.red_balance),
            ("green balance", self.green_balance),
            ("blue balance", self.blue_balance),
        ] {
            if !(Self::MIN_COLOR_ADJUSTMENT..=Self::MAX_COLOR_ADJUSTMENT).contains(&value) {
                return Err(invalid(format!(
                    "Weapon icon {label} {value} is outside the supported range {}..={}",
                    Self::MIN_COLOR_ADJUSTMENT,
                    Self::MAX_COLOR_ADJUSTMENT
                )));
            }
        }
        if !(Self::MIN_OPACITY..=Self::MAX_OPACITY).contains(&self.opacity_percent) {
            return Err(invalid(format!(
                "Weapon icon opacity {} is outside the supported range {}..={}",
                self.opacity_percent,
                Self::MIN_OPACITY,
                Self::MAX_OPACITY
            )));
        }
        Ok(())
    }

    /// Applies this edit to tightly packed RGBA8 pixels.
    #[cfg(test)]
    pub(super) fn apply_to_rgba8(&self, pixels: &mut [u8]) -> AuthoringResult<()> {
        let width = pixels.len() / 4;
        self.apply_to_rgba8_sized(pixels, width, usize::from(width != 0))
    }

    pub(super) fn apply_to_rgba8_sized(
        &self,
        pixels: &mut [u8],
        width: usize,
        height: usize,
    ) -> AuthoringResult<()> {
        self.validate()?;
        let expected = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| invalid("RGBA8 icon dimensions overflowed"))?;
        if pixels.len() % 4 != 0 || pixels.len() != expected {
            return Err(invalid(format!(
                "RGBA8 icon data has {} bytes but dimensions {width}×{height} require {expected}",
                pixels.len(),
            )));
        }
        if self.rotation_quarter_turns % 2 == 1 && width != height {
            return Err(invalid(format!(
                "A 90° or 270° icon rotation requires a square texture; this layer is {width}×{height}"
            )));
        }
        if self.is_identity() {
            return Ok(());
        }
        if let Some(imported) = &self.imported_image {
            if width == 0 || height == 0 || width > 2048 || height > 2048 {
                return Err(invalid("Imported icon target dimensions are invalid"));
            }
            pixels.copy_from_slice(imported.fit_to(width as u32, height as u32).as_raw());
        }
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = pixel[3];
            let source_rgb = [pixel[0], pixel[1], pixel[2]];
            let mut rgb = source_rgb;
            if self.hue_shift_degrees != 0 {
                rgb = rotate_hue(rgb, self.hue_shift_degrees);
            }
            if self.saturation != 0 {
                rgb = adjust_saturation(rgb, self.saturation);
            }
            if self.brightness != 0 {
                rgb = adjust_brightness(rgb, self.brightness);
            }
            if self.contrast != 0 {
                rgb = adjust_contrast(rgb, self.contrast);
            }
            if self.red_balance != 0 || self.green_balance != 0 || self.blue_balance != 0 {
                rgb = adjust_channel_balance(
                    rgb,
                    [self.red_balance, self.green_balance, self.blue_balance],
                );
            }
            if self.invert {
                for channel in &mut rgb {
                    *channel = 255 - *channel;
                }
            }
            if alpha != 0 {
                // Match the original artwork but blend last so global adjustments cannot
                // change an explicitly chosen replacement color (for example, red to green).
                rgb = color_selection::apply(&self.color_replacements, source_rgb, rgb);
            }
            pixel[..3].copy_from_slice(&rgb);
            pixel[3] =
                div_round_positive(i32::from(alpha) * i32::from(self.opacity_percent), 100) as u8;
        }
        if self.rotation_quarter_turns != 0 || self.flip_horizontal || self.flip_vertical {
            transform_rgba8(
                pixels,
                width,
                height,
                self.rotation_quarter_turns,
                self.flip_horizontal,
                self.flip_vertical,
            );
        }
        Ok(())
    }
}

fn transform_rgba8(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    quarter_turns: u8,
    flip_horizontal: bool,
    flip_vertical: bool,
) {
    if pixels.is_empty() {
        return;
    }
    let source = pixels.to_vec();
    for source_y in 0..height {
        for source_x in 0..width {
            let (mut destination_x, mut destination_y) = match quarter_turns {
                0 => (source_x, source_y),
                1 => (height - 1 - source_y, source_x),
                2 => (width - 1 - source_x, height - 1 - source_y),
                3 => (source_y, width - 1 - source_x),
                _ => unreachable!("validated quarter-turn count"),
            };
            if flip_horizontal {
                destination_x = width - 1 - destination_x;
            }
            if flip_vertical {
                destination_y = height - 1 - destination_y;
            }
            let source_offset = (source_y * width + source_x) * 4;
            let destination_offset = (destination_y * width + destination_x) * 4;
            pixels[destination_offset..destination_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
}

fn rotate_hue(rgb: [u8; 3], degrees: i16) -> [u8; 3] {
    let red = i32::from(rgb[0]);
    let green = i32::from(rgb[1]);
    let blue = i32::from(rgb[2]);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let chroma = maximum - minimum;
    if chroma == 0 || degrees == 0 {
        return rgb;
    }

    // Six hue sectors, each represented by 256 integer units. This avoids platform-dependent
    // floating-point trigonometry while retaining sub-degree precision for the UI's degree input.
    let mut hue = if maximum == red {
        div_round_signed((green - blue) * 256, chroma)
    } else if maximum == green {
        512 + div_round_signed((blue - red) * 256, chroma)
    } else {
        1024 + div_round_signed((red - green) * 256, chroma)
    };
    hue = hue.rem_euclid(1536);
    let shift = div_round_signed(i32::from(degrees) * 256, 60);
    hue = (hue + shift).rem_euclid(1536);

    let sector = hue / 256;
    let fraction = hue % 256;
    let rising = div_round_positive(chroma * fraction, 256);
    let falling = chroma - rising;
    let (red, green, blue) = match sector {
        0 => (chroma, rising, 0),
        1 => (falling, chroma, 0),
        2 => (0, chroma, rising),
        3 => (0, falling, chroma),
        4 => (rising, 0, chroma),
        _ => (chroma, 0, falling),
    };
    [
        (red + minimum) as u8,
        (green + minimum) as u8,
        (blue + minimum) as u8,
    ]
}

fn adjust_brightness(mut rgb: [u8; 3], brightness: i16) -> [u8; 3] {
    for channel in &mut rgb {
        let value = i32::from(*channel);
        *channel = if brightness > 0 {
            (value + div_round_positive((255 - value) * i32::from(brightness), 100)) as u8
        } else {
            div_round_positive(value * i32::from(100 + brightness), 100) as u8
        };
    }
    rgb
}

fn adjust_saturation(rgb: [u8; 3], saturation: i16) -> [u8; 3] {
    let gray = div_round_positive(
        i32::from(rgb[0]) * 54 + i32::from(rgb[1]) * 183 + i32::from(rgb[2]) * 19,
        256,
    );
    let factor = i32::from(100 + saturation);
    rgb.map(|channel| {
        (gray + div_round_signed((i32::from(channel) - gray) * factor, 100)).clamp(0, 255) as u8
    })
}

fn adjust_contrast(rgb: [u8; 3], contrast: i16) -> [u8; 3] {
    let factor = i32::from(100 + contrast);
    rgb.map(|channel| {
        (128 + div_round_signed((i32::from(channel) - 128) * factor, 100)).clamp(0, 255) as u8
    })
}

fn adjust_channel_balance(rgb: [u8; 3], balances: [i16; 3]) -> [u8; 3] {
    std::array::from_fn(|index| {
        (i32::from(rgb[index]) + div_round_signed(i32::from(balances[index]) * 255, 100))
            .clamp(0, 255) as u8
    })
}

fn div_round_signed(numerator: i32, denominator: i32) -> i32 {
    debug_assert!(denominator > 0);
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        (numerator - denominator / 2) / denominator
    }
}

fn div_round_positive(numerator: i32, denominator: i32) -> i32 {
    debug_assert!(numerator >= 0 && denominator > 0);
    (numerator + denominator / 2) / denominator
}
