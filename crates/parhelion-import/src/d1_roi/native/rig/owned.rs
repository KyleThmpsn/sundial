//! Preserve a source weapon section when its native carrier has the same control topology.
//! Unchanged arm groups remain native. A different trailing weapon group protects source defaults
//! from retained donor clips, while converted source clips explicitly address the private group.
use super::*;
mod controls;
mod motion;
mod skeleton;
type Bone = (usize, String, i32, [f32; 8]);

fn tag(value: &Value) -> Result<u32> {
    Ok(if let Some(n) = value.as_u64() {
        u32::try_from(n)?
    } else {
        u32::from_str_radix(value.as_str().context("Rig tag")?, 16)?
    })
}

fn owner(rig: &Value, fp: bool, class: u32) -> Result<u32> {
    let root = tag(&rig["runtime_entity"])?;
    let mut rows = Vec::new();
    for row in rig["components"].as_array().context("Rig components")? {
        if (tag(&row["entity"])? != root) == fp && tag(&row["class"])? == class {
            rows.push(tag(&row["owner"])?);
        }
    }
    ensure!(
        rows.len() == 1,
        "Private rig role has no unique {class:08X} component"
    );
    Ok(rows[0])
}

fn skeleton(rig: &Value, fp: bool) -> Result<&Value> {
    let rows = rig["skeletons"]
        .as_array()
        .context("Rig skeletons")?
        .iter()
        .filter(|s| (s["entity"] != rig["runtime_entity"]) == fp)
        .collect::<Vec<_>>();
    ensure!(rows.len() == 1, "Private rig skeleton role is ambiguous");
    Ok(rows[0])
}

fn groups(p: &Payload, field: usize, class: u32) -> Result<Vec<(u32, u32)>> {
    p.array(field, 8, Some(class))?
        .into_iter()
        .map(|a| Ok((p.u32(a)?, p.u32(a + 4)?)))
        .collect()
}

struct Converted {
    first_person: bool,
    source_skeleton: u32,
    native_skeleton: u32,
    source_controls: u32,
    native_controls: u32,
    skeleton: Payload,
    controls: Payload,
    changed_bones: BTreeSet<usize>,
    changed_slots: BTreeSet<usize>,
}

fn convert(
    input: &Source,
    source: &Value,
    target: &Value,
    native: &mut Reader,
) -> Result<Vec<Converted>> {
    let source_weapon = bone_signature(&skeleton(source, false)?["bones"])?;
    let native_weapon = bone_signature(&skeleton(target, false)?["bones"])?;
    ensure!(
        (1..=72).contains(&source_weapon.len()) && source_weapon.len() == native_weapon.len(),
        "Private rig needs equal bounded weapon palettes"
    );
    let source_full = bone_signature(&skeleton(source, true)?["bones"])?;
    let native_full = bone_signature(&skeleton(target, true)?["bones"])?;
    let arms = source_full
        .len()
        .checked_sub(source_weapon.len())
        .context("Shared arm boundary")?;
    ensure!(
        arms > 0
            && source_full.len() == native_full.len()
            && source_full[..arms] == native_full[..arms],
        "Private rig changes the shared arm hierarchy or binds"
    );
    for (weapon, full) in [
        (&source_weapon, &source_full),
        (&native_weapon, &native_full),
    ] {
        ensure!(
            full[arms].2 >= 0 && (full[arms].2 as usize) < arms,
            "Private weapon attachment parent is invalid"
        );
        for (i, bone) in weapon.iter().enumerate() {
            ensure!(
                bone.1 == full[arms + i].1 && (i == 0 || bone.2 + arms as i32 == full[arms + i].2),
                "Equipment and first-person weapon sections disagree"
            );
        }
    }
    let mut result = Vec::new();
    for fp in [false, true] {
        let source_skeleton = owner(source, fp, 0x8080049A)?;
        let native_skeleton = owner(target, fp, 0x80808546)?;
        let source_controls = owner(source, fp, 0x8080099B)?;
        let native_controls = owner(target, fp, 0x80808F8F)?;
        let ss = input.read(source_skeleton)?;
        let original = native.tag(native_skeleton, Some(0x80809C36))?;
        let sb = if fp { &source_full } else { &source_weapon };
        let nb = if fp { &native_full } else { &native_weapon };
        let (ns, changed_bones) =
            skeleton::convert(&ss, &original, sb, nb, if fp { arms } else { 0 })?;
        let sc = input.read(source_controls)?;
        let original = native.tag(native_controls, Some(0x80809C36))?;
        let (nc, changed_slots) = controls::convert(&sc, &original, &changed_bones)?;
        result.push(Converted {
            first_person: fp,
            source_skeleton,
            native_skeleton,
            source_controls,
            native_controls,
            skeleton: ns,
            controls: nc,
            changed_bones,
            changed_slots,
        });
    }
    ensure!(
        crate::tiger::animation::rig::validate_weapon_pair(
            &result[0].skeleton,
            &result[1].skeleton
        )? == arms,
        "Private rig changes the equipment attachment boundary"
    );
    Ok(result)
}

