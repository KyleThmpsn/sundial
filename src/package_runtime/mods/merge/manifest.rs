//! Loading-manifest union (Sunrise f1e7c66 `table_merge/manifest_set.cpp`). Several mods may each
//! list their own entries in one installed manifest, and the loader keeps every listed entry.
use super::Status;
use super::layout::{
    ARRAY_MARKER, Array, HEADER_SIZE, MARKER_BACK, align, i64_at, optional_array_at, put, u16_at,
    u32_at, u64_at,
};

/// Manifest fields, from its start. An offset field counts from its own position.
const SIZE: usize = 0x00;
const OWN_TAG: usize = 0x08;
const ROOT_TAG: usize = 0x0C;
const GROUP_COUNT: usize = 0x10;
const GROUP_ARRAY: usize = 0x18;
const KEY_COUNT: usize = 0x20;
const KEY_ARRAY: usize = 0x28;
/// The reader needs the key offset field whole.
const MINIMUM_SIZE: usize = KEY_ARRAY + 8;

/// Fields of one 40-byte group row. Rows are sorted by package id.
const GROUP_PACKAGE: usize = 0;
const GROUP_BITMAP_COUNT: usize = 8;
const GROUP_BITMAP: usize = 16;
const GROUP_LIST_COUNT: usize = 24;
const GROUP_LIST: usize = 32;
const GROUP_SIZE: usize = 40;

const GROUP_CLASS: u32 = 0x8080_9EFB;
const BITMAP_CLASS: u32 = 0x8080_000B;
const ENTRY_LIST_CLASS: u32 = 0x8080_000A;
/// Entry indices keep 13 bits, the package entry limit.
const ENTRY_MASK: u16 = 0x1FFF;
const WORD_BITS: usize = 32;
const KEY_SIZE: usize = 8;
const ARRAY_ALIGNMENT: u64 = 8;

/// One package's listed entries, sorted and unique.
struct Group {
    package: u16,
    entries: Vec<u16>,
}

/// The rows of the typed array whose descriptor is at `field`, when they fit and match.
fn array_at(data: &[u8], field: usize, class: u32, stride: usize) -> Option<Array> {
    let array = optional_array_at(data, field)?;
    let fits = array.data <= data.len()
        && array.count <= ((data.len() - array.data) / stride) as u64
        && (array.count == 0 || array.class == class);
    fits.then_some(array)
}

/// Where a group's bitmap words start. Its count is a u32, so its descriptor is not a typed
/// array descriptor.
fn bitmap_words(data: &[u8], row: usize, words: u32) -> Option<usize> {
    let field = row + GROUP_BITMAP;
    let relative = i64_at(data, field)?;
    if relative < -(field as i64) || relative > data.len() as i64 {
        return None;
    }
    let header = usize::try_from(field as i64 + relative).ok()?;
    let size = HEADER_SIZE + words as usize * 4;
    let valid = header >= MARKER_BACK
        && header <= data.len()
        && size <= data.len() - header
        && u32_at(data, header - MARKER_BACK)? == ARRAY_MARKER
        && u64_at(data, header)? == u64::from(words)
        && u32_at(data, header + 8)? == BITMAP_CLASS;
    valid.then_some(header + HEADER_SIZE)
}

/// Adds every entry a group's bitmap marks. `None` when the bitmap leaves the data.
fn read_bitmap(data: &[u8], row: usize, entries: &mut Vec<u16>) -> Option<()> {
    let words = u32_at(data, row + GROUP_BITMAP_COUNT)?;
    if i64_at(data, row + GROUP_BITMAP)? == 0 {
        return (words == 0).then_some(());
    }
    let first = bitmap_words(data, row, words)?;
    for word in 0..words as usize {
        let bits = u32_at(data, first + word * 4)?;
        for bit in 0..WORD_BITS {
            if bits & (1 << bit) == 0 {
                continue;
            }
            // A bit past the entry limit names no entry a group can hold.
            let entry = u16::try_from(word * WORD_BITS + bit)
                .ok()
                .filter(|&entry| entry <= ENTRY_MASK)?;
            entries.push(entry);
        }
    }
    Some(())
}

