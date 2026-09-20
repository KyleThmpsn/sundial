//! Runtime presentation only. Keep existing badge hashes and recipe identities stable.
use crate::{AuthoringResult, presentation::Artwork};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Branding {
    #[default]
    Sunrise,
    Dawn,
}

#[cfg(test)]
mod tests;

impl Branding {
    pub(crate) fn detect(install: &Path) -> Self {
        if sundial::package_authoring::uses_dawn(install) {
            Self::Dawn
        } else {
            Self::Sunrise
        }
    }

    pub(crate) fn for_packages(packages: &Path) -> Self {
        packages.parent().map_or(Self::Sunrise, Self::detect)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Sunrise => crate::badge::SUNRISE_BADGE_NAME,
            Self::Dawn => "Dawn",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Sunrise => crate::badge::SUNRISE_BADGE_DESCRIPTION,
            Self::Dawn => "Ardens aurora semper oritur.",
        }
    }

    pub(crate) fn corner(self) -> AuthoringResult<Option<Artwork>> {
        match self {
            Self::Sunrise => Ok(None),
            Self::Dawn => Artwork::from_png(include_bytes!(
                "../../../assets/parhelion/watermark/dawn-watermark-3-45x45.png"
            ))
            .map(Some)
            .map_err(crate::error::input),
        }
    }

    pub(crate) fn badge(self) -> AuthoringResult<Option<Artwork>> {
        if self == Self::Sunrise {
            return Ok(None);
        }
        let logo = image::load_from_memory(include_bytes!(
            "../../../assets/parhelion/dawn-badge-source.png"
        ))
        .map_err(|e| crate::error::input(e.to_string()))?
        .into_rgba8();
        Artwork::from_source(logo)
            .and_then(|art| {
                art.with_composition(crate::presentation::composition::Composition {
                    scale: 80,
                    offset: [0, -3],
                    background: crate::presentation::composition::Background::Dawn,
                    ..Default::default()
                })
            })
            .map(Some)
            .map_err(crate::error::input)
    }

    pub(crate) fn watermark(self) -> AuthoringResult<image::RgbaImage> {
        self.texture(0)
    }

    pub(crate) fn texture(self, index: usize) -> AuthoringResult<image::RgbaImage> {
        match self {
            Self::Dawn => crate::watermark::render_dawn_texture(index),
            Self::Sunrise => crate::watermark::render_output_texture(index),
        }
    }
}

pub(crate) fn dawn_background(y: u32, height: u32) -> image::Rgba<u8> {
    let stops = [[3, 15, 38], [4, 24, 62], [8, 89, 242]];
    let t = f64::from(y) / f64::from(height.saturating_sub(1).max(1));
    let (from, to, mix) = if t < 0.6 {
        (stops[0], stops[1], t / 0.6)
    } else {
        (stops[1], stops[2], ((t - 0.6) / 0.4).powi(2))
    };
    let [r, g, b] = [0, 1, 2]
        .map(|i| (f64::from(from[i]) * (1.0 - mix) + f64::from(to[i]) * mix).round() as u8);
    image::Rgba([r, g, b, 255])
}
