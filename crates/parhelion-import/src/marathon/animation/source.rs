//! Source skeletons and control rigs, with the source FK composition subset.
//!
//! A control row's first 52 bytes keep the native field positions: parent bone at `+04`, parent
//! mode at `+08`, controlled bone at `+2C`, composition at `+30` and limb guide at `+32`. The
//! 20 bytes the source adds at `+34` are retained in the report. Modes 0 and 3 compose with the
//! parent bone and mode 2 with the root. Mode 1 composes with the parent carried to the bone's
//! bind, as the native bind frame does, so its rest values are identity. Uncontrolled bones follow
//! their parent through the bind poses. Each rig must reproduce its own object binds from its control defaults before any clip
//! is evaluated on it, which qualifies these modes for that rig.
use super::*;
use crate::tiger::animation::pose::Pose;

pub(super) const SKELETON: u32 = 0x8080_9FB7;
pub(super) const CONTROLS: u32 = 0x8080_AD7E;

pub(super) struct Control {
    pub bone: usize,
    parent: Option<usize>,
    root: bool,
    /// Mode 1: the value is relative to the bone's bind under its parent.
    bind: bool,
    /// Native limb guide byte. 3 and 6 select the guided two-bone hand and foot solves.
    pub limb: u8,
}

pub(super) struct Rig {
    pub names: Vec<u32>,
    pub parents: Vec<Option<usize>>,
    pub bind: Vec<Pose>,
    pub controls: Vec<Control>,
    pub defaults: Vec<[f32; 8]>,
    /// Control groups in clip order: name hash and control count.
    pub groups: Vec<(u32, usize)>,
    /// Named roles at controls `+118`. Their indices address bones.
    pub roles: BTreeMap<u32, usize>,
    pub extensions: usize,
}

fn transform(p: &Payload, at: usize) -> Result<[f32; 8]> {
    let mut v = [0.; 8];
    for (i, value) in v.iter_mut().enumerate() {
        *value = p.f32(at + i * 4)?;
    }
    Ok(Pose::checked(v)?.0)
}

