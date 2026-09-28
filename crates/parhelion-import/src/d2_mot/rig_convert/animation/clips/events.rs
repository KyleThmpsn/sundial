//! Carry a source clip's event timing on the native counterpart's event block.
//!
//! Event records name audio events by path string and reference their resources by tag, which
//! the source alone cannot supply. The name-matched native clip's event block is already valid
//! for this game, so the converted clip always carries it. When both clips hold the same events
//! (count, record kinds, marker hashes and audio paths), the source values are written into it.
//! When they differ, the native block keeps its own timing within the converted clip's frames,
//! because the animation must stay the source's either way.
//!
//! A record's kind word holds its type in the high half and its frame in the low half. Records
//! are packed with lengths that depend on their class, so a record is read no further than the
//! next structure in the block.
use super::word;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// Source event list, record and marker classes and their native counterparts.
const LIST: (u32, u32) = (0x80808C08, 0x8080902B);
const MARKERS: (u32, u32) = (0x80808C5F, 0x8080907F);
const RECORDS: [(u32, u32); 4] = [
    (0x80808C11, 0x80809031),
    (0x80808C12, 0x80809032),
    (0x80808C13, 0x80809033),
    (0x80808C1A, 0x8080903A),
];
/// The longest record, a source audio or sequence reference.
const RECORD_SPAN: usize = 0x40;
/// The kind and frame words every record begins with.
const RECORD_HEAD: usize = 8;
/// The native clip header's frame count.
const NATIVE_FRAMES: usize = 0x13C;

pub(super) mod library;

#[derive(Clone)]
struct Record {
    at: usize,
    end: usize,
    class: u32,
    kind: u32,
    // Event payload, not timing. The 6-byte sequence-event record stores a
    // native u16 selector here. Modern builds inserted selectors into the enum.
    data: u32,
    strings: Vec<String>,
    name: Option<u32>,
}

struct Events {
    /// First byte of the region holding the list, records, strings and markers.
    block_start: usize,
    list_header: Option<usize>,
    records: Vec<Record>,
    /// Marker rows as (hash, value) in array order.
    markers: Vec<(u32, u32)>,
    marker_header: Option<usize>,
}

fn strings(p: &Payload, record: usize, end: usize) -> Vec<String> {
    let mut out = Vec::new();
    for field in (0..end - record).step_by(8) {
        if record + field + 8 > end {
            break;
        }
        let Ok(target) = p.pointer(record + field) else {
            continue;
        };
        if target == record + field
            || target + 8 > p.0.len()
            || !p.0[target..].starts_with(b"content")
        {
            continue;
        }
        if let Some(end) = p.0[target..].iter().position(|b| *b == 0) {
            out.push(String::from_utf8_lossy(&p.0[target..target + end]).into_owned());
        }
    }
    out
}

