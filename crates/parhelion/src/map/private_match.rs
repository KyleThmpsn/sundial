//! New entries in the private match table (investment root slot 64, class `80807571`) and its
//! display table (investment globals slot 43, class `808059B9`).
//!
//! The table holds one group per private match family, the Crucible's then Gambit's. Each group
//! lists its game modes and its maps, and the native private match screen offers a group's maps
//! in the order of its map list. A map entry names the map's private match activity row, the
//! second of a Crucible map's two rows, which no mode playlist names. The display table holds the
//! same groups in the same order, and each group's map displays parallel its map list: the
//! screen draws map `n` with display `n`, its title, description and image. A map entry with no
//! display entry is offered without a name and with a generic emblem.
//!
//! Every list that holds a donor row gains a copy of that entry for the map's copy of the row,
//! after the stock entries, so no stock entry moves. The copy keeps the donor's traits, which
//! modes match against their requirements, so the map is offered in every mode that offers the
//! donor. Its display copies the donor's with the map's title and description, and keeps the
//! donor's image unless the map has a picture of its own ([`super::picture`]).
use std::collections::BTreeSet;

use super::{Resolved, activity::RowCopies};
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, bounded_relative_target, read_i64, read_u16, read_u32,
        synchronize_payload_size, write_localized_reference, write_relative_pointer, write_u16,
        write_u32,
    },
};

/// The groups at `+0x08` of both tables.
const GROUPS: usize = 0x08;
/// A private match group (class `808074A3`, 72 bytes), its identity hash at `+0`.
const GROUP_CLASS: u32 = 0x8080_74A3;
const GROUP_SIZE: usize = 72;
/// A group's map list, at `+0x18` of the group.
const GROUP_MAPS: usize = 0x18;
/// A map entry (class `808074AA`, 24 bytes): an identity hash, the activity ordinal of the map's
/// private match row as a `u16` at `+4`, and from `+0x08` its trait hashes, whose pointer is at
/// `+0x10`.
const MAP_CLASS: u32 = 0x8080_74AA;
const MAP_SIZE: usize = 24;
const MAP_ACTIVITY: usize = 4;
const MAP_TRAITS: usize = 0x10;
/// A display group (class `808059BD`, 80 bytes): the group's identity hash at `+0`, then its
/// mode displays at `+0x30` and its map displays at `+0x40`.
const DISPLAY_GROUP_CLASS: u32 = 0x8080_59BD;
const DISPLAY_GROUP_SIZE: usize = 80;
const DISPLAY_GROUP_MAPS: usize = 0x40;
/// A map display (class `808059C2`, 20 bytes): the title as a bank ordinal and string hash, the
/// description the same way at `+8`, and an image index at `+16`.
const MAP_DISPLAY_CLASS: u32 = 0x8080_59C2;
const MAP_DISPLAY_SIZE: usize = 20;
const MAP_DISPLAY_TITLE: usize = 0;
const MAP_DISPLAY_DESCRIPTION: usize = 8;
pub(super) const MAP_DISPLAY_IMAGE: usize = 16;

/// A map's new display entry: its map, its offset in the display table, and the image row its
/// donor's display names, which the entry keeps until the map has a picture of its own.
pub(super) struct Display {
    pub map: usize,
    pub offset: usize,
    pub donor_image: u32,
}

/// A new map entry: the donor entry it copies, its hash, its activity row and its map.
struct Added {
    source: usize,
    hash: u32,
    row: u16,
    map: usize,
}

