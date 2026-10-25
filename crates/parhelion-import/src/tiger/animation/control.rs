//! The checked FK composition subset used when translating character-arm motion.
use super::pose::Pose;
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

pub struct Control {
    pub bone: usize,
    pub parent: Option<usize>,
    pub root: bool,
    frame: Frame,
    position: Option<Position>,
}

enum Frame {
    Parent,
    Bind,
    Root,
    Aim(Position),
}

enum Position {
    Parent(Option<usize>),
    Bind(Option<usize>),
    Root,
    RootBind,
    Midpoint(usize, usize),
}

fn bone(p: &Payload, at: usize, count: usize) -> Result<Option<usize>> {
    let index = p.u16(at)? as i16;
    ensure!(
        index >= -1 && (index < 0 || (index as usize) < count),
        "Control parent is outside the skeleton"
    );
    Ok((index >= 0).then_some(index as usize))
}

impl Position {
    fn read(p: &Payload, at: usize, count: usize) -> Result<Self> {
        let parent = bone(p, at, count)?;
        Ok(match p.u8(at + 4)? {
            0 | 3 => Self::Parent(parent),
            1 => Self::Bind(parent),
            2 => Self::Root,
            4 => Self::RootBind,
            5 => Self::Midpoint(
                parent.context("Midpoint control has no first parent")?,
                bone(p, at + 2, count)?.context("Midpoint control has no second parent")?,
            ),
            _ => anyhow::bail!("Unsupported character control position mode"),
        })
    }

    fn get(&self, index: usize, objects: &[Pose], bind: &[Pose]) -> [f32; 3] {
        let pose = match *self {
            Self::Parent(parent) => parent.map_or(Pose::IDENTITY, |p| objects[p]),
            Self::Bind(parent) => parent.map_or(bind[index], |p| {
                objects[p].then(bind[p].inverse()).then(bind[index])
            }),
            Self::Root => objects[0],
            Self::RootBind => objects[0].then(bind[index]),
            Self::Midpoint(a, b) => {
                return std::array::from_fn(|i| (objects[a].0[i + 4] + objects[b].0[i + 4]) * 0.5);
            }
        };
        pose.0[4..7].try_into().unwrap()
    }
}

pub struct Rig {
    pub controls: Vec<Control>,
    pub defaults: Vec<[f32; 8]>,
    pub bind: Vec<Pose>,
    pub names: Vec<u32>,
    /// Hierarchy parents, which uncontrolled bones and weapon sections follow.
    pub parents: Vec<Option<usize>>,
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
        Self::read_modes(skeleton, controls, false)
    }

    /// Full character controls evaluated in the neutral +X aim frame. Imported
    /// clips retain the native aim response and subsequent limb constraints.
    pub fn read_character(skeleton: &Payload, controls: &Payload) -> Result<Self> {
        Self::read_modes(skeleton, controls, true)
    }

    fn read_modes(skeleton: &Payload, controls: &Payload, character: bool) -> Result<Self> {
        let s = skeleton.pointer(24)?;
        let c = controls.pointer(24)?;
        ensure!(
            skeleton.u32(s - 4)? == 0x80808546 && controls.u32(c - 4)? == 0x80808F8F,
            "Motion translation requires native FK controls"
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
        let parents = hierarchy
            .iter()
            .map(|&at| {
                let parent = skeleton.u32(at + 4)? as i32;
                Ok((parent >= 0).then_some(parent as usize))
            })
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
        if character {
            ensure!(
                bind.iter().all(|p| (p.0[7] - 1.).abs() < 1e-5)
                    && defaults.iter().all(|p| (p[7] - 1.).abs() < 1e-5),
                "Character control translation requires unit-scale native poses"
            );
        }
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
            let parent = self::bone(controls, row + 4, bind.len())?;
            let mode = controls.u8(row + 8)?;
            let composition = controls.u8(row + 48)?;
            let constraint = controls.u8(row + 49)?;
            let frame = match mode {
                0 | 3 => Frame::Parent,
                2 => Frame::Root,
                1 if character => Frame::Bind,
                6 if character => {
                    let weight = controls.f32(row + 16)?;
                    ensure!(
                        controls.u8(row + 22)? == 0 && weight.is_finite(),
                        "Character control requires an unsupported aim source"
                    );
                    Frame::Aim(Position::read(controls, row + 10, bind.len())?)
                }
                _ => anyhow::bail!("Unsupported native control parent mode"),
            };
            let position = match composition {
                0 => None,
                1 if character => Some(Position::read(controls, row + 24, bind.len())?),
                _ => anyhow::bail!("Unsupported native control composition mode"),
            };
            ensure!(
                matches!(constraint, 0 | 255) || (character && constraint == 1),
                "Unsupported native control constraint mode"
            );
            result.push(Control {
                bone,
                parent,
                root: mode == 2,
                frame,
                position,
            });
        }
        Ok(Self {
            controls: result,
            defaults,
            bind,
            names,
            parents,
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
        let mut result = match &c.frame {
            Frame::Parent => c.parent.map_or(Pose::IDENTITY, |p| objects[p]),
            Frame::Bind => c.parent.map_or(self.bind[c.bone], |p| {
                objects[p]
                    .then(self.bind[p].inverse())
                    .then(self.bind[c.bone])
            }),
            Frame::Root => objects[0],
            Frame::Aim(position) => {
                let mut frame = Pose::IDENTITY;
                frame.0[4..7].copy_from_slice(&position.get(c.bone, objects, &self.bind));
                frame
            }
        };
        if let Some(position) = &c.position {
            result.0[4..7].copy_from_slice(&position.get(c.bone, objects, &self.bind));
            result.0[7] = 1.;
        }
        result
    }

    /// Convert a neutral object-space target into the native clip control space.
    pub fn local(&self, slot: usize, objects: &[Pose], target: Pose) -> Result<Pose> {
        let parent = self.parent(slot, objects);
        let mut local = parent.inverse().then(target);
        if self.controls[slot].position.is_some() {
            ensure!(
                (target.0[7] - objects[self.controls[slot].bone].0[7]).abs() < 1e-5,
                "Split character controls cannot change scale"
            );
            for i in 4..7 {
                local.0[i] = target.0[i] - parent.0[i];
            }
            local.0[7] = 1.;
        }
        Pose::checked(local.0)
    }

    pub fn compose(&self, slot: usize, objects: &[Pose], local: Pose) -> Pose {
        let parent = self.parent(slot, objects);
        let mut result = parent.then(local);
        if self.controls[slot].position.is_some() {
            for i in 4..7 {
                result.0[i] = parent.0[i] + local.0[i];
            }
            result.0[7] = objects[self.controls[slot].bone].0[7];
        }
        result
    }

    /// Direct-control poses only. The game applies arm constraints separately.
    pub fn objects(&self, values: &[[f32; 8]]) -> Result<Vec<Pose>> {
        ensure!(
            values.len() == self.controls.len(),
            "Control pose count differs"
        );
        let mut result = self.bind.clone();
        for (slot, c) in self.controls.iter().enumerate() {
            result[c.bone] = self.compose(slot, &result, Pose::checked(values[slot])?);
        }
        Ok(result)
    }
}
