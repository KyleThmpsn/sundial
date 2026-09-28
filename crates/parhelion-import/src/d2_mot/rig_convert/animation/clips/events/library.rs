//! Lower event sequences independently of the clip that supplied format evidence.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Library {
    selectors: BTreeMap<u32, BTreeSet<u32>>,
    resources: BTreeMap<(u32, String), BTreeSet<u32>>,
    formats: BTreeSet<(u32, u32, u32)>,
}

impl Library {
    /// Only complete, matching event sequences establish enum and record-layout
    /// evidence. Resource paths can be resolved independently in native clips.
    pub fn derive<'a>(pairs: impl IntoIterator<Item = (&'a Payload, &'a Payload)>) -> Self {
        let mut library = Self::default();
        for (source, native) in pairs {
            let (Ok(Some(s)), Ok(Some(n))) = (events(source, true), events(native, false)) else {
                continue;
            };
            library.add_native(native);
            if same_shape(&s, &n).is_err() {
                continue;
            }
            for (a, b) in s.records.iter().zip(&n.records) {
                if validate(source, a, true).is_err() || validate(native, b, false).is_err() {
                    continue;
                }
                if a.class == RECORDS[3].0 {
                    library.selectors.entry(a.data).or_default().insert(b.data);
                } else if a.data == b.data
                    && (a.class != RECORDS[0].0
                        || source.u32(a.at + 8).ok() == native.u32(b.at + 8).ok())
                {
                    library.formats.insert((a.class, a.kind >> 16, a.data));
                }
            }
        }
        library
    }

    pub fn add_native(&mut self, native: &Payload) {
        let Ok(Some(events)) = events(native, false) else {
            return;
        };
        for record in events.records {
            if record.class != RECORDS[2].1 || validate(native, &record, false).is_err() {
                continue;
            }
            if let (Ok(parent), Ok(tag), Some(path)) = (
                native.u32(record.at + 8),
                native.u32(record.at + 24),
                record.strings.first(),
            ) {
                self.resources
                    .entry((parent, path.clone()))
                    .or_default()
                    .insert(tag);
            }
        }
    }

    pub(crate) fn carry(&self, source: &Payload, lowered: &mut Payload) -> Result<Value> {
        let from = events(source, true)?.context("source clip has no event block")?;
        let frames = u32::from(lowered.u16(NATIVE_FRAMES)?);
        ensure!(frames > 0, "converted clip has no frames");
        let mut converted = Vec::new();
        for record in &from.records {
            validate(source, record, true)?;
            ensure!(
                record.kind & 0xFFFF < frames,
                "source event lies outside its clip"
            );
            let class = RECORDS
                .iter()
                .find(|p| p.0 == record.class)
                .context("event class")?
                .1;
            let mut bytes = source.0[record.at..record.at + size(record.class)?].to_vec();
            if record.class == RECORDS[3].0 {
                let values = self
                    .selectors
                    .get(&record.data)
                    .context("event selector has no paired native evidence")?;
                ensure!(
                    values.len() == 1,
                    "event selector has contradictory native evidence"
                );
                bytes[4..8].copy_from_slice(&values.first().unwrap().to_le_bytes());
            } else {
                ensure!(
                    self.formats
                        .contains(&(record.class, record.kind >> 16, record.data)),
                    "event record layout has no paired native evidence"
                );
            }
            let path = if record.class == RECORDS[2].0 {
                let key = (source.u32(record.at + 8)?, record.strings[0].clone());
                let tags = self.resources.get(&key).with_context(|| {
                    format!("event resource has no native resolution: {}", key.1)
                })?;
                ensure!(
                    tags.len() == 1,
                    "event resource path has ambiguous native resolution: {}",
                    key.1
                );
                bytes.truncate(32);
                bytes[16..24].fill(0);
                bytes[24..28].copy_from_slice(&tags.first().unwrap().to_le_bytes());
                bytes[28..32].fill(0);
                Some(key.1)
            } else {
                None
            };
            converted.push((class, bytes, path));
        }
        // Do not remove animation storage. The old event tail becomes unreachable,
        // and every new event/marker pointer targets independently appended storage.
        // Keeping it also preserves stream internals that may follow an event block.
        let list = allocate_array(lowered, 0x160, converted.len(), 8, LIST.1);
        for (i, (class, bytes, path)) in converted.iter().enumerate() {
            let at = allocate(lowered, *class, bytes.len(), 8);
            lowered.0[at..at + bytes.len()].copy_from_slice(bytes);
            pointer(lowered, list + i * 8, at);
            if let Some(path) = path {
                let target = lowered.0.len();
                lowered.0.extend_from_slice(path.as_bytes());
                lowered.0.push(0);
                pointer(lowered, at + 16, target);
            }
        }
        let markers = allocate_array(lowered, 0x170, from.markers.len(), 8, MARKERS.1);
        for (i, (name, value)) in from.markers.iter().enumerate() {
            word(lowered, markers + i * 8, *name);
            word(lowered, markers + i * 8 + 4, *value);
        }
        let len = lowered.0.len() as u64;
        lowered.0[..8].copy_from_slice(&len.to_le_bytes());
        let check = events(lowered, false)?.context("converted event block is missing")?;
        ensure!(
            check.markers == from.markers && check.records.len() == converted.len(),
            "converted event block changed its source sequence"
        );
        for (actual, (class, bytes, path)) in check.records.iter().zip(&converted) {
            validate(lowered, actual, false)?;
            ensure!(
                actual.class == *class
                    && lowered.bytes::<8>(actual.at)? == bytes[..8]
                    && actual.strings == path.iter().cloned().collect::<Vec<_>>(),
                "converted event record failed to relocate"
            );
            if *class == RECORDS[0].1 {
                ensure!(
                    lowered.bytes::<4>(actual.at + 8)? == bytes[8..12],
                    "named event hash changed"
                );
            }
        }
        Ok(
            json!({"events":"source sequence with validated native event formats",
            "event_count":converted.len(),"marker_count":from.markers.len(),
            "source_timing_preserved":true,"source_markers_preserved":true}),
        )
    }
}

