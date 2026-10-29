//! Compare reachable standing and crouching holds with an independent source-position witness.
use super::*;
use parhelion_import::tiger::animation::{control::Rig, motion_table, state::parameter};

fn tag(value: &Value, field: &str) -> Result<u32> {
    Ok(u32::try_from(
        value[field]
            .as_u64()
            .with_context(|| format!("Character {field}"))?,
    )?)
}

fn witness() -> Result<Option<Value>> {
    let Some(path) = env::var_os("PARHELION_REACH_CHARACTER_GRIP_WITNESS") else {
        return Ok(None);
    };
    let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        witness["schema"] == 1 && witness["items"].as_object().is_some_and(|v| !v.is_empty()),
        "Empty or unsupported character hold witness"
    );
    Ok(Some(witness))
}

pub(super) fn finish(output: &Path) -> Result<()> {
    let Some(witness) = witness()? else {
        return Ok(());
    };
    for (identity, expected) in witness["items"].as_object().context("Source hold items")? {
        let identity = u32::from_str_radix(identity, 16).context("Source hold identity")?;
        let receipt: Value = serde_json::from_slice(&fs::read(
            output.join(format!("character-hold-{identity:08X}.json")),
        )?)?;
        ensure!(
            receipt["item"] == format!("{identity:08X}")
                && &receipt["witness"] == expected
                && receipt["samples"].as_array().is_some_and(|v| !v.is_empty()),
            "Required source hold was not verified in this run"
        );
    }
    Ok(())
}

pub(super) fn frame(clip: &Payload, defaults: &[[f32; 8]], sample: usize) -> Result<Vec<[f32; 8]>> {
    let count = usize::from(clip.u16(0x13C)?);
    ensure!(sample < count, "Character hold sample is outside its clip");
    ensure!(
        usize::from(clip.u16(0x140)?) == defaults.len(),
        "Character hold slot count differs from its rig"
    );
    let mut poses = defaults.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    for (field, maps, frames, sample) in [(16, 0xA8, 1, 0), (24, 0xD8, count, sample)] {
        let slots = (0..3)
            .map(|kind| clip.array(maps + kind * 16, 2, Some(0x8080000A)))
            .collect::<Result<Vec<_>>>()?;
        if clip.u64(field)? == 0 {
            ensure!(
                slots.iter().all(Vec::is_empty),
                "Character pose maps have no codec"
            );
            continue;
        }
        let codec = clip.pointer(field)?;
        let encoding = clip.u16(codec)?;
        ensure!(
            codec >= 4
                && matches!(
                    (encoding, clip.u32(codec - 4)?),
                    (0, 0x80808F73) | (2, 0x80808F71)
                )
                && clip.u32(codec + 16)? as usize == frames,
            "Unsupported emitted character pose codec"
        );
        let components: usize = slots.iter().zip([1, 4, 3]).map(|(s, w)| s.len() * w).sum();
        let packed = if encoding == 2 {
            let values = clip.array(codec + 0x18, 2, Some(0x8080000A))?;
            let ranges = clip.array(codec + 0x28, 4, Some(0x8080000F))?;
            let biases = clip.array(codec + 0x38, 4, Some(0x8080000F))?;
            ensure!(
                values.len() == components * frames
                    && ranges.len() == components
                    && biases.len() == components,
                "Incomplete packed character pose channels"
            );
            Some((values, ranges, biases))
        } else {
            None
        };
        let mut component = 0;
        for (kind, (start, width, stride)) in
            [(7, 1, 4), (0, 4, 16), (4, 3, 16)].into_iter().enumerate()
        {
            ensure!(
                usize::from(clip.u16(codec + 2 + kind * 2)?) == slots[kind].len(),
                "Character pose map and codec counts differ"
            );
            let values = if encoding == 0 {
                let values = clip.array(
                    codec + 24 + kind * 16,
                    stride,
                    Some([0x8080000F, 0x80800096, 0x80800091][kind]),
                )?;
                ensure!(
                    values.len() == slots[kind].len() * frames,
                    "Incomplete float character pose channels"
                );
                values
            } else {
                Vec::new()
            };
            for (track, &slot) in slots[kind].iter().enumerate() {
                let slot = usize::from(clip.u16(slot)?);
                ensure!(seen.insert((kind, slot)), "Repeated character pose channel");
                let pose = poses.get_mut(slot).context("Character pose slot")?;
                for axis in 0..width {
                    let value = if let Some((values, ranges, biases)) = &packed {
                        let lane = component + axis;
                        let range = clip.f32(ranges[lane])?;
                        let bias = clip.f32(biases[lane])?;
                        ensure!(
                            range.is_finite() && range >= 0. && bias.is_finite(),
                            "Invalid packed character pose interval"
                        );
                        let at = component * frames + sample * width + axis;
                        bias + range * (f32::from(clip.u16(values[at])?) / 65535.)
                    } else {
                        clip.f32(values[track * frames + sample] + axis * 4)?
                    };
                    ensure!(value.is_finite(), "Nonfinite character pose sample");
                    pose[start + axis] = value;
                }
                component += width;
            }
        }
    }
    Ok(poses)
}

