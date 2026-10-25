//! Relocate selector markers and direct cue events without changing frame or dispatch fields.
use super::*;
mod library;
pub(super) use library::Library;

pub(super) fn convert(source: &Payload, native: &mut [u8], frames: u16) -> Result<Value> {
    let rows = source.array(0x140, 8, Some(0x808004E1))?;
    move_array(source, native, 0x140, 0x160, (0x808004E1, 0x8080902B))?;
    let mut events = Vec::new();
    let mut cues = BTreeSet::new();
    let mut records = BTreeSet::new();
    let mut patches = Vec::new();
    for row in rows {
        let at = source.pointer(row)?;
        ensure!(
            records.insert(at),
            "Source clip repeats a resource-event record"
        );
        ensure!(
            at >= SOURCE_HEADER + 4,
            "Source event points into the clip header"
        );
        let class = source.u32(at - 4)?;
        let frame = source.i16(at)?;
        ensure!(
            frame >= 0 && (frame as u16) < frames,
            "Source animation event frame exceeds the clip"
        );
        if class == 0x80800436 {
            let record = source.bytes::<6>(at)?;
            ensure!(
                record[2] == 2 && record[3] == 0 && record[5] == 0,
                "Source selector-event flags are unsupported"
            );
            // Both original lookup consumers compare kind byte +2, read the
            // signed frame at +0 and read the selector as a signed byte at +4.
            put(native, at + SHIFT - 4, &0x8080903Au32.to_le_bytes());
            events.push(json!({"frame":frame,"selector":record[4] as i8,
                "source_offset":at,"native_offset":at + SHIFT}));
            continue;
        }
        ensure!(
            class == 0x8080060E,
            "Source event {class:08X} needs selector or named-event translation"
        );
        let record = source.bytes::<32>(at)?;
        ensure!(
            record[2] == 3 && record[3] == 0,
            "Source resource-event frame or kind is unsupported"
        );
        let flags = source.u32(at + 4)?;
        ensure!(
            flags & !0x103 == 0
                && flags & 3 <= 2
                && source.u32(at + 12)? == 0
                && source.u32(at + 28)? == 0,
            "Source resource-event dispatch fields are unsupported"
        );
        let path = source.pointer(at + 16)?;
        ensure!(
            path >= SOURCE_HEADER,
            "Source event path points into the clip header"
        );
        let end = source.0[path..]
            .iter()
            .position(|b| *b == 0)
            .context("Source event path has no terminator")?;
        let path = std::str::from_utf8(&source.0[path..path + end])?;
        ensure!(
            (path.starts_with("content/") || path.starts_with("content\\"))
                && path.ends_with(".wwise_event"),
            "Source resource event requires sequence translation"
        );
        let cue = source.u32(at + 24)?;
        ensure!(
            !matches!(cue, 0 | u32::MAX | 0x811C9DC5),
            "Source event has no cue"
        );
        // The row pointer and path pointer move together with their targets. The class and
        // resource reference are the only words that change within the original record.
        put(native, at + SHIFT - 4, &0x80809033u32.to_le_bytes());
        put(native, at + SHIFT + 24, &u32::MAX.to_le_bytes());
        cues.insert(cue);
        patches
            .push(json!({"offset":at + SHIFT + 24,"symbol":crate::d1_roi::audio::cue_symbol(cue)}));
        events.push(
            json!({"frame":frame,"source_cue":cue,"path":path,"dispatch_fields":flags,
            "source_offset":at,"native_offset":at + SHIFT}),
        );
    }
    Ok(json!({"events":events,"cues":cues,"patches":patches,"gameplay_verified":false}))
}
