//! A Reach multiplayer level's own picture, as MCC's menus show it. MCC keeps it outside the
//! level's cache, in the level texture pack of its UI data: `levelstexturepack.perm.bin` lists
//! each picture by name, and `levelstexturepack.temp.bin` holds its pixels. A level's picture is
//! named `HREACHMP_` and the level's English name in capitals without spaces, with `LARGE` for the
//! wide version, so High Noon's (`cex_hangemhigh`) is `HREACHMP_HIGHNOONLARGE`. The English name is
//! the first of the level's names in its `maps/info/<map>.mapinfo`.
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// The `.mapinfo` blob's level chunk, its tag, and its first name: big-endian UTF-16, 32 units.
const LEVEL_CHUNK: usize = 48;
const LEVEL_TAG: &[u8; 4] = b"levl";
const FIRST_NAME: usize = 68;
const NAME_UNITS: usize = 32;
/// A pack row's tag, before its name, and the fields after the name: width and height as `u16`,
/// then the data's size and its offset in the pixel file as `u64`.
const ROW_TAG: u32 = 0xCDBF_A090;
const ROW_TAG_BEFORE_NAME: usize = 0x44;
const NAME_SIZE: usize = 0x24;
const ROW_DIMENSIONS: usize = 0x30;
const ROW_DATA_SIZE: usize = 0x44;
const ROW_DATA_OFFSET: usize = 0x4C;

/// A level picture's straight RGBA pixels and the name MCC lists it under.
pub struct Picture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The picture of `map` in the MCC Reach caches folder `maps`, the wide one when the pack has it,
/// or `None` when the level has no `.mapinfo`, the folder is not inside MCC, or the pack lists no
/// picture of its name.
pub fn read(maps: &Path, map: &str) -> Result<Option<Picture>> {
    let stem = Path::new(map)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("Reach map file has no name")?;
    let Ok(info) = std::fs::read(maps.join("info").join(format!("{stem}.mapinfo"))) else {
        return Ok(None);
    };
    let name = english_name(&info).with_context(|| format!("{stem}.mapinfo"))?;
    let Some(packs) = maps
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("Data").join("UI").join("texturepacks"))
    else {
        return Ok(None);
    };
    let (Ok(rows), Ok(mut pixels)) = (
        std::fs::read(packs.join("levelstexturepack.perm.bin")),
        std::fs::File::open(packs.join("levelstexturepack.temp.bin")),
    ) else {
        return Ok(None);
    };
    let key = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect::<String>();
    for wanted in [format!("HREACHMP_{key}LARGE"), format!("HREACHMP_{key}")] {
        if let Some(picture) = picture(&rows, &mut pixels, &wanted)? {
            return Ok(Some(picture));
        }
    }
    Ok(None)
}

/// The level's English name, the first of the level chunk's names.
fn english_name(info: &[u8]) -> Result<String> {
    ensure!(
        info.get(..4) == Some(b"_blf") && info.get(LEVEL_CHUNK..LEVEL_CHUNK + 4) == Some(LEVEL_TAG),
        "Not a level information blob"
    );
    let units = info
        .get(FIRST_NAME..FIRST_NAME + NAME_UNITS * 2)
        .context("Level information ends before its names")?
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect::<Vec<_>>();
    let name = String::from_utf16(&units).context("Level name is not UTF-16")?;
    ensure!(
        !name.trim().is_empty(),
        "Level information has no English name"
    );
    Ok(name)
}

/// The pack row named `name` and its pixels, which must be BC1 blocks of its size.
fn picture(rows: &[u8], pixels: &mut std::fs::File, name: &str) -> Result<Option<Picture>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut field = name.as_bytes().to_vec();
    field.push(0);
    let Some(at) = rows
        .windows(field.len())
        .enumerate()
        .filter(|&(at, window)| {
            window == field.as_slice()
                && at >= ROW_TAG_BEFORE_NAME
                && rows.get(at - ROW_TAG_BEFORE_NAME..at - ROW_TAG_BEFORE_NAME + 4)
                    == Some(&ROW_TAG.to_le_bytes()[..])
        })
        .map(|(at, _)| at)
        .next()
    else {
        return Ok(None);
    };
    let word = |offset: usize, size: usize| -> Result<u64> {
        let bytes = rows
            .get(at + offset..at + offset + size)
            .context("Level picture row ends early")?;
        Ok(bytes
            .iter()
            .rev()
            .fold(0u64, |value, &byte| value << 8 | u64::from(byte)))
    };
    ensure!(
        rows.get(at + name.len()..at + NAME_SIZE)
            .is_some_and(|rest| rest.iter().all(|&b| b == 0)),
        "Level picture {name} has a longer name"
    );
    let width = u32::try_from(word(ROW_DIMENSIONS, 2)?)?;
    let height = u32::try_from(word(ROW_DIMENSIONS + 2, 2)?)?;
    let size = usize::try_from(word(ROW_DATA_SIZE, 8)?)?;
    let offset = word(ROW_DATA_OFFSET, 8)?;
    let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096 && size == blocks * 8,
        "Level picture {name} is not {width} by {height} BC1 blocks"
    );
    let mut data = vec![0; size];
    pixels.seek(SeekFrom::Start(offset))?;
    pixels
        .read_exact(&mut data)
        .with_context(|| format!("Level picture {name} leaves its pixel file"))?;
    let mut rgba = vec![0; width as usize * height as usize * 4];
    let across = width.div_ceil(4) as usize;
    for (index, block) in data.chunks_exact(8).enumerate() {
        let mut texels = [0u8; 64];
        bcdec_rs::bc1(block, &mut texels, 16);
        let (bx, by) = (index % across * 4, index / across * 4);
        for y in 0..4 {
            for x in 0..4 {
                let (px, py) = (bx + x, by + y);
                if px < width as usize && py < height as usize {
                    let to = (py * width as usize + px) * 4;
                    rgba[to..to + 4].copy_from_slice(&texels[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4]);
                }
            }
        }
    }
    Ok(Some(Picture {
        name: name.to_owned(),
        width,
        height,
        rgba,
    }))
}
