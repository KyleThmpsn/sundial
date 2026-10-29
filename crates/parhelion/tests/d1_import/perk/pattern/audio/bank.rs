//! Decode the reached Wwise event branches without invoking the converter's bank parser.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source_tag: u32,
    tag: u32,
) -> CheckResult<(Value, BTreeSet<u32>)> {
    assert_eq!(source.reference(source_tag)?, 0x8080080A);
    let original = source.read(source_tag)?;
    assert!(stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none());
    let cue = emitted.tag(tag, Some(0x80809802))?;
    assert_eq!(cue.u64(0)?, cue.0.len() as u64);
    let bank_tag = cue.u32(0x14)?;
    let profile_tag = cue.u32(0x10)?;
    let auxiliary_tag = cue.u32(0x28)?;
    let mut required = BTreeSet::from([tag, bank_tag, profile_tag, auxiliary_tag]);
    for tag in &required {
        assert!(stock.manager.get_entry(tiger_pkg::TagHash(*tag)).is_none());
    }
    let original_profile = source.read(original.u32(0x30)?)?;
    let profile = emitted.tag(profile_tag, Some(0x80808E68))?;
    assert_eq!(
        profile.0, original_profile.0,
        "Private acoustic profile lost a source field"
    );
    let auxiliary = emitted.tag(auxiliary_tag, Some(0x80808E69))?;
    assert_eq!(auxiliary.0.len(), 40);
    assert_eq!(auxiliary.u64(0)?, 40);
    assert_eq!(auxiliary.bytes::<32>(8)?, [0; 32]);
    let source_bank_tag = original.u32(0x34)?;
    let source_bank = source.read(source_bank_tag)?;
    let bank = emitted.tag(bank_tag, None)?;
    let storage = emitted
        .manager
        .get_entry(tiger_pkg::TagHash(bank_tag))
        .ok_or("Private bank storage")?;
    assert_eq!((storage.file_type, storage.file_subtype), (26, 5));
    let from = hierarchy(&source_bank, 89)?;
    let to = hierarchy(&bank, 113)?;
    assert_eq!(from.len(), to.len(), "An original audio object was dropped");
    let types = |objects: &BTreeMap<u32, (u8, Payload)>| {
        let mut result = BTreeMap::new();
        for (kind, _) in objects.values() {
            *result.entry(*kind).or_insert(0usize) += 1;
        }
        result
    };
    assert_eq!(types(&from), types(&to));
    let mut identities = BTreeMap::new();
    let mut branches = Vec::new();
    let mut media = BTreeMap::new();
    for offset in [8, 12] {
        let event = original.u32(offset)?;
        let native_event = cue.u32(offset)?;
        if matches!(event, 0 | u32::MAX | 0x811C9DC5) {
            assert_eq!(native_event, event);
            continue;
        }
        bind(&mut identities, event, native_event)?;
        let event = object(&from, event, 4)?;
        let native_event = object(&to, native_event, 4)?;
        let count = usize::try_from(event.u32(4)?)?;
        assert_eq!(count, 2, "Required event lost a Play branch");
        assert_eq!(event.0.len(), 8 + count * 4);
        assert_eq!(native_event.u32(4)?, count as u32);
        assert_eq!(native_event.0.len(), event.0.len());
        for ordinal in 0..count {
            let id = event.u32(8 + ordinal * 4)?;
            let native_id = native_event.u32(8 + ordinal * 4)?;
            bind(&mut identities, id, native_id)?;
            let action = object(&from, id, 3)?;
            let native_action = object(&to, native_id, 3)?;
            assert_eq!(action.u16(4)?, 0x0403);
            assert_eq!(native_action.u16(4)?, action.u16(4)?);
            // The independent v89/v113 dumps give the same complete Play body:
            // delay property 0E, empty randomized properties and a bank reference.
            assert_eq!(action.0.len(), 23);
            assert_eq!(native_action.0.len(), 23);
            assert_eq!(action.bytes::<9>(10)?, native_action.bytes::<9>(10)?);
            assert_eq!(action.bytes::<3>(10)?, [0, 1, 0x0E]);
            assert_eq!(action.u32(19)?, source_bank.u32(12)?);
            assert_eq!(native_action.u32(19)?, bank.u32(12)?);
            let child = action.u32(6)?;
            let native_child = native_action.u32(6)?;
            bind(&mut identities, child, native_child)?;
            let (kind, container) = from.get(&child).ok_or("Source Play target")?;
            let native_container = object(&to, native_child, *kind)?;
            let (sound_id, native_sound_id) = match kind {
                5 => (random(container)?, random(native_container)?),
                6 => switch(container, native_container)?,
                _ => return Err("Required sound event changed its container family".into()),
            };
            bind(&mut identities, sound_id, native_sound_id)?;
            let sound = object(&from, sound_id, 2)?;
            let native_sound = object(&to, native_sound_id, 2)?;
            let plugin = sound.u32(4)?;
            let detail = if plugin == 0x00650002 {
                assert_eq!(*kind, 6);
                assert_eq!(sound.u32(8)?, 0);
                assert_eq!(sound.bytes::<13>(16)?, [0; 13]);
                assert_eq!(sound.u32(29)?, 0);
                assert_eq!(native_sound.u32(4)?, plugin);
                assert_eq!(native_sound.u8(8)?, 0);
                assert_eq!(native_sound.bytes::<9>(13)?, [0; 9]);
                let preset_id = sound.u32(12)?;
                let native_preset_id = native_sound.u32(9)?;
                bind(&mut identities, preset_id, native_preset_id)?;
                let preset = object(&from, preset_id, 19)?;
                let native_preset = object(&to, native_preset_id, 19)?;
                assert_eq!((preset.0.len(), native_preset.0.len()), (27, 29));
                assert_eq!(preset.u32(4)?, plugin);
                assert_eq!(preset.u32(8)?, 12);
                assert_eq!(&preset.0[4..], &native_preset.0[4..27]);
                assert_eq!(native_preset.bytes::<2>(27)?, [0; 2]);
                json!({"generated":true,"source_preset":preset_id,"preset":native_preset_id,
                    "parameters":hex::encode(preset.bytes::<12>(12)?),
                    "preset_sha256":hex::encode(Sha256::digest(&native_preset.0))})
            } else {
                assert_eq!(*kind, 5);
                assert_eq!(plugin, 0x00040001);
                assert_eq!(sound.u32(8)?, 1);
                assert_eq!(native_sound.u32(4)?, 0x00010001);
                assert_eq!(native_sound.u8(8)?, 2);
                let original_id = sound.u32(12)?;
                assert_eq!(sound.u32(16)?, original_id);
                let medium = native_sound.u32(9)?;
                assert!(media.insert(original_id, medium).is_none());
                let payload = emitted.tag(medium, None)?;
                assert_eq!(native_sound.u32(13)? as usize, payload.0.len());
                json!({"generated":false,"source_medium_id":original_id,"medium":medium})
            };
            branches.push(
                json!({"ordinal":ordinal,"source_action":id,"action":native_id,
                "delay_ms":action.u32(13)?,"source_container":child,"container":native_child,
                "source_sound":sound_id,"sound":native_sound_id,"detail":detail}),
            );
        }
    }
    assert_eq!(branches.len(), 2);
    assert!(
        branches
            .iter()
            .any(|branch| branch["detail"]["generated"] == true)
    );
    assert!(
        branches
            .iter()
            .any(|branch| branch["detail"]["generated"] == false)
    );
    let original_media = original.array(0x38, 4, Some(0x80800014))?;
    let cue_media = cue.array(0x18, 4, Some(0x80800014))?;
    assert_eq!(original_media.len(), cue_media.len());
    let mut recordings = Vec::new();
    let mut reached = BTreeSet::new();
    for (s, n) in original_media.into_iter().zip(cue_media) {
        let source_medium = original.u32(s)?;
        let medium = cue.u32(n)?;
        if source_medium == u32::MAX {
            assert_eq!(medium, u32::MAX);
            continue;
        }
        assert!(
            stock
                .manager
                .get_entry(tiger_pkg::TagHash(medium))
                .is_none()
        );
        let id = source.reference(source_medium)?;
        assert_eq!(
            media.get(&id),
            Some(&medium),
            "Cue and streamed branch select different media"
        );
        assert!(reached.insert(id));
        let from = source.read(source_medium)?;
        let to = emitted.tag(medium, None)?;
        let expected = wave(&from, true)?;
        let actual = wave(&to, false)?;
        assert_eq!(actual["channels"], expected["channels"]);
        assert_eq!(actual["rate"], expected["rate"]);
        assert_eq!(actual["frames"], expected["frames"]);
        assert_eq!(actual["speaker_mask"], expected["speaker_mask"]);
        let storage = emitted
            .manager
            .get_entry(tiger_pkg::TagHash(medium))
            .ok_or("Private medium storage")?;
        assert_eq!((storage.file_type, storage.file_subtype), (26, 6));
        assert_eq!(storage.reference, medium);
        let physical = super::storage::check(emitted, medium, &to)?;
        required.insert(medium);
        recordings.push(
            json!({"source":source_medium,"source_id":id,"medium":medium,
            "source_sha256":hex::encode(Sha256::digest(&from.0)),
            "sha256":hex::encode(Sha256::digest(&to.0)),"layout":actual,"physical":physical}),
        );
    }
    assert!(!recordings.is_empty());
    assert_eq!(reached, media.keys().copied().collect());
    Ok((
        json!({"source_cue":source_tag,"cue":tag,"source_bank":source_bank_tag,"bank":bank_tag,
        "profile":profile_tag,"auxiliary":auxiliary_tag,"branches":branches,"recordings":recordings,
        "object_count":from.len(),"reachable_object_identities":identities,
        "cue_sha256":hex::encode(Sha256::digest(&cue.0)),
        "bank_sha256":hex::encode(Sha256::digest(&bank.0)),
        "profile_sha256":hex::encode(Sha256::digest(&profile.0))}),
        required,
    ))
}

