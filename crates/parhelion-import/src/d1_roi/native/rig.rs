//! The unchanged body palette is accepted only with matching hierarchy and bind data.
use super::*;

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

/// Same bones in the same order and hierarchy, with binds equal within rounding.
fn same_bones(
    source: &[(usize, String, i32, [f32; 8])],
    target: &[(usize, String, i32, [f32; 8])],
) -> bool {
    source.len() == target.len()
        && source.iter().zip(target).all(|(a, b)| {
            a.0 == b.0
                && a.1 == b.1
                && a.2 == b.2
                && a.3
                    .iter()
                    .zip(&b.3)
                    .all(|(x, y)| (x - y).abs() <= BIND_TOLERANCE)
        })
}

pub(super) fn weapon(source: &Value, donor: u32, native: &mut Reader) -> Result<Value> {
    let tag = crate::d2_mot::assets::item::find_native(native, donor)?;
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
    let target_bones = targets
        .iter()
        .map(|target| bone_signature(&target["bones"]))
        .collect::<Result<Vec<_>>>()?;
    for root in roots {
        let source_bones = bone_signature(&root["bones"])?;
        let candidates = target_bones
            .iter()
            .enumerate()
            .filter(|(_, candidate)| same_bones(&source_bones, candidate))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1 && matched.insert(candidates[0]),
            "Source and native weapon hierarchy or inverse bind data differ"
        );
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
    Ok(
        json!({"source_runtime":source,"native_runtime":target,"bones":bones,
        "hierarchy_and_inverse_bind":"identical_within_rounding_for_root_and_selected_first_person"}),
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
