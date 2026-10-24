//! Match source bones by identity while preserving hierarchy and inverse bind transforms.
use super::*;
mod owned;
pub(super) use owned::{finish as finish_owned, prepare as prepare_owned};

/// The largest difference between two inverse bind values that still counts as the same bind.
/// Source and native exports of one rig differ only in rounding, by at most about 1.2e-7 in the
/// checked Rise of Iron rocket launcher rigs, while a moved or rotated bone differs by far more.
const BIND_TOLERANCE: f32 = 1e-5;

#[allow(clippy::type_complexity)]
fn bone_signature(value: &Value) -> Result<Vec<(usize, String, i32, [f32; 8])>> {
    #[derive(serde::Deserialize)]
    struct Bone {
        index: usize,
        name_hash: String,
        parent: i32,
        inverse_object_transform: [f32; 8],
    }
    let bones: Vec<Bone> = serde_json::from_value(value.clone())?;
    bones
        .into_iter()
        .map(|bone| {
            ensure!(
                bone.inverse_object_transform.iter().all(|v| v.is_finite()),
                "Nonfinite inverse bind transform"
            );
            Ok((
                bone.index,
                bone.name_hash,
                bone.parent,
                bone.inverse_object_transform,
            ))
        })
        .collect()
}

/// A changed index is safe only when all source bones have unique native names and mapped parents.
fn bone_map(
    source: &[(usize, String, i32, [f32; 8])],
    target: &[(usize, String, i32, [f32; 8])],
) -> Option<Vec<u16>> {
    if source.is_empty() || target.is_empty() {
        return None;
    }
    for bones in [source, target] {
        if bones
            .iter()
            .enumerate()
            .any(|(i, b)| b.0 != i || b.2 < -1 || b.2 >= i as i32)
            || bones.iter().map(|b| &b.1).collect::<BTreeSet<_>>().len() != bones.len()
        {
            return None;
        }
    }
    let mapping = source
        .iter()
        .map(|bone| {
            target
                .iter()
                .position(|b| b.1 == bone.1)
                .and_then(|i| u16::try_from(i).ok())
        })
        .collect::<Option<Vec<_>>>()?;
    if mapping[0] != 0 {
        return None;
    }
    for (bone, &index) in source.iter().zip(&mapping) {
        let native = &target[usize::from(index)];
        let parent = if bone.2 == -1 {
            -1
        } else {
            i32::from(mapping[bone.2 as usize])
        };
        if native.2 != parent
            || bone
                .3
                .iter()
                .zip(&native.3)
                .any(|(a, b)| (a - b).abs() > BIND_TOLERANCE)
        {
            return None;
        }
    }
    Some(mapping)
}

