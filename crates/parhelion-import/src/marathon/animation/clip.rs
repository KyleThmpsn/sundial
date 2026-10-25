//! Lower checked Marathon track containers through the shared Tiger codec translator.
//!
//! The source clip keeps the Destiny field layout with its own class words. Pose codecs occupy
//! `+10` and `+18`, clip-level scalar and four-lane curves `+20` and `+28`, the hand constraint
//! bindings `+30` with their weights at `+38` and `+40` and target transforms at `+48`, and an
//! auxiliary transform track `+60`. Constraint binding rows `8080AD61` have the 64-byte layout of
//! native `80808F5C`.
use super::*;

fn array(
    p: &Payload,
    out: &mut Payload,
    at: usize,
    stride: usize,
    from: u32,
    to: u32,
) -> Result<()> {
    p.array_range(at, stride, Some(from))?;
    if p.u64(at)? == 0 {
        ensure!(
            p.u64(at + 8)? == 0,
            "Empty source animation array has a pointer"
        );
        return Ok(());
    }
    let header = p.pointer(at + 8)?;
    ensure!(
        header >= 4 && p.u32(header - 4)? == 0x8080BFCD,
        "Source animation allocation marker differs"
    );
    put(&mut out.0, header - 4, &0x80809FB8u32.to_le_bytes())?;
    put(&mut out.0, header + 8, &to.to_le_bytes())
}

type Fields = &'static [(usize, usize, u32, u32)];

/// Source stream classes, the modern class whose checked translator lowers them, the encoding
/// discriminator where the stream has one, and the arrays relabeled on the way.
fn codec(class: u32) -> Result<(u32, Option<u16>, Fields)> {
    const CURVE: Fields = &[
        (0x10, 2, 0x80800006, 0x80800006),
        (0x20, 2, 0x80800006, 0x80800006),
        (0x30, 1, 0x80800009, 0x80800009),
        (0x40, 1, 0x80800009, 0x80800009),
        (0x50, 4, 0x8080000F, 0x8080000F),
        (0x60, 4, 0x8080000F, 0x8080000F),
        (0x70, 2, 0x80800006, 0x80800006),
    ];
    Ok(match class {
        0x8080B13B => (0x80808B40, Some(3), &[(0x38, 2, 0x8080000A, 0x8080000A)]),
        0x8080B13E => (
            0x80808B42,
            Some(2),
            &[
                (0x18, 2, 0x8080000A, 0x8080000A),
                (0x28, 4, 0x8080000F, 0x8080000F),
                (0x38, 4, 0x8080000F, 0x8080000F),
            ],
        ),
        0x8080B13F => (0x80808B43, Some(1), CURVE),
        0x8080B146 => (0x80808B48, Some(1), CURVE),
        0x8080B14A => (
            0x80808B4C,
            Some(2),
            &[
                (0x10, 2, 0x8080B150, 0x80808B52),
                (0x20, 2, 0x8080000A, 0x8080000A),
                (0x30, 4, 0x8080000F, 0x8080000F),
                (0x40, 4, 0x8080000F, 0x8080000F),
            ],
        ),
        // The scalar curve shares native 80808F7E's arrays and its codec 1 discriminator.
        0x8080B14C => (
            0x80808B4E,
            Some(1),
            &[
                (0x10, 2, 0x80800006, 0x80800006),
                (0x20, 2, 0x80800006, 0x80800006),
                (0x30, 1, 0x80800009, 0x80800009),
                (0x40, 1, 0x80800009, 0x80800009),
                (0x50, 2, 0x80800006, 0x80800006),
            ],
        ),
        // The binding container is the row array itself, without a discriminator.
        0x8080AD5E => (0x80808B2A, None, &[(0x00, 64, 0x8080AD61, 0x80808B2D)]),
        _ => anyhow::bail!("Unsupported source animation codec {class:08X}"),
    })
}

/// Named type-5 events the native carrier's own first-person clips contain, as (value, name).
pub(super) type Vocabulary = BTreeSet<(u32, u32)>;

struct Event {
    class: u32,
    kind: u32,
    value: u32,
    name: u32,
}