/// Adds every entry of a group's u16 list.
fn read_list(data: &[u8], row: usize, entries: &mut Vec<u16>) -> Option<()> {
    let list = array_at(data, row + GROUP_LIST_COUNT, ENTRY_LIST_CLASS, 2)?;
    if list.count > u64::from(ENTRY_MASK) + 1 {
        return None;
    }
    for index in 0..list.count as usize {
        entries.push(u16_at(data, list.data + index * 2)? & ENTRY_MASK);
    }
    Some(())
}

/// Every group of one manifest, or `None` when an offset or a count leaves the data.
fn read_groups(data: &[u8]) -> Option<Vec<Group>> {
    if data.len() < MINIMUM_SIZE {
        return None;
    }
    let array = array_at(data, GROUP_COUNT, GROUP_CLASS, GROUP_SIZE)?;
    let mut groups = Vec::new();
    for index in 0..array.count as usize {
        let row = array.data + index * GROUP_SIZE;
        let mut entries = Vec::new();
        read_bitmap(data, row, &mut entries)?;
        read_list(data, row, &mut entries)?;
        entries.sort_unstable();
        entries.dedup();
        groups.push(Group {
            package: u16_at(data, row + GROUP_PACKAGE)?,
            entries,
        });
    }
    Some(groups)
}

/// Writes one typed u16 entry list whose header starts at `header`.
fn write_list(data: &mut [u8], header: usize, entries: &[u16]) {
    put(data, header - MARKER_BACK, &ARRAY_MARKER.to_le_bytes());
    put(data, header, &(entries.len() as u64).to_le_bytes());
    put(data, header + 8, &ENTRY_LIST_CLASS.to_le_bytes());
    for (index, entry) in entries.iter().enumerate() {
        put(data, header + HEADER_SIZE + index * 2, &entry.to_le_bytes());
    }
}

/// Inserts a group row for a package the manifest lacks, in package order. Every offset that
/// crosses the new row moves, and the entry list goes at the end.
fn add_group(
    data: &mut Vec<u8>,
    groups: &[Group],
    package: u16,
    entries: &[u16],
) -> Result<(), Status> {
    let array = array_at(data, GROUP_COUNT, GROUP_CLASS, GROUP_SIZE)
        .filter(|array| array.count != 0 && !entries.is_empty())
        .ok_or(Status::Unsupported)?;
    let header = array.data - HEADER_SIZE;
    let before = groups
        .iter()
        .position(|group| group.package >= package)
        .unwrap_or(groups.len());
    let insert = array.data + before * GROUP_SIZE;
    // One relative offset field and the absolute position it names.
    let target = |field: usize| (field as i64 + i64_at(data, field).unwrap_or(0)) as usize;
    let mut links = vec![(GROUP_ARRAY, header)];
    if i64_at(data, KEY_COUNT) != Some(0) && i64_at(data, KEY_ARRAY) != Some(0) {
        links.push((KEY_ARRAY, target(KEY_ARRAY)));
    }
    for index in 0..groups.len() {
        let row = array.data + index * GROUP_SIZE;
        for field in [row + GROUP_BITMAP, row + GROUP_LIST] {
            if i64_at(data, field) != Some(0) {
                links.push((field, target(field)));
            }
        }
    }
    let shift = |offset: usize| {
        if offset >= insert {
            offset + GROUP_SIZE
        } else {
            offset
        }
    };
    let list_header =
        align((data.len() + GROUP_SIZE) as u64, ARRAY_ALIGNMENT) as usize + HEADER_SIZE;
    let mut result = vec![0; list_header + HEADER_SIZE + entries.len() * 2];
    result[..insert].copy_from_slice(&data[..insert]);
    result[insert + GROUP_SIZE..insert + GROUP_SIZE + data.len() - insert]
        .copy_from_slice(&data[insert..]);
    for (field, target) in links {
        let relative = shift(target) as i64 - shift(field) as i64;
        put(&mut result, shift(field), &relative.to_le_bytes());
    }
    put(&mut result, insert + GROUP_PACKAGE, &package.to_le_bytes());
    put(
        &mut result,
        insert + GROUP_LIST_COUNT,
        &(entries.len() as i64).to_le_bytes(),
    );
    let list = list_header as i64 - (insert + GROUP_LIST) as i64;
    put(&mut result, insert + GROUP_LIST, &list.to_le_bytes());
    write_list(&mut result, list_header, entries);
    let count = array.count as i64 + 1;
    put(&mut result, GROUP_COUNT, &count.to_le_bytes());
    put(&mut result, header, &count.to_le_bytes());
    let size = result.len() as u64;
    put(&mut result, SIZE, &size.to_le_bytes());
    *data = result;
    Ok(())
}