/// The table and its display table with an entry and a display for each copy of a listed donor
/// row, and where each new display is. `copies` holds each map's donor rows with the rows that
/// copy them, in the order of `maps`, and `bank` is the localization bank of the maps' titles and
/// descriptions.
pub(super) fn append(
    table: &[u8],
    displays: &[u8],
    maps: &[Resolved],
    copies: &[RowCopies],
    bank: u32,
) -> AuthoringResult<(Vec<u8>, Vec<u8>, Vec<Display>)> {
    let mut data = table.to_vec();
    let mut display = displays.to_vec();
    let (groups, _, first, class) = array_at(&data, GROUPS)?;
    if class != GROUP_CLASS {
        return Err(invalid(format!(
            "The private match groups are class {class:08X}, expected {GROUP_CLASS:08X}"
        )));
    }
    let (display_groups, _, display_first, display_class) = array_at(&display, GROUPS)?;
    if display_class != DISPLAY_GROUP_CLASS || display_groups != groups {
        return Err(invalid(
            "The private match display groups do not match the private match groups",
        ));
    }
    // Every entry hash in the table, which the new entries avoid.
    let mut taken = BTreeSet::new();
    for group in 0..groups {
        let (count, _, entries, _) = array_at(&data, first + group * GROUP_SIZE + GROUP_MAPS)?;
        for entry in 0..count {
            taken.insert(read_u32(&data, entries + entry * MAP_SIZE)?);
        }
    }
    let mut offered = vec![false; maps.len()];
    let mut placed = Vec::new();
    for group in 0..groups {
        let group_at = first + group * GROUP_SIZE;
        let display_at = display_first + group * DISPLAY_GROUP_SIZE;
        if read_u32(&data, group_at)? != read_u32(&display, display_at)? {
            return Err(invalid(format!(
                "Private match group {group} and its display name different groups"
            )));
        }
        let descriptor = group_at + GROUP_MAPS;
        let display_descriptor = display_at + DISPLAY_GROUP_MAPS;
        let (count, _, entries, class) = array_at(&data, descriptor)?;
        if count == 0 {
            continue;
        }
        let (display_count, _, display_entries, display_class) =
            array_at(&display, display_descriptor)?;
        if class != MAP_CLASS || display_class != MAP_DISPLAY_CLASS || display_count != count {
            return Err(invalid(format!(
                "Private match group {group} has {count} maps of class {class:08X} and \
                 {display_count} displays of class {display_class:08X}"
            )));
        }
        let mut added = Vec::new();
        for entry in 0..count {
            let row = read_u16(&data, entries + entry * MAP_SIZE + MAP_ACTIVITY)?;
            for (index, (map, rows)) in maps.iter().zip(copies).enumerate() {
                for &(_, copy) in rows.iter().filter(|(donor, _)| *donor == row) {
                    let hash = super::allocate(&map.recipe.name, "private_match", |hash| {
                        !taken.contains(&hash)
                    })?;
                    taken.insert(hash);
                    added.push(Added {
                        source: entry,
                        hash,
                        row: copy,
                        map: index,
                    });
                    offered[index] = true;
                }
            }
        }
        if added.is_empty() {
            continue;
        }
        extend_maps(&mut data, descriptor, count, entries, &added)?;
        let mut bytes = display
            .get(display_entries..display_entries + count * MAP_DISPLAY_SIZE)
            .ok_or_else(|| invalid("The private match map displays leave their table"))?
            .to_vec();
        let mut donor_images = Vec::with_capacity(added.len());
        for item in &added {
            let source = display_entries + item.source * MAP_DISPLAY_SIZE;
            let mut entry = display[source..source + MAP_DISPLAY_SIZE].to_vec();
            donor_images.push(read_u32(&entry, MAP_DISPLAY_IMAGE)?);
            let map = &maps[item.map];
            write_localized_reference(&mut entry, MAP_DISPLAY_TITLE, bank, map.title_hash)?;
            write_localized_reference(
                &mut entry,
                MAP_DISPLAY_DESCRIPTION,
                bank,
                map.description_hash,
            )?;
            bytes.extend_from_slice(&entry);
        }
        append_native_array(
            &mut display,
            display_descriptor,
            MAP_DISPLAY_CLASS,
            count + added.len(),
            &bytes,
        )?;
        let (_, _, moved, _) = array_at(&display, display_descriptor)?;
        for (index, (item, donor_image)) in added.iter().zip(donor_images).enumerate() {
            placed.push(Display {
                map: item.map,
                offset: moved + (count + index) * MAP_DISPLAY_SIZE,
                donor_image,
            });
        }
    }
    if let Some(map) = maps
        .iter()
        .zip(&offered)
        .find_map(|(map, offered)| (!offered).then_some(map))
    {
        return Err(invalid(format!(
            "No private match list offers donor {}, so map {} would not be offered",
            map.recipe.donor, map.recipe.name
        )));
    }
    Ok((
        synchronize_payload_size(data)?,
        synchronize_payload_size(display)?,
        placed,
    ))
}

/// Moves a map list to the end of `data` with `added` after its `count` entries. Each moved or
/// copied entry keeps its traits' target, and each copy takes its hash and activity row.
fn extend_maps(
    data: &mut Vec<u8>,
    descriptor: usize,
    count: usize,
    entries: usize,
    added: &[Added],
) -> AuthoringResult<()> {
    let sources = (0..count)
        .map(|entry| (entries + entry * MAP_SIZE, None))
        .chain(added.iter().map(|item| {
            (
                entries + item.source * MAP_SIZE,
                Some((item.hash, item.row)),
            )
        }))
        .collect::<Vec<_>>();
    let mut bytes = Vec::with_capacity(sources.len() * MAP_SIZE);
    let mut traits = Vec::with_capacity(sources.len());
    for &(source, _) in &sources {
        bytes.extend_from_slice(
            data.get(source..source + MAP_SIZE)
                .ok_or_else(|| invalid("A private match map entry leaves its table"))?,
        );
        traits.push(if read_i64(data, source + MAP_TRAITS)? == 0 {
            None
        } else {
            Some(bounded_relative_target(
                data,
                source + MAP_TRAITS,
                "Private match map traits",
            )?)
        });
    }
    append_native_array(data, descriptor, MAP_CLASS, sources.len(), &bytes)?;
    let (_, _, moved, _) = array_at(data, descriptor)?;
    for (index, ((_, added), traits)) in sources.iter().zip(traits).enumerate() {
        let entry = moved + index * MAP_SIZE;
        if let Some(traits) = traits {
            write_relative_pointer(data, entry + MAP_TRAITS, traits)?;
        }
        if let Some((hash, row)) = added {
            write_u32(data, entry, *hash)?;
            write_u16(data, entry + MAP_ACTIVITY, *row)?;
        }
    }
    Ok(())
}
