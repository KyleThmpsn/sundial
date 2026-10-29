//! Follow private action selectors to the source pose at the retained native contact.
use super::*;
use parhelion_import::tiger::animation::{control::Rig, state::parameter};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn number(value: &Value, field: &str) -> Result<u32> {
    Ok(u32::try_from(
        value[field]
            .as_u64()
            .with_context(|| format!("Character action {field}"))?,
    )?)
}

fn witness() -> Result<Option<Value>> {
    let Some(path) = env::var_os("PARHELION_REACH_CHARACTER_ACTION_WITNESS") else {
        return Ok(None);
    };
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        value["schema"] == 1
            && value["items"]
                .as_object()
                .is_some_and(|items| !items.is_empty()),
        "Empty or unsupported character action witness"
    );
    Ok(Some(value))
}

pub(super) fn finish(output: &Path) -> Result<()> {
    let Some(witness) = witness()? else {
        return Ok(());
    };
    for (identity, expected) in witness["items"]
        .as_object()
        .context("Action witness items")?
    {
        let identity = u32::from_str_radix(identity, 16)?;
        let receipt: Value = serde_json::from_slice(&fs::read(
            output.join(format!("character-actions-{identity:08X}.json")),
        )?)?;
        ensure!(
            receipt["item"] == format!("{identity:08X}")
                && &receipt["witness"] == expected
                && receipt["actions"]
                    .as_array()
                    .is_some_and(|actions| !actions.is_empty()),
            "Required character action was not verified in this run"
        );
    }
    Ok(())
}

fn events(clip: &Payload) -> Result<Value> {
    let mut result = Vec::new();
    for row in clip.array(0x160, 8, Some(0x8080902B))? {
        let event = clip.pointer(row)?;
        let class = clip.u32(event.checked_sub(4).context("Action event class")?)?;
        let mut value = json!({"class":class,"frame":clip.u16(event)?,
            "type":clip.u16(event + 2)?,"value":clip.u32(event + 4)?});
        match class {
            0x80809031 => value["extra"] = json!(clip.u32(event + 8)?),
            0x80809032 | 0x8080903A => (),
            0x80809033 => {
                let path = clip.pointer(event + 16)?;
                let tail = clip.0.get(path..).context("Action resource path")?;
                let end = tail
                    .iter()
                    .position(|&byte| byte == 0)
                    .context("Action path terminator")?;
                value["path"] = json!(std::str::from_utf8(&tail[..end])?);
                value["resource"] = json!(clip.u32(event + 24)?);
                value["extra"] = json!([
                    clip.u32(event + 8)?,
                    clip.u32(event + 12)?,
                    clip.u32(event + 28)?
                ]);
            }
            _ => anyhow::bail!("Unqualified character event {class:08X}"),
        }
        result.push(value);
    }
    Ok(json!(result))
}

fn slots(clip: &Payload) -> Result<BTreeSet<u16>> {
    let mut result = BTreeSet::new();
    for field in [0xA8, 0xB8, 0xC8, 0xD8, 0xE8, 0xF8] {
        for row in clip.array(field, 2, Some(0x8080000A))? {
            result.insert(clip.u16(row)?);
        }
    }
    Ok(result)
}