fn events(p: &Payload, modern: bool) -> Result<Option<Events>> {
    let count = p.u64(0x160)?;
    let marker_count = p.u64(0x170)?;
    if count == 0 && marker_count == 0 {
        ensure!(
            p.u64(0x168)? == 0 && p.u64(0x178)? == 0,
            "empty event list has a pointer"
        );
        return Ok(None);
    }
    ensure!(count <= 256, "clip event count is unsupported");
    let list_class = if modern { LIST.0 } else { LIST.1 };
    let header = if count > 0 {
        let header = p.pointer(0x168)?;
        ensure!(
            header >= 4 && p.u64(header)? == count && p.u32(header + 8)? == list_class,
            "clip event list header differs"
        );
        Some(header)
    } else {
        ensure!(p.u64(0x168)? == 0, "empty event list has a pointer");
        None
    };
    let mut block_start = header.map_or(p.0.len(), |at| at - 4);
    let starts = (0..count as usize)
        .map(|i| {
            let at = p.pointer(header.context("event list header")? + 16 + i * 8)?;
            ensure!(
                at >= 4 && at + RECORD_HEAD <= p.0.len(),
                "clip event record outside payload"
            );
            Ok(at)
        })
        .collect::<Result<Vec<_>>>()?;
    let marker_header = if marker_count > 0 {
        let at = p.pointer(0x178)?;
        ensure!(at >= 4, "clip marker header outside payload");
        Some(at)
    } else {
        None
    };
    // Each structure in the block is preceded by its class word, which is where the record
    // before it ends.
    let boundaries = starts
        .iter()
        .chain(header.iter())
        .chain(marker_header.iter())
        .map(|at| at - 4)
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for at in starts {
        let class = p.u32(at - 4)?;
        ensure!(
            RECORDS
                .iter()
                .any(|(s, n)| class == if modern { *s } else { *n }),
            "unrecognized clip event record {class:08X}"
        );
        let end = boundaries
            .iter()
            .copied()
            .filter(|boundary| *boundary > at)
            .chain([at + RECORD_SPAN, p.0.len()])
            .min()
            .context("record end")?;
        ensure!(
            end >= at + RECORD_HEAD,
            "clip event record overlaps the next"
        );
        let strings = strings(p, at, end);
        block_start = block_start.min(at - 4);
        for field in (0..end - at)
            .step_by(8)
            .filter(|field| at + field + 8 <= end)
        {
            if let Ok(target) = p.pointer(at + field)
                && target != at + field
                && target + 8 <= p.0.len()
                && p.0[target..].starts_with(b"content")
            {
                block_start = block_start.min(target);
            }
        }
        records.push(Record {
            at,
            end,
            class,
            kind: p.u32(at)?,
            data: p.u32(at + 4)?,
            strings,
            name: if [RECORDS[0].0, RECORDS[0].1].contains(&class) {
                ensure!(at + 12 <= end, "truncated named animation event");
                Some(p.u32(at + 8)?)
            } else {
                None
            },
        });
    }
    let mut markers = Vec::new();
    if let Some(at) = marker_header {
        let marker_class = if modern { MARKERS.0 } else { MARKERS.1 };
        for row in p.array(0x170, 8, Some(marker_class))? {
            markers.push((p.u32(row)?, p.u32(row + 4)?));
        }
        block_start = block_start.min(at - 4);
    }
    ensure!(
        block_start >= 0x190 && block_start.is_multiple_of(4),
        "clip event block overlaps the fixed header"
    );
    Ok(Some(Events {
        block_start,
        list_header: header,
        records,
        markers,
        marker_header,
    }))
}

/// Check that the native counterpart carries the same events as the source,
/// apart from timing, so its block can stand in for a converted one.
fn same_shape(source: &Events, native: &Events) -> Result<()> {
    ensure!(
        source.records.len() == native.records.len(),
        "source and native counterpart event counts differ"
    );
    for (s, n) in source.records.iter().zip(&native.records) {
        ensure!(
            RECORDS.contains(&(s.class, n.class)),
            "event record {:08X} does not correspond to native {:08X}",
            s.class,
            n.class
        );
        ensure!(s.kind >> 16 == n.kind >> 16, "event record kinds differ");
        ensure!(s.strings == n.strings, "event audio references differ");
        ensure!(s.name == n.name, "named animation event identities differ");
    }
    Ok(())
}

/// Marker values follow the source in either case, and some of them are track slots that the
/// slot alignment renumbers later, so the native block's rows must name the same markers.
fn same_markers(source: &Events, native: &Events) -> Result<()> {
    ensure!(
        source.markers.len() == native.markers.len()
            && source
                .markers
                .iter()
                .zip(&native.markers)
                .all(|(s, n)| s.0 == n.0),
        "event marker hashes differ"
    );
    Ok(())
}