pub(super) fn inspect(
    input: &Source,
    source: &Value,
    target: &Value,
    native: &mut Reader,
) -> Result<Value> {
    let converted = convert(input, source, target, native)?;
    let mut pairs = Vec::new();
    for row in &converted {
        let count = row.skeleton.u64(row.skeleton.pointer(24)? + 0x80)?;
        pairs.push(json!({"source_owner":format!("{:08X}",row.source_skeleton),"native_owner":format!("{:08X}",row.native_skeleton),
            "first_person":row.first_person,"source_bones":count,"native_bones":count,"bone_map":(0..count).collect::<Vec<_>>() }));
    }
    Ok(
        json!({"source_owned":true,"source_runtime":source,"native_runtime":target,"pairs":pairs,
        "bones":pairs[0]["source_bones"],"bone_map":pairs[0]["bone_map"],"remapped":false,
        "hierarchy_and_inverse_bind":"source_owned_with_equal_native_control_topology"}),
    )
}

pub(in crate::d1_roi::native) fn prepare(
    input: &Source,
    source: &Value,
    native: &mut Reader,
    rig: &mut Value,
    graph: &Path,
) -> Result<()> {
    let converted = convert(input, source, rig, native)?;
    fs::create_dir_all(graph.join("animation"))?;
    let mut descriptors = Vec::new();
    let mut controls = serde_json::Map::new();
    let mut boundaries = Vec::new();
    for row in converted {
        for (kind, from, to, payload) in [
            (
                "skeleton",
                row.source_skeleton,
                row.native_skeleton,
                &row.skeleton,
            ),
            (
                "controls",
                row.source_controls,
                row.native_controls,
                &row.controls,
            ),
        ] {
            let file = format!("animation/owned-{kind}-{from:08X}.bin");
            let template = format!("animation/owned-{kind}-{from:08X}-template.bin");
            fs::write(graph.join(&file), &payload.0)?;
            fs::write(graph.join(&template), &native.tag(to, Some(0x80809C36))?.0)?;
            descriptors.push(
                json!({"kind":kind,"source":from,"native":to,"first_person":row.first_person,
                "file":file,"template_file":template}),
            );
            if kind == "controls" {
                controls.insert(row.first_person.to_string(), json!(graph.join(&file)));
            }
        }
        boundaries.push(json!({"first_person":row.first_person,"changed_bones":row.changed_bones,"changed_slots":row.changed_slots}));
        let source_bones = skeleton(source, row.first_person)?["bones"].clone();
        let root = rig["runtime_entity"].clone();
        for s in rig["skeletons"]
            .as_array_mut()
            .context("Runtime skeletons")?
        {
            if (s["entity"] != root) == row.first_person {
                s["bones"] = source_bones.clone();
            }
        }
    }
    rig["owned_rigs"] = json!(descriptors);
    rig["owned_controls"] = Value::Object(controls);
    rig["owned_boundaries"] = json!(boundaries);
    Ok(())
}

