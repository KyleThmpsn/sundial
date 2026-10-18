//! New rows in the public activity table and its display table.
//!
//! Each table is an index of 16-byte rows, a definition hash then a self-relative pointer to a
//! record preceded by its class word. A map's rows copy the donor's records and keep every
//! pointer on the donor's nested data, which the copies share read-only. Only the internal name
//! and the display text change. The index moves to the end of the payload with the new rows, so
//! no existing record or pointer moves. The registry layouts of the record classes name every
//! self-relative field below.
//!
//! A game mode's activity lists the maps it plays in a playlist, by activity ordinal, so each
//! playlist naming a donor row gains an entry for the map's copy of that row. The private match
//! screen does not read playlists. Its map list is the private match table's.
use std::collections::{BTreeMap, BTreeSet};

use super::Resolved;
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, read_i64, read_u16, read_u32, synchronize_payload_size,
        write_relative_pointer, write_u16, write_u32,
    },
};

const INDEX_DESCRIPTOR: usize = 0x08;
const INDEX_ROW_SIZE: usize = 16;
const INDEX_RECORD_FIELD: usize = 8;

const ACTIVITY_ROW_CLASS: u32 = 0x8080_76FC;
const ACTIVITY_RECORD_CLASS: u32 = 0x8080_7711;
const ACTIVITY_RECORD_SIZE: usize = 0xE8;
/// Every self-relative field of an activity record (registry class `80807711`).
const ACTIVITY_POINTERS: [usize; 17] = [
    0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x68, 0x70, 0x80, 0x90, 0xA8, 0xC0,
    0xD0,
];
const ACTIVITY_NAME_FIELD: usize = 0x68;
/// The record's playlist (class `80807733`): a hash, then at `+0x08` its items.
const ACTIVITY_PLAYLIST_FIELD: usize = 0x18;
const PLAYLIST_CLASS: u32 = 0x8080_7733;
const PLAYLIST_ITEMS: usize = 0x08;
/// A playlist item (class `80807736`, 56 bytes): a weight at `+4`, the activity ordinal as a
/// `u16` at `+8`, then mode arrays whose pointers are at `+0x18` and `+0x30`.
const PLAYLIST_ITEM_CLASS: u32 = 0x8080_7736;
const PLAYLIST_ITEM_SIZE: usize = 56;
const PLAYLIST_ITEM_ACTIVITY: usize = 0x08;
const PLAYLIST_ITEM_POINTERS: [usize; 2] = [0x18, 0x30];

const DISPLAY_ROW_CLASS: u32 = 0x8080_5E15;
const DISPLAY_RECORD_CLASS: u32 = 0x8080_5E18;
const DISPLAY_RECORD_SIZE: usize = 0x88;
/// Every self-relative field of a display record (registry class `80805E18`).
const DISPLAY_POINTERS: [usize; 9] = [0x00, 0x10, 0x20, 0x30, 0x48, 0x50, 0x60, 0x70, 0x78];
const DISPLAY_TEXT_FIELD: usize = 0x00;
/// Display text blocks (class `80805E23`) form the array at `+0x18`, and each display record's
/// `+0x00` points at one. A block holds title and description references at `+4` and `+12`, each
/// a bank ordinal then a string hash.
const TEXT_BLOCKS: usize = 0x18;
const TEXT_BLOCK_CLASS: u32 = 0x8080_5E23;
const TEXT_BLOCK_SIZE: usize = 36;
const TEXT_TITLE: usize = 4;
const TEXT_DESCRIPTION: usize = 12;

/// The checked index of a table.
pub(super) struct Rows {
    count: usize,
    rows: usize,
}

impl Rows {
    pub(super) fn read(data: &[u8]) -> AuthoringResult<Self> {
        Self::read_class(data, ACTIVITY_ROW_CLASS, "activity table")
    }

    fn read_class(data: &[u8], class: u32, description: &str) -> AuthoringResult<Self> {
        let (count, _, rows, row_class) = array_at(data, INDEX_DESCRIPTOR)?;
        if row_class != class {
            return Err(invalid(format!(
                "The {description} index has class {row_class:08X}, expected {class:08X}"
            )));
        }
        Ok(Self { count, rows })
    }

    fn row(&self, ordinal: usize) -> usize {
        self.rows + ordinal * INDEX_ROW_SIZE
    }

    fn record(&self, data: &[u8], ordinal: usize) -> AuthoringResult<usize> {
        target(data, self.row(ordinal) + INDEX_RECORD_FIELD)
    }