/// Replace the lowered clip's source event block with the native counterpart's
/// block, then write the source timing into it when both clips hold the same
/// events. `lowered` must still hold the source layout, with its event
/// structures at the end of the payload, and the native frame count.
pub(super) fn carry(
    source: &Payload,
    lowered: &mut Payload,
    counterpart: &Payload,
) -> Result<Value> {
    let from = events(source, true)?.context("source clip has no events")?;
    let to = events(counterpart, false)?.context("native counterpart has no events")?;
    carry_selected(source, lowered, counterpart, from, to)
}

/// New clips need no name-matched donor when all their events are an ordered
/// subset of an already validated source/native pair. Selector identity is
/// compared in the source era, retaining the paired native selector value.
pub(super) fn carry_subset(
    source: &Payload,
    lowered: &mut Payload,
    source_template: &Payload,
    counterpart: &Payload,
) -> Result<Value> {
    let from = events(source, true)?.context("source clip has no events")?;
    let basis = events(source_template, true)?.context("source template has no events")?;
    let mut to = events(counterpart, false)?.context("native template has no events")?;
    same_shape(&basis, &to)?;
    same_markers(&basis, &to)?;
    same_markers(&from, &to)?;
    let mut selected = Vec::new();
    let mut cursor = 0;
    for record in &from.records {
        let index = (cursor..basis.records.len())
            .find(|&i| {
                let candidate = &basis.records[i];
                record.class == candidate.class
                    && record.kind >> 16 == candidate.kind >> 16
                    && record.data == candidate.data
                    && record.strings == candidate.strings
                    && record.name == candidate.name
            })
            .context("new clip events have no validated native subset")?;
        selected.push(to.records[index].clone());
        cursor = index + 1;
    }
    to.records = selected;
    let mut report = carry_selected(source, lowered, counterpart, from, to)?;
    report["events"] = json!("validated native event subset with source timing");
    Ok(report)
}

