//! Perk artwork is white ink on transparency, not a transparent item render.
pub(crate) const EDGE: u32 = 96;

/// None rejects unusable artwork. Some(true) identifies white glyphs, Some(false) other colors.
pub(crate) fn inspect(pixels: impl IntoIterator<Item = [u8; 4]>) -> Option<bool> {
    let mut count = 0u64;
    let mut clear = 0u64;
    let mut opaque = 0u64;
    let mut ink = 0u64;
    let mut colored = 0u64;
    let mut white = 0u64;
    for [r, g, b, a] in pixels {
        count += 1;
        clear += u64::from(a <= 8);
        opaque += u64::from(a >= 240);
        // Ignore invisible RGB and faint edge noise from texture compression.
        if a < 16 {
            continue;
        }
        let weight = u64::from(a);
        let low = r.min(g).min(b);
        let high = r.max(g).max(b);
        ink += weight;
        if high - low > 12 {
            colored += weight;
        } else if low >= 224 {
            white += weight;
        }
    }
    let usable =
        count > 0 && clear * 10 >= count && opaque * 200 >= count && ink * 100 >= count * 255;
    usable.then_some(colored * 200 <= ink && white * 100 >= ink * 95)
}

#[cfg(test)]
pub(crate) fn accepts(pixels: impl IntoIterator<Item = [u8; 4]>) -> bool {
    inspect(pixels) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(color: [u8; 4]) -> Vec<[u8; 4]> {
        let mut pixels = vec![[120, 0, 240, 0]; (EDGE * EDGE) as usize];
        for y in 24..72 {
            for x in 24..72 {
                pixels[(y * EDGE + x) as usize] = color;
            }
        }
        pixels
    }

    #[test]
    fn accepts_white_glyphs_with_antialiasing_and_invisible_rgb() {
        let mut pixels = glyph([255; 4]);
        pixels[24 * EDGE as usize + 24] = [255, 255, 255, 32];
        pixels[24 * EDGE as usize + 25] = [240, 244, 248, 255];
        assert!(accepts(pixels));
    }

    #[test]
    fn rejects_colored_dim_blank_and_background_filled_artwork() {
        assert!(!accepts(glyph([100, 200, 255, 255])));
        assert_eq!(inspect(glyph([100, 200, 255, 255])), Some(false));
        assert!(!accepts(glyph([120, 120, 120, 255])));
        assert!(!accepts(glyph([255, 255, 255, 32])));
        assert!(!accepts(glyph([0; 4])));
        let mut opaque = vec![[255; 4]; (EDGE * EDGE) as usize];
        opaque[0] = [0; 4];
        assert!(!accepts(opaque));
        let mut tiny = glyph([0; 4]);
        tiny[100] = [255; 4];
        assert!(!accepts(tiny));
    }
}