fn bind(identities: &mut BTreeMap<u32, u32>, from: u32, to: u32) -> CheckResult<()> {
    assert_ne!(from, to, "Audio object retained a shared Wwise identity");
    assert!(!matches!(to, 0 | u32::MAX | 0x811C9DC5));
    if let Some(before) = identities.insert(from, to) {
        assert_eq!(
            before, to,
            "One source object acquired inconsistent references"
        );
    }
    assert_eq!(
        identities.values().copied().collect::<BTreeSet<_>>().len(),
        identities.len()
    );
    Ok(())
}

fn object(objects: &BTreeMap<u32, (u8, Payload)>, id: u32, kind: u8) -> CheckResult<&Payload> {
    let (actual, bytes) = objects
        .get(&id)
        .ok_or("Referenced audio object is absent")?;
    assert_eq!(*actual, kind);
    Ok(bytes)
}

pub(super) fn hierarchy(bank: &Payload, version: u32) -> CheckResult<BTreeMap<u32, (u8, Payload)>> {
    let mut at = 0usize;
    let mut header = false;
    let mut objects = None;
    while at < bank.0.len() {
        let size = usize::try_from(bank.u32(at + 4)?)?;
        let end = at.checked_add(8 + size).ok_or("Bank chunk overflow")?;
        assert!(end <= bank.0.len());
        match bank.0.get(at..at + 4) {
            Some(b"BKHD") => {
                assert!(!header);
                assert_eq!(bank.u32(at + 8)?, version);
                header = true;
            }
            Some(b"HIRC") => {
                let count = usize::try_from(bank.u32(at + 8)?)?;
                assert!(count > 0);
                let mut rows = BTreeMap::new();
                let mut row = at + 12;
                for _ in 0..count {
                    let length = usize::try_from(bank.u32(row + 1)?)?;
                    assert!(length >= 4 && row + 5 + length <= end);
                    let data = Payload(bank.0[row + 5..row + 5 + length].to_vec());
                    assert!(rows.insert(data.u32(0)?, (bank.u8(row)?, data)).is_none());
                    row += 5 + length;
                }
                assert_eq!(row, end);
                assert!(objects.replace(rows).is_none());
            }
            _ => {}
        }
        at = end;
    }
    assert!(header);
    objects.ok_or_else(|| "Required bank has no hierarchy".into())
}

