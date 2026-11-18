//! Follow the installed native equipment requests to their local pose stream.
//! Source motion fidelity and live playback are checked separately from this package contract.
use super::*;
use parhelion_import::tiger::animation::{clip, control::Rig, pose};

fn requested_descriptors(states: &Payload, action: u32) -> Result<BTreeSet<usize>> {
    let names = states.array(8, 8, Some(0x8080342E))?;
    let row = names
        .iter()
        .find(|&&r| states.u32(r).ok() == Some(action))
        .context("Required native equipment action is absent")?;
    let nodes = states.array(24, 16, Some(0x8080342F))?;
    let node = *nodes
        .get(states.u32(row + 4)? as usize)
        .context("Equipment action node")?;
    let data = states.pointer(node + 8)?;
    let choices = match states.u64(node)? {
        1 => vec![data],
        2 => states.array(data, 16, Some(0x80803437))?,
        kind => anyhow::bail!("Unsupported equipment state kind {kind}"),
    };
    ensure!(!choices.is_empty(), "Equipment request has no choices");
    let mut result = BTreeSet::new();
    for choice in choices {
        let mut previous = 0.;
        for row in states.array(choice, 8, Some(0x80803439))? {
            let cutoff = states.f32(row + 4)?;
            ensure!(
                cutoff.is_finite() && cutoff > previous && cutoff <= 1.,
                "Equipment request has invalid cumulative weights"
            );
            result.insert(states.u32(row)? as usize);
            previous = cutoff;
        }
        ensure!(previous == 1., "Equipment choice is incomplete");
    }
    Ok(result)
}

pub(super) fn readback(
    reader: &mut Reader,
    original: &mut Reader,
    actual: &runtime::RuntimeEntitySource,
    donor: &runtime::RuntimeEntitySource,
    expected: &Value,
    output: &Path,
) -> Result<Value> {
    let Some(required) = expected.as_array() else {
        return Ok(
            json!({"status":"excluded","reason":"No equipment request contract configured"}),
        );
    };
    ensure!(!required.is_empty(), "Required equipment corpus is empty");
    let components = owners(reader, actual.entity_tag)?;
    let stock = owners(original, donor.entity_tag)?;
    let (_, lookup, base) = one(&components, 0x8080344B)?;
    let (_, old_lookup, old_base) = one(&stock, 0x8080344B)?;
    let states = reader.tag(lookup.u32(base + 0x9C)?, Some(0x80803465))?;
    let old_states = original.tag(old_lookup.u32(old_base + 0x9C)?, Some(0x80803465))?;
    ensure!(
        states.0 == old_states.0,
        "Native equipment request selection changed"
    );
    let bank_tag = lookup.u32(base + 0x90)?;
    let bank = reader.tag(bank_tag, Some(0x808036F6))?;
    let old_bank = original.tag(old_lookup.u32(old_base + 0x90)?, Some(0x808036F6))?;
    let (_, consumer, _) = one(&components, 0x808036CF)?;
    ensure!(
        consumer.u32(parhelion_import::tiger::animation::bank_field(&consumer)?)? == bank_tag,
        "Equipment output is not connected to its lookup's bank"
    );
    let (_, skeleton, _) = one(&components, 0x80808546)?;
    let (_, controls, _) = one(&components, 0x80808F8F)?;
    let rig = Rig::read(&skeleton, &controls)?;
    let defaults = pose::rows(&controls, controls.pointer(24)? + 0xB0)?
        .into_iter()
        .map(|p| p.0)
        .collect::<Vec<_>>();
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let old_descriptors = old_bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let old_clips = old_bank.array(8, 4, Some(0x80808F48))?;
    let folder = output.join("equipment");
    fs::create_dir(&folder)?;
    for (name, payload) in [
        ("skeleton", &skeleton),
        ("controls", &controls),
        ("bank", &bank),
        ("states", &states),
    ] {
        fs::write(folder.join(format!("{name}.bin")), &payload.0)?;
    }
    let mut evidence = Vec::new();
    for action in required {
        let name = number(&action["name"])?;
        for index in requested_descriptors(&states, name)? {
            let row = *descriptors.get(index).context("Equipment descriptor")?;
            let old = *old_descriptors
                .get(index)
                .context("Stock equipment descriptor")?;
            let tag = bank.u32(clips[usize::from(bank.u16(row + 24)?)])?;
            let old_tag = old_bank.u32(old_clips[usize::from(old_bank.u16(old + 24)?)])?;
            if action["preserve_native"] == true {
                ensure!(
                    tag == old_tag
                        && bank.u32(row + 20)? == old_bank.u32(old + 20)?
                        && reader.tag(tag, Some(0x80808F49))?.0
                            == original.tag(old_tag, Some(0x80808F49))?.0,
                    "An unsupported equipment action changed its native motion"
                );
                evidence.push(json!({"action":name,"descriptor":index,"tag":tag,
                    "preserved_native":true}));
                continue;
            }
            ensure!(
                tag != old_tag,
                "Native request still plays the stock equipment clip"
            );
            let payload = reader.tag(tag, Some(0x80808F49))?;
            let frames = payload.u16(0x13C)?;
            ensure!(
                frames > 0 && payload.u8(0x181)? == 0,
                "Equipment action lacks absolute poses"
            );
            ensure!(
                bank.f32(row + 20)? == f32::from(frames - 1) / 30.,
                "Native duration scaling reads another clip length"
            );
            // The independent source census supplies the named mechanism's stored length.
            if let Some(expected) = action["frames"].as_u64() {
                ensure!(
                    u64::from(frames) == expected,
                    "Equipment action carries {frames} frames, the source mechanism has {expected}"
                );
            }
            let first = clip::sample(&payload, &defaults, 0)?;
            let mut changed = BTreeSet::new();
            for frame in 1..usize::from(frames) {
                let sampled = clip::sample(&payload, &defaults, frame)?;
                for (slot, (a, b)) in first.iter().zip(&sampled).enumerate() {
                    if a.iter().zip(b).any(|(a, b)| (a - b).abs() > 1e-4) {
                        changed.insert(rig.names[rig.controls[slot].bone]);
                    }
                }
            }
            if action["moving"] == true {
                ensure!(
                    !changed.is_empty(),
                    "Required equipment reload has no moving joint"
                );
            }
            ensure!(
                !changed.contains(&rig.names[0]),
                "Equipment motion moves the attachment root"
            );
            let file = format!("{tag:08X}.bin");
            fs::write(folder.join(&file), &payload.0)?;
            ensure!(
                original.tag(old_tag, Some(0x80808F49))?.0
                    == reader.tag(old_tag, Some(0x80808F49))?.0,
                "Stock equipment clip changed"
            );
            evidence.push(
                json!({"action":name,"descriptor":index,"tag":tag,"stock":old_tag,
            "file":file,"sha256":digest(&payload.0),"frames":frames,"moving_bones":changed}),
            );
        }
    }
    let result = json!({"status":"pass","actions":evidence,"bank":bank_tag,
        "limits":"Native request references, duration scaling inputs and local joint motion. Live synchronization, skinning and cleanup require gameplay."});
    fs::write(
        folder.join("equipment.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
