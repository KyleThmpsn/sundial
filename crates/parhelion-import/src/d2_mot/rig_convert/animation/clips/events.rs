//! Carry a source clip's event timing on the native counterpart's event block.
//!
//! Event records name audio events by path string and typed enums whose native
//! values are not derivable from the source alone. When the name-matched native
//! clip has the same event shape (count, record kinds, marker hashes and audio
//! paths), its event block is already valid for this game and only the timing
//! belongs to the source. The converted clip therefore keeps the native block
//! and writes the source frame of each record and marker into it.
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
const RECORD_SPAN: usize = 0x40;

struct Record {
    at: usize,
    class: u32,
    kind: u32,
    frame: u32,
    strings: Vec<String>,
}

struct Events {
    /// First byte of the region holding the list, records, strings and markers.
    block_start: usize,
    records: Vec<Record>,
    /// Marker rows as (hash, value) in array order.
    markers: Vec<(u32, u32)>,
    marker_header: Option<usize>,
}

fn strings(p: &Payload, record: usize) -> Vec<String> {
    let mut out = Vec::new();
    for field in (0..RECORD_SPAN).step_by(8) {
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
    ensure!(count > 0 && count <= 256, "clip event count is unsupported");
    let list_class = if modern { LIST.0 } else { LIST.1 };
    let header = p.pointer(0x168)?;
    ensure!(
        header >= 4 && p.u64(header)? == count && p.u32(header + 8)? == list_class,
        "clip event list header differs"
    );
    let mut block_start = header - 4;
    let mut records = Vec::new();
    for i in 0..count as usize {
        let at = p.pointer(header + 16 + i * 8)?;
        ensure!(
            at >= 4 && at + RECORD_SPAN <= p.0.len(),
            "clip event record outside payload"
        );
        let class = p.u32(at - 4)?;
        ensure!(
            RECORDS
                .iter()
                .any(|(s, n)| class == if modern { *s } else { *n }),
            "unrecognized clip event record {class:08X}"
        );
        let strings = strings(p, at);
        block_start = block_start.min(at - 4);
        for field in (0..RECORD_SPAN).step_by(8) {
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
            class,
            kind: p.u32(at)?,
            frame: p.u32(at + 4)?,
            strings,
        });
    }
    let mut markers = Vec::new();
    let mut marker_header = None;
    if marker_count > 0 {
        let marker_class = if modern { MARKERS.0 } else { MARKERS.1 };
        for row in p.array(0x170, 8, Some(marker_class))? {
            markers.push((p.u32(row)?, p.u32(row + 4)?));
        }
        let at = p.pointer(0x178)?;
        ensure!(at >= 4, "clip marker header outside payload");
        marker_header = Some(at);
        block_start = block_start.min(at - 4);
    }
    ensure!(
        block_start >= 0x190 && block_start.is_multiple_of(4),
        "clip event block overlaps the fixed header"
    );
    Ok(Some(Events {
        block_start,
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
        ensure!(s.kind == n.kind, "event record kinds differ");
        ensure!(s.strings == n.strings, "event audio references differ");
    }
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
/// block, then write the source timing into it. `lowered` must still hold the
/// source layout, with its event structures at the end of the payload.
pub(super) fn carry(
    source: &Payload,
    lowered: &mut Payload,
    counterpart: &Payload,
) -> Result<Value> {
    let from = events(source, true)?.context("source clip has no events")?;
    let to = events(counterpart, false)?.context("native counterpart has no events")?;
    same_shape(&from, &to)?;
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
    let list_header = shift(counterpart.pointer(0x168)?)?;
    word(lowered, 0x160, to.records.len() as u32);
    word(lowered, 0x164, 0);
    lowered.0[0x168..0x170].copy_from_slice(&(i64::try_from(list_header)? - 0x168).to_le_bytes());
    for (s, n) in from.records.iter().zip(&to.records) {
        let at = shift(n.at)?;
        word(lowered, at + 4, s.frame);
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
        check.records.len() == to.records.len()
            && check
                .records
                .iter()
                .zip(&from.records)
                .all(|(c, s)| c.frame == s.frame && c.kind == s.kind && c.strings == s.strings),
        "carried event block did not relocate cleanly"
    );
    Ok(json!({
        "event_count": from.records.len(),
        "marker_count": from.markers.len(),
        "events": "native counterpart block with source timing",
        "frame_shift": from.records.iter().zip(&to.records).map(|(s, n)| s.frame as i64 - n.frame as i64).collect::<Vec<_>>(),
    }))
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
        let source = clip(true, 15, 0x40, path, 0x0002_0003);
        let native = clip(false, 14, 0x3E, path, 0x0002_0003);
        let mut lowered = source.clone();
        let report = carry(&source, &mut lowered, &native).unwrap();
        assert_eq!(report["event_count"], 1);
        assert_eq!(report["frame_shift"], json!([1]));
        let carried = events(&lowered, false).unwrap().unwrap();
        assert_eq!(carried.records[0].class, RECORDS[2].1);
        assert_eq!(carried.records[0].frame, 15);
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
    fn different_kinds_paths_or_counts_keep_the_native_clip() {
        let path = b"content\\audio\\wwise_events\\reload.wwise_event";
        let source = clip(true, 15, 0x40, path, 0x0002_0003);
        let other_kind = clip(false, 14, 0x3E, path, 0x0002_0004);
        assert!(carry(&source, &mut source.clone(), &other_kind).is_err());
        let other_path = clip(
            false,
            14,
            0x3E,
            b"content\\audio\\wwise_events\\other.wwise_event",
            0x0002_0003,
        );
        assert!(carry(&source, &mut source.clone(), &other_path).is_err());
        let mut none = other_kind.clone();
        none.0[0x160..0x180].fill(0);
        assert!(carry(&source, &mut source.clone(), &none).is_err());
    }
}
