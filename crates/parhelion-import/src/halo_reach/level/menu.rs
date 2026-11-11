//! A level's English menu title and description, from MCC's `maps/info/<map>.mapinfo` beside the
//! caches. The blob is a big-endian BLF chunk stream whose `levl` chunk (version 7.1) holds twelve
//! language names of 32 UTF-16 units from payload `+0x08` and twelve descriptions of 128 units from
//! `+0x308`, English first. A description that is a `$` key names its English text in MCC's
//! `Data/UI/Localization/EN_Global.bin` or `EN_HaloReach.bin`.
use anyhow::{Context, Result, bail, ensure};
use std::path::Path;

const LEVEL_TAG: &[u8; 4] = b"levl";
const LEVEL_VERSION: (u16, u16) = (7, 1);
const NAME: usize = 0x08;
const NAME_UNITS: usize = 32;
const DESCRIPTION: usize = 0x308;
const DESCRIPTION_UNITS: usize = 128;
/// A localization container's magic, repeated at `+0x40`, its file-stem hash at `+0x28`, and the
/// lengths of its key array and string data at `+0x78`, both from `+0x80`.
const LOCALIZATION_MAGIC: u32 = 0x90CE_6B7A;
const LOCALIZATION_FILES: [&str; 2] = ["EN_Global.bin", "EN_HaloReach.bin"];

/// A level's English menu text, with Reach's `|n` display token as a line break.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Text {
    pub title: String,
    pub description: String,
}

/// The menu text of `map` in the MCC Reach caches folder `maps`, or `None` when the level has no
/// `.mapinfo`.
pub fn read(maps: &Path, map: &str) -> Result<Option<Text>> {
    let stem = Path::new(map)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("Reach map file has no name")?;
    let Ok(info) = std::fs::read(maps.join("info").join(format!("{stem}.mapinfo"))) else {
        return Ok(None);
    };
    let payload = level_chunk(&info).with_context(|| format!("{stem}.mapinfo"))?;
    let title = field(payload, NAME, NAME_UNITS)?;
    let mut description = field(payload, DESCRIPTION, DESCRIPTION_UNITS)?;
    ensure!(
        !title.trim().is_empty(),
        "{stem}.mapinfo has no English title"
    );
    if let Some(key) = description.strip_prefix('$') {
        description =
            localized(maps, key).with_context(|| format!("{stem}.mapinfo description"))?;
    }
    Ok(Some(Text {
        title,
        description: description.replace("|n", "\n"),
    }))
}

/// The payload of the blob's one `levl` chunk, walking the chunks by their lengths.
fn level_chunk(data: &[u8]) -> Result<&[u8]> {
    ensure!(data.get(..4) == Some(b"_blf"), "Not a BLF blob");
    let mut at = 0;
    let mut found = None;
    while at + 12 <= data.len() {
        let tag = &data[at..at + 4];
        let size = u32::from_be_bytes(data[at + 4..at + 8].try_into()?) as usize;
        ensure!(
            size >= 12 && at + size <= data.len(),
            "Invalid BLF chunk length"
        );
        let version = (
            u16::from_be_bytes(data[at + 8..at + 10].try_into()?),
            u16::from_be_bytes(data[at + 10..at + 12].try_into()?),
        );
        if tag == LEVEL_TAG {
            ensure!(found.is_none(), "More than one level chunk");
            ensure!(version == LEVEL_VERSION, "Level chunk version {version:?}");
            found = Some(&data[at + 12..at + size]);
        }
        if tag == b"_eof" {
            return found.context("No level chunk");
        }
        at += size;
    }
    bail!("Missing BLF end chunk")
}

/// The English field of `units` big-endian UTF-16 code units at `offset`, to its first zero.
fn field(payload: &[u8], offset: usize, units: usize) -> Result<String> {
    let bytes = payload
        .get(offset..offset + units * 2)
        .context("Level chunk ends before its field")?;
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&units).context("Field is not UTF-16")
}

/// CRC-32/MPEG-2 of `text`: polynomial `04C11DB7`, initial value `FFFFFFFF`, no reflection and no
/// final XOR, the hash MCC's localization keys and file stems take.
fn crc_mpeg(text: &str) -> u32 {
    let mut value = u32::MAX;
    for byte in text.bytes() {
        value ^= u32::from(byte) << 24;
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04C1_1DB7
            } else {
                value << 1
            };
        }
    }
    value
}

/// The English string of localization `key`, without its `$`, from the MCC install around `maps`.
/// Every container that holds the key must agree on its text.
fn localized(maps: &Path, key: &str) -> Result<String> {
    let folder = maps
        .parent()
        .and_then(Path::parent)
        .context("Reach maps folder is not inside MCC")?
        .join("Data")
        .join("UI")
        .join("Localization");
    let target = crc_mpeg(&key.to_ascii_uppercase());
    let mut found: Option<String> = None;
    for name in LOCALIZATION_FILES {
        let path = folder.join(name);
        let data = std::fs::read(&path).with_context(|| format!("Reading {}", path.display()))?;
        let u32_at = |at: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                data.get(at..at + 4)
                    .context("Short localization file")?
                    .try_into()?,
            ))
        };
        ensure!(
            u32_at(0)? == LOCALIZATION_MAGIC && u32_at(0x40)? == LOCALIZATION_MAGIC,
            "{name} is not a localization container"
        );
        let stem = name.trim_end_matches(".bin").to_ascii_uppercase();
        ensure!(
            u32_at(0x28)? == crc_mpeg(&stem),
            "{name} names another file"
        );
        let (keys, text) = (u32_at(0x78)? as usize, u32_at(0x7C)? as usize);
        ensure!(
            keys % 4 == 0 && 0x80 + keys + text <= data.len(),
            "{name} lengths run past the file"
        );
        let hashes = (0..keys / 4)
            .map(|i| u32_at(0x80 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        let strings = data[0x80 + keys..0x80 + keys + text]
            .split(|&b| b == 0)
            .collect::<Vec<_>>();
        ensure!(
            strings.len() > hashes.len(),
            "{name} has fewer strings than keys"
        );
        ensure!(
            strings[hashes.len()..].iter().all(|s| s.is_empty()),
            "{name} has unconsumed string data"
        );
        let matches = hashes
            .iter()
            .enumerate()
            .filter(|&(_, &hash)| hash == target)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "{name} holds key {key} more than once");
        if let Some(&i) = matches.first() {
            let value = String::from_utf8(strings[i].to_vec())
                .with_context(|| format!("{name} string for {key}"))?;
            if let Some(previous) = &found {
                ensure!(previous == &value, "Localization banks disagree on {key}");
            }
            found = Some(value);
        }
    }
    found.with_context(|| format!("No English string for {key}"))
}
