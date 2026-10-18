//! Entries a mod's package file holds itself, neither compressed nor encrypted, which Sunrise
//! f1e7c66 reads for merges (`config_bake/package_config_bake_read.cpp`). The file is checked
//! whole first: its header, both regions, and every block it stores with their digests. Entry
//! records alone come from the header and metadata (`merge/package_merge_read.cpp`).
use crate::package_payload::bytes_at;
use sha1::{Digest, Sha1};
use std::{
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
};

/// Version 38 Windows packages with the variant 1 table layout.
const VERSION: u16 = 38;
const PLATFORM: u16 = 2;
const VARIANT: u8 = 1;
const METADATA_LAYOUT: u64 = 1;
const HEADER_SIZE: usize = 0x180;

/// Header fields.
const PACKAGE_PLATFORM: usize = 0x02;
const PACKAGE_ID: usize = 0x04;
const PACKAGE_VARIANT: usize = 0x1A;
const PATCH_ID: usize = 0x20;
const SIGNATURE_OFFSET: usize = 0xB0;
const ENTRY_COUNT: usize = 0xB4;
const BLOCK_COUNT: usize = 0xD0;
const MISC_OFFSET: usize = 0xF0;
const MISC_DIGEST: usize = 0xF8;
const METADATA_OFFSET: usize = 0x110;
const METADATA_DIGEST: usize = 0x118;
const FOOTER_OFFSET: usize = 0x160;
const FILE_SIZE: usize = 0x164;

/// Metadata fields. Each table pointer counts from its own field to the table's capacity.
const TOTAL_SIZE: usize = 0x00;
const LAYOUT: usize = 0x08;
const TABLE_ENTRY_COUNT: usize = 0x10;
const ENTRY_POINTER: usize = 0x18;
const TABLE_BLOCK_COUNT: usize = 0x20;
const BLOCK_POINTER: usize = 0x28;
const TAG_PAIR_COUNT: usize = 0x30;
const TAG_PAIR_POINTER: usize = 0x38;
const ENTRY_TABLE: usize = 0x60;
/// The block table follows the entry table's capacity after this gap.
const BLOCK_TABLE_GAP: usize = 32;
/// A table's prefix sits before its first record: a marker, a capacity and an element class.
const PREFIX_MARKER: usize = 20;
const PREFIX_CAPACITY: usize = 16;
const PREFIX_CLASS: usize = 8;
const TABLE_MARKER: u32 = 0x8080_9FBD;
const ENTRY_CLASS: u32 = 0x8080_9EF3;
const BLOCK_CLASS: u32 = 0x8080_9EEE;
const ENTRY_RECORD: usize = 16;
const BLOCK_RECORD: usize = 48;

const ENTRY_LIMIT: u32 = 1 << 13;
const BLOCK_LIMIT: u32 = 1 << 14;
const PATCH_LIMIT: u16 = 0xFF;
const REGION_LIMIT: u32 = 16 << 20;
/// The loader reads no larger entry.
const ENTRY_BYTE_LIMIT: u32 = 64 << 20;
const BLOCK_SIZE: u64 = 0x40000;
const DISK_ALIGNMENT: u64 = 0x800;
const FOOTER_MARKER: u32 = 0xDEAD_BEEF;

/// Why the loader can't read a file's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
    File,
    Format,
    Missing,
    Damaged,
    Blocks,
    Capacity,
}

impl Unreadable {
    /// What is wrong with the package file, after its name.
    pub fn text(self) -> &'static str {
        match self {
            Self::File => "can't be read",
            Self::Format => "isn't a package layout Sunrise reads",
            Self::Missing => "lacks an entry its merge data describes",
            Self::Damaged => "doesn't match its own digests",
            Self::Blocks => {
                "stores an entry its merge data describes compressed or in another file"
            }
            Self::Capacity => "is larger than Sunrise reads",
        }
    }
}

fn u16_of(bytes: &[u8], at: usize) -> Result<u16, Unreadable> {
    bytes_at(bytes, at)
        .map(u16::from_le_bytes)
        .map_err(|_| Unreadable::Format)
}
fn u32_of(bytes: &[u8], at: usize) -> Result<u32, Unreadable> {
    bytes_at(bytes, at)
        .map(u32::from_le_bytes)
        .map_err(|_| Unreadable::Format)
}
fn u64_of(bytes: &[u8], at: usize) -> Result<u64, Unreadable> {
    bytes_at(bytes, at)
        .map(u64::from_le_bytes)
        .map_err(|_| Unreadable::Format)
}

