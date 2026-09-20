//! Source acceptance differs from the dimensions emitted by each native renderer.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    #[default]
    Perk,
    Badge,
    Watermark,
}
impl Purpose {
    pub fn max_edge(self) -> u32 {
        if self == Self::Perk { 512 } else { 4096 }
    }
    pub fn svg_edge(self) -> u32 {
        if self == Self::Perk { 96 } else { 512 }
    }
    pub fn max_bytes(self) -> u64 {
        if self == Self::Perk {
            1024 * 1024
        } else {
            16 * 1024 * 1024
        }
    }
    pub fn transparent(self) -> bool {
        self != Self::Badge
    }
    pub fn accepts_size(self, w: u32, h: u32) -> bool {
        w >= 16
            && h >= 16
            && w <= self.max_edge()
            && h <= self.max_edge()
            && (self != Self::Perk || (w == h && w >= super::perk_quality::EDGE))
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::Perk => {
                "Glyph on a transparent square PNG, 96 to 512 pixels per side, up to 1 MiB. Imported artwork is resized to the native 96 × 96 perk format."
            }
            Self::Badge => {
                "PNG or JPEG, 16 to 4096 pixels per side, up to 16 MiB. Crop and placement are applied in the badge editor."
            }
            Self::Watermark => {
                "Transparent PNG, 16 to 4096 pixels per side, up to 16 MiB. The watermark editor generates the required game sizes."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perk_sources_must_be_square_and_never_upscaled() {
        for edge in [96, 192, 256, 512] {
            assert!(Purpose::Perk.accepts_size(edge, edge));
        }
        for (w, h) in [(16, 16), (64, 64), (96, 128), (128, 96), (1024, 1024)] {
            assert!(!Purpose::Perk.accepts_size(w, h));
        }
        assert!(Purpose::Badge.accepts_size(768, 256));
        assert!(Purpose::Watermark.accepts_size(768, 256));
    }
}
