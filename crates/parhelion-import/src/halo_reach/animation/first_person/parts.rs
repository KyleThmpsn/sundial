//! Add missing source joints to both ends of the equipment and first-person link.
use super::*;
use crate::tiger::animation::pose::Pose;
use std::sync::Arc;

pub(super) struct Owner {
    pub tag: u32,
    pub original: Arc<Payload>,
    pub converted: Payload,
    pub first_person: bool,
    pub kind: &'static str,
    pub relocation: Option<(usize, usize)>,
}

pub(super) struct Prepared {
    pub owners: Vec<Owner>,
    pub skeleton: Value,
    pub controls: Arc<Payload>,
    pub rig: animation::control::Rig,
}

fn poses(payload: &Payload) -> Result<Vec<Pose>> {
    payload
        .array(payload.pointer(24)? + 0x90, 32, Some(0x80809F75))?
        .into_iter()
        .map(|at| {
            let mut values = [0.; 8];
            for (i, value) in values.iter_mut().enumerate() {
                *value = payload.f32(at + i * 4)?;
            }
            Pose::checked(values)
        })
        .collect()
}

pub(super) fn prepare(
    source: &source::Source,
    scene: &Scene,
    carrier: &carrier::Carrier,
    aliases: &BTreeMap<String, String>,
) -> Result<Option<Prepared>> {
    ensure!(
        scene.models.len() == 1 && scene.models[0].parent.is_none(),
        "Weapon part motion requires one complete source model"
    );
    let model = &scene.models[0].model;
    let (equipment_tag, equipment) = &carrier.equipment_skeleton;
    let definition = equipment.pointer(24)?;
    let hierarchy = equipment.array(definition + 0x80, 16, Some(0x80808A08))?;
    let mut equipment_bind = poses(equipment)?;
    let first = carrier
        .rig
        .bind
        .len()
        .checked_sub(equipment_bind.len())
        .context("First-person weapon section is smaller than its equipment rig")?;
    ensure!(
        first > 0 && hierarchy.len() == equipment_bind.len(),
        "Weapon section boundary differs"
    );
    let mut names = BTreeMap::new();
    let fp_definition = carrier.skeleton.pointer(24)?;
    let fp_hierarchy = carrier
        .skeleton
        .array(fp_definition + 0x80, 16, Some(0x80808A08))?;
    for (index, &row) in hierarchy.iter().enumerate() {
        let name = equipment.u32(row)?;
        ensure!(
            carrier.rig.names[first + index] == name && names.insert(name, index).is_none(),
            "Equipment and first-person weapon joint names differ"
        );
        if index > 0 {
            let parent = equipment.u32(row + 4)? as i32;
            ensure!(
                parent >= 0
                    && carrier.skeleton.u32(fp_hierarchy[first + index] + 4)? as usize
                        == first + parent as usize,
                "Equipment and first-person weapon joint parents differ"
            );
        }
    }
    let gun = *names
        .get(&hash("b_handle"))
        .context("Equipment weapon root missing")?;
    let source_bind = source.bind_objects();
    let source_gun = source.root;
    let mut fp_bind = carrier.rig.bind.clone();
    let mut equipment_extra = Vec::new();
    let mut fp_extra = Vec::new();
    let mut mapped = Vec::<usize>::new();
    // The donor's procedural bones move behind the appended bones, so an appended bone
    // whose native parent is procedural takes that bone's nearest FK ancestor as its parent
    // instead. Its bind keeps its object pose, and its clips carry its source motion.
    let procedural = animation::rig::procedural_bones(equipment)?;
    let native_parents = hierarchy
        .iter()
        .map(|&row| equipment.u32(row + 4).map(|parent| parent as i32))
        .collect::<Result<Vec<_>>>()?;
    let mut reparented = Vec::new();
    for (index, bone) in model.bones.iter().enumerate() {
        ensure!(
            source.weapon.contains(&bone.name),
            "Source world and first-person weapon joints differ"
        );
        let role = aliases
            .get(&bone.name)
            .map(String::as_str)
            .unwrap_or_else(|| source.weapon_role(&bone.name));
        let name = hash(role);
        if let Some(&native) = names.get(&name) {
            mapped.push(native);
            continue;
        }
        ensure!(
            !aliases.contains_key(&bone.name),
            "Explicit native bone alias {role} is absent"
        );
        let parent = bone.parent.context("Unmapped source weapon root")?;
        ensure!(parent < index, "Source weapon parts are not parent-first");
        let mut native_parent = mapped[parent];
        while procedural.contains(&native_parent) {
            let ancestor = usize::try_from(native_parents[native_parent])
                .context("Procedural donor bone has no FK ancestor")?;
            reparented.push(json!({"joint":bone.name,"procedural_parent":format!("{:08X}",equipment.u32(hierarchy[native_parent])?),
                "parent":format!("{:08X}",equipment.u32(hierarchy[ancestor])?)}));
            native_parent = ancestor;
        }
        let source_index = *source
            .names
            .get(&bone.name)
            .context("Source weapon joint missing")?;
        let relative = source_bind[source_gun]
            .inverse()
            .then(source_bind[source_index]);
        let object = equipment_bind[gun].then(relative);
        let fp_object = fp_bind[first + gun].then(relative);
        let native_index = equipment_bind.len();
        names.insert(name, native_index);
        mapped.push(native_index);
        equipment_extra.push(animation::rig::Bone {
            name,
            parent: native_parent,
            local: equipment_bind[native_parent].inverse().then(object).0,
            object: object.0,
            inverse: object.inverse().0,
        });
        fp_extra.push(animation::rig::Bone {
            name,
            parent: first + native_parent,
            local: fp_bind[first + native_parent].inverse().then(fp_object).0,
            object: fp_object.0,
            inverse: fp_object.inverse().0,
        });
        equipment_bind.push(object);
        fp_bind.push(fp_object);
    }
    if equipment_extra.is_empty() {
        return Ok(None);
    }
    // Equipment ranges bind one to one to the first-person ranges from the weapon root, so
    // both skeletons place the joints the same way.
    let placement = animation::rig::Placement::required(equipment)?
        .max(animation::rig::Placement::required(&carrier.skeleton)?);
    let equipment_extension = animation::rig::extend(
        equipment,
        *equipment_tag,
        &carrier.equipment_controls.1,
        &equipment_extra,
        placement,
    )?;
    let fp_extension = animation::rig::extend(
        &carrier.skeleton,
        carrier.skeleton_tag,
        &carrier.controls,
        &fp_extra,
        placement,
    )?;
    let mut skeleton = crate::tiger::rig::decode(
        &equipment_extension.skeleton,
        equipment_extension.skeleton.pointer(24)?,
        0x80808546,
        *equipment_tag,
        false,
    )?;
    skeleton["entity"] = json!(format!("{:08X}", carrier.runtime));
    skeleton["source_parts"] = json!({"appended":equipment_extra.len(),"placement":format!("{placement:?}"),"layout":equipment_extension.layout,"first_appended":equipment_extension.renumbering.first_appended,"reparented":reparented,"method":"Missing source joints appended to both equipment and first-person weapon sections"});
    let rig = animation::control::Rig::read(&fp_extension.skeleton, &fp_extension.controls)?;
    let controls = Arc::new(fp_extension.controls.clone());
    Ok(Some(Prepared {
        skeleton,
        controls,
        rig,
        owners: vec![
            Owner {
                tag: *equipment_tag,
                original: equipment.clone(),
                converted: equipment_extension.skeleton,
                first_person: false,
                kind: "skeleton",
                relocation: Some(equipment_extension.relocation),
            },
            Owner {
                tag: carrier.equipment_controls.0,
                original: carrier.equipment_controls.1.clone(),
                converted: equipment_extension.controls,
                first_person: false,
                kind: "controls",
                relocation: None,
            },
            Owner {
                tag: carrier.skeleton_tag,
                original: carrier.skeleton.clone(),
                converted: fp_extension.skeleton,
                first_person: true,
                kind: "skeleton",
                relocation: Some(fp_extension.relocation),
            },
            Owner {
                tag: carrier.control_tag,
                original: carrier.controls.clone(),
                converted: fp_extension.controls,
                first_person: true,
                kind: "controls",
                relocation: None,
            },
        ],
    }))
}
