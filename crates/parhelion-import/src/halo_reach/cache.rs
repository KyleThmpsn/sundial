//! Bounded MCC Reach U13 cache reads. Addresses remain local to their source cache.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const BUILD: &str = "Jun 21 2023 15:35:31";
const HEADER: usize = 40960;
const MAX_METADATA: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tag {
    pub datum: u32,
    pub group: String,
    pub path: String,
    pub offset: Option<usize>,
}

impl Tag {
    pub fn address(&self) -> Result<usize> {
        self.offset
            .with_context(|| format!("{} {} has no metadata", self.group, self.path))
    }
}

struct Span {
    start: usize,
    bytes: Vec<u8>,
}

pub struct Cache {
    pub path: PathBuf,
    pub build: String,
    pub tags: Vec<Option<Tag>>,
    pub file_size: u64,
    header: Vec<u8>,
    debug: Span,
    metadata: Span,
    section_offsets: [u32; 4],
    magic: i64,
}

fn extent(offset: usize, size: usize, length: usize) -> Result<std::ops::Range<usize>> {
    ensure!(
        offset <= length && size <= length - offset,
        "Extent {offset:#x}+{size:#x} outside {length:#x}"
    );
    Ok(offset..offset + size)
}

pub fn file_range(file: &mut File, offset: u64, size: usize) -> Result<Vec<u8>> {
    let length = file.metadata()?.len();
    ensure!(
        offset <= length && size as u64 <= length - offset,
        "Read outside cache file"
    );
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; size];
    file.read_exact(&mut data)?;
    Ok(data)
}