pub(super) fn verify(staged: &mut Reader, stock: &mut Reader, graph: &Value) -> Result<()> {
    let Some(witness) = witness()? else {
        return Ok(());
    };
    let identity = number(graph, "item_hash")?;
    let Some(expected) = witness["items"].get(format!("{identity:08X}")) else {
        return Ok(());
    };
    let character = &graph["animation"]["character"];
    ensure!(
        character["weapon_grip"] == true,
        "Required action has no weapon grip translation"
    );
    ensure!(
        character["source_reference"] == expected["source_reference"],
        "Source action reference stance differs"
    );
    let expected_actions = expected["actions"]
        .as_array()
        .context("Required source actions")?;
    ensure!(
        !expected_actions.is_empty(),
        "Empty required source action corpus"
    );
    let skeleton = staged.tag(number(character, "skeleton")?, Some(0x80809C36))?;
    let controls = staged.tag(number(character, "controls")?, Some(0x80809C36))?;
    let rig = Rig::read_character(&skeleton, &controls)?;
    let grip = rig
        .names
        .iter()
        .position(|&name| name == sundial::package_authoring::fnv1_name_hash("b_r_grip"))
        .context("Character carrying grip")?;
    let lookup = staged.tag(
        number(&graph["import_settings"]["character"]["native"], "lookup")?,
        Some(0x80809C36),
    )?;
    let definition = lookup.pointer(24)?;
    let bank_tag = lookup.u32(definition + 0x90)?;
    let bank = staged.tag(bank_tag, Some(0x808036F6))?;
    let old_bank = stock.tag(bank_tag, Some(0x808036F6))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let old_descriptors = old_bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let old_clips = old_bank.array(8, 4, Some(0x80808F48))?;
    let dictionary_tag = lookup.u32(definition + 0x94)?;
    let states_tag = lookup.u32(definition + 0x9C)?;
    let dictionary = staged.tag(dictionary_tag, Some(0x80808EE1))?;
    let states = staged.tag(states_tag, Some(0x80803465))?;
    let old_dictionary = stock.tag(dictionary_tag, Some(0x80808EE1))?;
    let old_states = stock.tag(states_tag, Some(0x80803465))?;
    let private = number(&character["binding"], "private")?;
    let parent = number(&character["binding"], "parent")?;
    let mut observed = Vec::new();
    let mut seen = BTreeSet::new();
    for action in expected_actions {
        let label = action["state"].as_str().context("Required action state")?;
        let state = if let Some(hex) = label.strip_prefix("0x") {
            u32::from_str_radix(hex, 16)?
        } else {
            sundial::package_authoring::fnv1_name_hash(label)
        };
        ensure!(seen.insert(state), "Repeated required character action");
        ensure!(
            graph["import_settings"]["character"]["actions"][label] == action["source"],
            "Native action {label} does not select its required source role"
        );
        let old = parameter::descriptors(&old_states, &old_dictionary, state, 3, parent)?;
        ensure!(
            parameter::descriptors(&states, &dictionary, state, 3, parent)? == old,
            "Private character action changed the stock state route"
        );
        let template_index = number(action, "template_descriptor")? as usize;
        ensure!(
            old.contains(&template_index),
            "Source witness names an unreachable native template"
        );
        let selected = parameter::descriptors(&states, &dictionary, state, 3, private)?;
        ensure!(
            selected.len() == 1 && old.len() == 1,
            "Action witness needs one qualified state branch"
        );
        let index = *selected.first().context("Private action descriptor")?;
        ensure!(
            index >= old_descriptors.len(),
            "Imported action still selects a stock clip"
        );
        let row = *descriptors
            .get(index)
            .context("Private action descriptor row")?;
        let old_row = *old_descriptors
            .get(template_index)
            .context("Stock action descriptor row")?;
        ensure!(
            bank.f32(row + 20)? == old_bank.f32(old_row + 20)?
                && bank.u16(row + 26)? == old_bank.u16(old_row + 26)?,
            "Imported character action changed native timing or playback"
        );
        let clip_tag = bank.u32(
            *clips
                .get(usize::from(bank.u16(row + 24)?))
                .context("Private clip slot")?,
        )?;
        let old_tag = old_bank.u32(
            *old_clips
                .get(usize::from(old_bank.u16(old_row + 24)?))
                .context("Stock clip slot")?,
        )?;
        let clip = staged.tag(clip_tag, Some(0x80808F49))?;
        let original = stock.tag(old_tag, Some(0x80808F49))?;
        ensure!(
            staged.tag(old_tag, Some(0x80808F49))?.0 == original.0,
            "Stock action clip changed"
        );
        let frames = usize::from(clip.u16(0x13C)?);
        ensure!(
            frames == usize::from(original.u16(0x13C)?),
            "Native action frame count changed"
        );
        let envelope = events(&original)?;
        ensure!(
            events(&clip)? == envelope,
            "Native action events or resources changed"
        );
        if bank.u16(row + 26)? == 2 {
            ensure!(
                slots(&clip)? == slots(&original)?,
                "Character overlay changed its control mask"
            );
        }
        let contacts = envelope
            .as_array()
            .context("Native event array")?
            .iter()
            .filter(|event| {
                event["class"] == 0x8080903Au32 && event["type"] == 2 && event["value"] == 1
            })
            .collect::<Vec<_>>();
        let samples = action["samples"]
            .as_array()
            .context("Source action pose samples")?;
        let contact_required = action["contact_required"]
            .as_bool()
            .context("Source contact requirement")?;
        let overlay = bank.u16(row + 26)? == 2;
        ensure!(
            !samples.is_empty() || overlay,
            "Required source action has no pose samples"
        );
        ensure!(
            samples.is_empty() || !overlay,
            "Overlay poses need a separately qualified base pose"
        );
        let mut sampled = BTreeSet::new();
        let mut results = Vec::new();
        for sample in samples {
            let point = sample["point"]
                .as_str()
                .context("Source action sample point")?;
            ensure!(sampled.insert(point), "Repeated source action sample point");
            let frame = match point {
                "first" => 0,
                "last" => frames.checked_sub(1).context("Empty action clip")?,
                "middle" => frames / 2,
                "contact" => {
                    ensure!(
                        contacts.len() == 1 && bank.u16(row + 26)? == 1,
                        "Unqualified native contact witness"
                    );
                    number(contacts[0], "frame")? as usize
                }
                _ => anyhow::bail!("Unknown action sample point {point}"),
            };
            let pose = rig.objects(&grip::frame(&clip, &rig.defaults, frame)?)?[grip];
            let expected = sample["grip_rotation"]
                .as_array()
                .context("Source carrying-grip rotation")?;
            ensure!(expected.len() == 4, "Invalid source grip quaternion");
            let expected = expected
                .iter()
                .map(|v| v.as_f64().context("Source rotation component"))
                .collect::<Result<Vec<_>>>()?;
            let norm = expected.iter().map(|v| v * v).sum::<f64>();
            let dot = pose.0[..4]
                .iter()
                .zip(&expected)
                .map(|(&a, &b)| f64::from(a) * b)
                .sum::<f64>()
                .abs();
            ensure!(
                norm.is_finite() && (norm - 1.).abs() < 1e-4 && (1. - dot).abs() < 1e-5,
                "Character action {label} loses the source grip pose at {point}"
            );
            results.push(
                json!({"point":point,"frame":frame,"actual":pose.0[..4],"expected":expected}),
            );
        }
        ensure!(
            overlay
                || (sampled.contains("first")
                    && sampled.contains("last")
                    && (sampled.contains("contact") || sampled.contains("middle"))),
            "Action witness must cover both endpoints and an interior pose"
        );
        ensure!(
            !contact_required || sampled.contains("contact"),
            "Required source primary pose was not checked at native contact"
        );
        observed.push(
            json!({"state":label,"source":action["source"],"descriptor":index,
            "template_descriptor":template_index,"clip":format!("{clip_tag:08X}"),
            "template":format!("{old_tag:08X}"),"sha256":hex::encode(Sha256::digest(&clip.0)),
            "events":envelope,"samples":results}),
        );
    }
    fs::write(
        staged
            .output
            .join(format!("character-actions-{identity:08X}.json")),
        serde_json::to_vec_pretty(&json!({"item":format!("{identity:08X}"),
            "source_build":witness["source_build"],"native_build":witness["native_build"],
            "witness":expected,"actions":observed,
            "limits":"Staged action selection and neutral-aim FK. Final constraints, attachment placement and gameplay remain unverified."}))?,
    )?;
    Ok(())
}
