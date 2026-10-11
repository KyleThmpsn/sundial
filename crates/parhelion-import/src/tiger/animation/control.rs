//! The checked FK composition subset used when translating character-arm motion.
use super::pose::Pose;
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

pub struct Control {
    pub bone: usize,
    pub parent: Option<usize>,
    pub root: bool,
}

pub struct Rig {
    pub controls: Vec<Control>,
    pub defaults: Vec<[f32; 8]>,
    pub bind: Vec<Pose>,
    pub names: Vec<u32>,
}

fn poses(p: &Payload, at: usize) -> Result<Vec<[f32; 8]>> {
    p.array(at, 32, Some(0x80809F75))?
        .into_iter()
        .map(|at| {
            let mut v = [0.; 8];
            for (i, value) in v.iter_mut().enumerate() {
                *value = p.f32(at + i * 4)?;
            }
            Ok(Pose::checked(v)?.0)
        })
        .collect()
}

impl Rig {
    pub fn read(skeleton: &Payload, controls: &Payload) -> Result<Self> {
        let s = skeleton.pointer(24)?;
        let c = controls.pointer(24)?;
        ensure!(
            skeleton.u32(s - 4)? == 0x80808546 && controls.u32(c - 4)? == 0x80808F8F,
            "Arm translation requires native FK controls"
        );
        let bind = poses(skeleton, s + 0x90)?
            .into_iter()
            .map(Pose)
            .collect::<Vec<_>>();
        let hierarchy = skeleton.array(s + 0x80, 16, Some(0x80808A08))?;
        let names = hierarchy
            .iter()
            .map(|&at| skeleton.u32(at))
            .collect::<Result<Vec<_>>>()?;
        let defaults = poses(controls, c + 0xB0)?;
        let inverse = controls.array(c + 0xD8, 2, None)?;
        let forward = controls.array(c + 0xE8, 2, None)?;
        let rows = controls.array(c + 0xA0, 52, None)?;
        ensure!(
            bind.len() == names.len()
                && inverse.len() == bind.len()
                && defaults.len() == rows.len()
                && forward.len() == rows.len(),
            "Control maps disagree with the native rig"
        );
        let mut result = Vec::new();
        let mut bones = std::collections::BTreeSet::new();
        for (slot, row) in rows.into_iter().enumerate() {
            let bone = usize::from(controls.u16(row + 44)?);
            ensure!(
                bone < bind.len()
                    && bones.insert(bone)
                    && controls.u16(inverse[bone])? as usize == slot
                    && controls.u16(forward[slot])? as usize == bone,
                "Control and bone maps are not reciprocal"
            );
            let parent = controls.u16(row + 4)? as i16;
            ensure!(
                parent >= -1 && (parent < 0 || (parent as usize) < bind.len()),
                "Control parent is outside the skeleton"
            );
            let mode = controls.u8(row + 8)?;
            ensure!(
                matches!(mode, 0 | 2 | 3)
                    && controls.u8(row + 48)? == 0
                    && matches!(controls.u8(row + 49)?, 0 | 255),
                "Native arm control needs an unsupported composition mode"
            );
            result.push(Control {
                bone,
                parent: (parent >= 0).then_some(parent as usize),
                root: mode == 2,
            });
        }
        Ok(Self {
            controls: result,
            defaults,
            bind,
            names,
        })
    }

    pub fn slot(&self, bone: usize) -> Result<usize> {
        self.controls
            .iter()
            .position(|c| c.bone == bone)
            .context("Requested arm bone has no direct control")
    }

    pub fn parent(&self, slot: usize, objects: &[Pose]) -> Pose {
        let c = &self.controls[slot];
        if c.root {
            objects[0]
        } else {
            c.parent.map_or(Pose::IDENTITY, |p| objects[p])
        }
    }

    /// Direct-control poses only. Procedural forearms are retained by the game.
    pub fn objects(&self, values: &[[f32; 8]]) -> Result<Vec<Pose>> {
        ensure!(
            values.len() == self.controls.len(),
            "Control pose count differs"
        );
        let mut result = self.bind.clone();
        for (slot, c) in self.controls.iter().enumerate() {
            result[c.bone] = self
                .parent(slot, &result)
                .then(Pose::checked(values[slot])?);
        }
        Ok(result)
    }
}
