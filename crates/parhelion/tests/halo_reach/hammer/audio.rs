//! Follow the actual timed cue through its playable children into source PCM.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn pcm(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "Impact recording is not WAVE"
    );
    let p = Payload(bytes.to_vec());
    ensure!(
        p.u32(4)? as usize + 8 == bytes.len(),
        "Impact RIFF extent differs"
    );
    let mut at = 12;
    let mut format = None;
    let mut data = None;
    while at < bytes.len() {
        let size = p.u32(at + 4)? as usize;
        let body = bytes
            .get(at + 8..at + 8 + size)
            .context("Impact WAVE chunk")?;
        match bytes.get(at..at + 4) {
            Some(b"fmt ") => {
                ensure!(format.is_none(), "Repeated impact audio format");
                let f = Payload(body.to_vec());
                ensure!(matches!(f.u16(0)?, 1 | 0xfffe), "Impact audio is not PCM");
                format = Some((f.u16(2)?, f.u32(4)?, f.u16(12)?, f.u16(14)?));
            }
            Some(b"data") => {
                ensure!(data.is_none(), "Repeated impact PCM");
                data = Some((body.len(), hex::encode(Sha256::digest(body))));
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let (channels, rate, frame, bits) = format.context("Impact audio format absent")?;
    let (bytes, digest) = data.context("Impact PCM absent")?;
    ensure!(
        channels > 0 && frame > 0 && bytes > 0 && bytes % usize::from(frame) == 0,
        "Invalid impact PCM extent"
    );
    Ok(
        json!({"channels":channels,"rate":rate,"bits":bits,"frames":bytes / usize::from(frame),"pcm_sha256":digest}),
    )
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &Reader,
    tag: u32,
    directory: &Path,
    witness: &Value,
) -> Result<Value> {
    ensure!(
        stock.reference(tag).is_err(),
        "Hammer retains a stock impact cue"
    );
    let cue = staged.tag(tag, Some(0x80809802))?;
    let bank_tag = cue.u32(20)?;
    ensure!(
        stock.reference(bank_tag).is_err(),
        "Hammer retains a stock impact bank"
    );
    let bank = staged.tag(bank_tag, None)?;
    ensure!(
        super::super::audio::verify_events(&bank, cue.u32(8)?, cue.u32(12)?, false)? == 1,
        "Impact event is not one playable layer"
    );
    let mut objects = BTreeMap::new();
    let mut cursor = 0;
    while cursor < bank.0.len() {
        let end = cursor + 8 + bank.u32(cursor + 4)? as usize;
        ensure!(end <= bank.0.len(), "Truncated impact bank");
        if bank.0.get(cursor..cursor + 4) == Some(b"HIRC") {
            let mut at = cursor + 12;
            for _ in 0..bank.u32(cursor + 8)? {
                let length = bank.u32(at + 1)? as usize;
                let body = bank
                    .0
                    .get(at + 5..at + 5 + length)
                    .context("Impact sound object")?;
                let p = Payload(body.to_vec());
                ensure!(
                    objects.insert(p.u32(0)?, (bank.u8(at)?, p)).is_none(),
                    "Duplicate impact sound object"
                );
                at += 5 + length;
            }
            ensure!(at == end, "Impact hierarchy extent differs");
        }
        cursor = end;
    }
    let event = &objects.get(&cue.u32(8)?).context("Impact event")?.1;
    let action = &objects.get(&event.u32(8)?).context("Impact action")?.1;
    let (kind, random) = objects
        .get(&action.u32(6)?)
        .context("Impact random container")?;
    ensure!(*kind == 5, "Impact event does not select variations");
    // Native v113 common-node properties precede positioning and voice limits.
    ensure!(
        random.0.get(4..7) == Some(&[0, 0, 0]),
        "Impact container has unaccounted effects"
    );
    let properties = random.u8(16)? as usize;
    let randomized = 17 + properties * 5;
    ensure!(
        random.u8(randomized)? == 0,
        "Impact has unaccounted randomized properties"
    );
    let limits = randomized + 3;
    ensure!(
        random.u8(limits)? & 12 == 12,
        "Impact voice limit is not shared across transient effects"
    );
    ensure!(
        u64::from(random.u16(limits + 2)?)
            == witness["voice_limit"]
                .as_u64()
                .context("Source voice limit")?,
        "Impact voice limit differs from source"
    );
    let children = limits + 36;
    let count = random.u32(children)? as usize;
    let mut playable = BTreeSet::new();
    for i in 0..count {
        let (kind, sound) = objects
            .get(&random.u32(children + 4 + i * 4)?)
            .context("Impact variation")?;
        ensure!(
            *kind == 2 && sound.u32(4)? == 0x10001 && sound.u8(8)? == 2,
            "Impact variation is not streamed PCM"
        );
        ensure!(
            playable.insert(sound.u32(9)?),
            "Impact variations repeat media"
        );
    }
    let mut actual = Vec::new();
    let declared = cue
        .array(0x18, 4, Some(0x80800014))?
        .into_iter()
        .map(|at| cue.u32(at))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !playable.is_empty() && playable == declared,
        "Impact event does not reach its declared recordings"
    );
    for medium in playable {
        ensure!(
            stock.reference(medium).is_err(),
            "Hammer impact plays stock PCM"
        );
        actual.push(serde_json::to_string(&pcm(&staged.tag(medium, None)?.0)?)?);
    }
    let recordings = directory
        .parent()
        .context("Impact source directory")?
        .join("source/audio");
    let mut expected = Vec::new();
    for row in witness["media"]
        .as_array()
        .context("Impact source witness recordings")?
    {
        let id = row["id"].as_u64().context("Impact source recording ID")?;
        let decoded = pcm(&fs::read(recordings.join(format!("{id:08X}.wav")))?)?;
        ensure!(
            decoded["frames"] == row["frames"]
                && decoded["rate"] == row["rate"]
                && decoded["channels"] == row["channels"],
            "Impact source layout differs from independent FSB witness"
        );
        expected.push(serde_json::to_string(&decoded)?);
    }
    actual.sort();
    expected.sort();
    ensure!(
        !expected.is_empty() && actual == expected,
        "Hammer impact PCM differs from source recordings"
    );
    Ok(
        json!({"cue":format!("{tag:08X}"),"bank":format!("{bank_tag:08X}"),"recordings":actual,"witness":witness}),
    )
}
