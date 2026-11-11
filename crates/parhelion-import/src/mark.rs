//! Each import source's own mark, read from the source game's files, for the Collections badge of
//! the mod that holds the source's content: the Halo: Reach logo of MCC's upsell screen, the seal
//! of Destiny 2's Monument of Triumph, the weathered tricorn of Rise of Iron's boot flow and
//! Marathon's logo.
use crate::{d2_mot::service::Settings, tiger::reader::Reader};
use anyhow::{Context, Result, bail, ensure};
use image::{RgbaImage, imageops::FilterType};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// MCC's pak, two folders above Halo: Reach's maps.
const PAK: &str = "MCC/Content/Paks/MCC-WindowsNoEditor.pak";
const PAK_MAGIC: u32 = 0x5A6F_12E1;
const PAK_VERSION: u32 = 7;
const PAK_FOOTER: u64 = 44;
/// The Halo: Reach logo of MCC's upsell screen. Its texture is 511 by 256 and the screen draws it
/// at 512 by 175, so it takes that shape.
const REACH_LOGO: &str = "MCC/Content/UI/Features/Upsell/T_Im_ReachLogo.uexp";
const REACH_SHAPE: [u32; 2] = [512, 175];
/// The Monument of Triumph's presentation node, whose display names the seal's icon.
const MONUMENT_NODE: u32 = 0xEFDA_1CAA;
/// Presentation node displays: 44-byte rows with the node's hash at +0 and its icon index at +4.
const NODE_DISPLAYS: u32 = 0x8080_5803;
const NODE_DISPLAY_SIZE: usize = 44;
const NODE_DISPLAY_ICON: usize = 4;
/// The weathered tricorn of Rise of Iron's boot flow, a 384 by 384 texture of
/// `xboxone_ui_bootflow`.
const TRICORN: u32 = 0x8089_2384;
/// Marathon's logo, the inventory icon whose primary artwork is 552 by 552.
const MARATHON_LOGO: usize = 61;

/// The mark of the import source named `source`, read from the game folders `settings` chose.
pub fn read(source: &str, settings: &Settings) -> Result<RgbaImage> {
    let folder = |path: &Option<PathBuf>, game: &str| -> Result<PathBuf> {
        path.clone()
            .with_context(|| format!("No {game} folder is chosen"))
    };
    match source {
        "halo_reach" => reach(&folder(&settings.reach_maps, "Halo: Reach")?),
        "d2_mot" => monument(&folder(&settings.modern_packages, "Destiny 2")?),
        "marathon" => marathon(&folder(&settings.marathon_packages, "Marathon")?),
        "d1_roi" => {
            crate::d1_roi::Source::open(&folder(&settings.d1_roi, "Destiny 1")?, |_, _| {})?
                .texture_image(TRICORN)
        }
        _ => bail!("No mark is known for {source}"),
    }
}

fn reach(maps: &Path) -> Result<RgbaImage> {
    let root = maps
        .parent()
        .and_then(Path::parent)
        .context("The Halo: Reach maps folder is not inside MCC")?;
    let logo = bgra_texture(&pak_entry(&root.join(PAK), REACH_LOGO)?)?;
    Ok(image::imageops::resize(
        &logo,
        REACH_SHAPE[0],
        REACH_SHAPE[1],
        FilterType::Lanczos3,
    ))
}

fn monument(packages: &Path) -> Result<RgbaImage> {
    let scratch = tempfile::tempdir()?;
    let mut r = Reader::discovery(packages, scratch.path(), true)?;
    let mut icon = None;
    for tag in r.classes(NODE_DISPLAYS) {
        let table = r.tag(tag, Some(NODE_DISPLAYS))?;
        for row in table.array(8, NODE_DISPLAY_SIZE, None)? {
            if table.u32(row)? == MONUMENT_NODE {
                icon = Some(table.u16(row + NODE_DISPLAY_ICON)?);
            }
        }
    }
    let icon = icon.context("No presentation node display names the Monument of Triumph")?;
    let image = crate::d2_mot::icon::read_index(&mut r, usize::from(icon))?;
    RgbaImage::from_raw(image.width.into(), image.height.into(), image.rgba)
        .context("The Monument of Triumph seal's size differs from its pixels")
}

fn marathon(packages: &Path) -> Result<RgbaImage> {
    let scratch = tempfile::tempdir()?;
    let mut r = Reader::marathon_discovery(packages, scratch.path())?;
    let layer = crate::marathon::inventory_icon(&mut r, MARATHON_LOGO)?;
    let rgba = crate::tiger::icon::decode(&layer)?;
    RgbaImage::from_raw(layer.width.into(), layer.height.into(), rgba)
        .context("The Marathon logo's size differs from its pixels")
}