/// A package's identity and its 16-byte entry records.
#[derive(Clone, Debug)]
pub struct Records {
    pub package: u16,
    pub patch: u16,
    /// The header's metadata digest, which merge data binds to.
    pub digest: [u8; 20],
    pub rows: Vec<[u8; 16]>,
}

impl Records {
    /// The class word of entry `index`.
    pub fn class(&self, index: usize) -> u32 {
        u32::from_le_bytes(
            self.rows[index][..4]
                .try_into()
                .expect("a row holds a class"),
        )
    }
}

/// One block record.
struct Block {
    offset: u32,
    size: u32,
    patch: u16,
    flags: u16,
    digest: [u8; 20],
}

/// One open package and its checked tables.
struct Package<S> {
    source: S,
    size: u64,
    header: [u8; HEADER_SIZE],
    misc: Vec<u8>,
    metadata: Vec<u8>,
    id: u16,
    patch: u16,
    entries: u32,
    blocks: u32,
    block_table: usize,
    block_capacity: usize,
}

fn read_at(source: &mut (impl Read + Seek), at: u64, output: &mut [u8]) -> Result<(), Unreadable> {
    source
        .seek(SeekFrom::Start(at))
        .and_then(|_| source.read_exact(output))
        .map_err(|_| Unreadable::File)
}

/// Whether `bytes` hash to the SHA-1 `digest`.
fn matches(bytes: &[u8], digest: &[u8]) -> bool {
    Sha1::digest(bytes).as_slice() == digest
}

/// Checks a table prefix, returning its capacity.
fn prefix(metadata: &[u8], table: usize, class: u32) -> Result<u64, Unreadable> {
    if table < PREFIX_MARKER
        || u32_of(metadata, table - PREFIX_MARKER)? != TABLE_MARKER
        || u32_of(metadata, table - PREFIX_CLASS)? != class
    {
        return Err(Unreadable::Format);
    }
    u64_of(metadata, table - PREFIX_CAPACITY)
}

impl<S: Read + Seek> Package<S> {
    /// Reads the header, both regions and the table positions.
    fn open(mut source: S, size: u64) -> Result<Self, Unreadable> {
        if size > u64::from(u32::MAX) {
            return Err(Unreadable::Capacity);
        }
        let mut header = [0; HEADER_SIZE];
        read_at(&mut source, 0, &mut header)?;
        if u16_of(&header, 0)? != VERSION
            || u16_of(&header, PACKAGE_PLATFORM)? != PLATFORM
            || header[PACKAGE_VARIANT] != VARIANT
        {
            return Err(Unreadable::Format);
        }
        let patch = u16_of(&header, PATCH_ID)?;
        let entries = u32_of(&header, ENTRY_COUNT)?;
        let blocks = u32_of(&header, BLOCK_COUNT)?;
        let misc_size = u32_of(&header, MISC_OFFSET + 4)?;
        let metadata_size = u32_of(&header, METADATA_OFFSET + 4)?;
        if !(1..=ENTRY_LIMIT).contains(&entries)
            || !(1..=BLOCK_LIMIT).contains(&blocks)
            || patch > PATCH_LIMIT
            || misc_size > REGION_LIMIT
            || metadata_size > REGION_LIMIT
            || (metadata_size as usize) < ENTRY_TABLE
        {
            return Err(Unreadable::Format);
        }
        let mut misc = vec![0; misc_size as usize];
        let mut metadata = vec![0; metadata_size as usize];
        read_at(
            &mut source,
            u64::from(u32_of(&header, MISC_OFFSET)?),
            &mut misc,
        )?;
        read_at(
            &mut source,
            u64::from(u32_of(&header, METADATA_OFFSET)?),
            &mut metadata,
        )?;
        if u64_of(&metadata, LAYOUT)? != METADATA_LAYOUT {
            return Err(Unreadable::Format);
        }
        let entry_capacity = prefix(&metadata, ENTRY_TABLE, ENTRY_CLASS)?;
        if entry_capacity < u64::from(entries) || entry_capacity > u64::from(ENTRY_LIMIT) {
            return Err(Unreadable::Format);
        }
        let block_table = ENTRY_TABLE + entry_capacity as usize * ENTRY_RECORD + BLOCK_TABLE_GAP;
        let block_capacity = prefix(&metadata, block_table, BLOCK_CLASS)?;
        if block_capacity < u64::from(blocks)
            || block_capacity > u64::from(BLOCK_LIMIT)
            || block_table + block_capacity as usize * BLOCK_RECORD > metadata.len()
        {
            return Err(Unreadable::Format);
        }
        Ok(Self {
            source,
            size,
            id: u16_of(&header, PACKAGE_ID)?,
            header,
            misc,
            metadata,
            patch,
            entries,
            blocks,
            block_table,
            block_capacity: block_capacity as usize,
        })
    }