pub(super) fn verify(staged: &mut Reader, stock: &mut Reader, graph: &Value) -> Result<()> {
    let Some(witness) = witness()? else {
        return Ok(());
    };
    let identity = tag(graph, "item_hash")?;
    let Some(expected) = witness["items"].get(format!("{identity:08X}")) else {
        return Ok(());
    };
    let character = &graph["animation"]["character"];
    ensure!(
        character["weapon_grip"] == true,
        "Required source hold has no weapon grip translation"
    );
    let configured_stances = graph["import_settings"]["character"]["stance"]
        .as_object()
        .context("Saved weapon character stances")?;
    ensure!(
        !configured_stances.is_empty(),
        "Required source hold has no translated stance"
    );
    ensure!(
        expected["source_reference"] == character["source_reference"],
        "Source hold reference differs"
    );
    let stances = expected["stances"]
        .as_object()
        .context("Source hold stances")?;
    ensure!(
        stances.len() == configured_stances.len(),
        "Source hold witness does not cover every requested stance"
    );
    let skeleton = staged.tag(tag(character, "skeleton")?, Some(0x80809C36))?;
    let controls = staged.tag(tag(character, "controls")?, Some(0x80809C36))?;
    let rig = Rig::read_character(&skeleton, &controls)?;
    let bone = |name: &str| {
        rig.names
            .iter()
            .position(|&key| key == sundial::package_authoring::fnv1_name_hash(name))
            .with_context(|| format!("Character hold bone {name}"))
    };
    let pelvis = bone("b_pelvis")?;
    let grip = bone("b_r_grip")?;
    let lookup = staged.tag(
        tag(&graph["import_settings"]["character"]["native"], "lookup")?,
        Some(0x80809C36),
    )?;
    let definition = lookup.pointer(24)?;
    let bank_tag = lookup.u32(definition + 0x90)?;
    ensure!(
        bank_tag == tag(character, "bank")?,
        "Character bank differs"
    );
    let bank = staged.tag(bank_tag, Some(0x808036F6))?;
    let old_bank = stock.tag(bank_tag, Some(0x808036F6))?;
    let old_count = old_bank.array(0x68, 32, Some(0x80809002))?.len();
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let dictionary = staged.tag(lookup.u32(definition + 0x94)?, Some(0x80808EE1))?;
    let selector = staged.tag(lookup.u32(definition + 0xB8)?, Some(0x80803478))?;
    let table = staged.tag(lookup.u32(definition + 0xB4)?, Some(0x8080345F))?;
    let private = tag(&character["binding"], "private")?;
    let mut observed = Vec::new();
    for (stance, expected) in stances {
        ensure!(
            configured_stances.get(stance) == Some(&expected["source"]),
            "Source hold stance does not match its requested action"
        );
        let endpoints = expected["pelvis_to_grip_metres"]
            .as_array()
            .context("Source hold endpoints")?;
        ensure!(endpoints.len() == 2, "Source hold requires both endpoints");
        let choices = parameter::table_choices_for(
            &selector,
            &dictionary,
            &BTreeMap::from([
                (1, sundial::package_authoring::fnv1_name_hash(stance)),
                (3, private),
            ]),
        )?;
        ensure!(!choices.is_empty(), "No private character stance choice");
        for choice in choices {
            for index in motion_table::descriptors(&table, motion_table::Kind::Stance, choice, 0)? {
                ensure!(
                    index >= old_count,
                    "Imported hold still selects a stock clip"
                );
                let descriptor = *descriptors
                    .get(index)
                    .context("Character hold descriptor")?;
                ensure!(
                    bank.u16(descriptor + 26)? == 1,
                    "Character stance is not absolute"
                );
                let slot = usize::from(bank.u16(descriptor + 24)?);
                let clip_tag = bank.u32(*clips.get(slot).context("Character hold clip")?)?;
                let clip = staged.tag(clip_tag, Some(0x80808F49))?;
                let frames = usize::from(clip.u16(0x13C)?);
                ensure!(frames >= 2, "Character hold has no endpoint interval");
                for (sample, expected) in [0, frames - 1].into_iter().zip(endpoints) {
                    let expected = expected.as_array().context("Source hold displacement")?;
                    ensure!(expected.len() == 3, "Invalid source hold displacement");
                    let objects = rig.objects(&frame(&clip, &rig.defaults, sample)?)?;
                    let actual: [f32; 3] = std::array::from_fn(|axis| {
                        objects[grip].0[4 + axis] - objects[pelvis].0[4 + axis]
                    });
                    for (axis, value) in expected.iter().enumerate() {
                        let value = value.as_f64().context("Source hold coordinate")?;
                        ensure!(
                            value.is_finite() && (f64::from(actual[axis]) - value).abs() < 0.0001,
                            "Character hold {stance} loses source body placement on axis {axis}"
                        );
                    }
                    observed.push(json!({"stance":stance,"choice":choice,"descriptor":index,
                        "clip":format!("{clip_tag:08X}"),"frame":sample,"actual":actual,"expected":expected}));
                }
            }
        }
    }
    ensure!(!observed.is_empty(), "Empty required character hold corpus");
    fs::write(
        staged
            .output
            .join(format!("character-hold-{identity:08X}.json")),
        serde_json::to_vec_pretty(
            &json!({"item":format!("{identity:08X}"),"source_build":witness["source_build"],
            "native_build":witness["native_build"],"witness":expected,"samples":observed,
            "limits":"Staged character FK at neutral aim. Final IK, attachment placement and gameplay remain unverified."}),
        )?,
    )?;
    Ok(())
}
