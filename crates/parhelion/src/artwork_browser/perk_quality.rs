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