fn random(container: &Payload) -> CheckResult<u32> {
    // The examined v89/v113 playlist suffix has one declared child and one weighted
    // entry. Reading both catches a detached sound that still happens to exist in HIRC.
    let at = container.0.len().checked_sub(18).ok_or("Random playlist")?;
    assert_eq!(container.u32(at)?, 1);
    assert_eq!(container.u16(at + 8)?, 1);
    assert_eq!(container.u32(at + 4)?, container.u32(at + 10)?);
    assert_eq!(container.u32(at + 14)?, 50000);
    Ok(container.u32(at + 4)?)
}

fn switch(source: &Payload, native: &Payload) -> CheckResult<(u32, u32)> {
    // Independent wwiser v20260808 dumps establish these complete one-child tails.
    // Group/state identities stay global. Only child object identities become private.
    let s = source.0.len().checked_sub(67).ok_or("Source switch tail")?;
    let n = native.0.len().checked_sub(60).ok_or("Native switch tail")?;
    assert_eq!(source.u32(s)?, 0);
    assert_eq!(native.u8(n)?, 0);
    assert_eq!(source.bytes::<9>(s + 4)?, native.bytes::<9>(n + 1)?);
    let (s, n) = (s + 13, n + 10);
    assert_eq!(source.u32(s)?, 1);
    assert_eq!(native.u32(n)?, 1);
    let child = source.u32(s + 4)?;
    let native_child = native.u32(n + 4)?;
    assert_eq!(source.u32(s + 8)?, 2);
    assert_eq!(native.u32(n + 8)?, 2);
    for offset in [12, 24] {
        assert_eq!(source.u32(s + offset)?, native.u32(n + offset)?);
        assert_eq!(source.u32(s + offset + 4)?, native.u32(n + offset + 4)?);
    }
    assert_eq!(source.u32(s + 16)?, 1);
    assert_eq!(source.u32(s + 20)?, child);
    assert_eq!(native.u32(n + 20)?, native_child);
    assert_eq!(source.u32(s + 28)?, 0);
    assert_eq!(source.u32(s + 32)?, 1);
    assert_eq!(native.u32(n + 32)?, 1);
    assert_eq!(source.u32(s + 36)?, child);
    assert_eq!(native.u32(n + 36)?, native_child);
    assert_eq!(source.bytes::<2>(s + 40)?, [0; 2]);
    assert_eq!(native.u8(n + 40)?, 0);
    assert_eq!(source.u32(s + 42)?, u32::from(native.u8(n + 41)?));
    assert_eq!(source.u32(s + 42)?, 1);
    assert_eq!(source.bytes::<8>(s + 46)?, native.bytes::<8>(n + 42)?);
    Ok((child, native_child))
}

