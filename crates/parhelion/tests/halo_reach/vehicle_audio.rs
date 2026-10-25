//! Follow vehicle playback to source PCM from an independently retained working recipe.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn pcm(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "Vehicle media is not WAVE"
    );
    let payload = Payload(bytes.to_vec());
    ensure!(
        payload.u32(4)? as usize + 8 == bytes.len(),
        "Vehicle media RIFF extent differs"
    );
    let mut at = 12;
    let mut format = None;
    let mut data = None;
    while at + 8 <= bytes.len() {
        let size = payload.u32(at + 4)? as usize;
        let body = bytes
            .get(at + 8..at + 8 + size)
            .context("Truncated vehicle audio chunk")?;
        match &bytes[at..at + 4] {
            b"fmt " => format = Some(digest(body)),
            b"data" => data = Some(digest(body)),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    Ok(json!({"format":format.context("Vehicle audio format")?,"pcm":data.context("Vehicle PCM")?}))
}

fn sound(
    staged: &mut Reader,
    stock: &Reader,
    tag: u32,
    looping: bool,
    expected: &Value,
) -> Result<Value> {
    let cue = staged.tag(tag, Some(0x80809802))?;
    let bank_tag = cue.u32(20)?;
    ensure!(
        stock.reference(bank_tag).is_err(),
        "Vehicle still plays a stock audio bank"
    );
    let bank = staged.tag(bank_tag, None)?;
    let layers = super::audio::verify_events(&bank, cue.u32(8)?, cue.u32(12)?, looping)?;
    let mut media = Vec::new();
    let mut actual = Vec::new();
    for at in cue.array(0x18, 4, Some(0x80800014))? {
        let tag = cue.u32(at)?;
        ensure!(
            stock.reference(tag).is_err(),
            "Vehicle still plays stock media"
        );
        let bytes = staged.tag(tag, None)?;
        let decoded = pcm(&bytes.0)?;
        actual.push(serde_json::to_string(&decoded)?);
        media.push(json!({"tag":format!("{tag:08X}"),"sha256":digest(&bytes.0),"decoded":decoded}));
    }
    let mut wanted = expected
        .as_array()
        .context("Vehicle audio witness recordings")?
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(!wanted.is_empty(), "Empty required vehicle audio witness");
    actual.sort();
    wanted.sort();
    ensure!(
        actual == wanted,
        "Vehicle audio differs from the retained source recordings"
    );
    Ok(
        json!({"cue":format!("{tag:08X}"),"bank":format!("{bank_tag:08X}"),"bank_sha256":digest(&bank.0),"started_layers":layers,"looping":looping,"media":media}),
    )
}

fn engine_event(payload: &Payload, definition: usize, names: &[u32]) -> Result<usize> {
    let controls = payload.array(definition + 0x158, 24, Some(0x808093e6))?;
    let events = payload.array(definition + 0x168, 24, Some(0x808093e6))?;
    let mut pending = vec![(0u16, 0u16)];
    let mut visited = BTreeSet::new();
    let mut reached = Vec::new();
    while let Some((kind, index)) = pending.pop() {
        ensure!(
            visited.insert((kind, index)),
            "Vehicle engine control cycle"
        );
        if kind == 1 {
            reached.push(index as usize);
            continue;
        }
        ensure!(kind == 0, "Unknown engine control selector");
        let at = payload.pointer(
            *controls
                .get(index as usize)
                .context("Engine control is absent")?
                + 16,
        )?;
        for child in payload.array(at + 0x38, 4, Some(0x808093fb))? {
            pending.push((payload.u16(child)?, payload.u16(child + 2)?));
        }
    }
    ensure!(
        reached.len() == 1,
        "Unexpected donor engine event remains reachable"
    );
    let at = payload.pointer(*events.get(reached[0]).context("Engine event is absent")? + 16)?;
    ensure!(
        payload.u32(at - 4)? == 0x80806b38,
        "Engine event is not audio"
    );
    let parameters = payload.array(at + 0x48, 4, Some(0x80800007))?;
    ensure!(
        parameters.len() == 1
            && names.get(payload.u32(parameters[0])? as usize) == Some(&0x5daeb921),
        "Engine lost its RPM parameter"
    );
    Ok(at)
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &Reader,
    identity: u32,
    runtime: &Value,
) -> Result<()> {
    let Some(path) = env::var_os("PARHELION_REACH_VEHICLE_AUDIO_WITNESS") else {
        return Ok(());
    };
    let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
    let expected = witness
        .get(format!("{identity:08X}"))
        .context("Required vehicle audio witness is absent")?;
    let runtime = staged.tag(
        u32::from_str_radix(
            runtime["runtime_entity"]
                .as_str()
                .context("Vehicle runtime entity")?,
            16,
        )?,
        Some(0x80809c0f),
    )?;
    let mut engines = Vec::new();
    let mut projectiles = BTreeSet::new();
    for row in runtime.array(16, 12, Some(0x80809c04))? {
        let owner = runtime.u32(row)?;
        let payload = staged.tag(owner, Some(0x80809c36))?;
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        if payload.u32(definition - 4)? == 0x808084e9 {
            let inputs = payload.array(definition + 0x188, 40, Some(0x80809789))?;
            let names = inputs
                .iter()
                .map(|at| payload.u32(at + 32))
                .collect::<Result<Vec<_>>>()?;
            if names.contains(&0x5daeb921)
                && names.contains(&0x157aa4e5)
                && expected.get("engine").is_some()
            {
                let at = engine_event(&payload, definition, &names)?;
                engines.push(sound(
                    staged,
                    stock,
                    payload.u32(at + 0x40)?,
                    true,
                    &expected["engine"],
                )?);
            }
        }
        if expected.get("firing").is_some() {
            for at in (0..payload.0.len().saturating_sub(0xb94)).step_by(8) {
                if payload.u32(at)? == owner && payload.u32(at + 4)? == 0x80803889 {
                    ensure!(
                        payload.u32(at + 0xb80)? == payload.u32(at + 0xb90)?,
                        "Vehicle projectile routes differ"
                    );
                    projectiles.insert(payload.u32(at + 0xb80)?);
                }
            }
        }
    }
    if expected.get("engine").is_some() {
        ensure!(
            engines.len() == 1,
            "Required source engine is absent or ambiguous"
        );
    }
    let mut firing = Vec::new();
    if expected.get("firing").is_some() {
        ensure!(
            projectiles.len() == 1,
            "Required vehicle firing projectile is absent or ambiguous"
        );
        let projectile = staged.tag(*projectiles.first().unwrap(), Some(0x80809c0f))?;
        for row in projectile.array(16, 12, Some(0x80809c04))? {
            let flow = staged.tag(projectile.u32(row)?, Some(0x80809c36))?;
            let d = usize::try_from(flow.u64(flow.pointer(16)? + 8)?)?;
            if flow.u32(d - 4)? != 0x808084e9 {
                continue;
            }
            for row in flow.array(d + 0x168, 24, Some(0x808093e6))? {
                let event = flow.pointer(row + 16)?;
                if flow.u32(event - 4)? != 0x80806b38 {
                    continue;
                }
                let cue = flow.u32(event + 0x40)?;
                if stock.reference(cue).is_err() {
                    firing.push(sound(staged, stock, cue, false, &expected["firing"])?);
                }
            }
        }
        ensure!(
            firing.len() == 1,
            "Required source firing cue is absent or ambiguous"
        );
    }
    fs::write(
        staged
            .output
            .join(format!("vehicle-audio-{identity:08X}.json")),
        serde_json::to_vec_pretty(
            &json!({"engine":engines,"firing":firing,"witness":expected,"limits":"Stored routes, lifecycle actions and source PCM verified. Native activation and cleanup still require gameplay acceptance."}),
        )?,
    )?;
    Ok(())
}
