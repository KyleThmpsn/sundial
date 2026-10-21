use super::*;
use parhelion_import::tiger::payload::Payload;

pub fn audio(
    reader: &mut Reader,
    source: &Source,
    item: u32,
    graph: &Value,
    directory: &Path,
) -> Result<Value> {
    let expected = graph["audio"]["matched_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if env::var_os("PARHELION_D1_REQUIRE_NAMED_AUDIO").is_some() {
        assert!(
            !expected.is_empty(),
            "Required source named audio was not linked"
        );
    }
    if expected.is_empty() {
        return Ok(json!([]));
    }
    let rig = parhelion_import::tiger::rig::inspect_with_audio(reader, item, false)?;
    let groups = rig["audio"]["named_groups"]
        .as_array()
        .ok_or("Emitted named audio")?;
    let mut result = Vec::new();
    for event in expected {
        let matches = groups
            .iter()
            .flat_map(|g| g["events"].as_array().into_iter().flatten())
            .filter(|e| e["hash"] == event["hash"])
            .collect::<Vec<_>>();
        assert_eq!(
            matches.len(),
            1,
            "Named event is absent or ambiguous in the actual item runtime"
        );
        let sounds = matches[0]["sounds"].as_array().ok_or("Emitted sounds")?;
        let sources = event["sounds"].as_array().ok_or("Source sounds")?;
        assert_eq!(sounds.len(), sources.len(), "Named event lost a source cue");
        for (sound, original) in sounds.iter().zip(sources) {
            let tag = u32::from_str_radix(sound["tag"].as_str().ok_or("Emitted cue")?, 16)?;
            let cue = reader.tag(tag, Some(0x80809802))?;
            let source_tag =
                u32::from_str_radix(original["tag"].as_str().ok_or("Source cue")?, 16)?;
            let input = source.read(source_tag)?;
            let media = input.array(0x38, 4, Some(0x80800014))?;
            assert_eq!(cue.u64(0x18)?, media.len() as u64);
            assert_eq!(cue.u64(0x40)?, media.len() as u64);
            let (streams, count) = audio_bank(reader, &cue)?;
            for (i, at) in media.iter().enumerate() {
                let source_medium = format!("{:08X}", input.u32(*at)?);
                let row = graph["audio"]["transcoded_media"]
                    .as_array()
                    .ok_or("Converted media")?
                    .iter()
                    .find(|r| r["source_tag"] == source_medium)
                    .ok_or("Source medium was omitted")?;
                let private = cue.u32(0x50 + i * 4)?;
                assert!(
                    streams.contains(&private),
                    "Cue medium is not referenced by its bank"
                );
                let entry = reader
                    .manager
                    .get_entry(tiger_pkg::TagHash(private))
                    .ok_or("Stored medium")?;
                assert_eq!((entry.file_type, entry.file_subtype), (26, 6));
                assert_eq!(
                    reader.tag(private, None)?.0,
                    fs::read(directory.join(row["file"].as_str().ok_or("PCM file")?))?
                );
            }
            result.push(json!({"event":event["hash"],"cue":tag,"bank":cue.u32(0x14)?,"event_actions":count,"streams":streams}));
        }
    }
    Ok(json!(result))
}

fn audio_bank(reader: &mut Reader, cue: &Payload) -> Result<(BTreeSet<u32>, u32)> {
    let bank = reader.tag(cue.u32(0x14)?, None)?;
    assert_eq!(&bank.0[..4], b"BKHD");
    assert_eq!(bank.u32(8)?, 113);
    let objects = audio_objects(&bank)?;
    let event_record = objects
        .get(&cue.u32(8)?)
        .ok_or("Cue event is absent from its persisted bank")?;
    assert_eq!(event_record.0, 4);
    let payload = Payload(event_record.1.clone());
    let count = payload.u32(4)?;
    assert!(count > 0);
    for i in 0..count as usize {
        assert_eq!(
            objects
                .get(&payload.u32(8 + i * 4)?)
                .ok_or("Persisted event action")?
                .0,
            3
        );
    }
    let mut streams = BTreeSet::new();
    for (kind, bytes) in objects.values() {
        if *kind == 2 {
            let p = Payload(bytes.clone());
            assert_eq!(p.u32(4)?, 0x10001);
            assert_eq!(p.u8(8)?, 2);
            streams.insert(p.u32(9)?);
            assert_eq!(reader.tag(p.u32(9)?, None)?.0.len(), p.u32(13)? as usize);
        }
    }
    Ok((streams, count))
}

type AudioObjects = BTreeMap<u32, (u8, Vec<u8>)>;