    /// Every activity hash the table holds.
    pub(super) fn hashes(&self, data: &[u8]) -> AuthoringResult<BTreeSet<u32>> {
        (0..self.count)
            .map(|ordinal| read_u32(data, self.row(ordinal)))
            .collect()
    }

    /// Ordinals whose activity record names `name`, in table order.
    pub(super) fn ordinals_named(&self, data: &[u8], name: &str) -> AuthoringResult<Vec<usize>> {
        let mut found = Vec::new();
        for ordinal in 0..self.count {
            let record = self.record(data, ordinal)?;
            if read_i64(data, record + ACTIVITY_NAME_FIELD)? == 0 {
                continue;
            }
            let start = target(data, record + ACTIVITY_NAME_FIELD)?;
            let end = data[start..]
                .iter()
                .position(|&byte| byte == 0)
                .map(|length| start + length)
                .ok_or_else(|| invalid("An activity name is unterminated"))?;
            if &data[start..end] == name.as_bytes() {
                found.push(ordinal);
            }
        }
        Ok(found)
    }
}

fn target(data: &[u8], field: usize) -> AuthoringResult<usize> {
    let relative = read_i64(data, field)?;
    let target = field
        .checked_add_signed(relative as isize)
        .filter(|&target| target < data.len())
        .ok_or_else(|| invalid(format!("Pointer at 0x{field:X} leaves its table")))?;
    Ok(target)
}

/// Appends `bytes` after `class` on an 8-byte record boundary and returns the record offset.
fn push_record(data: &mut Vec<u8>, class: u32, bytes: &[u8]) -> usize {
    let start = (data.len() + 4).next_multiple_of(8);
    data.resize(start - 4, 0);
    data.extend_from_slice(&class.to_le_bytes());
    data.extend_from_slice(bytes);
    start
}

