//! Match named actions and specialize profile-selected motion on private native controllers.
use super::*;
mod conditional;

struct Routes {
    choices: BTreeMap<u32, Vec<(u32, u32)>>,
    unbound: BTreeSet<u32>,
}

fn descriptor_index(bank: &Payload, row: usize) -> Result<Option<usize>> {
    let index = bank.u16(row + 24)?;
    if index != u16::MAX {
        return Ok(Some(usize::from(index)));
    }
    ensure!(
        bank.bytes::<16>(row)? == [0; 16]
            && bank.u32(row + 20)? == 0xFF7F_FFFF
            && bank.bytes::<6>(row + 26)? == [0; 6],
        "Unbound action descriptor differs"
    );
    Ok(None)
}

fn choices(states: &Payload, bank: &Payload, source: bool) -> Result<Routes> {
    let names = states.array(8, 8, Some(if source { 0x80802153 } else { 0x8080342E }))?;
    let nodes = states.array(24, 16, Some(if source { 0x808023C9 } else { 0x8080342F }))?;
    let descriptors = bank.array(0x68, 32, Some(if source { 0x80800577 } else { 0x80809002 }))?;
    let clips = bank.array(8, 4, Some(if source { 0x8080408D } else { 0x80808F48 }))?;
    let mut result = BTreeMap::new();
    let mut unbound = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for name in names {
        let name_hash = states.u32(name)?;
        ensure!(seen.insert(name_hash), "Action names repeat");
        let node = *nodes
            .get(states.u32(name + 4)? as usize)
            .context("Action state index")?;
        // Profile-selected actions are planned separately with private state and descriptor rows.
        if states.u64(node)? != 1 {
            continue;
        }
        let at = states.pointer(node + 8)?;
        let mut choices = Vec::new();
        let mut absent = false;
        let mut previous = 0.;
        for row in states.array(at, 8, Some(if source { 0x80802831 } else { 0x80803439 }))? {
            let weight = states.f32(row + 4)?;
            ensure!(
                weight.is_finite() && weight >= previous && weight <= 1.,
                "Invalid cumulative action weight"
            );
            previous = weight;
            let descriptor = *descriptors
                .get(states.u32(row)? as usize)
                .context("Action descriptor index")?;
            ensure!(
                bank.bytes::<16>(descriptor)? == [0; 16],
                "Action descriptor has inline tracks"
            );
            let Some(index) = descriptor_index(bank, descriptor)? else {
                absent = true;
                continue;
            };
            let clip = *clips.get(index).context("Action clip index")?;
            choices.push((bank.u32(clip)?, weight.to_bits()));
        }
        ensure!(
            (!choices.is_empty() || absent) && previous == 1.,
            "Action choices do not cover a complete interval"
        );
        // A partly unbound weighted action cannot be paired by deleting one choice.
        if absent {
            unbound.insert(name_hash);
        } else {
            result.insert(name_hash, choices);
        }
    }
    Ok(Routes {
        choices: result,
        unbound,
    })
}