fn events(p: &Payload) -> Result<Vec<Event>> {
    let count = p.u64(0x160)?;
    if count == 0 {
        ensure!(p.u64(0x168)? == 0, "Empty source event list has a pointer");
        return Ok(Vec::new());
    }
    ensure!(count <= 256, "Source event count is unsupported");
    let header = p.pointer(0x168)?;
    ensure!(
        p.u64(header)? == count && p.u32(header + 8)? == 0x8080AE29,
        "Source event list header differs"
    );
    (0..count as usize)
        .map(|i| {
            let at = p.pointer(header + 16 + i * 8)?;
            ensure!(at >= 4, "Source event record has no class");
            let class = p.u32(at - 4)?;
            ensure!(
                matches!(class, 0x8080AE32 | 0x8080AE34 | 0x8080AE3B),
                "Source event record {class:08X} is unrecognized"
            );
            let kind = p.u32(at)?;
            let named = class == 0x8080AE32;
            ensure!(
                (kind >> 16)
                    == [5, 3, 2][[0x8080AE32, 0x8080AE34, 0x8080AE3B]
                        .iter()
                        .position(|c| *c == class)
                        .unwrap()],
                "Source event kind and record class disagree"
            );
            Ok(Event {
                class,
                kind,
                value: p.u32(at + 4)?,
                name: if named { p.u32(at + 8)? } else { 0 },
            })
        })
        .collect()
}

/// Append a native event block holding the retained named events. Type-2 records carry
/// sequence selectors whose numbering differs between the games, and type-3 records reference
/// source resources, so both stay in the omission record.
fn write_events(native: &mut Payload, retained: &[&Event], frames: u32) -> Result<()> {
    if retained.is_empty() {
        put(&mut native.0, 0x160, &[0; 16])?;
        return Ok(());
    }
    let out = &mut native.0;
    let header = (out.len() + 4 + 15) & !15;
    out.resize(header - 4, 0);
    out.extend(0x80809FBDu32.to_le_bytes());
    out.extend((retained.len() as u64).to_le_bytes());
    out.extend(0x8080902Bu32.to_le_bytes());
    out.extend([0; 4]);
    let rows = out.len();
    out.resize(rows + retained.len() * 8, 0);
    for (i, event) in retained.iter().enumerate() {
        ensure!(
            event.kind & 0xFFFF < frames,
            "Source event lies outside its clip"
        );
        let record = (out.len() + 4 + 7) & !7;
        out.resize(record - 4, 0);
        out.extend(0x80809031u32.to_le_bytes());
        out.extend(event.kind.to_le_bytes());
        out.extend(event.value.to_le_bytes());
        out.extend(event.name.to_le_bytes());
        let field = rows + i * 8;
        put(
            out,
            field,
            &((record as i64) - (field as i64)).to_le_bytes(),
        )?;
    }
    let end = (out.len() + 7) & !7;
    out.resize(end, 0);
    put(out, 0x160, &(retained.len() as u64).to_le_bytes())?;
    put(out, 0x168, &((header as i64) - 0x168).to_le_bytes())?;
    let size = out.len() as u64;
    put(out, 0, &size.to_le_bytes())
}

/// The source uniform scalar codec `8080B14A` holds selectors at `+10` and samples at `+20` as
/// natively, then an extra per-track array at `+30`, ranges at `+40` and biases at `+50`. Native
/// `80808F7C` reads ranges at `+30` and biases at `+40`. Every examined source codec leaves the
/// extra array empty, and a constant weight track reads its value from the bias, so the range and
/// bias descriptors move down one field with their relative offsets adjusted to the same data.
fn uniform_scalar(stripped: &mut Payload, out: &mut Payload, base: usize) -> Result<()> {
    ensure!(
        stripped.u64(base + 0x30)? == 0,
        "Source uniform scalar codec has an extra per-track array"
    );
    let tracks = u64::from(stripped.u16(base + 2)?);
    for (from, to) in [(0x40, 0x30), (0x50, 0x40)] {
        let count = stripped.u64(base + from)?;
        ensure!(
            count == tracks,
            "Source uniform scalar ranges and biases do not cover the tracks"
        );
        let delta = i64::from_le_bytes(stripped.bytes(base + from + 8)?) + (from - to) as i64;
        for payload in [&mut *stripped, &mut *out] {
            put(&mut payload.0, base + to, &count.to_le_bytes())?;
            put(&mut payload.0, base + to + 8, &delta.to_le_bytes())?;
        }
    }
    for payload in [stripped, out] {
        put(&mut payload.0, base + 0x50, &[0; 16])?;
    }
    Ok(())
}