    fn read_at(&mut self, at: u64, output: &mut [u8]) -> Result<(), Unreadable> {
        read_at(&mut self.source, at, output)
    }

    fn block(&self, index: u32) -> Result<Block, Unreadable> {
        let at = self.block_table + index as usize * BLOCK_RECORD;
        Ok(Block {
            offset: u32_of(&self.metadata, at)?,
            size: u32_of(&self.metadata, at + 4)?,
            patch: u16_of(&self.metadata, at + 8)?,
            flags: u16_of(&self.metadata, at + 10)?,
            digest: bytes_at(&self.metadata, at + 12).map_err(|_| Unreadable::Format)?,
        })
    }

    fn records(&self) -> Result<Records, Unreadable> {
        let rows = (0..self.entries as usize)
            .map(|index| bytes_at(&self.metadata, ENTRY_TABLE + index * ENTRY_RECORD))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Unreadable::Format)?;
        Ok(Records {
            package: self.id,
            patch: self.patch,
            digest: bytes_at(&self.header, METADATA_DIGEST).map_err(|_| Unreadable::Format)?,
            rows,
        })
    }

    /// Whether the metadata's own counts and pointers agree with its tables.
    fn metadata_valid(&self) -> Result<bool, Unreadable> {
        let data = &self.metadata;
        let pointers = u64_of(data, TOTAL_SIZE)? == data.len() as u64
            && u64_of(data, TABLE_ENTRY_COUNT)? == u64::from(self.entries)
            && u64_of(data, TABLE_BLOCK_COUNT)? == u64::from(self.blocks)
            && u64_of(data, ENTRY_POINTER)?
                == (ENTRY_TABLE - ENTRY_POINTER - PREFIX_CAPACITY) as u64
            && u64_of(data, BLOCK_POINTER)?
                == (self.block_table - BLOCK_POINTER - PREFIX_CAPACITY) as u64;
        let count = u64_of(data, TAG_PAIR_COUNT)?;
        if !pointers || count == 0 {
            return Ok(pointers);
        }
        // The tag pair pointer also counts from its own field to the table's capacity.
        let adjustment = TAG_PAIR_POINTER + PREFIX_CAPACITY;
        let relative = u64_of(data, TAG_PAIR_POINTER)? as i64;
        if relative < 0 || relative as u64 > (data.len() - adjustment) as u64 {
            return Ok(false);
        }
        let first = relative as usize + adjustment;
        let tail = self.block_table + self.block_capacity * BLOCK_RECORD;
        Ok(first >= tail + PREFIX_MARKER
            && first < data.len()
            && u32_of(data, first - PREFIX_MARKER)? == TABLE_MARKER
            && u64_of(data, first - PREFIX_CAPACITY)? >= count
            && count <= (data.len() - first) as u64)
    }

    /// Checks the file's size, footer and digests, and that its regions and own blocks don't
    /// overlap.
    fn verify(&mut self) -> Result<(), Unreadable> {
        let (header, size) = (self.header, self.size);
        let footer = u64::from(u32_of(&header, FOOTER_OFFSET)?);
        if u64::from(u32_of(&header, FILE_SIZE)?) != size
            || footer % DISK_ALIGNMENT != 0
            || footer + DISK_ALIGNMENT != size
            || !self.metadata_valid()?
        {
            return Err(Unreadable::Format);
        }
        let mut marker = [0; 4];
        self.read_at(footer, &mut marker)
            .map_err(|_| Unreadable::Format)?;
        if u32::from_le_bytes(marker) != FOOTER_MARKER {
            return Err(Unreadable::Format);
        }
        if !matches(&self.misc, &header[MISC_DIGEST..MISC_DIGEST + 20])
            || !matches(
                &self.metadata,
                &header[METADATA_DIGEST..METADATA_DIGEST + 20],
            )
        {
            return Err(Unreadable::Damaged);
        }
        let mut ranges = vec![(0, HEADER_SIZE as u64), (footer, size)];
        let signature = u64::from(u32_of(&header, SIGNATURE_OFFSET)?);
        if signature != 0 {
            ranges.push((signature, signature + DISK_ALIGNMENT));
        }
        for field in [MISC_OFFSET, METADATA_OFFSET] {
            let first = u64::from(u32_of(&header, field)?);
            let count = u64::from(u32_of(&header, field + 4)?);
            if count != 0 {
                ranges.push((first, first + count));
            }
        }
        let mut bytes = Vec::new();
        for index in 0..self.blocks {
            let block = self.block(index)?;
            if block.size == 0 || u64::from(block.size) > BLOCK_SIZE || block.patch > self.patch {
                return Err(Unreadable::Blocks);
            }
            // Blocks of earlier generations live in other files.
            if block.patch != self.patch {
                continue;
            }
            let end = u64::from(block.offset) + u64::from(block.size);
            if end > footer {
                return Err(Unreadable::Blocks);
            }
            ranges.push((u64::from(block.offset), end));
            bytes.resize(block.size as usize, 0);
            self.read_at(u64::from(block.offset), &mut bytes)?;
            if !matches(&bytes, &block.digest) {
                return Err(Unreadable::Damaged);
            }
        }
        ranges.sort_by_key(|range| range.0);
        let apart = ranges.windows(2).all(|pair| pair[0].1 <= pair[1].0);
        if !apart || ranges.iter().any(|range| range.1 > size) {
            return Err(Unreadable::Format);
        }
        Ok(())
    }

    /// Reads one entry from blocks the file itself stores, neither compressed nor encrypted.
    fn read_entry(&mut self, index: u32) -> Result<Vec<u8>, Unreadable> {
        if index >= self.entries {
            return Err(Unreadable::Missing);
        }
        let placement = u64_of(
            &self.metadata,
            ENTRY_TABLE + index as usize * ENTRY_RECORD + 8,
        )?;
        let mut block_index = (placement & 0x3FFF) as u32;
        let mut offset = (((placement >> 14) & 0x3FFF) << 4) as u32;
        let size = (placement >> 28) as u32;
        let end = u64::from(offset) + u64::from(size);
        if size == 0
            || size > ENTRY_BYTE_LIMIT
            || u64::from(block_index) + (end - 1) / BLOCK_SIZE >= u64::from(self.blocks)
        {
            return Err(Unreadable::Blocks);
        }
        let mut bytes = vec![0; size as usize];
        let mut copied = 0;
        while copied < bytes.len() {
            let block = self.block(block_index)?;
            if block.flags != 0 || block.patch != self.patch || offset >= block.size {
                return Err(Unreadable::Blocks);
            }
            let take = (bytes.len() - copied).min((BLOCK_SIZE - u64::from(offset)) as usize);
            if take > (block.size - offset) as usize {
                return Err(Unreadable::Blocks);
            }
            let at = u64::from(block.offset) + u64::from(offset);
            self.read_at(at, &mut bytes[copied..copied + take])
                .map_err(|_| Unreadable::Blocks)?;
            copied += take;
            block_index += 1;
            offset = 0;
        }
        Ok(bytes)
    }
}

