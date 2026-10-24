//! Native event witnesses bind source timing to matching, unambiguous native resources.
use super::*;
mod native;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    class: u32,
    data: Vec<u8>,
    path: Option<String>,
}

struct Record {
    at: usize,
    frame: i16,
    key: Key,
    resource: Option<u32>,
}

fn resource_class(path: &str, source: bool) -> u32 {
    match (source, path.ends_with(".wwise_event")) {
        (true, true) => 0x8080080A,
        (true, false) => 0x80800734,
        (false, true) => 0x80809802,
        (false, false) => 0x80809C0F,
    }
}

fn read(p: &Payload, source: bool) -> Result<Vec<Record>> {
    let mut result = Vec::new();
    let mut offsets = BTreeSet::new();
    let header = if source { SOURCE_HEADER } else { NATIVE_HEADER };
    let frames = p.u16(if source { 0x128 } else { 0x13C })?;
    for row in p.array(
        if source { 0x140 } else { 0x160 },
        8,
        Some(if source { 0x808004E1 } else { 0x8080902B }),
    )? {
        let at = p.pointer(row)?;
        ensure!(
            at >= header + 4 && offsets.insert(at),
            "Event record boundary or alias differs"
        );
        let frame = p.i16(at)?;
        ensure!(
            frame >= 0 && (frame as u16) < frames,
            "Event frame exceeds clip"
        );
        let class = match (source, p.u32(at - 4)?) {
            (true, 0x80800436) | (false, 0x8080903A) => 0x8080903A,
            (true, 0x80800607) | (false, 0x80809031) => 0x80809031,
            (true, 0x8080060E) | (false, 0x80809033) => 0x80809033,
            (_, other) => anyhow::bail!("Event {other:08X} has no validated native layout"),
        };
        let (size, kind) = match class {
            0x8080903A => (6, 2),
            0x80809031 => (12, 5),
            _ => (32, 3),
        };
        ensure!(p.u16(at + 2)? == kind, "Event dispatch kind is unsupported");
        let bytes = p.0.get(at..at + size).context("Event record extent")?;
        let (data, path, resource) = if class == 0x80809033 {
            ensure!(
                p.u32(at + 12)? == 0 && p.u32(at + 28)? == 0,
                "Resource event has extra payload"
            );
            let start = p.pointer(at + 16)?;
            ensure!(start >= header, "Event path points inside header");
            let text = p.0.get(start..).context("Event path extent")?;
            let end = text
                .iter()
                .position(|&b| b == 0)
                .context("Event path terminator")?;
            let path = std::str::from_utf8(&text[..end])?.to_owned();
            ensure!(
                (path.starts_with("content\\") || path.starts_with("content/"))
                    && (path.ends_with(".wwise_event") || path.ends_with(".sequence.tft")),
                "Event resource type is unsupported"
            );
            let tag = p.u32(at + 24)?;
            ensure!(
                ![0, u32::MAX, 0x811C9DC5].contains(&tag),
                "Event has no resource"
            );
            (bytes[2..16].to_vec(), Some(path), Some(tag))
        } else {
            (bytes[2..].to_vec(), None, None)
        };
        result.push(Record {
            at,
            frame,
            key: Key { class, data, path },
            resource,
        });
    }
    Ok(result)
}

pub(in crate::d1_roi::native::animation) struct Library {
    records: BTreeMap<Key, BTreeSet<Option<u32>>>,
}

impl Library {
    pub(in crate::d1_roi::native::animation) fn validate_source(
        &self,
        source: &Source,
        clip: &Payload,
    ) -> Result<()> {
        for record in read(clip, true)? {
            if let Some(tag) = record.resource {
                ensure!(
                    source.reference(tag)?
                        == resource_class(
                            record.key.path.as_deref().context("Source event path")?,
                            true
                        ),
                    "Source event resource class differs"
                );
                source
                    .read(tag)
                    .context("Source event resource is unavailable")?;
            }
        }
        Ok(())
    }

    pub(in crate::d1_roi::native::animation) fn read(
        reader: &mut Reader,
        bank: &Payload,
    ) -> Result<Self> {
        let mut records: BTreeMap<Key, BTreeSet<Option<u32>>> = BTreeMap::new();
        let tags = bank
            .array(8, 4, Some(0x80808F48))?
            .into_iter()
            .map(|r| bank.u32(r))
            .collect::<Result<BTreeSet<_>>>()?;
        for tag in tags {
            let clip = reader.tag(tag, Some(0x80808F49))?;
            // Unsupported witness clips do not weaken the requirements on a source clip.
            let Ok(events) = read(&clip, false) else {
                continue;
            };
            for event in events {
                if let Some(resource) = event.resource {
                    reader
                        .tag(
                            resource,
                            Some(resource_class(
                                event.key.path.as_deref().context("Native event path")?,
                                false,
                            )),
                        )
                        .context("Native event resource is unavailable")?;
                }
                records.entry(event.key).or_default().insert(event.resource);
            }
        }
        // A selected weapon bank need not contain every shared event resource.
        // Its local bindings remain stronger than the complete native corpus.
        for (key, candidates) in native::catalog(reader)?.iter() {
            records
                .entry(key.clone())
                .or_insert_with(|| candidates.clone());
        }
        Ok(Self { records })
    }

    pub(in crate::d1_roi::native::animation) fn convert(
        &self,
        source: &Payload,
        native: &mut [u8],
        frames: u16,
    ) -> Result<Value> {
        ensure!(
            frames == source.u16(0x128)?,
            "Event clip frame count changed"
        );
        let records = read(source, true)?;
        let mut events = Vec::new();
        move_array(source, native, 0x140, 0x160, (0x808004E1, 0x8080902B))?;
        for record in records {
            if record.key.class == 0x8080903A {
                ensure!(
                    record.key.data[1] == 0 && record.key.data[3] == 0,
                    "Source selector event has unsupported flags"
                );
                put(
                    native,
                    record.at + SHIFT - 4,
                    &record.key.class.to_le_bytes(),
                );
                events.push(
                    json!({"frame":record.frame,"selector":record.key.data[2] as i8,
                    "source_offset":record.at,"native_offset":record.at + SHIFT}),
                );
                continue;
            }
            let candidates = self
                .records
                .get(&record.key)
                .context("Source event has no matching native dispatch witness")?;
            ensure!(
                candidates.len() == 1,
                "Event path has ambiguous native resources"
            );
            let target = *candidates.first().unwrap();
            ensure!(
                target.is_some() == record.resource.is_some(),
                "Event resource shape changed"
            );
            put(
                native,
                record.at + SHIFT - 4,
                &record.key.class.to_le_bytes(),
            );
            if let Some(tag) = target {
                put(native, record.at + SHIFT + 24, &tag.to_le_bytes());
            }
            events.push(json!({"frame":record.frame,"source_offset":record.at,"native_offset":record.at + SHIFT,
                "native_class":record.key.class,"path":record.key.path,"source_resource":record.resource,"native_resource":target}));
        }
        Ok(
            json!({"events":events,"cues":[],"patches":[],"binding":"matching_native_event_dispatch_and_resource_path","source_timing_preserved":true,"gameplay_verified":false}),
        )
    }
}