pub(super) fn weapon(
    input: &Source,
    source: &Value,
    donor: u32,
    native: &mut Reader,
) -> Result<Value> {
    let tag = crate::tiger::shadowkeep::item_tag(native, donor)?;
    let target = crate::tiger::rig::inspect(native, tag, false)?;
    let roots = source["skeletons"]
        .as_array()
        .context("Source weapon skeletons")?;
    let targets = target["skeletons"]
        .as_array()
        .context("Native weapon skeletons")?;
    ensure!(
        roots.len() == targets.len() && !roots.is_empty(),
        "Weapon attachment skeleton count differs"
    );
    let mut matched = BTreeSet::new();
    let mut pairs = Vec::new();
    let mut remapped = false;
    let target_bones = targets
        .iter()
        .map(|target| bone_signature(&target["bones"]))
        .collect::<Result<Vec<_>>>()?;
    for root in roots {
        let source_bones = bone_signature(&root["bones"])?;
        let first_person = root["entity"] != source["runtime_entity"];
        let candidates = target_bones
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                if (targets[index]["entity"] != target["runtime_entity"]) != first_person {
                    return None;
                }
                bone_map(&source_bones, candidate).map(|mapping| (index, mapping))
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return owned::inspect(input, source, &target, native)
                .context("Source and native weapon hierarchy or inverse bind data differ");
        }
        ensure!(
            matched.insert(candidates[0].0),
            "Native skeleton matched twice"
        );
        let (index, mapping) = &candidates[0];
        let native_bones = target_bones[*index].len();
        remapped |= source_bones.len() != native_bones
            || mapping
                .iter()
                .enumerate()
                .any(|(i, &n)| i != usize::from(n));
        pairs.push(json!({"source_owner":root["owner"],"native_owner":targets[*index]["owner"],
            "first_person":first_person,"source_bones":source_bones.len(),"native_bones":native_bones,
            "bone_map":mapping}));
    }
    let root = roots
        .iter()
        .find(|s| s["entity"] == source["runtime_entity"])
        .context("Source weapon root skeleton")?;
    let bones = root["bones"]
        .as_array()
        .context("Source weapon root bones")?
        .len();
    ensure!(
        (1..=72).contains(&bones),
        "Weapon palette exceeds the source shader capacity"
    );
    let root_map = pairs
        .iter()
        .find(|p| p["first_person"] == false)
        .context("Mapped equipment skeleton")?;
    ensure!(
        root_map["native_bones"].as_u64().is_some_and(|n| n <= 72),
        "Native weapon palette exceeds the source shader capacity"
    );
    if remapped {
        let attachments = pairs
            .iter()
            .filter(|p| p["first_person"] == true)
            .collect::<Vec<_>>();
        ensure!(
            attachments.len() == 1,
            "Mapped weapon needs one selected first-person rig"
        );
        let attachment = attachments[0];
        let source_start = attachment["source_bones"]
            .as_u64()
            .context("Source attachment bones")?
            .checked_sub(bones as u64)
            .context("Source attachment weapon boundary")? as usize;
        let native_start = attachment["native_bones"]
            .as_u64()
            .context("Native attachment bones")?
            .checked_sub(
                root_map["native_bones"]
                    .as_u64()
                    .context("Native equipment bones")?,
            )
            .context("Native attachment weapon boundary")? as usize;
        let map = attachment["bone_map"]
            .as_array()
            .context("Attachment bone map")?;
        let equipment = root_map["bone_map"]
            .as_array()
            .context("Equipment bone map")?;
        let source_attachment = roots
            .iter()
            .find(|s| s["owner"] == attachment["source_owner"])
            .context("Source attachment skeleton")?;
        ensure!(
            source_start > 0
                && source_start == native_start
                && map[..source_start]
                    .iter()
                    .enumerate()
                    .all(|(i, n)| n.as_u64() == Some(i as u64))
                && equipment
                    .iter()
                    .enumerate()
                    .all(|(i, n)| map[source_start + i].as_u64()
                        == n.as_u64().map(|n| n + native_start as u64)
                        && source_attachment["bones"][source_start + i]["name_hash"]
                            == root["bones"][i]["name_hash"]),
            "Mapped equipment and first-person weapon sections disagree"
        );
    }
    Ok(
        json!({"source_runtime":source,"native_runtime":target,"bones":bones,"pairs":pairs,
        "bone_map":root_map["bone_map"],"remapped":remapped,
        "hierarchy_and_inverse_bind":"mapped_names_and_parents_with_inverse_binds_identical_within_rounding"}),
    )
}

pub(super) fn validate(source: &Source, native: &mut Reader) -> Result<Value> {
    let target_tag = 0x80C22410;
    let target = native.tag(target_tag, Some(0x80809C36))?;
    let tr = target.pointer(24)?;
    ensure!(
        target.u32(tr - 4)? == 0x80808546,
        "Native human skeleton type differs"
    );
    let bones = target.array(tr + 0x80, 16, Some(0x80808A08))?;
    ensure!(bones.len() == 72, "Native human palette differs");
    let mut selected = source
        .human_skeleton
        .lock()
        .map_err(|_| anyhow::anyhow!("Source rig cache is unavailable"))?;
    let tag = if let Some(tag) = *selected {
        tag
    } else {
        let mut matching = Vec::new();
        for (&id, package) in &source.packages {
            if !package
                .header
                .path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("globals_"))
            {
                continue;
            }
            for (index, entry) in package.reader.entries().iter().enumerate() {
                if entry.reference != 0x80800861 {
                    continue;
                }
                let tag = TagHash::new(id, u16::try_from(index)?).0;
                let owner = source.read(tag)?;
                let resource = owner.pointer(24)?;
                if owner.u32(resource - 4)? != 0x8080049A {
                    continue;
                }
                let rows = owner.array(resource + 0x98, 16, Some(0x808004F4))?;
                if rows.len() == bones.len()
                    && rows
                        .iter()
                        .zip(&bones)
                        .all(|(a, b)| owner.bytes::<16>(*a).ok() == target.bytes::<16>(*b).ok())
                {
                    matching.push(tag);
                }
            }
        }
        ensure!(
            matching.len() == 1,
            "Source human body skeleton is missing or ambiguous"
        );
        *selected = Some(matching[0]);
        matching[0]
    };
    let owner = source.read(tag)?;
    let sr = owner.pointer(24)?;
    for (so, no, stride, sc, nc) in [
        (0x98, 0x80, 16, 0x808004F4, 0x80808A08),
        (0xB8, 0xA0, 32, 0x808001B0, 0x80809F75),
    ] {
        let a = owner.array(sr + so, stride, Some(sc))?;
        let b = target.array(tr + no, stride, Some(nc))?;
        ensure!(
            a.len() == 72
                && b.len() == 72
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| owner.0[*a..*a + stride] == target.0[b..b + stride]),
            "Source and native body hierarchy or inverse bind data differ"
        );
    }
    Ok(
        json!({"source_skeleton":tag,"native_skeleton":target_tag,"bones":72,"hierarchy_and_inverse_bind":"byte_identical"}),
    )
}