fn carry_selected(
    source: &Payload,
    lowered: &mut Payload,
    counterpart: &Payload,
    from: Events,
    to: Events,
) -> Result<Value> {
    same_markers(&from, &to)?;
    let differs = same_shape(&from, &to).err().map(|error| error.to_string());
    let frames = u32::from(lowered.u16(NATIVE_FRAMES)?);
    ensure!(frames > 0, "converted clip has no frames");
    ensure!(
        lowered.0.len() == source.0.len(),
        "lowered clip layout differs from its source"
    );
    // The source block must be the tail of the payload so nothing else moves.
    let source_tail = &source.0[from.block_start..];
    ensure!(
        !source_tail.is_empty(),
        "source event block is not at the end of the clip"
    );
    let block = &counterpart.0[to.block_start..];
    lowered.0.truncate(from.block_start);
    let start = ((lowered.0.len() + 15) & !15) + to.block_start % 16;
    lowered.0.resize(start, 0);
    lowered.0.extend_from_slice(block);
    let shift = |native_at: usize| -> Result<usize> {
        Ok(start
            + native_at
                .checked_sub(to.block_start)
                .context("event offset")?)
    };
    word(lowered, 0x160, to.records.len() as u32);
    word(lowered, 0x164, 0);
    if let Some(header) = to.list_header {
        let list_header = shift(header)?;
        lowered.0[0x168..0x170]
            .copy_from_slice(&(i64::try_from(list_header)? - 0x168).to_le_bytes());
        lowered.0[list_header..list_header + 8]
            .copy_from_slice(&(to.records.len() as u64).to_le_bytes());
        for (i, record) in to.records.iter().enumerate() {
            let field = list_header + 16 + i * 8;
            let target = shift(record.at)?;
            lowered.0[field..field + 8]
                .copy_from_slice(&(i64::try_from(target)? - i64::try_from(field)?).to_le_bytes());
        }
    } else {
        lowered.0[0x168..0x170].fill(0);
    }
    let mut clamped = 0;
    if differs.is_none() {
        for (s, n) in from.records.iter().zip(&to.records) {
            let at = shift(n.at)?;
            ensure!(
                s.kind & 0xFFFF < frames,
                "source event lies outside its clip"
            );
            word(lowered, at, s.kind);
        }
    } else {
        // An event past the converted clip's last frame fires on that frame instead.
        for n in &to.records {
            if n.kind & 0xFFFF >= frames {
                word(lowered, shift(n.at)?, (n.kind & !0xFFFF) | (frames - 1));
                clamped += 1;
            }
        }
    }
    if let Some(header) = to.marker_header {
        let header = shift(header)?;
        lowered.0[0x178..0x180].copy_from_slice(&(i64::try_from(header)? - 0x178).to_le_bytes());
        for (i, (s, _)) in from.markers.iter().zip(&to.markers).enumerate() {
            word(lowered, header + 16 + i * 8 + 4, s.1);
        }
    } else {
        lowered.0[0x170..0x180].fill(0);
    }
    let size = lowered.0.len() as u64;
    lowered.0[..8].copy_from_slice(&size.to_le_bytes());
    // The relocated block must read back as the native shape it came from.
    let check = events(lowered, false)?.context("carried events unreadable")?;
    ensure!(
        check.records.len() == to.records.len(),
        "carried event block did not relocate cleanly"
    );
    for (i, (c, n)) in check.records.iter().zip(&to.records).enumerate() {
        let kind = match &differs {
            None => from.records[i].kind,
            Some(_) if n.kind & 0xFFFF >= frames => (n.kind & !0xFFFF) | (frames - 1),
            Some(_) => n.kind,
        };
        ensure!(
            c.kind == kind && c.data == n.data && c.strings == n.strings,
            "carried event block did not relocate cleanly"
        );
    }
    Ok(match differs {
        None => json!({
            "event_count": to.records.len(),
            "marker_count": from.markers.len(),
            "events": "native counterpart block with source timing",
            "frame_shift": from.records.iter().zip(&to.records).map(|(s, n)| i64::from(s.kind & 0xFFFF) - i64::from(n.kind & 0xFFFF)).collect::<Vec<_>>(),
        }),
        Some(reason) => json!({
            "event_count": to.records.len(),
            "marker_count": from.markers.len(),
            "events": "native counterpart block with native timing",
            "source_events_differ": reason,
            "clamped_to_last_frame": clamped,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip whose only content after the fixed header is one event record
    /// with an audio path and one marker row.
    fn clip(modern: bool, frame: u32, marker: u32, path: &[u8], kind: u32) -> Payload {
        let (list, record, markers) = if modern {
            (LIST.0, RECORDS[2].0, MARKERS.0)
        } else {
            (LIST.1, RECORDS[2].1, MARKERS.1)
        };
        let mut b = vec![0u8; 0x190];
        // event list header at 0x1A0, one row, record at 0x1C4, string at 0x210, markers at 0x250
        b.resize(0x270, 0);
        b[NATIVE_FRAMES..NATIVE_FRAMES + 2].copy_from_slice(&30u16.to_le_bytes());
        b[0x160..0x168].copy_from_slice(&1u64.to_le_bytes());
        b[0x168..0x170].copy_from_slice(&(0x1A0i64 - 0x168).to_le_bytes());
        b[0x1A0 - 4..0x1A0].copy_from_slice(&0x80809FBDu32.to_le_bytes());
        b[0x1A0..0x1A8].copy_from_slice(&1u64.to_le_bytes());
        b[0x1A8..0x1AC].copy_from_slice(&list.to_le_bytes());
        b[0x1B0..0x1B8].copy_from_slice(&(0x1C4i64 - 0x1B0).to_le_bytes());
        b[0x1C0..0x1C4].copy_from_slice(&record.to_le_bytes());
        b[0x1C4..0x1C8].copy_from_slice(&kind.to_le_bytes());
        b[0x1C8..0x1CC].copy_from_slice(&frame.to_le_bytes());
        b[0x1E4..0x1EC].copy_from_slice(&(0x210i64 - 0x1E4).to_le_bytes());
        b[0x210..0x210 + path.len()].copy_from_slice(path);
        b[0x170..0x178].copy_from_slice(&1u64.to_le_bytes());
        b[0x178..0x180].copy_from_slice(&(0x250i64 - 0x178).to_le_bytes());
        b[0x24C..0x250].copy_from_slice(&0x80809FBDu32.to_le_bytes());
        b[0x250..0x258].copy_from_slice(&1u64.to_le_bytes());
        b[0x258..0x25C].copy_from_slice(&markers.to_le_bytes());
        b[0x260..0x264].copy_from_slice(&0xD59A5FE6u32.to_le_bytes());
        b[0x264..0x268].copy_from_slice(&marker.to_le_bytes());
        let len = b.len() as u64;
        b[..8].copy_from_slice(&len.to_le_bytes());
        Payload(b)
    }

    #[test]
    fn native_block_is_carried_with_source_timing() {
        let path = b"content\\audio\\wwise_events\\reload.wwise_event";
        let source = clip(true, 15, 0x40, path, 0x0002_0004);
        let native = clip(false, 14, 0x3E, path, 0x0002_0003);
        let mut lowered = source.clone();
        let report = carry(&source, &mut lowered, &native).unwrap();
        assert_eq!(report["event_count"], 1);
        assert_eq!(report["frame_shift"], json!([1]));
        let carried = events(&lowered, false).unwrap().unwrap();
        assert_eq!(carried.records[0].class, RECORDS[2].1);
        assert_eq!(carried.records[0].kind, 0x0002_0004);
        assert_eq!(carried.records[0].data, 14);
        assert_eq!(carried.markers, vec![(0xD59A5FE6, 0x40)]);
        assert_eq!(
            carried.records[0].strings,
            vec![String::from_utf8_lossy(path).into_owned()]
        );
        assert_eq!(lowered.u64(0).unwrap(), lowered.0.len() as u64);
        // Source classes never survive into the converted clip.
        assert!(
            !lowered
                .0
                .windows(4)
                .any(|w| w == RECORDS[2].0.to_le_bytes())
        );
    }

    #[test]
    fn different_events_keep_the_native_block_and_its_timing() {
        let path = b"content\\audio\\wwise_events\\reload.wwise_event";
        let other = b"content\\audio\\wwise_events\\other.wwise_event";
        let source = clip(true, 15, 0x40, path, 0x0002_0003);
        for (native, kind) in [
            (clip(false, 14, 0x3E, path, 0x0003_0004), 0x0003_0004),
            (clip(false, 14, 0x3E, other, 0x0002_0003), 0x0002_0003),
            // An event past the converted clip's 30 frames fires on its last one.
            (clip(false, 14, 0x3E, other, 0x0002_0040), 0x0002_001D),
        ] {
            let mut lowered = source.clone();
            let report = carry(&source, &mut lowered, &native).unwrap();
            assert_eq!(
                report["events"],
                "native counterpart block with native timing"
            );
            let carried = events(&lowered, false).unwrap().unwrap();
            assert_eq!(
                (carried.records[0].kind, carried.records[0].data),
                (kind, 14)
            );
            assert_eq!(
                carried.records[0].strings,
                events(&native, false).unwrap().unwrap().records[0].strings
            );
            assert_eq!(carried.markers, vec![(0xD59A5FE6, 0x40)]);
        }
        let mut none = clip(false, 14, 0x3E, path, 0x0002_0003);
        none.0[0x160..0x180].fill(0);
        assert!(carry(&source, &mut source.clone(), &none).is_err());
    }

    #[test]
    fn sequence_selectors_stay_native_and_marker_only_counterparts_are_supported() {
        let mut source = clip(true, 15, 0x40, b"", 0x0002_0008);
        let mut native = clip(false, 14, 0x3E, b"", 0x0002_0003);
        word(&mut source, 0x1C0, RECORDS[3].0);
        word(&mut native, 0x1C0, RECORDS[3].1);
        source.0[0x1CC..0x24C].fill(0);
        native.0[0x1CC..0x24C].fill(0);
        let mut lowered = source.clone();
        let report = carry(&source, &mut lowered, &native).unwrap();
        assert_eq!(report["frame_shift"], json!([5]));
        let actual = events(&lowered, false).unwrap().unwrap();
        assert_eq!(actual.records[0].kind & 0xFFFF, 8);
        assert_eq!(actual.records[0].data, 14);

        // A source-only event has no native equivalent. Keep the counterpart's
        // empty event list and still carry source marker/track-slot values.
        native.0[0x160..0x170].fill(0);
        lowered = source.clone();
        carry(&source, &mut lowered, &native).unwrap();
        let actual = events(&lowered, false).unwrap().unwrap();
        assert!(actual.records.is_empty());
        assert_eq!(actual.markers, vec![(0xD59A5FE6, 0x40)]);
    }

    /// Two audio records with their own paths, `spacing` bytes apart. Native records are
    /// packed closer than the longest record.
    fn two(modern: bool, spacing: usize) -> Payload {
        let (list, record, markers) = if modern {
            (LIST.0, RECORDS[2].0, MARKERS.0)
        } else {
            (LIST.1, RECORDS[2].1, MARKERS.1)
        };
        let mut b = vec![0u8; 0x320];
        let put = |b: &mut Vec<u8>, at: usize, bytes: &[u8]| {
            b[at..at + bytes.len()].copy_from_slice(bytes)
        };
        put(&mut b, NATIVE_FRAMES, &30u16.to_le_bytes());
        put(&mut b, 0x160, &2u64.to_le_bytes());
        put(&mut b, 0x168, &(0x1A0i64 - 0x168).to_le_bytes());
        put(&mut b, 0x19C, &0x80809FBDu32.to_le_bytes());
        put(&mut b, 0x1A0, &2u64.to_le_bytes());
        put(&mut b, 0x1A8, &list.to_le_bytes());
        for (i, path) in [&b"content\\a.wwise_event"[..], b"content\\b.wwise_event"]
            .into_iter()
            .enumerate()
        {
            let (at, string) = (0x1C4 + i * spacing, 0x260 + i * 0x40);
            put(
                &mut b,
                0x1B0 + i * 8,
                &((at - (0x1B0 + i * 8)) as i64).to_le_bytes(),
            );
            put(&mut b, at - 4, &record.to_le_bytes());
            put(&mut b, at, &(0x0003_0002 + i as u32 * 5).to_le_bytes());
            put(
                &mut b,
                at + 0x10,
                &((string - (at + 0x10)) as i64).to_le_bytes(),
            );
            put(&mut b, string, path);
        }
        put(&mut b, 0x170, &1u64.to_le_bytes());
        put(&mut b, 0x178, &(0x300i64 - 0x178).to_le_bytes());
        put(&mut b, 0x2FC, &0x80809FBDu32.to_le_bytes());
        put(&mut b, 0x300, &1u64.to_le_bytes());
        put(&mut b, 0x308, &markers.to_le_bytes());
        put(&mut b, 0x310, &0xD59A5FE6u32.to_le_bytes());
        let len = b.len() as u64;
        put(&mut b, 0, &len.to_le_bytes());
        Payload(b)
    }

    #[test]
    fn packed_native_records_are_read_no_further_than_the_next() {
        let (source, native) = (two(true, 0x40), two(false, 0x28));
        let read = events(&native, false).unwrap().unwrap();
        assert_eq!(
            read.records[0].strings,
            vec!["content\\a.wwise_event".to_owned()]
        );
        let report = carry(&source, &mut source.clone(), &native).unwrap();
        assert_eq!(
            report["events"],
            "native counterpart block with source timing"
        );
    }
}