impl Rig {
    pub fn read(skeleton: &Payload, controls: &Payload) -> Result<Self> {
        let s = skeleton.pointer(24)?;
        let c = controls.pointer(24)?;
        ensure!(
            skeleton.u32(s - 4)? == SKELETON && controls.u32(c - 4)? == CONTROLS,
            "Source rig requires a skeleton and control definition"
        );
        let mut names = Vec::new();
        let mut parents = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, row) in skeleton
            .array(s + 0xB0, 16, Some(0x8080AF42))?
            .into_iter()
            .enumerate()
        {
            let name = skeleton.u32(row)?;
            let parent = skeleton.u32(row + 4)? as i32;
            ensure!(
                seen.insert(name) && (parent == -1 || (parent >= 0 && (parent as usize) < index)),
                "Source skeleton hierarchy is invalid"
            );
            names.push(name);
            parents.push((parent >= 0).then_some(parent as usize));
        }
        let bind = skeleton
            .array(s + 0xC0, 32, Some(0x8080BF47))?
            .into_iter()
            .map(|at| transform(skeleton, at).map(Pose))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !names.is_empty()
                && bind.len() == names.len()
                && bind.iter().all(|p| (p.0[7] - 1.).abs() < 1e-5),
            "Source skeleton binds differ from its hierarchy"
        );
        let rows = controls.array(c + 0xA8, 72, Some(0x8080AE76))?;
        let defaults = controls
            .array(c + 0xB8, 32, Some(0x8080BF47))?
            .into_iter()
            .map(|at| transform(controls, at))
            .collect::<Result<Vec<_>>>()?;
        let forward = controls.array(c + 0xE0, 2, Some(0x80800006))?;
        let reverse = controls.array(c + 0xF0, 2, Some(0x80800006))?;
        ensure!(
            defaults.len() == rows.len()
                && reverse.len() == rows.len()
                && forward.len() == names.len(),
            "Source control maps disagree with the rig"
        );
        let mut result = Vec::new();
        let mut controlled = BTreeSet::new();
        let mut extensions = 0;
        for (slot, row) in rows.into_iter().enumerate() {
            let bone = usize::from(controls.u16(row + 0x2C)?);
            let parent = controls.u16(row + 4)? as i16;
            let mode = controls.u8(row + 8)?;
            ensure!(
                bone < names.len()
                    && controlled.insert(bone)
                    && usize::from(controls.u16(forward[bone])?) == slot
                    && usize::from(controls.u16(reverse[slot])?) == bone,
                "Source control and bone maps are not reciprocal"
            );
            ensure!(
                parent >= -1 && (parent < 0 || (parent as usize) < names.len()),
                "Source control parent is outside the skeleton"
            );
            ensure!(
                matches!(mode, 0..=3) && controls.u8(row + 0x30)? == 0,
                "Source control uses an untranslated parent or composition mode"
            );
            if controls.0[row + 0x34..row + 0x48].iter().any(|b| *b != 0) {
                extensions += 1;
            }
            result.push(Control {
                bone,
                parent: (parent >= 0).then_some(parent as usize),
                root: mode == 2,
                bind: mode == 1,
                limb: controls.u8(row + 0x32)?,
            });
        }
        let groups = controls
            .array(c + 0x98, 8, Some(0x8080AE7F))?
            .into_iter()
            .map(|at| Ok((controls.u32(at)?, controls.u32(at + 4)? as usize)))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            groups.iter().map(|g| g.1).sum::<usize>() == result.len(),
            "Source control groups do not cover the controls"
        );
        let mut roles = BTreeMap::new();
        for at in controls.array(c + 0x118, 8, Some(0x8080ADFB))? {
            let index = controls.u32(at + 4)? as usize;
            ensure!(
                index < names.len() && roles.insert(controls.u32(at)?, index).is_none(),
                "Source rig role table is ambiguous"
            );
        }
        let rig = Self {
            names,
            parents,
            bind,
            controls: result,
            defaults,
            groups,
            roles,
            extensions,
        };
        rig.verify_rest()?;
        Ok(rig)
    }

    /// Object poses for control values. Control parents can follow their bone in index order,
    /// so bones resolve on demand with a cycle check.
    pub fn objects(&self, values: &[Pose]) -> Result<Vec<Pose>> {
        ensure!(
            values.len() == self.controls.len(),
            "Source pose count differs"
        );
        let mut slot = vec![None; self.names.len()];
        for (index, control) in self.controls.iter().enumerate() {
            slot[control.bone] = Some(index);
        }
        let mut objects: Vec<Option<Pose>> = vec![None; self.names.len()];
        fn resolve(
            rig: &Rig,
            bone: usize,
            values: &[Pose],
            slot: &[Option<usize>],
            objects: &mut Vec<Option<Pose>>,
            active: &mut BTreeSet<usize>,
        ) -> Result<Pose> {
            if let Some(pose) = objects[bone] {
                return Ok(pose);
            }
            ensure!(active.insert(bone), "Source control parents form a cycle");
            let pose = if let Some(index) = slot[bone] {
                let control = &rig.controls[index];
                let frame = if control.root {
                    if bone == 0 {
                        Pose::IDENTITY
                    } else {
                        resolve(rig, 0, values, slot, objects, active)?
                    }
                } else if let Some(parent) = control.parent {
                    let object = resolve(rig, parent, values, slot, objects, active)?;
                    if control.bind {
                        object.then(rig.bind[parent].inverse()).then(rig.bind[bone])
                    } else {
                        object
                    }
                } else if control.bind {
                    rig.bind[bone]
                } else {
                    Pose::IDENTITY
                };
                frame.then(values[index])
            } else if let Some(parent) = rig.parents[bone] {
                resolve(rig, parent, values, slot, objects, active)?
                    .then(rig.bind[parent].inverse())
                    .then(rig.bind[bone])
            } else {
                rig.bind[bone]
            };
            active.remove(&bone);
            objects[bone] = Some(pose);
            Ok(pose)
        }
        let mut active = BTreeSet::new();
        (0..self.names.len())
            .map(|bone| resolve(self, bone, values, &slot, &mut objects, &mut active))
            .collect()
    }

    fn verify_rest(&self) -> Result<()> {
        let defaults = self.defaults.iter().map(|v| Pose(*v)).collect::<Vec<_>>();
        for (object, bind) in self.objects(&defaults)?.iter().zip(&self.bind) {
            let distance = (0..3)
                .map(|i| (object.0[4 + i] - bind.0[4 + i]).powi(2))
                .sum::<f32>()
                .sqrt();
            let alignment = (0..4).map(|i| object.0[i] * bind.0[i]).sum::<f32>().abs();
            ensure!(
                distance < 1e-4 && alignment > 0.99999,
                "Source control defaults do not reproduce the rig's object binds"
            );
        }
        Ok(())
    }

    /// The frame a control's value composes with, for already evaluated objects.
    pub fn parent_frame(&self, slot: usize, objects: &[Pose]) -> Pose {
        let control = &self.controls[slot];
        if control.root {
            if control.bone == 0 {
                Pose::IDENTITY
            } else {
                objects[0]
            }
        } else if control.bind {
            control.parent.map_or(self.bind[control.bone], |p| {
                objects[p]
                    .then(self.bind[p].inverse())
                    .then(self.bind[control.bone])
            })
        } else {
            control.parent.map_or(Pose::IDENTITY, |p| objects[p])
        }
    }

    pub fn bone(&self, name: u32) -> Option<usize> {
        self.names.iter().position(|n| *n == name)
    }

    pub fn role(&self, name: &str) -> Result<usize> {
        self.roles
            .get(&fnv(name))
            .copied()
            .with_context(|| format!("Source rig has no {name} role"))
    }
}

pub(super) fn fnv(name: &str) -> u32 {
    name.bytes()
        .fold(0x811C9DC5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}