/// The intermediate class names select existing codec translators. Only fields reached through
/// the checked clip descriptors are relabeled. Encoded sample data is never searched or patched.
/// Without a vocabulary, a clip with events refuses conversion.
pub(super) fn lower(p: &Payload, vocabulary: Option<&Vocabulary>) -> Result<(Payload, Value)> {
    ensure!(
        p.0.len() >= 0x190 && p.u64(0)? == p.0.len() as u64,
        "Source animation clip size differs"
    );
    let source_events = events(p)?;
    ensure!(
        source_events.is_empty() || vocabulary.is_some(),
        "Source animation events require their own controller translation"
    );
    let mut out = p.clone();
    // The source block stays as unreferenced bytes. The native block is written afterwards.
    put(&mut out.0, 0x160, &[0; 16])?;
    let mut stripped = out.clone();
    let mut streams = Vec::new();
    for at in (0x10..0x68).step_by(8) {
        if stripped.u64(at)? == 0 {
            continue;
        }
        let base = stripped.pointer(at)?;
        let class = stripped.u32(
            base.checked_sub(4)
                .context("Source animation codec class")?,
        )?;
        let (target, encoding, fields) = codec(class)?;
        if let Some(encoding) = encoding {
            ensure!(
                stripped.u16(base)? == encoding,
                "Source animation encoding differs"
            );
        }
        if class == 0x8080B14A {
            uniform_scalar(&mut stripped, &mut out, base)?;
        }
        for &(offset, stride, from, to) in fields {
            array(&stripped, &mut out, base + offset, stride, from, to)?;
        }
        put(&mut out.0, base - 4, &target.to_le_bytes())?;
        streams.push(json!({"field":at,"source_class":format!("{class:08X}")}));
    }
    for at in [0xA8, 0xB8, 0xC8, 0xD8, 0xE8, 0xF8, 0x108] {
        array(&stripped, &mut out, at, 2, 0x8080000A, 0x8080000A)?;
    }
    for at in [0x90, 0x150] {
        array(&stripped, &mut out, at, 8, 0x80800070, 0x80800070)?;
    }
    array(&stripped, &mut out, 0x170, 8, 0x8080AE7F, 0x80808C5F)?;
    let (mut native, mut report) = crate::tiger::animation::clip::lowering::convert(&out.0)?;
    let frames = u32::from(native.u16(0x13C)?);
    let mut retained = Vec::new();
    let mut omitted = Vec::new();
    for (index, event) in source_events.iter().enumerate() {
        let carried = event.class == 0x8080AE32
            && vocabulary.is_some_and(|v| v.contains(&(event.value, event.name)));
        if carried {
            retained.push(event);
        } else {
            omitted.push(
                json!({"index":index,"type":event.kind >> 16,"frame":event.kind & 0xFFFF,
                "source_class":format!("{:08X}",event.class),"value":event.value,
                "name":(event.class == 0x8080AE32).then(|| format!("{:08X}", event.name)),
                "reason":match event.kind >> 16 {
                    5 => "Named event absent from the native carrier vocabulary",
                    3 => "Source resource reference has no native counterpart",
                    _ => "Sequence selector numbering differs between the games",
                }}),
            );
        }
    }
    write_events(&mut native, &retained, frames)?;
    report["source_class"] = json!("8080AE01");
    report["source_streams"] = json!(streams);
    report["source_events"] = json!(source_events.len());
    report["event_count"] = json!(retained.len());
    report["retained_events"] = json!(
        retained
            .iter()
            .map(
                |e| json!({"frame":e.kind & 0xFFFF,"value":e.value,"name":format!("{:08X}",e.name)})
            )
            .collect::<Vec<_>>()
    );
    report["omitted_events"] = json!(omitted);
    report["controller_linked"] = json!(false);
    Ok((native, report))
}

/// A clip's event list header offset and its records as (record offset, class).
type Records = (usize, Vec<(usize, u32)>);