fn wave(bytes: &Payload, source: bool) -> CheckResult<Value> {
    assert_eq!(bytes.0.get(..4), Some(&b"RIFF"[..]));
    assert_eq!(bytes.0.get(8..12), Some(&b"WAVE"[..]));
    assert_eq!(bytes.u32(4)? as usize + 8, bytes.0.len());
    let mut at = 12;
    let mut format = None;
    let mut samples = None;
    while at < bytes.0.len() {
        let size = usize::try_from(bytes.u32(at + 4)?)?;
        let end = at.checked_add(8 + size).ok_or("WAVE chunk overflow")?;
        let chunk = bytes.0.get(at + 8..end).ok_or("Truncated WAVE chunk")?;
        match bytes.0.get(at..at + 4) {
            Some(b"fmt ") => assert!(format.replace(Payload(chunk.to_vec())).is_none()),
            Some(b"data") => assert!(samples.replace(chunk).is_none()),
            _ => {}
        }
        at = end + (size & 1);
    }
    assert_eq!(at, bytes.0.len());
    let format = format.ok_or("Missing audio format")?;
    let samples = samples.ok_or("Missing audio samples")?;
    assert!(!samples.is_empty());
    let channels = format.u16(2)?;
    let rate = format.u32(4)?;
    let (frames, mask) = if source {
        assert_eq!(format.u16(0)?, 0xFFFF);
        assert_eq!(format.0.len(), 66);
        assert_eq!(format.u16(16)?, 48);
        (usize::try_from(format.u32(24)?)?, format.u32(20)?)
    } else {
        assert_eq!(format.u16(0)?, 0xFFFE);
        assert_eq!(format.u16(14)?, 16);
        assert_eq!(format.u16(12)?, channels * 2);
        assert_eq!(format.u16(18)?, 16);
        let block = usize::from(format.u16(12)?);
        assert!(block > 0 && samples.len() % block == 0);
        (samples.len() / block, format.u32(20)?)
    };
    assert!(channels > 0 && rate > 0 && frames > 0);
    Ok(
        json!({"channels":channels,"rate":rate,"frames":frames,"speaker_mask":mask,
        "sample_sha256":hex::encode(Sha256::digest(samples))}),
    )
}