pub(in crate::d1_roi::native) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    rig: &Value,
    graph: &Path,
) -> Result<Value> {
    let mapping = controls::Mapping::read(source, runtime, native, rig, true)?;
    let source_owner = controls::owner(runtime, true, SOURCE_LOOKUP, true)?;
    let native_owner = controls::owner(rig, true, 0x8080344B, false)?;
    let source_lookup = source.read(source_owner)?;
    let lookup = native.tag(native_owner, Some(0x80809C36))?;
    let sr = source_lookup.pointer(24)?;
    let nr = lookup.pointer(24)?;
    let source_bank = source.read(source_lookup.u32(sr + 0xA8)?)?;
    let bank_tag = lookup.u32(nr + 0x90)?;
    let bank = native.tag(bank_tag, Some(0x808036F6))?;
    let source_states = source.read(source_lookup.u32(sr + 0xB4)?)?;
    let native_states = native.tag(lookup.u32(nr + 0x9C)?, Some(0x80803465))?;
    let from = choices(&source_states, &source_bank, true)?;
    let to = choices(&native_states, &bank, false)?;
    let library = event::Library::read(native, &bank)?;
    let mut candidates = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut actions = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (name, source_choices) in &from.choices {
        let Some(native_choices) = to.choices.get(name) else {
            continue;
        };
        if !source_choices
            .iter()
            .map(|c| c.1)
            .eq(native_choices.iter().map(|c| c.1))
        {
            continue;
        }
        for (&(source_tag, _), &(native_tag, _)) in source_choices.iter().zip(native_choices) {
            candidates.entry(native_tag).or_default().insert(source_tag);
            actions.entry(native_tag).or_default().insert(*name);
        }
    }
    let folder = graph.join("animation");
    fs::create_dir_all(&folder)?;
    let mut converted = Vec::new();
    let mut rejected = from.unbound.iter().map(|name| json!({
        "action":name,"reason":"Source action contains an unbound clip descriptor. The complete native action route is retained."
    })).collect::<Vec<_>>();
    for (native_tag, sources) in candidates {
        let attempt = (|| -> Result<Value> {
            ensure!(
                sources.len() == 1,
                "Named action routes disagree about their source clip"
            );
            let source_tag = *sources.first().context("Action source clip")?;
            ensure!(
                source.reference(source_tag)? == SOURCE_CLIP,
                "Action source is not a clip"
            );
            let original = source.read(source_tag)?;
            let template = native.tag(native_tag, Some(0x80808F49))?;
            library.validate_source(source, &original)?;
            let (payload, mut report) = convert_events(
                &original,
                Some((&template, &mapping.bone_map)),
                Some(&library),
            )?;
            let (mut payload, slots) = mapping.apply(&original, &payload)?;
            // The native lookup can also address this name outside the state descriptors.
            let name = template.u32(0x120)?;
            put(&mut payload.0, 0x120, &name.to_le_bytes());
            let file = format!("animation/first-person-clip-{native_tag:08X}.bin");
            fs::write(graph.join(&file), &payload.0)?;
            report["control_mapping"] = slots;
            Ok(
                json!({"native":native_tag,"source":source_tag,"name":name,"source_name":original.u32(0x110)?,
                "actions":actions.get(&native_tag),"file":file,"frames":payload.u16(0x13C)?,"report":report}),
            )
        })();
        match attempt {
            Ok(clip) => converted.push(clip),
            Err(error) => rejected
                .push(json!({"native":native_tag,"sources":sources,"reason":format!("{error:#}")})),
        }
    }
    let mut output = patch_bank(&bank, &converted)?;
    let selected = conditional::prepare(
        source, runtime, native, rig, graph, &mapping, &output, &library,
    );
    let selected = match selected {
        Ok(selected) => selected,
        Err(error) => {
            rejected.push(json!({"scope":"conditional_actions","reason":format!("{error:#}")}));
            conditional::Prepared::default()
        }
    };
    if converted.is_empty() && selected.clips.is_empty() {
        return Ok(
            json!({"first_person_status":"native","reason":"No compatible source first-person action converted", "unconverted":rejected,
                "conditional_actions":selected.report,"gameplay_verified":false}),
        );
    }
    let components = rig["components"]
        .as_array()
        .context("Native runtime components")?;
    let entity = components
        .iter()
        .find(|c| c["owner"] == format!("{native_owner:08X}"))
        .context("First-person lookup entity")?["entity"]
        .as_str()
        .context("First-person entity")?;
    let entity = u32::from_str_radix(entity, 16)?;
    let attachment_owner = controls::owner(rig, false, 0x80804221, false)?;
    let mut files = BTreeMap::new();
    for (key, tag, class) in [
        ("attachment_owner", attachment_owner, 0x80809C36),
        ("entity", entity, 0x80809C0F),
        ("lookup_owner", native_owner, 0x80809C36),
        ("bank", bank_tag, 0x808036F6),
    ] {
        let file = format!("animation/first-person-{key}.bin");
        fs::write(graph.join(&file), &native.tag(tag, Some(class))?.0)?;
        files.insert(key.to_owned(), file);
    }
    if let Some(bank) = &selected.bank {
        output = bank.clone();
    }
    files.extend(selected.files);
    let file = "animation/first-person-bank-converted.bin";
    fs::write(graph.join(file), &output.0)?;
    files.insert("converted_bank".to_owned(), file.to_owned());
    let mut consumers = Vec::new();
    for component in components {
        if component["entity"] != format!("{entity:08X}")
            || !matches!(component["class"].as_str(), Some("80803640" | "808036CF"))
        {
            continue;
        }
        let owner = u32::from_str_radix(
            component["owner"].as_str().context("Animation consumer")?,
            16,
        )?;
        let payload = native.tag(owner, Some(0x80809C36))?;
        ensure!(
            payload.u32(crate::tiger::animation::bank_field(&payload)?)? == bank_tag,
            "First-person consumer names another bank"
        );
        let key = format!("consumer_{}", consumers.len());
        let file = format!("animation/first-person-consumer-{owner:08X}.bin");
        fs::write(graph.join(&file), &payload.0)?;
        files.insert(key.clone(), file);
        consumers.push(json!({"owner":owner,"file_key":key}));
    }
    ensure!(
        consumers.len() == 2,
        "First-person bank consumers are incomplete"
    );
    Ok(
        json!({"first_person_status":"linked","runtime_entity":u32::from_str_radix(rig["runtime_entity"].as_str().context("Runtime entity")?,16)?,
        "first_person":{"attachment_owner":attachment_owner,"entity":entity,"lookup_owner":native_owner,"bank":bank_tag,
            "files":files,"clips":converted,"extra_clips":selected.clips,"unconverted":rejected,"bank_consumers":consumers,
            "conditional_actions":selected.report,"pose_layers":selected.layers,
            "controller":"native action routes with qualified source first-person motion","state_conversion":selected.states},
        "gameplay_verified":false}),
    )
}

/// Only replaced clips change duration. Unbound descriptors retain their native sentinel.
fn patch_bank(bank: &Payload, converted: &[Value]) -> Result<Payload> {
    let mut output = bank.clone();
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    for row in bank.array(0x68, 32, Some(0x80809002))? {
        let Some(index) = descriptor_index(bank, row)? else {
            continue;
        };
        let clip = bank.u32(*clips.get(index).context("Native descriptor clip index")?)?;
        if let Some(converted) = converted
            .iter()
            .find(|c| c["native"].as_u64() == Some(u64::from(clip)))
        {
            ensure!(
                bank.bytes::<16>(row)? == [0; 16],
                "Converted action has inline descriptor tracks"
            );
            let frames = converted["frames"]
                .as_u64()
                .context("Converted frame count")?;
            ensure!(frames > 0, "Converted clip has no frames");
            put(
                &mut output.0,
                row + 20,
                &((frames - 1) as f32 / 30.).to_le_bytes(),
            );
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
