//! Follow an emitted status identity through its private row, artwork and localized names.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: u32,
    actual: u32,
) -> CheckResult<()> {
    let table = stock.tag(0x80D5FFD3, Some(0x80804A55))?;
    let old = table.array(8, 112, Some(0x80804A59))?;
    if old.iter().any(|&r| table.u32(r).ok() == Some(original)) {
        assert_eq!(actual, original);
        return Ok(());
    }
    assert_ne!(actual, original);
    let new = emitted.tag(0x80D5FFD3, Some(0x80804A55))?;
    let rows = new.array(8, 112, Some(0x80804A59))?;
    for r in old {
        assert_ne!(table.u32(r)?, actual);
        let n = *rows
            .iter()
            .find(|&&n| new.u32(n).ok() == table.u32(r).ok())
            .ok_or("Lost stock HUD row")?;
        assert_eq!(&table.0[r..r + 112], &new.0[n..n + 112]);
    }
    let row = *rows
        .iter()
        .find(|&&r| new.u32(r).ok() == Some(actual))
        .ok_or("Missing private HUD row")?;
    let layer_tag = new.u32(row + 4)?;
    assert!(stock.tag(layer_tag, None).is_err());
    let layer = emitted.tag(layer_tag, Some(0x80804A69))?;
    let (width, height, original_pixels) = artwork(source, original)?;
    let mut matched_source_size = false;
    let lanes = layer.array(32, 16, Some(0x80804A6A))?;
    assert!(!lanes.is_empty());
    for lane in lanes {
        let frames = layer.array(lane, 4, Some(0x80804A6E))?;
        assert!(!frames.is_empty());
        for frame in frames {
            let tag = layer.u32(frame)?;
            assert!(stock.tag(tag, None).is_err());
            let header = emitted.tag(tag, None)?;
            assert!(matches!(header.u32(4)?, 28 | 29));
            let pixels_tag = emitted
                .manager
                .get_entry(tiger_pkg::TagHash(tag))
                .ok_or("HUD texture entry")?
                .reference;
            assert!(stock.tag(pixels_tag, None).is_err());
            let pixels = emitted.tag(pixels_tag, None)?;
            assert_eq!(
                pixels.0.len(),
                usize::from(header.u16(14)?) * usize::from(header.u16(16)?) * 4
            );
            assert!(pixels.0.chunks_exact(4).any(|p| p[3] != 0));
            if (header.u16(14)?, header.u16(16)?) == (width, height) {
                assert_eq!(pixels.0, original_pixels);
                matched_source_size = true;
            }
        }
    }
    assert!(
        matched_source_size,
        "No native HUD image retains the source dimensions"
    );
    let mut expected = BTreeSet::new();
    for tag in source.classes(0x8080035A) {
        let header = source.read(tag)?;
        if let Some(index) = header
            .array(8, 4, Some(0x80800070))?
            .iter()
            .position(|&r| header.u32(r).ok() == Some(original))
        {
            expected.insert(text(&source.read(header.u32(24)?)?, index)?);
        }
    }
    assert_eq!(expected.len(), 1);
    let names = emitted.tag(0x815B961E, Some(0x80809A88))?;
    let index = names
        .array(8, 4, None)?
        .iter()
        .position(|&r| names.u32(r).ok() == Some(actual))
        .ok_or("Private HUD name")?;
    // The private builder publishes the imported English text into all thirteen locales.
    for at in (24..76).step_by(4) {
        let locale = emitted.tag(names.u32(at)?, None)?;
        assert!(expected.contains(&text(&locale, index)?));
    }
    Ok(())
}

fn artwork(source: &Source, name: u32) -> CheckResult<(u16, u16, Vec<u8>)> {
    use parhelion_import::d1_roi::texture::{Descriptor, decode};
    let mut layers = BTreeSet::new();
    for tag in source.classes(0x808022D2) {
        let table = source.read(tag)?;
        for row in table.array(8, 112, Some(0x8080295C))? {
            if table.u32(row)? == name {
                layers.insert(table.u32(row + 4)?);
            }
        }
    }
    assert_eq!(layers.len(), 1);
    let layer = source.read(*layers.first().ok_or("Source HUD layer")?)?;
    let lanes = layer.array(layer.pointer(16)?, 16, Some(0x80802478))?;
    assert_eq!(lanes.len(), 1);
    let frames = layer.array(lanes[0], 4, Some(0x80802B99))?;
    assert_eq!(frames.len(), 1);
    let texture = layer.u32(frames[0])?;
    let header = source.read(texture)?;
    let (width, height) = (header.u16(0x30)?, header.u16(0x32)?);
    assert!(matches!(header.u32(0)?, 28 | 29));
    assert_eq!(header.u16(0x34)?, 1);
    assert_eq!(header.u16(0x36)?, 1);
    assert_eq!(header.u8(0x39)?, 1);
    let pixels = decode(
        &Descriptor {
            format: header.u32(0)?,
            width: width.into(),
            height: height.into(),
            depth: 1,
            layers: 1,
            mips: 1,
            tile_mode: header.u32(4)?,
        },
        &source.read(source.reference(texture)?)?.0,
    )?;
    Ok((width, height, pixels))
}

fn text(data: &Payload, index: usize) -> CheckResult<String> {
    let combos = data.array(0x48, 16, None)?;
    let combo = *combos.get(index).ok_or("Localized index")?;
    let parts = data.array(8, 32, None)?;
    let first = parts
        .iter()
        .position(|&r| data.pointer(combo).ok() == Some(r))
        .ok_or("Localized parts")?;
    let count = usize::try_from(data.u64(combo + 8)?)?;
    let bytes = data.array_range(0x38, 1, None)?;
    let mut result = String::new();
    for &part in parts.get(first..first + count).ok_or("Localized span")? {
        let start = data.pointer(part + 8)?;
        let end = start + usize::from(data.u16(part + 20)?);
        assert!(start >= bytes.start && end <= bytes.end);
        for c in std::str::from_utf8(&data.0[start..end])?.chars() {
            result.push(
                char::from_u32(c as u32 + u32::from(data.u16(part + 24)?))
                    .ok_or("Localized scalar")?,
            );
        }
    }
    Ok(result)
}
