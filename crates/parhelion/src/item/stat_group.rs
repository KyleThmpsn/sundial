//! The stat group table with the project's own stat display groups appended.
//!
//! The client investment globals name the stat group table at slot 60 (class `0x80805CFC`, in
//! `w64_investment_globals_client_0709`). Its root array at +8 holds 0x38-byte group rows
//! (class `0x80805D02`): the group hash at +0, a scaled-stat array at +0x10, an array at +0x20
//! that every stock row leaves empty, and the maximum at +0x30. A scaled stat (0x18 bytes, class
//! `0x80805D06`) is the definition index at +0, the numeric flag at +1, the linear flag at +3
//! and a display curve array at +8 (class `0x80807D1A`, pairs of investment value and shown
//! value). Items name a group by its row index.
//!
//! A group of the project's own is appended after the stock rows, which keep their indices. The
//! stock rows move to a new root array at the end of the tag, their nested arrays staying where
//! they are, and each new row's arrays follow it. The authored table is decoded again and every
//! stock group must read back unchanged.
use super::*;
use crate::stat_group::CustomStatGroup;
use crate::tag_payload::{
    append_native_array, array_at, read_array, read_i32, read_i64, read_u64,
    synchronize_payload_size, write_i32, write_i64,
};

const TABLE_SLOT: usize = 60;
const TABLE_CLASS: u32 = 0x8080_5CFC;
const GROUP_CLASS: u32 = 0x8080_5D02;
const SCALED_CLASS: u32 = 0x8080_5D06;
const POINT_CLASS: u32 = 0x8080_7D1A;
const GROUP_ROW: usize = 0x38;
const SCALED_ROW: usize = 0x18;
const POINT_ROW: usize = 8;
const SCALED_ARRAY: usize = 0x10;
const SPARE_ARRAY: usize = 0x20;
const MAXIMUM: usize = 0x30;
const CURVE_ARRAY: usize = 8;

/// One group as the table holds it, to compare the authored table with the stock one.
#[derive(Debug, PartialEq, Eq)]
struct Group {
    head: [u8; 0x10],
    maximum: i32,
    stats: Vec<([u8; 8], Vec<[i32; 2]>)>,
}