/// The bytes of `name` in the version 7 Unreal pak at `path`. The footer names the index, which
/// lists each entry's record: offset, size and raw size, compression method, SHA-1, each
/// compressed block's bounds, then the encryption flag and block size. A copy of the record
/// precedes the entry's data.
fn pak_entry(path: &Path, name: &str) -> Result<Vec<u8>> {
    let mut file = File::open(path).with_context(|| format!("Opening {}", path.display()))?;
    let length = file.metadata()?.len();
    ensure!(length >= PAK_FOOTER, "MCC's pak is truncated");
    file.seek(SeekFrom::Start(length - PAK_FOOTER))?;
    let mut footer = [0; PAK_FOOTER as usize];
    file.read_exact(&mut footer)?;
    let mut tail = Cursor {
        bytes: &footer,
        at: 0,
    };
    ensure!(
        tail.u32()? == PAK_MAGIC && tail.u32()? == PAK_VERSION,
        "MCC's pak is not a version 7 Unreal pak"
    );
    let (offset, size) = (tail.u64()?, tail.u64()?);
    ensure!(
        offset
            .checked_add(size)
            .is_some_and(|end| end <= length - PAK_FOOTER)
            && size < 1 << 30,
        "MCC's pak index is out of bounds"
    );
    file.seek(SeekFrom::Start(offset))?;
    let mut index = vec![0; size as usize];
    file.read_exact(&mut index)?;
    let mut index = Cursor {
        bytes: &index,
        at: 0,
    };
    index.string()?;
    for _ in 0..index.u32()? {
        let entry = index.string()?;
        let (offset, _, raw) = (index.u64()?, index.u64()?, index.u64()?);
        let method = index.u32()?;
        index.skip(20)?;
        let mut blocks = Vec::new();
        if method != 0 {
            for _ in 0..index.u32()? {
                blocks.push((index.u64()?, index.u64()?));
            }
        }
        let flags = index.u8()?;
        index.skip(4)?;
        if entry != name {
            continue;
        }
        ensure!(flags & 1 == 0, "{name} is encrypted");
        ensure!(raw < 64 << 20, "{name} is too large for a logo");
        let record = 53
            + if method == 0 {
                0
            } else {
                4 + 16 * blocks.len() as u64
            };
        let mut data = Vec::with_capacity(raw as usize);
        match method {
            0 => {
                file.seek(SeekFrom::Start(offset + record))?;
                data.resize(raw as usize, 0);
                file.read_exact(&mut data)?;
            }
            1 => {
                for (start, end) in blocks {
                    // Later paks give block bounds from the entry's record, earlier ones from the
                    // file's start.
                    let base = if start < offset { offset } else { 0 };
                    ensure!(
                        end >= start && end - start < 1 << 24,
                        "{name} has a bad block"
                    );
                    file.seek(SeekFrom::Start(base + start))?;
                    let mut block = vec![0; (end - start) as usize];
                    file.read_exact(&mut block)?;
                    flate2::read::ZlibDecoder::new(&block[..]).read_to_end(&mut data)?;
                }
            }
            _ => bail!("{name} uses compression method {method}"),
        }
        ensure!(data.len() as u64 == raw, "{name} is not its recorded size");
        return Ok(data);
    }
    bail!("MCC's pak has no {name}")
}

/// The top mip of a cooked Texture2D export in 8-bit BGRA. Its platform data holds the size, then
/// the pixel format's name, then each mip's bulk data: flags, element count, size on disk and
/// offset, with the pixels inline after them.
fn bgra_texture(export: &[u8]) -> Result<RgbaImage> {
    const FORMAT: &[u8] = b"PF_B8G8R8A8\0";
    let read = |at: usize| -> Result<i32> {
        Ok(i32::from_le_bytes(
            export
                .get(at..at + 4)
                .context("Truncated texture export")?
                .try_into()?,
        ))
    };
    let at = export
        .windows(FORMAT.len())
        .position(|window| window == FORMAT)
        .context("The texture is not 8-bit BGRA")?;
    ensure!(
        at >= 16 && read(at - 4)? == FORMAT.len() as i32,
        "The texture's pixel format is not a name"
    );
    let (width, height) = (read(at - 16)?, read(at - 12)?);
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height),
        "The texture is {width} by {height}"
    );
    let size = width * height * 4;
    let header = (at + FORMAT.len()..export.len().saturating_sub(20))
        .find(|&header| read(header + 4).ok() == Some(size) && read(header + 8).ok() == Some(size))
        .context("The texture holds no inline top mip")?;
    let mut rgba = export
        .get(header + 20..header + 20 + size as usize)
        .context("The texture's top mip is truncated")?
        .to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    RgbaImage::from_raw(width as u32, height as u32, rgba)
        .context("The texture's size differs from its pixels")
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let bytes = self
            .bytes
            .get(self.at..self.at.checked_add(count).context("Pak index overflow")?)
            .context("Truncated pak index")?;
        self.at += count;
        Ok(bytes)
    }

    fn skip(&mut self, count: usize) -> Result<()> {
        self.take(count).map(drop)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }

    /// An Unreal string: a length, negative for UTF-16, then the characters with a terminator.
    fn string(&mut self) -> Result<String> {
        let count = self.u32()? as i32;
        let text = if count < 0 {
            let units = self
                .take(count.unsigned_abs() as usize * 2)?
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .collect::<Vec<_>>();
            String::from_utf16(&units)?
        } else {
            self.take(count as usize)?
                .iter()
                .map(|&byte| char::from(byte))
                .collect()
        };
        Ok(text.trim_end_matches('\0').to_owned())
    }
}
