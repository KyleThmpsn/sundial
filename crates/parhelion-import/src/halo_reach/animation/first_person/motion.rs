use super::source::Source;
use super::*;
use crate::tiger::animation::{control::Rig, pose::Pose};

pub(super) struct Translated {
    pub poses: Vec<Vec<[f32; 8]>>,
    pub hand_slots: Vec<usize>,
}

pub(super) fn translate(
    source: &Source,
    objects: &[Vec<Pose>],
    rig: &Rig,
    clip: &Payload,
) -> Result<Translated> {
    let index = |name: &str| {
        source
            .names
            .get(name)
            .copied()
            .with_context(|| format!("Source arm role {name} missing"))
    };
    let native = |name: &str| {
        rig.names
            .iter()
            .position(|&h| h == hash(name))
            .with_context(|| format!("Native arm role {name} missing"))
    };
    let defaults = animation::clip::sample(clip, &rig.defaults, 0)?;
    let baseline = rig.objects(&defaults)?;
    let gun = native("b_handle")?;
    let grip = native("b_r_grip")?;
    let source_gun = index("b_gun")?;
    let align = baseline[gun].then(objects[0][source_gun].inverse());
    let grip_to_gun = baseline[grip].inverse().then(baseline[gun]);
    let roles = roles(index, native)?;
    let mut selected = BTreeMap::new();
    for (bone, source_bone, shoulder, finger) in roles {
        let correction = objects[0][source_bone]
            .inverse()
            .then(align.inverse())
            .then(baseline[bone]);
        selected.insert(rig.slot(bone)?, (source_bone, correction, shoulder, finger));
    }
    let grip_slot = rig.slot(grip)?;
    ensure!(
        selected.keys().all(|&slot| slot > grip_slot),
        "Arm controls are evaluated before the weapon grip"
    );
    let mut output = vec![Vec::new(); defaults.len()];
    for source_frame in objects {
        let mut current = rig.bind.clone();
        for (slot, control) in rig.controls.iter().enumerate() {
            let parent = rig.parent(slot, &current);
            let mut local = Pose(defaults[slot]);
            if slot == grip_slot {
                let target = align
                    .then(source_frame[source_gun])
                    .then(grip_to_gun.inverse());
                local = parent.inverse().then(target);
            } else if let Some(&(source_bone, correction, shoulder, finger)) = selected.get(&slot) {
                let mut target = align.then(source_frame[source_bone]).then(correction);
                if shoulder {
                    target.0[4..7].copy_from_slice(&baseline[control.bone].0[4..7]);
                }
                local = parent.inverse().then(target);
                if finger {
                    local.0[4..8].copy_from_slice(&defaults[slot][4..8]);
                }
            }
            local = Pose::checked(local.0)?;
            current[control.bone] = parent.then(local);
            output[slot].push(local.0);
        }
    }
    Ok(Translated {
        poses: output,
        hand_slots: vec![
            rig.slot(native("b_l_hand")?)?,
            rig.slot(native("b_r_hand")?)?,
        ],
    })
}

fn roles(
    index: impl Fn(&str) -> Result<usize>,
    native: impl Fn(&str) -> Result<usize>,
) -> Result<Vec<(usize, usize, bool, bool)>> {
    let mut roles = Vec::new();
    for name in ["l_upperarm", "r_upperarm", "l_hand", "r_hand"] {
        roles.push((
            native(&format!("b_{name}"))?,
            index(name)?,
            name.ends_with("upperarm"),
            false,
        ));
    }
    for side in ["l", "r"] {
        for finger in ["thumb", "index", "middle", "ring", "pinky"] {
            for (i, suffix) in ["low", "mid", "tip"].iter().enumerate() {
                roles.push((
                    native(&format!("b_{side}_{finger}_{}", i + 1))?,
                    index(&format!("{side}_{finger}_{suffix}"))?,
                    false,
                    true,
                ));
            }
        }
    }
    Ok(roles)
}