fn decode(table: &[u8]) -> AuthoringResult<Vec<Group>> {
    if read_u64(table, 0)? != table.len() as u64 {
        return Err(invalid(
            "The stat group table's size field differs from its length",
        ));
    }
    let (count, _, rows, class) = array_at(table, 8)?;
    if class != GROUP_CLASS {
        return Err(invalid("The stat group table's rows are not stat groups"));
    }
    (0..count)
        .map(|index| {
            let row = rows + index * GROUP_ROW;
            let (spare, ..) = array_at(table, row + SPARE_ARRAY)?;
            if spare != 0 {
                return Err(invalid(format!(
                    "Stat group {index} fills a list Parhelion does not read"
                )));
            }
            let (scaled, _, scaled_rows, scaled_class) = array_at(table, row + SCALED_ARRAY)?;
            if scaled != 0 && scaled_class != SCALED_CLASS {
                return Err(invalid(format!("Stat group {index} lists unknown rows")));
            }
            let stats = (0..scaled)
                .map(|stat| {
                    let at = scaled_rows + stat * SCALED_ROW;
                    let (points, _, point_rows, point_class) = array_at(table, at + CURVE_ARRAY)?;
                    if points != 0 && point_class != POINT_CLASS {
                        return Err(invalid(format!(
                            "Stat group {index} has a curve of unknown points"
                        )));
                    }
                    let curve = (0..points)
                        .map(|point| {
                            let at = point_rows + point * POINT_ROW;
                            Ok([read_i32(table, at)?, read_i32(table, at + 4)?])
                        })
                        .collect::<AuthoringResult<Vec<_>>>()?;
                    Ok((read_array(table, at)?, curve))
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            Ok(Group {
                head: read_array(table, row)?,
                maximum: read_i32(table, row + MAXIMUM)?,
                stats,
            })
        })
        .collect()
}

/// The group a custom stat group becomes: the stock rows' common head with its own hash, and its
/// stats with their flags and curves.
fn group(head: [u8; 0x10], hash: u32, custom: &CustomStatGroup) -> Group {
    let mut head = head;
    head[..4].copy_from_slice(&hash.to_le_bytes());
    Group {
        head,
        maximum: custom.maximum_value,
        stats: custom
            .stats
            .iter()
            .map(|stat| {
                let mut row = [0; 8];
                // The definition index fits a byte, which the recipe checks.
                row[0] = stat.definition_index as u8;
                row[1] = u8::from(stat.display_as_numeric);
                row[3] = u8::from(stat.is_linear);
                (row, stat.display.clone())
            })
            .collect(),
    }
}

/// A hash for a new group from its content, apart from every hash already in the table.
fn hash(custom: &CustomStatGroup, taken: &BTreeSet<u32>) -> AuthoringResult<u32> {
    let content = serde_json::to_string(custom)
        .map_err(|error| invalid(format!("Custom stat group: {error}")))?;
    (0..u32::MAX)
        .map(|salt| {
            format!("parhelion/stat-group/{content}/{salt}")
                .bytes()
                .fold(0x811C_9DC5_u32, |hash, byte| {
                    (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
                })
        })
        .find(|hash| !taken.contains(hash) && *hash != 0x811C_9DC5)
        .ok_or_else(|| invalid("No free stat group hash"))
}

/// `table` with `groups` appended, the stock rows keeping their indices.
fn append(table: &[u8], groups: &[CustomStatGroup]) -> AuthoringResult<Vec<u8>> {
    let stock = decode(table)?;
    let (count, _, rows, _) = array_at(table, 8)?;
    // A new row copies the head most stock rows share outside their hash. In Shadowkeep 63 of
    // the 64 groups hold FFFF, the empty name hash and zero there. Group 61 alone holds 3 and
    // 4B2C7A94, whose meaning is not established.
    let mut shared = BTreeMap::<[u8; 12], usize>::new();
    for group in &stock {
        let tail: [u8; 12] = group.head[4..].try_into().unwrap_or_default();
        *shared.entry(tail).or_default() += 1;
    }
    let (tail, rows_sharing) = shared
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .ok_or_else(|| invalid("The stat group table is empty"))?;
    if rows_sharing * 2 <= stock.len() {
        return Err(invalid(
            "No head is shared by most stock stat groups for a custom group to copy",
        ));
    }
    let mut head = [0; 0x10];
    head[4..].copy_from_slice(&tail);
    let mut taken = stock
        .iter()
        .map(|group| u32::from_le_bytes(group.head[..4].try_into().unwrap_or_default()))
        .collect::<BTreeSet<_>>();
    let mut authored = Vec::with_capacity(groups.len());
    for custom in groups {
        let hash = hash(custom, &taken)?;
        taken.insert(hash);
        authored.push(group(head, hash, custom));
    }

    // The root array moves to the end: stock rows first, their nested arrays pointed at where
    // they already are, then the new rows with empty arrays filled below.
    let mut out = table.to_vec();
    let start = (out.len() + 0x23) & !0xF;
    let mut row_bytes = vec![0; (count + authored.len()) * GROUP_ROW];
    for index in 0..count {
        let from = rows + index * GROUP_ROW;
        let to = index * GROUP_ROW;
        row_bytes[to..to + GROUP_ROW].copy_from_slice(&table[from..from + GROUP_ROW]);
        for field in [SCALED_ARRAY, SPARE_ARRAY] {
            if read_i64(table, from + field + 8)? == 0 {
                continue;
            }
            // The pointer is relative to the row's new place in the tag.
            let target = crate::tag_payload::relative_target(table, from + field + 8)?;
            let relative = i64::try_from(target)
                .and_then(|target| i64::try_from(start + to + field + 8).map(|at| target - at))
                .map_err(|_| invalid("Stat group pointer does not fit 64 bits"))?;
            write_i64(&mut row_bytes, to + field + 8, relative)?;
        }
    }
    for (offset, group) in authored.iter().enumerate() {
        let to = (count + offset) * GROUP_ROW;
        row_bytes[to..to + 0x10].copy_from_slice(&group.head);
        write_i32(&mut row_bytes, to + MAXIMUM, group.maximum)?;
    }
    append_native_array(&mut out, 8, GROUP_CLASS, count + authored.len(), &row_bytes)?;
    let (_, _, new_rows, _) = array_at(&out, 8)?;
    if new_rows != start {
        return Err(validation(
            "The stat group rows did not land where they were planned",
        ));
    }
    for (offset, group) in authored.iter().enumerate() {
        let row = start + (count + offset) * GROUP_ROW;
        let mut scaled = vec![0; group.stats.len() * SCALED_ROW];
        for (index, (head, _)) in group.stats.iter().enumerate() {
            scaled[index * SCALED_ROW..index * SCALED_ROW + 8].copy_from_slice(head);
        }
        append_native_array(
            &mut out,
            row + SCALED_ARRAY,
            SCALED_CLASS,
            group.stats.len(),
            &scaled,
        )?;
        let (_, _, scaled_rows, _) = array_at(&out, row + SCALED_ARRAY)?;
        for (index, (_, curve)) in group.stats.iter().enumerate() {
            let points = curve
                .iter()
                .flat_map(|[value, shown]| {
                    value.to_le_bytes().into_iter().chain(shown.to_le_bytes())
                })
                .collect::<Vec<_>>();
            let descriptor = scaled_rows + index * SCALED_ROW + CURVE_ARRAY;
            append_native_array(&mut out, descriptor, POINT_CLASS, curve.len(), &points)?;
        }
    }
    let out = synchronize_payload_size(out)?;
    let read = decode(&out)?;
    if read.len() != stock.len() + authored.len()
        || read[..stock.len()] != stock[..]
        || read[stock.len()..] != authored[..]
    {
        return Err(validation(
            "The authored stat group table does not read back as planned",
        ));
    }
    Ok(out)
}

/// Appends each distinct custom stat group in the project to the stat group table and points
/// every weapon that describes one at its row, through the stock group index the definitions
/// already write. `None` when no weapon describes one.
pub(super) fn plan(
    sources: &sources::ProjectSources,
    resolved: &mut [resolve::ResolvedWeapon],
) -> AuthoringResult<Option<ReplacementSpec>> {
    let mut groups: Vec<CustomStatGroup> = Vec::new();
    for donor in resolved.iter() {
        if let Some(group) = &donor.weapon.overrides.custom_stat_group
            && !groups.contains(group)
        {
            groups.push(group.clone());
        }
    }
    if groups.is_empty() {
        return Ok(None);
    }
    let tag = globals_child_tag(&sources.globals_data, TABLE_SLOT)?;
    sources::validate_source_tag(&sources.manager, tag, TABLE_CLASS, "Stat group table")?;
    let table = read_tag(&sources.manager, tag, "stat group table")?;
    let (stock, ..) = array_at(&table, 8)?;
    let payload = append(&table, &groups)?;
    for donor in resolved.iter_mut() {
        let Some(group) = &donor.weapon.overrides.custom_stat_group else {
            continue;
        };
        let position = groups
            .iter()
            .position(|known| known == group)
            .ok_or_else(|| validation("A custom stat group was not planned"))?;
        let index = u16::try_from(stock + position)
            .map_err(|_| invalid("The stat group table has too many rows"))?;
        donor.weapon.overrides.stat_group_index = Some(index);
    }
    Ok(Some(ReplacementSpec { tag, payload }))
}