fn open_file(path: &Path) -> Result<Package<File>, Unreadable> {
    let file = File::open(path).map_err(|_| Unreadable::File)?;
    let size = file.metadata().map_err(|_| Unreadable::File)?.len();
    Package::open(file, size)
}

fn open_bytes(bytes: &[u8]) -> Result<Package<Cursor<&[u8]>>, Unreadable> {
    Package::open(Cursor::new(bytes), bytes.len() as u64)
}

fn read_all(
    mut package: Package<impl Read + Seek>,
    indices: &[u32],
) -> Result<Vec<Vec<u8>>, Unreadable> {
    package.verify()?;
    indices
        .iter()
        .map(|&index| package.read_entry(index))
        .collect()
}

/// Each of `indices`' entries, read from the file at `path` alone.
pub fn read_entries(path: &Path, indices: &[u32]) -> Result<Vec<Vec<u8>>, Unreadable> {
    read_all(open_file(path)?, indices)
}

/// As [`read_entries`], for a package held in memory.
pub fn read_entries_in(bytes: &[u8], indices: &[u32]) -> Result<Vec<Vec<u8>>, Unreadable> {
    read_all(open_bytes(bytes)?, indices)
}

/// The identity and entry records of the package at `path`, from its header and metadata alone.
pub fn records(path: &Path) -> Result<Records, Unreadable> {
    open_file(path)?.records()
}

/// As [`records`], for a package held in memory.
pub fn records_in(bytes: &[u8]) -> Result<Records, Unreadable> {
    open_bytes(bytes)?.records()
}
