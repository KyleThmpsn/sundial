//! Bind-relative body motion with native proportions and explicit limb guides.
use super::{Options, source::Source};
use crate::{
    halo_reach::rig::normalize,
    tiger::{
        animation::{control::Rig, pose::Pose},
        payload::Payload,
    },
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

fn hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811C9DC5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

fn rotation(mut pose: Pose) -> Pose {
    pose.0[4..7].fill(0.);
    pose.0[7] = 1.;
    pose
}

fn difference(a: Pose, b: Pose) -> [f32; 3] {
    std::array::from_fn(|i| a.0[i + 4] - b.0[i + 4])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) struct Calibration {
    parents: Vec<Option<usize>>,
    selected: BTreeMap<usize, (usize, Pose)>,
    source_bind: Vec<Pose>,
    pelvis: usize,
    scale: f32,
    limbs: Vec<([usize; 3], f32, f32)>,
    grip: Option<super::grip::Grip>,
}

impl Calibration {
    pub fn new(
        source: &Source,
        rig: &Rig,
        skeleton: &Payload,
        controls: &Payload,
        options: &Options,
        weapon_scale: Option<f32>,
    ) -> Result<Self> {
        let parent_rows = skeleton.array(skeleton.pointer(24)? + 0x80, 16, Some(0x80808A08))?;
        let parents = parent_rows
            .iter()
            .enumerate()
            .map(|(index, &row)| {
                let parent = skeleton.u16(row + 4)? as i16;
                ensure!(
                    parent >= -1 && (parent < 0 || (parent as usize) < index),
                    "Character skeleton is not parent-first"
                );
                Ok((parent >= 0).then_some(parent as usize))
            })
            .collect::<Result<Vec<_>>>()?;
        let native = |name: &str| {
            rig.names
                .iter()
                .position(|&key| key == hash(name))
                .with_context(|| format!("Native character bone {name} is absent"))
        };
        ensure!(
            native("b_pedestal")? == 0,
            "Character root is not its first bone"
        );
        let source_bind = source.objects(&source.bind);
        let mut selected = BTreeMap::new();
        for (name, &index) in &source.names {
            let alias = options.bone_aliases.get(name).cloned().unwrap_or_else(|| {
                if let Some(suffix) = name.strip_prefix("b_spine") {
                    return format!("b_spine_{suffix}");
                }
                if name == "b_neck0" {
                    return "b_neck_1".into();
                }
                if name == "b_neck1" {
                    return "b_neck_2".into();
                }
                if ["thumb", "index", "middle", "ring", "pinky"]
                    .iter()
                    .any(|finger| name.contains(finger))
                    && matches!(name.as_bytes().last(), Some(b'1' | b'2' | b'3'))
                {
                    return format!("{}_{}", &name[..name.len() - 1], &name[name.len() - 1..]);
                }
                name.clone()
            });
            if let Some(bone) = rig.names.iter().position(|&key| key == hash(&alias)) {
                let correction = rotation(source_bind[index])
                    .inverse()
                    .then(rotation(rig.bind[bone]));
                ensure!(
                    selected.insert(bone, (index, correction)).is_none(),
                    "Source character aliases overlap"
                );
            } else {
                ensure!(
                    !options.bone_aliases.contains_key(name),
                    "Explicit character alias has no native bone"
                );
            }
        }
        for name in [
            "b_pedestal",
            "b_pelvis",
            "b_spine_1",
            "b_spine_2",
            "b_spine_3",
            "b_neck_1",
            "b_neck_2",
            "b_head",
        ] {
            ensure!(
                selected.contains_key(&native(name)?),
                "Required character bone {name} is unmapped"
            );
        }
        let pelvis = native("b_pelvis")?;
        let source_pelvis = selected[&pelvis].0;
        let scale = rig.bind[pelvis].0[6] / source_bind[source_pelvis].0[6];
        ensure!(
            scale.is_finite() && (0.25..=4.).contains(&scale),
            "Character height adaptation is invalid"
        );
        let control_rows = controls.array(controls.pointer(24)? + 0xA0, 52, None)?;
        let mut limbs = Vec::new();
        for side in ["l", "r"] {
            for (roles, guide) in [
                (["upperarm", "forearm", "hand"], 3),
                (["thigh", "calf", "foot"], 6),
            ] {
                let bones = roles
                    .map(|role| native(&format!("b_{side}_{role}")))
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?;
                let bones: [usize; 3] = bones.try_into().unwrap();
                ensure!(
                    bones.iter().all(|bone| selected.contains_key(bone))
                        && parents[bones[1]] == Some(bones[0])
                        && parents[bones[2]] == Some(bones[1]),
                    "Character limb ancestry differs"
                );
                let end = control_rows[rig.slot(bones[2])?];
                ensure!(
                    controls.u8(end + 49)? == 0 && controls.u8(end + 50)? == guide,
                    "Character limb requires an unsupported native constraint"
                );
                let offset = rig.bind[bones[0]].inverse().then(rig.bind[bones[1]]);
                ensure!(
                    offset.0[4].abs() > 1e-5
                        && offset.0[5].abs() < 1e-5
                        && offset.0[6].abs() < 1e-5,
                    "Native character limb does not use the qualified longitudinal axis"
                );
                limbs.push((
                    bones,
                    offset.0[4].signum(),
                    if guide == 3 { 1. } else { -1. },
                ));
            }
        }
        let grip = weapon_scale
            .map(|scale| super::grip::Grip::new(source, rig, &parents, scale))
            .transpose()?;
        Ok(Self {
            parents,
            selected,
            source_bind,
            pelvis,
            scale,
            limbs,
            grip,
        })
    }

    pub fn translate(&self, frames: &[Vec<Pose>], rig: &Rig) -> Result<Vec<Vec<[f32; 8]>>> {
        let mut output = vec![Vec::with_capacity(frames.len()); rig.controls.len()];
        for frame in frames {
            ensure!(
                frame.len() == self.source_bind.len(),
                "Source character frame is incomplete"
            );
            let mut targets = rig.bind.clone();
            for bone in 0..targets.len() {
                let mut target = self.parents[bone].map_or(rig.bind[bone], |parent| {
                    targets[parent].then(rig.bind[parent].inverse().then(rig.bind[bone]))
                });
                if let Some(&(source, correction)) = self.selected.get(&bone) {
                    target.0[..4].copy_from_slice(&rotation(frame[source]).then(correction).0[..4]);
                    if bone == 0 || bone == self.pelvis {
                        let delta = difference(frame[source], self.source_bind[source]);
                        for (axis, value) in delta.into_iter().enumerate() {
                            target.0[axis + 4] = rig.bind[bone].0[axis + 4] + value * self.scale;
                        }
                    }
                }
                targets[bone] = Pose::checked(target.0)?;
            }
            if let Some(grip) = &self.grip {
                grip.apply(frame, &mut targets)?;
            }
            // Supply the plane expected by the native arm and leg postpass. Its
            // segment lengths come from the retained native bind hierarchy.
            for &(bones, direction, guide) in &self.limbs {
                let [upper, middle, end] = bones.map(|bone| targets[bone]);
                let x = normalize(difference(middle, upper))?.map(|v| v * direction);
                let normal = cross(difference(middle, upper), difference(end, upper));
                let z = normalize(normal)
                    .map(|v| v.map(|v| v * guide))
                    .unwrap_or_else(|_| {
                        let axis = upper.then(Pose([0., 0., 0., 1., 0., 0., 1., 1.]));
                        difference(axis, upper)
                    });
                let y = normalize(cross(z, x))?;
                let z = normalize(cross(x, y))?;
                let mut target = super::super::first_person::palm::axes(x, y, z)?;
                target.0[4..7].copy_from_slice(&upper.0[4..7]);
                targets[bones[0]] = target;
            }
            let mut current = rig.bind.clone();
            for (slot, control) in rig.controls.iter().enumerate() {
                let local = if self.selected.contains_key(&control.bone) {
                    rig.local(slot, &current, targets[control.bone])?
                } else {
                    Pose::checked(rig.defaults[slot])?
                };
                current[control.bone] = rig.compose(slot, &current, local);
                output[slot].push(local.0);
            }
        }
        Ok(output)
    }
}