fn size(class: u32) -> Result<usize> {
    Ok(match class {
        0x80808C11 | 0x80809031 => 12,
        0x80808C12 | 0x80809032 | 0x80808C1A | 0x8080903A => 8,
        0x80808C13 => 56,
        0x80809033 => 32,
        _ => anyhow::bail!("unsupported animation event layout {class:08X}"),
    })
}

fn validate(p: &Payload, r: &Record, modern: bool) -> Result<()> {
    let end =
        r.at.checked_add(size(r.class)?)
            .context("event record overflow")?;
    ensure!(
        end <= r.end && end <= p.0.len(),
        "truncated animation event record"
    );
    let index = RECORDS
        .iter()
        .position(|pair| r.class == if modern { pair.0 } else { pair.1 })
        .context("event record era differs")?;
    ensure!(
        r.kind >> 16 == [5, 4, 3, 2][index],
        "event class and kind disagree"
    );
    if index == 2 {
        ensure!(
            r.strings.len() == 1 && p.u32(r.at + 12)? == 0,
            "resource event prefix differs"
        );
        let path = p.pointer(r.at + 16)?;
        ensure!(
            p.0.get(path..path + r.strings[0].len()) == Some(r.strings[0].as_bytes()),
            "resource event path pointer differs"
        );
        if modern {
            ensure!(
                p.0[r.at + 40..end].iter().all(|b| *b == 0),
                "resource event extension differs"
            );
        } else {
            ensure!(
                p.u32(r.at + 28)? == 0 && (0x80800000..0x90000000).contains(&p.u32(r.at + 24)?),
                "native event resource reference differs"
            );
        }
    } else {
        ensure!(r.strings.is_empty(), "non-resource event has a path");
        if index == 3 {
            ensure!(
                r.data <= u16::MAX.into(),
                "sequence selector extension differs"
            );
        }
    }
    Ok(())
}

fn pointer(p: &mut Payload, field: usize, target: usize) {
    p.0[field..field + 8].copy_from_slice(&((target as i64) - (field as i64)).to_le_bytes());
}

fn allocate(p: &mut Payload, class: u32, size: usize, alignment: usize) -> usize {
    let at = (p.0.len() + 4 + alignment - 1) & !(alignment - 1);
    p.0.resize(at + size, 0);
    word(p, at - 4, class);
    at
}

fn allocate_array(p: &mut Payload, field: usize, count: usize, stride: usize, class: u32) -> usize {
    p.0[field..field + 16].fill(0);
    if count == 0 {
        return p.0.len();
    }
    let at = allocate(p, 0x80809FBD, 16 + count * stride, 16);
    p.0[field..field + 8].copy_from_slice(&(count as u64).to_le_bytes());
    pointer(p, field + 8, at);
    p.0[at..at + 8].copy_from_slice(&(count as u64).to_le_bytes());
    word(p, at + 8, class);
    at + 16
}