/// Copies the record at `source` to the end of `data`, keeping each nonzero pointer on the
/// target it had in the source record.
fn copy_record(
    data: &mut Vec<u8>,
    source: usize,
    class: u32,
    size: usize,
    pointers: &[usize],
) -> AuthoringResult<usize> {
    if read_u32(data, source - 4)? != class {
        return Err(invalid(format!(
            "A donor record at 0x{source:X} is not class {class:08X}"
        )));
    }
    let bytes = data
        .get(source..source + size)
        .ok_or_else(|| invalid("A donor record leaves its table"))?
        .to_vec();
    let targets = pointers
        .iter()
        .map(|&field| {
            Ok(if read_i64(data, source + field)? == 0 {
                None
            } else {
                Some(target(data, source + field)?)
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let copy = push_record(data, class, &bytes);
    for (&field, target) in pointers.iter().zip(targets) {
        if let Some(target) = target {
            write_relative_pointer(data, copy + field, target)?;
        }
    }
    Ok(copy)
}

/// Adds an item for each copy of a donor row to every playlist that names the donor row, after
/// the playlist's own items. Each moved or copied item keeps its arrays' targets.
fn extend_playlists(
    data: &mut Vec<u8>,
    rows: &Rows,
    copies: &BTreeMap<u16, Vec<u16>>,
) -> AuthoringResult<()> {
    for ordinal in 0..rows.count {
        let record = rows.record(data, ordinal)?;
        if read_i64(data, record + ACTIVITY_PLAYLIST_FIELD)? == 0 {
            continue;
        }
        let playlist = target(data, record + ACTIVITY_PLAYLIST_FIELD)?;
        if read_u32(data, playlist - 4)? != PLAYLIST_CLASS {
            return Err(invalid(format!(
                "Activity {ordinal} names a playlist that is not class {PLAYLIST_CLASS:08X}"
            )));
        }
        let (count, _, items, class) = array_at(data, playlist + PLAYLIST_ITEMS)?;
        if count == 0 {
            continue;
        }
        if class != PLAYLIST_ITEM_CLASS {
            return Err(invalid(format!(
                "Activity {ordinal}'s playlist items are class {class:08X}"
            )));
        }
        let mut sources = (0..count)
            .map(|item| (items + item * PLAYLIST_ITEM_SIZE, None))
            .collect::<Vec<_>>();
        for item in 0..count {
            let item = items + item * PLAYLIST_ITEM_SIZE;
            let activity = read_u16(data, item + PLAYLIST_ITEM_ACTIVITY)?;
            if let Some(added) = copies.get(&activity) {
                sources.extend(added.iter().map(|&copy| (item, Some(copy))));
            }
        }
        if sources.len() == count {
            continue;
        }
        let mut bytes = Vec::with_capacity(sources.len() * PLAYLIST_ITEM_SIZE);
        let mut targets = Vec::with_capacity(sources.len());
        for &(source, _) in &sources {
            bytes.extend_from_slice(
                data.get(source..source + PLAYLIST_ITEM_SIZE)
                    .ok_or_else(|| invalid("A playlist item leaves its table"))?,
            );
            targets.push(
                PLAYLIST_ITEM_POINTERS
                    .iter()
                    .map(|&field| {
                        Ok(if read_i64(data, source + field)? == 0 {
                            None
                        } else {
                            Some(target(data, source + field)?)
                        })
                    })
                    .collect::<AuthoringResult<Vec<_>>>()?,
            );
        }
        append_native_array(
            data,
            playlist + PLAYLIST_ITEMS,
            PLAYLIST_ITEM_CLASS,
            sources.len(),
            &bytes,
        )?;
        let (_, _, moved, _) = array_at(data, playlist + PLAYLIST_ITEMS)?;
        for (index, ((_, copy), fields)) in sources.iter().zip(targets).enumerate() {
            let item = moved + index * PLAYLIST_ITEM_SIZE;
            for (&field, target) in PLAYLIST_ITEM_POINTERS.iter().zip(fields) {
                if let Some(target) = target {
                    write_relative_pointer(data, item + field, target)?;
                }
            }
            if let Some(copy) = copy {
                write_u16(data, item + PLAYLIST_ITEM_ACTIVITY, *copy)?;
            }
        }
    }
    Ok(())
}

/// Moves the display text blocks to the end of `data` with `added` after the existing ones, and
/// points every display record at its block's new place. Each of `copies` is a new display record
/// and the ordinal of its block among `added`.
fn extend_text_blocks(
    data: &mut Vec<u8>,
    rows: &Rows,
    added: &[Vec<u8>],
    copies: &[(usize, usize)],
) -> AuthoringResult<()> {
    let (count, _, blocks, class) = array_at(data, TEXT_BLOCKS)?;
    if class != TEXT_BLOCK_CLASS {
        return Err(invalid(format!(
            "The display text blocks are class {class:08X}, expected {TEXT_BLOCK_CLASS:08X}"
        )));
    }
    let end = blocks + count * TEXT_BLOCK_SIZE;
    let mut stock = Vec::with_capacity(rows.count);
    for ordinal in 0..rows.count {
        let record = rows.record(data, ordinal)?;
        let block = target(data, record + DISPLAY_TEXT_FIELD)?;
        if block < blocks || block >= end || (block - blocks) % TEXT_BLOCK_SIZE != 0 {
            return Err(invalid(format!(
                "Display row {ordinal} names text outside the text block array"
            )));
        }
        stock.push((record, (block - blocks) / TEXT_BLOCK_SIZE));
    }
    let mut bytes = data
        .get(blocks..end)
        .ok_or_else(|| invalid("The display text blocks leave their table"))?
        .to_vec();
    for block in added {
        bytes.extend_from_slice(block);
    }
    append_native_array(data, TEXT_BLOCKS, class, count + added.len(), &bytes)?;
    let (_, _, moved, _) = array_at(data, TEXT_BLOCKS)?;
    for (record, block) in stock.into_iter().chain(
        copies
            .iter()
            .map(|&(record, block)| (record, count + block)),
    ) {
        write_relative_pointer(
            data,
            record + DISPLAY_TEXT_FIELD,
            moved + block * TEXT_BLOCK_SIZE,
        )?;
    }
    Ok(())
}

/// Moves the index to the end of `data` with `added` rows after the existing ones, each a hash
/// and the record it points to.
fn extend_index(
    data: &mut Vec<u8>,
    rows: &Rows,
    class: u32,
    added: &[(u32, usize)],
) -> AuthoringResult<()> {
    let mut entries = Vec::with_capacity(rows.count + added.len());
    for ordinal in 0..rows.count {
        entries.push((
            read_u32(data, rows.row(ordinal))?,
            rows.record(data, ordinal)?,
        ));
    }
    entries.extend_from_slice(added);
    let mut bytes = vec![0u8; entries.len() * INDEX_ROW_SIZE];
    for (index, (hash, _)) in entries.iter().enumerate() {
        bytes[index * INDEX_ROW_SIZE..index * INDEX_ROW_SIZE + 4]
            .copy_from_slice(&hash.to_le_bytes());
    }
    append_native_array(data, INDEX_DESCRIPTOR, class, entries.len(), &bytes)?;
    let moved = Rows::read_class(data, class, "extended")?;
    for (ordinal, (_, record)) in entries.iter().enumerate() {
        write_relative_pointer(data, moved.row(ordinal) + INDEX_RECORD_FIELD, *record)?;
    }
    Ok(())
}

/// Each donor row of a map with the row that copies it.
pub(super) type RowCopies = Vec<(u16, u16)>;

/// The activity and display tables with every map's rows appended in the same order, so each
/// display row keeps its activity row's ordinal and hash, and each map's row copies.
pub(super) fn append(
    activities: &[u8],
    displays: &[u8],
    maps: &[Resolved],
) -> AuthoringResult<(Vec<u8>, Vec<u8>, Vec<RowCopies>)> {
    let activity_rows = Rows::read(activities)?;
    let display_rows = Rows::read_class(displays, DISPLAY_ROW_CLASS, "activity display table")?;
    if activity_rows.count != display_rows.count {
        return Err(invalid(
            "The activity and display tables have different row counts",
        ));
    }
    let mut activity = activities.to_vec();
    let mut display = displays.to_vec();
    let mut added_activities = Vec::new();
    let mut added_displays = Vec::new();
    // Each map's text block, and each new display record with the ordinal of its block.
    let mut text_blocks = Vec::new();
    let mut texts = Vec::new();
    // Each donor row's ordinal, with the ordinals its copies take after the stock rows.
    let mut copies = BTreeMap::<u16, Vec<u16>>::new();
    let mut map_copies = Vec::with_capacity(maps.len());
    for map in maps {
        let mut rows = RowCopies::new();
        let donors = activity_rows.ordinals_named(activities, &map.recipe.donor)?;
        if donors.len() != map.activities.len() {
            return Err(invalid(format!(
                "Map {} expected {} donor rows",
                map.recipe.name,
                map.activities.len()
            )));
        }
        let name = activity.len();
        activity.extend_from_slice(map.recipe.name.as_bytes());
        activity.push(0);
        let text = text_blocks.len();
        for (index, (ordinal, &hash)) in donors.iter().zip(&map.activities).enumerate() {
            if read_u32(displays, display_rows.row(*ordinal))?
                != read_u32(activities, activity_rows.row(*ordinal))?
            {
                return Err(invalid(
                    "A donor display row does not match its activity row",
                ));
            }
            let record = copy_record(
                &mut activity,
                activity_rows.record(activities, *ordinal)?,
                ACTIVITY_RECORD_CLASS,
                ACTIVITY_RECORD_SIZE,
                &ACTIVITY_POINTERS,
            )?;
            write_u32(&mut activity, record, hash)?;
            write_relative_pointer(&mut activity, record + ACTIVITY_NAME_FIELD, name)?;
            let narrow = |value: usize| {
                u16::try_from(value).map_err(|_| invalid("Activity ordinals exceed 16 bits"))
            };
            let pair = (
                narrow(*ordinal)?,
                narrow(activity_rows.count + added_activities.len())?,
            );
            copies.entry(pair.0).or_default().push(pair.1);
            rows.push(pair);
            added_activities.push((hash, record));

            let source = display_rows.record(displays, *ordinal)?;
            if index == 0 {
                // Both rows of a map show the same text, as the donor's do.
                let donor_text = target(displays, source + DISPLAY_TEXT_FIELD)?;
                let mut block = displays
                    .get(donor_text..donor_text + TEXT_BLOCK_SIZE)
                    .ok_or_else(|| invalid("A donor display text block leaves its table"))?
                    .to_vec();
                let bank = u32::try_from(crate::package_profile::LOCALIZATION_DONOR_TABLE_INDEX)
                    .map_err(|_| invalid("Localized bank index does not fit 32 bits"))?;
                crate::tag_payload::write_localized_reference(
                    &mut block,
                    TEXT_TITLE,
                    bank,
                    map.title_hash,
                )?;
                crate::tag_payload::write_localized_reference(
                    &mut block,
                    TEXT_DESCRIPTION,
                    bank,
                    map.description_hash,
                )?;
                text_blocks.push(block);
            }
            let copy = copy_record(
                &mut display,
                source,
                DISPLAY_RECORD_CLASS,
                DISPLAY_RECORD_SIZE,
                &DISPLAY_POINTERS,
            )?;
            texts.push((copy, text));
            added_displays.push((hash, copy));
        }
        map_copies.push(rows);
    }
    extend_text_blocks(&mut display, &display_rows, &text_blocks, &texts)?;
    extend_playlists(&mut activity, &activity_rows, &copies)?;
    extend_index(
        &mut activity,
        &activity_rows,
        ACTIVITY_ROW_CLASS,
        &added_activities,
    )?;
    extend_index(
        &mut display,
        &display_rows,
        DISPLAY_ROW_CLASS,
        &added_displays,
    )?;
    Ok((
        synchronize_payload_size(activity)?,
        synchronize_payload_size(display)?,
        map_copies,
    ))
}