fn audio_objects(bank: &Payload) -> Result<AudioObjects> {
    let mut cursor = 0;
    let mut objects = BTreeMap::new();
    while cursor < bank.0.len() {
        let end = cursor + 8 + bank.u32(cursor + 4)? as usize;
        assert!(end <= bank.0.len());
        if bank.0[cursor..cursor + 4] == *b"HIRC" {
            let mut at = cursor + 12;
            for _ in 0..bank.u32(cursor + 8)? {
                let size = bank.u32(at + 1)? as usize;
                assert!(at + 5 + size <= end);
                assert!(
                    objects
                        .insert(
                            bank.u32(at + 5)?,
                            (bank.u8(at)?, bank.0[at + 5..at + 5 + size].to_vec())
                        )
                        .is_none()
                );
                at += 5 + size;
            }
            assert_eq!(at, end);
        }
        cursor = end;
    }
    assert_eq!(cursor, bank.0.len());
    Ok(objects)
}

fn inherited_pose(directory: &Path, name: u32) -> Result<(u32, [u8; 64])> {
    let report: Value = serde_json::from_slice(&fs::read(directory.join("weapon.json"))?)?;
    let root = report["runtime"]["runtime_entity"]
        .as_str()
        .ok_or("Source runtime")?;
    let entity = Payload(fs::read(directory.join(format!("{root}.bin")))?);
    let mut found = Vec::new();
    for at in entity.array(0x30, 88, Some(0x8080054E))? {
        if entity.u64(at + 24)? != 0x8080073D {
            continue;
        }
        let tag = entity.u64(at + 8)?;
        let owner = Payload(fs::read(directory.join(format!("{tag:08X}.bin")))?);
        let input = usize::try_from(entity.u64(at + 16)?)?;
        if owner.u32(input + 0x3C)? != name {
            continue;
        }
        let rows = owner.array(input + 0x48, 64, Some(0x80800608))?;
        assert_eq!(rows.len(), 1, "Source fallback is ambiguous");
        found.push((tag as u32, owner.bytes::<64>(rows[0])?));
    }
    assert_eq!(found.len(), 1, "Source fallback is absent or ambiguous");
    Ok(found[0])
}