macro_rules! scalar {
    ($name:ident, $ty:ty, $len:expr) => {
        pub fn $name(&self, offset: usize) -> Result<$ty> {
            Ok(<$ty>::from_le_bytes(
                self.bytes(offset, $len)?.try_into().unwrap(),
            ))
        }
    };
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("Opening {}", path.display()))?;
        let header = file_range(&mut file, 0, HEADER)?;
        ensure!(
            &header[..4] == b"daeh" && &header[HEADER - 4..] == b"toof",
            "Invalid MCC cache signatures"
        );
        let word = |o| u32::from_le_bytes(header[o..o + 4].try_into().unwrap());
        ensure!(
            word(4) == 13,
            "Only little-endian MCC Reach U13 caches are supported"
        );
        let file_size = file.metadata()?.len();
        ensure!(
            u64::from(word(8)) == file_size,
            "Declared cache length differs"
        );
        let build = c_string(&header[160..192])?.to_owned();
        ensure!(build == BUILD, "Unverified MCC Reach build {build}");
        let section_offsets = std::array::from_fn(|i| word(1228 + i * 4));
        let mut read_section = |i: usize| -> Result<Span> {
            let start = word(1244 + i * 8).wrapping_add(section_offsets[i]) as usize;
            let length = word(1248 + i * 8) as usize;
            ensure!(
                length <= MAX_METADATA,
                "Metadata section exceeds memory limit"
            );
            Ok(Span {
                start,
                bytes: file_range(&mut file, start as u64, length)?,
            })
        };
        let debug = read_section(0)?;
        let metadata = read_section(2)?;
        let base = u64::from_le_bytes(header[736..744].try_into().unwrap());
        let magic = i64::try_from(base)? - metadata.start as i64;
        let mut cache = Self {
            path: path.to_owned(),
            build,
            tags: Vec::new(),
            file_size,
            header,
            debug,
            metadata,
            section_offsets,
            magic,
        };
        let pointer = cache.u64(744)?;
        if pointer == 0 {
            ensure!(
                cache.u32(32)? == 0,
                "Missing tag directory with declared tags"
            );
            return Ok(cache);
        }
        let index = cache.address64(pointer)?;
        cache.meta(index, 76)?;
        let class_count = cache.u32(index)? as usize;
        let classes = cache.address64(cache.u64(index + 8)?)?;
        let count = cache.u32(index + 16)? as usize;
        let tags = cache.address64(cache.u64(index + 24)?)?;
        ensure!(
            count > 0 && count <= 65536 && count == cache.u32(32)? as usize,
            "Invalid tag directory count"
        );
        cache.meta(
            classes,
            class_count
                .checked_mul(16)
                .context("Class table overflow")?,
        )?;
        cache.meta(tags, count * 8)?;
        let names = cache.header_address(cache.u32(36)?, cache.u32(40)? as usize)?;
        let name_size = cache.u32(40)? as usize;
        let indices = cache.header_address(cache.u32(44)?, count * 4)?;
        for i in 0..count {
            let row = tags + i * 8;
            let class = cache.i16(row)?;
            if class == -1 {
                cache.tags.push(None);
                continue;
            }
            ensure!(
                class >= 0 && (class as usize) < class_count,
                "Tag class outside directory"
            );
            let name_offset = usize::try_from(cache.i32(indices + i * 4)?)?;
            ensure!(name_offset < name_size, "Tag name outside string table");
            let path = cache.string(names + name_offset, name_size - name_offset)?;
            let pointer = cache.u32(row + 4)?;
            let offset = if pointer == 0 {
                None
            } else {
                let at = cache.expand(pointer)?;
                cache.meta(at, 4)?;
                Some(at)
            };
            cache.tags.push(Some(Tag {
                datum: u32::from(cache.u16(row + 2)?) << 16 | i as u32,
                group: cache.code(classes + class as usize * 16)?,
                path,
                offset,
            }));
        }
        Ok(cache)
    }

    pub fn bytes(&self, offset: usize, size: usize) -> Result<&[u8]> {
        if offset < HEADER {
            return Ok(&self.header[extent(offset, size, HEADER)?]);
        }
        for span in [&self.metadata, &self.debug] {
            if offset >= span.start && offset - span.start <= span.bytes.len() {
                return Ok(&span.bytes[extent(offset - span.start, size, span.bytes.len())?]);
            }
        }
        anyhow::bail!(
            "Address {offset:#x}+{size:#x} outside loaded metadata in {}",
            self.path.display()
        )
    }
    pub fn meta(&self, offset: usize, size: usize) -> Result<&[u8]> {
        let offset = offset
            .checked_sub(self.metadata.start)
            .context("Address before metadata")?;
        Ok(&self.metadata.bytes[extent(offset, size, self.metadata.bytes.len())?])
    }
    scalar!(u16, u16, 2);
    scalar!(i16, i16, 2);
    scalar!(u32, u32, 4);
    scalar!(i32, i32, 4);
    scalar!(u64, u64, 8);
    pub fn u8(&self, offset: usize) -> Result<u8> {
        Ok(self.bytes(offset, 1)?[0])
    }
    pub fn f32(&self, offset: usize) -> Result<f32> {
        let v = f32::from_le_bytes(self.bytes(offset, 4)?.try_into().unwrap());
        ensure!(v.is_finite(), "Nonfinite source number at {offset:#x}");
        Ok(v)
    }
    pub fn floats<const N: usize>(&self, offset: usize) -> Result<[f32; N]> {
        let mut out = [0.; N];
        for (i, value) in out.iter_mut().enumerate() {
            *value = self.f32(offset + i * 4)?;
        }
        Ok(out)
    }
    fn address64(&self, pointer: u64) -> Result<usize> {
        Ok(usize::try_from(i64::try_from(pointer)? - self.magic)?)
    }
    pub fn expand(&self, pointer: u32) -> Result<usize> {
        self.address64((u64::from(pointer) << 2) + 0x50000000)
    }
    fn header_address(&self, pointer: u32, length: usize) -> Result<usize> {
        let offset = pointer.wrapping_add(self.section_offsets[0]) as usize;
        extent(
            offset
                .checked_sub(self.debug.start)
                .context("Address before debug section")?,
            length,
            self.debug.bytes.len(),
        )?;
        Ok(offset)
    }
    pub fn string(&self, offset: usize, maximum: usize) -> Result<String> {
        Ok(c_string(self.bytes(offset, maximum)?)?.to_owned())
    }
    pub fn code(&self, offset: usize) -> Result<String> {
        let b = self.bytes(offset, 4)?;
        ensure!(b.iter().all(u8::is_ascii), "Non-ASCII class code");
        Ok(b.iter().rev().map(|&b| char::from(b)).collect())
    }
    pub fn block(&self, offset: usize, stride: usize) -> Result<Vec<usize>> {
        self.meta(offset, 12)?;
        ensure!(stride > 0, "Zero block stride");
        let count = usize::try_from(self.i32(offset)?)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let pointer = self.u32(offset + 4)?;
        ensure!(pointer != 0, "Nonempty block with null pointer");
        let start = self.expand(pointer)?;
        self.meta(
            start,
            count.checked_mul(stride).context("Block size overflow")?,
        )?;
        Ok((0..count).map(|i| start + i * stride).collect())
    }
    pub fn reference(&self, offset: usize, group: Option<&str>) -> Result<Option<Tag>> {
        self.meta(offset, 16)?;
        let datum = self.u32(offset + 12)?;
        if datum == u32::MAX {
            return Ok(None);
        }
        let tag = self
            .tags
            .get((datum & 65535) as usize)
            .and_then(Option::as_ref)
            .context("Missing referenced tag")?;
        ensure!(
            tag.datum == datum && tag.group == self.code(offset)?,
            "Reference salt or class differs"
        );
        ensure!(
            group.is_none_or(|g| g == tag.group),
            "Unexpected reference class {}",
            tag.group
        );
        Ok(Some(tag.clone()))
    }
    pub fn only(&self, group: &str) -> Result<&Tag> {
        let mut tags = self.tags.iter().flatten().filter(|t| t.group == group);
        let tag = tags
            .next()
            .with_context(|| format!("Missing global {group}"))?;
        ensure!(tags.next().is_none(), "Ambiguous global {group}");
        Ok(tag)
    }
    pub fn find(&self, group: &str, path: &str) -> Result<&Tag> {
        let mut tags = self
            .tags
            .iter()
            .flatten()
            .filter(|t| t.group == group && t.path == path);
        let tag = tags
            .next()
            .with_context(|| format!("Missing {group} {path}"))?;
        ensure!(tags.next().is_none(), "Ambiguous source identity");
        Ok(tag)
    }
    pub fn string_id(&self, value: u32) -> Result<String> {
        if value == 0 || value == u32::MAX {
            return Ok(String::new());
        }
        let mask = (1 << 19) - 1;
        let mut index = value & mask;
        let ns = ((value >> 19) & 255) as usize;
        let count = self.u32(64)? as usize;
        ensure!(ns < count, "String namespace outside table");
        let at = self.header_address(
            self.u32(68)?,
            count.checked_mul(4).context("Namespace size overflow")?,
        )?;
        let sizes = (0..count)
            .map(|i| self.u32(at + i * 4).map(|s| s & mask))
            .collect::<Result<Vec<_>>>()?;
        let addition = if ns != 0 {
            sizes[..ns].iter().sum::<u32>()
        } else if index >= sizes[0] {
            sizes[1..].iter().sum()
        } else {
            0
        };
        index = index
            .checked_add(addition)
            .context("String index overflow")?;
        let total = self.u32(48)? as usize;
        ensure!((index as usize) < total, "String index outside table");
        let indices = self.header_address(
            self.u32(60)?,
            total.checked_mul(4).context("String table overflow")?,
        )?;
        let offset = usize::try_from(self.i32(indices + index as usize * 4)?)?;
        let length = self.u32(56)? as usize;
        ensure!(offset < length, "String offset outside table");
        self.string(
            self.header_address(self.u32(52)?, length)? + offset,
            length - offset,
        )
    }
}

fn c_string(data: &[u8]) -> Result<&str> {
    let end = data
        .iter()
        .position(|&b| b == 0)
        .context("Unterminated cache string")?;
    Ok(std::str::from_utf8(&data[..end])?)
}