/// Adds entries to one package's group, and a new package gets a new group. A grown group's
/// bitmap and list become one typed list at the end.
fn add_entries(data: &mut Vec<u8>, package: u16, entries: &[u16]) -> Result<(), Status> {
    let groups = read_groups(data).ok_or(Status::Format)?;
    let Some(found) = groups.iter().position(|group| group.package == package) else {
        return add_group(data, &groups, package, entries);
    };
    let mut combined = groups[found].entries.clone();
    combined.extend_from_slice(entries);
    combined.sort_unstable();
    combined.dedup();
    if combined.len() == groups[found].entries.len() {
        return Ok(());
    }
    let array = array_at(data, GROUP_COUNT, GROUP_CLASS, GROUP_SIZE).ok_or(Status::Format)?;
    let row = array.data + found * GROUP_SIZE;
    let header = align((data.len() + MARKER_BACK) as u64, ARRAY_ALIGNMENT) as usize;
    data.resize(header + HEADER_SIZE + combined.len() * 2, 0);
    put(data, row + GROUP_BITMAP_COUNT, &0u32.to_le_bytes());
    put(data, row + GROUP_BITMAP, &0i64.to_le_bytes());
    put(
        data,
        row + GROUP_LIST_COUNT,
        &(combined.len() as i64).to_le_bytes(),
    );
    let list = header as i64 - (row + GROUP_LIST) as i64;
    put(data, row + GROUP_LIST, &list.to_le_bytes());
    write_list(data, header, &combined);
    let size = data.len() as u64;
    put(data, SIZE, &size.to_le_bytes());
    Ok(())
}

/// The key array bytes from its marker to its last key, empty when it has none.
fn key_bytes(data: &[u8]) -> Option<&[u8]> {
    let keys = optional_array_at(data, KEY_COUNT)?;
    if keys.data > data.len() || keys.count > ((data.len() - keys.data) / KEY_SIZE) as u64 {
        return None;
    }
    if keys.count == 0 {
        return Some(&[]);
    }
    let marker = keys.data - HEADER_SIZE - MARKER_BACK;
    data.get(marker..marker + MARKER_BACK + HEADER_SIZE + keys.count as usize * KEY_SIZE)
}

/// Whether both manifests name the same tag, root and record keys.
fn same_identity(first: &[u8], second: &[u8]) -> bool {
    let (Some(first_keys), Some(second_keys)) = (key_bytes(first), key_bytes(second)) else {
        return false;
    };
    u32_at(first, OWN_TAG) == u32_at(second, OWN_TAG)
        && u32_at(first, ROOT_TAG) == u32_at(second, ROOT_TAG)
        && i64_at(first, KEY_COUNT) == i64_at(second, KEY_COUNT)
        && first_keys == second_keys
}

/// Whether `built` keeps the manifest's tag, root and record keys and only adds group entries to
/// `stock`, the edit the loader unions with other mods' additions.
pub(super) fn adds_only(stock: &[u8], built: &[u8]) -> bool {
    if stock.len() < MINIMUM_SIZE || built.len() < MINIMUM_SIZE || !same_identity(stock, built) {
        return false;
    }
    let (Some(before), Some(after)) = (read_groups(stock), read_groups(built)) else {
        return false;
    };
    before.iter().all(|group| {
        after.iter().any(|other| {
            other.package == group.package
                && group
                    .entries
                    .iter()
                    .all(|entry| other.entries.binary_search(entry).is_ok())
        })
    })
}

/// Adds every group entry of a later manifest copy that `output` lacks.
pub(super) fn union_manifest(later: &[u8], output: &mut Vec<u8>) -> Result<(), Status> {
    if later.len() < MINIMUM_SIZE || output.len() < MINIMUM_SIZE {
        return Err(Status::Format);
    }
    if !same_identity(output, later) {
        return Err(Status::Mismatch);
    }
    for group in read_groups(later).ok_or(Status::Format)? {
        if !group.entries.is_empty() {
            add_entries(output, group.package, &group.entries)?;
        }
    }
    Ok(())
}