#[allow(clippy::cognitive_complexity)]
pub fn optics(
    reader: &mut Reader,
    entity: &Payload,
    source_entity: &Payload,
    source_directory: &Path,
) -> Result<Vec<Value>> {
    let mut expected = Vec::new();
    for at in source_entity.array(0x20, 12, Some(0x80800715))? {
        let tag = source_entity.u32(at)?;
        let p = Payload(fs::read(source_directory.join(format!("{tag:08X}.bin")))?);
        if p.u32(p.pointer(24)? - 4)? == 0x808027FC {
            expected.push((tag, p));
        }
    }
    let mut actual = Vec::new();
    for at in entity.array(16, 12, Some(0x80809C04))? {
        let tag = entity.u32(at)?;
        let p = reader.tag(tag, Some(0x80809C36))?;
        if p.u32(p.pointer(24)? - 4)? == 0x8080393B {
            actual.push((tag, p));
        }
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "Selected art lost or duplicated its optic"
    );
    let mut result = Vec::new();
    for ((source_tag, source), (tag, native)) in expected.into_iter().zip(actual) {
        let s = source.pointer(24)?;
        let n = usize::try_from(native.u64(native.pointer(16)? + 8)?)?;
        assert_eq!(source.bytes::<16>(s + 0x80)?, native.bytes::<16>(n + 0x70)?);
        assert_eq!(source.bytes::<16>(s + 0x90)?, native.bytes::<16>(n + 0x80)?);
        let source_code = source.array(s + 0xB8, 1, Some(0x80800009))?;
        let code = native.array(n + 0xA0, 1, Some(0x80800009))?;
        assert_eq!(source_code.len(), code.len());
        for (a, b) in source_code.into_iter().zip(code) {
            assert_eq!(source.u8(a)?, native.u8(b)?);
        }
        let constants = source.array(s + 0xC8, 16, Some(0x80800090))?;
        let values = native.array(n + 0xB0, 16, Some(0x80800090))?;
        assert_eq!(constants.len(), values.len());
        for (a, b) in constants.into_iter().zip(values) {
            assert_eq!(source.bytes::<16>(a)?, native.bytes::<16>(b)?);
        }
        let mut inputs = Vec::new();
        for (sd, nd) in [(0x1D0, 0x198), (0x270, 0x228)] {
            let old = source.array(s + sd, 64, Some(0x80800608))?;
            let inherited = if old.is_empty() {
                Some(inherited_pose(source_directory, source.u32(s + sd - 12)?)?)
            } else {
                None
            };
            assert_eq!(
                if inherited.is_some() {
                    1
                } else {
                    source.u32(s + sd - 32)?
                },
                native.u32(n + nd - 32)?
            );
            let new = native.array(n + nd, 64, Some(0x80809C00))?;
            let expected_rows = if let Some((_, row)) = inherited {
                vec![row]
            } else {
                old.into_iter()
                    .map(|at| source.bytes::<64>(at))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            assert_eq!(expected_rows.len(), new.len());
            for (row, b) in expected_rows.into_iter().zip(new) {
                assert_eq!(
                    row,
                    native.bytes::<64>(b)?,
                    "Effective source aim transform or bone binding changed"
                );
            }
            let from = source_entity
                .array(0x30, 88, Some(0x8080054E))?
                .into_iter()
                .filter(|&at| {
                    source_entity.u64(at + 8).ok() == Some(u64::from(source_tag))
                        && source_entity.u64(at + 16).ok() == Some((s + sd - 0x48) as u64)
                })
                .collect::<Vec<_>>();
            let to = entity
                .array(0x20, 72, Some(0x80809BC9))?
                .into_iter()
                .filter(|&at| {
                    entity.u32(at + 8).ok() == Some(tag)
                        && entity.u64(at + 16).ok() == Some((n + nd - 0x40) as u64)
                })
                .collect::<Vec<_>>();
            assert_eq!(from.len(), 1);
            assert_eq!(to.len(), 1);
            let (a, b) = (from[0], to[0]);
            if let Some((owner, _)) = inherited {
                assert_eq!(entity.u64(b + 32)?, 0xAE7D0AED);
                assert_eq!(entity.u64(b + 56)?, 0x3FFFF);
                assert_eq!(entity.u64(b + 64)?, 0);
                assert_eq!(
                    entity.bytes::<16>(b + 40)?,
                    [
                        255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0
                    ]
                );
                inputs.push(json!({"inherited_source_owner":owner,"marker":source.u32(s+sd-12)?,"native_connection":b,"provider_mode":entity.u64(b+56)?}));
                continue;
            }
            assert_eq!(source_entity.u64(a + 40)?, entity.u64(b + 32)?);
            assert_eq!(source_entity.u64(a + 72)?, entity.u64(b + 56)?);
            assert_eq!(source_entity.u64(a + 80)?, entity.u64(b + 64)?);
            match source_entity.u64(a + 72)? >> 16 {
                3 => assert_eq!(
                    entity.bytes::<16>(b + 40)?,
                    [
                        255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0
                    ]
                ),
                2 => {
                    let adapter = reader.tag(entity.u32(b + 40)?, Some(0x80809C36))?;
                    assert_eq!(entity.u32(b + 44)?, 0x80809BE3);
                    let offset = usize::try_from(entity.u64(b + 48)?)?;
                    let metadata = reader.tag(adapter.u32(offset + 8)?, None)?;
                    assert_eq!(metadata.u32(8)?, 0x80809BE3);
                    assert_eq!(metadata.u32(12)?, 0x80809BE2);
                    let methods = metadata.array(16, 24, Some(0x80809C56))?;
                    assert_eq!(methods.len(), 4);
                    for (at, index) in methods.into_iter().zip(9..13) {
                        assert_eq!(metadata.u32(at)?, 0x80809782);
                        assert_eq!(metadata.u32(at + 4)?, index);
                        assert_eq!(metadata.bytes::<16>(at + 8)?, [0; 16]);
                    }
                }
                _ => return Err("Unsupported source aim provider in configured corpus".into()),
            }
            inputs.push(json!({"source_connection":a,"native_connection":b,"provider_mode":entity.u64(b+56)?}));
        }
        result.push(json!({"source":source_tag,"native":tag,"inputs":inputs}));
    }
    Ok(result)
}

pub fn collections(reader: &mut Reader, globals: &Payload, item: u32) -> Result<Value> {
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let index = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .position(|at| items.u32(at).ok() == Some(item))
        .ok_or("Authored item ordinal")?;
    let collectibles = reader.tag(root.u32(8 + 19 * 16)?, None)?;
    let rows = collectibles.array(8, 0xB8, None)?;
    let matches = rows
        .iter()
        .enumerate()
        .filter(|(_, at)| collectibles.u16(**at + 0x2C).ok() == Some(index as u16))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    let (ordinal, &row) = matches[0];
    for field in [0x30, 0x40, 0x50, 0x60] {
        assert_eq!(
            collectibles.bytes::<16>(row + field)?,
            [0; 16],
            "Inherited Collections condition remains"
        );
    }
    let condition = collectibles.array(row + 0x70, 8, Some(0x80807D31))?;
    assert_eq!(condition.len(), 1);
    assert_eq!(
        collectibles.bytes::<8>(condition[0])?,
        [11, 0, 0, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        collectibles.u16(row + 0x98)?,
        0,
        "Collections reclaim is disabled"
    );
    let displays = reader.tag(globals.u32(16 + 15 * 16)?, None)?;
    let display = displays.array(8, 0x60, Some(0x80802E5F))?[ordinal];
    assert_eq!(
        displays.bytes::<16>(display + 0x48)?,
        [0; 16],
        "Collections display condition remains"
    );
    Ok(json!({"ordinal":ordinal,"item_index":index,"unconditional":true,"reclaim_enabled":true}))
}