/// A native clip's event records, as (record offset, class), after its list header.
fn native_records(clip: &Payload) -> Result<Option<Records>> {
    let count = clip.u64(0x160)?;
    if count == 0 {
        return Ok(None);
    }
    ensure!(count <= 256, "Native event count is unsupported");
    let header = clip.pointer(0x168)?;
    ensure!(
        header >= 4 && clip.u64(header)? == count && clip.u32(header + 8)? == 0x8080902B,
        "Native event list header differs"
    );
    let records = (0..count as usize)
        .map(|i| {
            let at = clip.pointer(header + 16 + i * 8)?;
            ensure!(
                at >= 4 && at + 8 <= clip.0.len(),
                "Native event record lies outside its clip"
            );
            Ok((at, clip.u32(at - 4)?))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some((header, records)))
}

/// The cue records of a native clip: value-2 named events, resource references and sequence
/// selectors, the kinds a source clip's omitted events have. Other named values request actions
/// and stay with their own controller.
pub(super) fn cues(clip: &Payload) -> Result<Vec<usize>> {
    let Some((_, records)) = native_records(clip)? else {
        return Ok(Vec::new());
    };
    let mut kept = Vec::new();
    for (at, class) in records {
        let carried = match class {
            0x80809031 => clip.u32(at + 4)? == 2,
            0x80809033 | 0x8080903A => true,
            _ => false,
        };
        if carried {
            kept.push(at);
        }
    }
    Ok(kept)
}

/// Append a donor clip's event block to a clip without events, listing only the donor's cue
/// records with their frames mapped by `retime`. Resource records point to their paths with
/// self-relative offsets and name resources by tag, so the block moves as one piece, keeping its
/// 16-byte phase, as the lowering's counterpart block does. The donor's marker array at `+170`
/// is not referenced, because `+170` holds the clip's own control groups.
pub(super) fn carry_cues(
    clip: &mut Payload,
    donor: &Payload,
    retime: &dyn Fn(u32) -> u32,
) -> Result<Value> {
    ensure!(
        clip.u64(0x160)? == 0 && clip.u64(0x168)? == 0,
        "Translated clip already carries events"
    );
    let frames = u32::from(clip.u16(0x13C)?);
    let (header, records) = native_records(donor)?.context("Donor clip has no events")?;
    let kept = cues(donor)?;
    ensure!(!kept.is_empty(), "Donor clip has no cues");
    // The block begins at the first structure or path it holds and runs to the payload's end.
    let mut start = header - 4;
    for &(at, _) in &records {
        start = start.min(at - 4);
        let end = records
            .iter()
            .map(|r| r.0 - 4)
            .filter(|&b| b > at)
            .chain([at + 0x40, donor.0.len()])
            .min()
            .context("Donor record end")?;
        for field in (at..end.saturating_sub(7)).step_by(8) {
            if let Ok(target) = donor.pointer(field)
                && target != field
                && donor
                    .0
                    .get(target..)
                    .is_some_and(|t| t.starts_with(b"content"))
            {
                start = start.min(target);
            }
        }
    }
    ensure!(start >= 0x190, "Donor event block overlaps the clip header");
    let out = &mut clip.0;
    let base = ((out.len() + 15) & !15) + start % 16;
    out.resize(base, 0);
    out.extend_from_slice(&donor.0[start..]);
    let shift = |at: usize| base + (at - start);
    let list = shift(header);
    put(out, list, &(kept.len() as u64).to_le_bytes())?;
    let mut report = Vec::new();
    for (i, &at) in kept.iter().enumerate() {
        let field = list + 16 + i * 8;
        let moved = shift(at);
        put(out, field, &((moved as i64) - (field as i64)).to_le_bytes())?;
        let kind = donor.u32(at)?;
        let frame = retime(kind & 0xFFFF);
        ensure!(frame < frames, "Retimed cue lies outside its clip");
        put(out, moved, &((kind & !0xFFFF) | frame).to_le_bytes())?;
        let class = donor.u32(at - 4)?;
        report.push(json!({"class":format!("{class:08X}"),"type":kind >> 16,
            "carrier_frame":kind & 0xFFFF,"frame":frame,
            "name":(class == 0x80809031).then(|| donor.u32(at + 8).map(|n| format!("{n:08X}"))).transpose()?,
            "resource":(class == 0x80809033).then(|| donor.u32(at + 0x18).map(|t| format!("{t:08X}"))).transpose()?}));
    }
    put(out, 0x160, &(kept.len() as u64).to_le_bytes())?;
    put(out, 0x168, &((list as i64) - 0x168).to_le_bytes())?;
    let end = (out.len() + 7) & !7;
    out.resize(end, 0);
    let size = out.len() as u64;
    put(out, 0, &size.to_le_bytes())?;
    // The moved block must read back as the cues it was given.
    let (_, moved) = native_records(clip)?.context("Carried cues are unreadable")?;
    ensure!(
        moved.len() == kept.len()
            && moved
                .iter()
                .zip(&kept)
                .all(|(m, &k)| m.1 == donor.u32(k - 4).unwrap_or(0)),
        "Carried cues did not relocate cleanly"
    );
    Ok(json!(report))
}

/// Named type-5 events a native clip contains.
pub(super) fn native_vocabulary(clip: &Payload) -> Result<Vocabulary> {
    let mut words = Vocabulary::new();
    let count = clip.u64(0x160)?;
    if count == 0 {
        return Ok(words);
    }
    let header = clip.pointer(0x168)?;
    ensure!(
        clip.u64(header)? == count && clip.u32(header + 8)? == 0x8080902B,
        "Native event list header differs"
    );
    for i in 0..count as usize {
        let at = clip.pointer(header + 16 + i * 8)?;
        if clip.u32(at - 4)? == 0x80809031 {
            words.insert((clip.u32(at + 4)?, clip.u32(at + 8)?));
        }
    }
    Ok(words)
}