pub(in crate::d1_roi::native) fn finish(
    native: &mut Reader,
    rig: &Value,
    graph: &Path,
    equipment: &mut Value,
    animation: &mut Value,
) -> Result<()> {
    ensure!(
        equipment["status"] == "linked" && animation["first_person_status"] == "linked",
        "Private source rig requires converted equipment and first-person actions: equipment {equipment}, first-person {animation}"
    );
    let mut retained = Vec::new();
    for fp in [false, true] {
        let controls = Payload(fs::read(
            rig["owned_controls"][fp.to_string()]
                .as_str()
                .context("Private controls")?,
        )?);
        let group = groups(&controls, controls.pointer(24)? + 0x90, 0x8080907F)?;
        let original_controls = native.tag(owner(rig, fp, 0x80808F8F)?, Some(0x80809C36))?;
        let original_groups = groups(
            &original_controls,
            original_controls.pointer(24)? + 0x90,
            0x8080907F,
        )?;
        let bounds = rig["owned_boundaries"]
            .as_array()
            .context("Private control boundaries")?
            .iter()
            .find(|r| r["first_person"] == fp)
            .context("Private control boundary")?;
        let changed: Vec<usize> = serde_json::from_value(bounds["changed_slots"].clone())?;
        let changed_bones: Vec<u16> = serde_json::from_value(bounds["changed_bones"].clone())?;
        let lookup = native.tag(owner(rig, fp, 0x8080344B)?, Some(0x80809C36))?;
        let bank = native.tag(lookup.u32(lookup.pointer(24)? + 0x90)?, Some(0x808036F6))?;
        let clips = if fp {
            &animation["first_person"]["clips"]
        } else {
            &equipment["clips"]
        };
        let replaced = clips
            .as_array()
            .context("Converted action clips")?
            .iter()
            .map(|c| tag(&c["native"]))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut fallback = Vec::new();
        for row in bank.array(8, 4, Some(0x80808F48))? {
            let id = bank.u32(row)?;
            if replaced.contains(&id) {
                continue;
            }
            let clip = native.tag(id, Some(0x80808F49))?;
            let (adapted, common) = motion::retain(&clip, &original_groups, &group, &changed)?;
            if clip.u64(0x30)? != 0 {
                let binding = clip.pointer(0x30)?;
                ensure!(
                    clip.u32(binding - 4)? == 0x80808F59,
                    "Retained clip constraint class differs"
                );
                for at in clip.array(binding, 64, Some(0x80808F5C))? {
                    for field in [0, 2, 16] {
                        ensure!(
                            !changed_bones.contains(&clip.u16(at + field)?),
                            "Retained constraint targets a changed weapon bone"
                        );
                    }
                }
            }
            if let Some(payload) = adapted {
                let file = format!("animation/retained-prefix-{fp}-{id:08X}.bin");
                fs::write(graph.join(&file), &payload.0)?;
                fallback.push(json!({"native":id,"name":clip.u32(0x120)?,"file":file,
                    "motion_policy":"retained_native_prefix","shared_slots":common,
                    "frames":clip.u16(0x13C)?,"patches":[],"source_motion":false}));
            }
            retained.push(json!({"first_person":fp,"clip":id,"shared_slots":common}));
        }
        let section = if fp {
            &mut animation["first_person"]
        } else {
            &mut *equipment
        };
        section["clips"]
            .as_array_mut()
            .context("Converted and retained clips")?
            .extend(fallback);
    }
    let lookup_tag = owner(rig, true, 0x8080344B)?;
    let lookup = native.tag(lookup_tag, Some(0x80809C36))?;
    let resource = lookup.pointer(24)?;
    let mut states = serde_json::Map::new();
    states.insert("status".into(), json!("native_states"));
    for (key, field, class) in [
        ("parameters", 0x94, 0x80808EE1),
        ("states", 0x9C, 0x80803465),
    ] {
        let id = lookup.u32(resource + field)?;
        let file = format!("animation/owned-{key}.bin");
        fs::write(graph.join(&file), &native.tag(id, Some(class))?.0)?;
        animation["first_person"]["files"][key] = json!(file);
        animation["first_person"]["files"][format!("{key}_template")] = json!(file);
        states.insert(key.into(), json!(id));
    }
    let attachments = native.tag(owner(rig, false, 0x80804221)?, Some(0x80809C36))?;
    let content = tag(&rig["content_key"])?;
    let rows = crate::d2_mot::rig::attachment_rows(
        &attachments,
        attachments.pointer(24)?,
        content,
        false,
    )?;
    ensure!(
        rows.len() == 1,
        "Private rig attachment profile is ambiguous"
    );
    let category = attachments.u32(rows[0] + 0x68)?;
    let profile = attachments.u32(rows[0] + 0x6C)?;
    animation["first_person"]["attachment_profile"] = json!({"status":"native",
        "patch":{"content_key":content,"category":category,"before":profile,"after":profile}});
    animation["first_person"]["state_conversion"] = Value::Object(states);
    animation["first_person"]["rigs"] = rig["owned_rigs"].clone();
    animation["first_person"]["retained_motion"] = json!(retained);
    animation["first_person"]["controller"] =
        json!("Native states with source weapon groups and checked shared-arm fallbacks");
    Ok(())
}
