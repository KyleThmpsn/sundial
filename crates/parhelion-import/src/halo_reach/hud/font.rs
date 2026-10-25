use super::*;

struct Font<'a>(&'a [u8]);
impl Font<'_> {
    fn bytes(&self, at: usize, length: usize) -> Result<&[u8]> {
        self.0
            .get(at..at.checked_add(length).context("Font extent overflow")?)
            .context("Font extent exceeds package")
    }
    fn word(&self, at: usize) -> Result<usize> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()) as usize)
    }
    fn short(&self, at: usize) -> Result<usize> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()) as usize)
    }
}

pub(super) fn read(bytes: &[u8], code: u16) -> Result<(image::RgbaImage, usize)> {
    let font = Font(bytes);
    ensure!(font.word(0)? == 0xc0000004, "Unsupported icon font package");
    let count = font.word(4)?;
    ensure!(
        (1..=64).contains(&count) && font.word(8)? == 1048,
        "Invalid icon font directory"
    );
    let mut selected = None;
    for index in 0..count {
        let header = font.word(8 + index * 12)?;
        ensure!(
            font.word(header)? == 0xf0000006,
            "Unsupported icon font header"
        );
        let name = font.bytes(header + 4, 32)?;
        let end = name
            .iter()
            .position(|&b| b == 0)
            .context("Unterminated font name")?;
        if &name[..end] == b"icon_x3\\fixedsys_hud-15" {
            ensure!(selected.replace(index).is_none(), "Duplicate HUD font");
        }
    }
    let selected = selected.context("HUD font is absent")?;
    let blocks = font.word(1044)?;
    let chunks = [0x8000usize, 0xc000, 0x10000]
        .into_iter()
        .filter(|&at| font.short(at).ok() == Some(8))
        .collect::<Vec<_>>();
    ensure!(
        chunks.len() == 1 && blocks > 0,
        "Ambiguous font character blocks"
    );
    let chunk = chunks[0];
    ensure!(
        (blocks + 1).checked_mul(chunk) == Some(bytes.len()),
        "Font block length differs"
    );
    let mut result = None;
    for block in 0..blocks {
        let start = (block + 1) * chunk;
        let count = font.short(start + 2)?;
        let table = font.short(start + 4)?;
        let data = font.short(start + 6)?;
        ensure!(
            font.short(start)? == 8 && 8 + count * 8 == table && table + data <= chunk,
            "Invalid font character block"
        );
        for row in (start + 8..start + table).step_by(8) {
            if font.short(row)? != usize::from(code) || font.short(row + 2)? != selected {
                continue;
            }
            let offset = font.word(row + 4)?;
            ensure!(
                offset >= table && offset + 16 <= table + data,
                "Glyph header exceeds character block"
            );
            let at = start + offset;
            let length = font.word(at + 4)?;
            let width = font.short(at + 8)?;
            let height = font.short(at + 10)?;
            ensure!(
                (1..=4096).contains(&width)
                    && (1..=4096).contains(&height)
                    && offset + 16 + length <= table + data,
                "Glyph dimensions or storage exceed limits"
            );
            let rgba = decode(font.bytes(at + 16, length)?, width * height)?;
            let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
                .context("Invalid glyph pixels")?;
            ensure!(
                result.replace((image, at)).is_none(),
                "Duplicate source HUD glyph"
            );
        }
    }
    result.context("Source HUD glyph is absent from its named font")
}

fn decode(bytes: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut colors = Vec::with_capacity(expected);
    let mut rgb = 0xfffu16;
    let mut at = 0;
    while at < bytes.len() {
        let code = bytes[at];
        at += 1;
        let kind = code >> 6;
        if code == 0 {
            let data = bytes.get(at..at + 2).context("Truncated glyph color")?;
            let color = u16::from_be_bytes(data.try_into().unwrap());
            at += 2;
            rgb = color & 0xfff;
            colors.push(color);
        } else if kind < 2 {
            colors.extend(std::iter::repeat_n(
                rgb | if kind == 1 { 0xf000 } else { 0 },
                usize::from(code & 63),
            ));
        } else {
            let alpha = u16::from((code >> 3) & 7);
            colors.push(rgb | (((alpha << 1) | (alpha & 1)) << 12));
            let tail = u16::from(code & 7);
            if kind == 3 {
                colors.push(rgb | (((tail << 1) | (tail & 1)) << 12));
            } else if tail != 0 {
                colors.extend(std::iter::repeat_n(
                    rgb | if tail & 4 == 0 { 0xf000 } else { 0 },
                    usize::from(5 - (tail & 3)),
                ));
            }
        }
        ensure!(colors.len() <= expected, "Glyph run exceeds its dimensions");
    }
    ensure!(
        colors.len() == expected,
        "Glyph pixel count differs from its dimensions"
    );
    Ok(colors
        .into_iter()
        .flat_map(|color| {
            [
                ((color >> 8) & 15) as u8 * 17,
                ((color >> 4) & 15) as u8 * 17,
                (color & 15) as u8 * 17,
                (color >> 12) as u8 * 17,
            ]
        })
        .collect())
}
