use super::source::Source;
use super::*;
use crate::tiger::animation::{control::Rig, pose::Pose};

pub(super) struct Translated {
    pub poses: Vec<Vec<[f32; 8]>>,
    pub hand_slots: Vec<usize>,
    pub shoulder_reach_adjustment: f32,
}

pub(super) struct Calibration {
    align: Pose,
    grip_to_gun: Pose,
    gun_slot: usize,
    grip_slot: usize,
    source_gun: usize,
    selected: BTreeMap<usize, (usize, Pose, bool)>,
    hands: Vec<usize>,
    arms: BTreeMap<usize, super::arm::Arm>,
    pub parts: Vec<Value>,
    defaults: Vec<[f32; 8]>,
}

fn rotation(mut pose: Pose) -> Pose {
    pose.0[4..7].fill(0.);
    pose.0[7] = 1.;
    pose
}

impl Calibration {
    pub fn new(
        source: &Source,
        rig: &Rig,
        controls: &Payload,
        reference: &[[f32; 8]],
        source_reference: &[Pose],
        aliases: &BTreeMap<String, String>,
        held_offset: [f32; 3],
    ) -> Result<Self> {
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
        let gun = native("b_handle")?;
        let grip = native("b_r_grip")?;
        let grip_slot = rig.slot(grip)?;
        let mut defaults = reference.to_vec();
        for (axis, offset) in held_offset.into_iter().enumerate() {
            defaults[grip_slot][4 + axis] += offset;
        }
        let baseline = rig.objects(&defaults)?;
        let source_gun = source.root;
        let align = baseline[gun].then(source_reference[source_gun].inverse());
        let source_bind = source.bind_objects();
        let mut selected = BTreeMap::new();
        let mut hands = Vec::new();
        let mut arms = BTreeMap::new();
        for side in ["l", "r"] {
            let arm = super::arm::Arm::new(
                side,
                source,
                rig,
                controls,
                &baseline,
                source_reference,
                align,
            )?;
            ensure!(
                arms.insert(arm.slot, arm).is_none(),
                "Native arm controls overlap"
            );
            let hand = native(&format!("b_{side}_hand"))?;
            let source_hand = index(&format!("{side}_hand"))?;
            let source_palm = palm::frame(
                source_bind[source_hand],
                source_bind[index(&format!("{side}_index_low"))?],
                source_bind[index(&format!("{side}_middle_low"))?],
                source_bind[index(&format!("{side}_pinky_low"))?],
            )?;
            let native_palm = palm::frame(
                rig.bind[hand],
                rig.bind[native(&format!("b_{side}_index_1"))?],
                rig.bind[native(&format!("b_{side}_middle_1"))?],
                rig.bind[native(&format!("b_{side}_pinky_1"))?],
            )?;
            let correction = source_palm.then(native_palm.inverse());
            let hand_slot = rig.slot(hand)?;
            selected.insert(hand_slot, (source_hand, correction, false));
            hands.push(hand_slot);
            for finger in ["thumb", "index", "middle", "ring", "pinky"] {
                for (i, suffix) in ["low", "mid", "tip"].iter().enumerate() {
                    let bone = native(&format!("b_{side}_{finger}_{}", i + 1))?;
                    let source_bone = index(&format!("{side}_{finger}_{suffix}"))?;
                    let correction = rotation(
                        source_bind[source_bone]
                            .inverse()
                            .then(source_bind[source_hand])
                            .then(correction)
                            .then(rig.bind[hand].inverse())
                            .then(rig.bind[bone]),
                    );
                    selected.insert(rig.slot(bone)?, (source_bone, correction, true));
                }
            }
        }
        let mut parts = Vec::new();
        for name in &source.weapon {
            let role = aliases
                .get(name)
                .map(String::as_str)
                .unwrap_or_else(|| source.weapon_role(name));
            let Some(bone) = rig.names.iter().position(|&value| value == hash(role)) else {
                parts.push(json!({"source":name,"native_role":role,"status":"unmapped"}));
                continue;
            };
            if bone == gun {
                continue;
            }
            let source_bone = index(name)?;
            let slot = rig.slot(bone)?;
            // A source joint's gun-relative deformation acts on the native
            // joint's bind frame. Copying its absolute pose would also copy the
            // source bind offset and move an otherwise stationary part.
            let correction = source_bind[source_bone]
                .inverse()
                .then(source_bind[source_gun])
                .then(rig.bind[gun].inverse())
                .then(rig.bind[bone]);
            ensure!(
                selected
                    .insert(slot, (source_bone, correction, false))
                    .is_none(),
                "Source weapon parts overlap another translated control"
            );
            parts.push(json!({"source":name,"native_role":role,"control":slot,"bone":bone,"status":"translated"}));
        }
        ensure!(
            selected
                .keys()
                .chain(arms.keys())
                .all(|&slot| slot > grip_slot),
            "Arm controls are evaluated before the weapon grip"
        );
        Ok(Self {
            align,
            grip_to_gun: baseline[grip].inverse().then(baseline[gun]),
            gun_slot: rig.slot(gun)?,
            grip_slot,
            source_gun,
            selected,
            hands,
            arms,
            parts,
            defaults,
        })
    }

    pub fn translate(&self, objects: &[Vec<Pose>], rig: &Rig) -> Result<Translated> {
        let mut output = vec![Vec::new(); self.defaults.len()];
        let mut shoulder_reach_adjustment = 0f32;
        for source_frame in objects {
            let mut current = rig.bind.clone();
            let gun = self.align.then(source_frame[self.source_gun]);
            for (slot, control) in rig.controls.iter().enumerate() {
                let parent = rig.parent(slot, &current);
                let mut local = Pose(self.defaults[slot]);
                if slot == self.grip_slot {
                    local = parent.inverse().then(gun.then(self.grip_to_gun.inverse()));
                } else if slot == self.gun_slot {
                    local = parent.inverse().then(gun);
                } else if let Some(arm) = self.arms.get(&slot) {
                    let (target, adjustment) = arm.target(self.align, source_frame)?;
                    shoulder_reach_adjustment = shoulder_reach_adjustment.max(adjustment);
                    local = parent.inverse().then(target);
                } else if let Some(&(source_bone, correction, finger)) = self.selected.get(&slot) {
                    let target = self.align.then(source_frame[source_bone]).then(correction);
                    local = parent.inverse().then(target);
                    if finger {
                        local.0[4..8].copy_from_slice(&rig.defaults[slot][4..8]);
                    }
                }
                local = Pose::checked(local.0)?;
                current[control.bone] = parent.then(local);
                output[slot].push(local.0);
            }
        }
        Ok(Translated {
            poses: output,
            hand_slots: self.hands.clone(),
            shoulder_reach_adjustment,
        })
    }
}
