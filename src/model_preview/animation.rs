//! Shadowkeep skeletal clips and shared position/normal deformation.
//! The stored tracks play at a preview cadence of 30 Hz. Animation graphs, root-motion
//! controllers and runtime IK are not evaluated.
//!
//! An object's bank lists every clip it can play. A clip identifies itself only by the FNV-1
//! hash of its name at 0x120, never by the name, so `clips` labels the hashes it knows and
//! numbers the rest.
use super::*;
mod codec;
mod pose;
use codec::decode;
use pose::Transform;

pub(crate) struct Weights {
    pub values: [u8; 4],
    pub bones: [u8; 4],
}

pub(crate) struct Animation {
    pub tag: u32,
    pub frames: usize,
    pub fps: f32,
    parents: Vec<Option<usize>>,
    inverse: Vec<Transform>,
    poses: Vec<Vec<Transform>>,
}

/// One composed owner keeps its own local bone numbering.
pub(crate) struct Rig {
    pub vertices: std::ops::Range<usize>,
    pub animation: Animation,
}

/// One sampled skeleton, shared by the software renderer, GPU and posed export.
pub(crate) struct Deformed {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    /// The single root's full change from its bind pose, used to follow the camera target.
    root: Transform,
    /// Stored root trajectory retained in verification receipts. Geometry keeps this motion.
    #[cfg(test)]
    pub root_translation: [f32; 3],
}

impl Deformed {
    pub(super) fn stored(model: &Model) -> Self {
        Self {
            positions: model.vertices.clone(),
            normals: model.normals.clone(),
            tangents: model.tangents.clone(),
            root: Transform::identity(),
            #[cfg(test)]
            root_translation: [0.0; 3],
        }
    }
    pub fn framing_center(&self, bind_center: [f32; 3]) -> [f32; 3] {
        self.root.point(bind_center)
    }
}

impl Animation {
    pub fn duration(&self) -> f32 {
        (self.frames - 1) as f32 / self.fps
    }

    pub fn looped_seconds(&self, seconds: f32) -> f32 {
        let duration = self.duration();
        if !seconds.is_finite() || duration <= 0.0 {
            0.0
        } else if (0.0..=duration).contains(&seconds) {
            // A paused timeline can select its final frame. The playing UI advances and
            // wraps its clock separately, while continuous material clocks still loop here.
            seconds
        } else {
            seconds.rem_euclid(duration)
        }
    }

    #[cfg(test)]
    pub fn vertices(&self, model: &Model, seconds: f32) -> Vec<[f32; 3]> {
        self.sample(model, seconds).positions
    }

    pub fn sample(&self, model: &Model, seconds: f32) -> Deformed {
        self.sample_from(model, seconds, None)
    }

    pub(super) fn sample_from(
        &self,
        model: &Model,
        seconds: f32,
        stored: Option<&Deformed>,
    ) -> Deformed {
        let mut result = Deformed::stored(model);
        if let Some(stored) = stored {
            result.positions.clone_from(&stored.positions);
            result.normals.clone_from(&stored.normals);
            result.tangents.clone_from(&stored.tangents);
        }
        self.apply(model, seconds, 0..model.vertices.len(), &mut result, true);
        result
    }

    pub(super) fn apply(
        &self,
        model: &Model,
        seconds: f32,
        vertices: std::ops::Range<usize>,
        result: &mut Deformed,
        follow_root: bool,
    ) {
        let frame = if seconds.is_finite() {
            seconds.clamp(0.0, self.duration()) * self.fps
        } else {
            0.0
        };
        let first = (frame.floor() as usize).min(self.frames - 1);
        let next = (first + 1).min(self.frames - 1);
        let mut world = Vec::<Transform>::with_capacity(self.parents.len());
        for (bone, parent) in self.parents.iter().enumerate() {
            let local = self.poses[first][bone].lerp(self.poses[next][bone], frame.fract());
            world.push(parent.map_or(local, |p| world[p].compose(local)));
        }
        let skin: Vec<_> = world
            .iter()
            .zip(&self.inverse)
            .map(|(a, b)| a.compose(*b))
            .collect();
        let single_root = !self.parents.iter().skip(1).any(Option::is_none);
        let root = if single_root && follow_root {
            skin[0]
        } else {
            Transform::identity()
        };
        #[cfg(test)]
        let root_translation = if !single_root || !follow_root {
            [0.0; 3]
        } else {
            self.inverse[0].inverse().map_or([0.0; 3], |bind| {
                std::array::from_fn(|axis| world[0].translation[axis] - bind.translation[axis])
            })
        };
        for index in vertices {
            let Some(weights) = model.weights.get(index).and_then(Option::as_ref) else {
                continue;
            };
            let total: u32 = weights.values.iter().map(|v| u32::from(*v)).sum();
            if total == 0 {
                continue;
            }
            let blend = |transform: &dyn Fn(Transform) -> [f32; 3]| {
                let mut value = [0.0; 3];
                for (&bone, &weight) in weights.bones.iter().zip(&weights.values) {
                    if weight == 0 {
                        continue;
                    }
                    let point = transform(skin[bone as usize]);
                    for axis in 0..3 {
                        value[axis] += point[axis] * f32::from(weight) / total as f32;
                    }
                }
                value
            };
            let point = result.positions[index];
            result.positions[index] = blend(&|t| t.point(point));
            if let Some(normal) = result.normals.get_mut(index) {
                let original = *normal;
                *normal =
                    shader::normal::normalize(blend(&|t| t.normal(original))).unwrap_or([0.0; 3]);
            }
            if let Some(tangent) = result.tangents.get_mut(index) {
                let original = [tangent[0], tangent[1], tangent[2]];
                let direction = shader::normal::normalize(blend(&|t| {
                    t.normal(original).map(|v| v * t.scale * t.scale)
                }))
                .unwrap_or([0.0; 3]);
                tangent[..3].copy_from_slice(&direction);
            }
        }
        if follow_root {
            result.root = root;
            #[cfg(test)]
            {
                result.root_translation = root_translation;
            }
        }
    }
}

/// One playable clip found on the object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Clip {
    /// A label for a picker: the packages' own name for the clip resource when they carry one,
    /// the known name of its stored name hash when that hash is one this preview has verified,
    /// and otherwise its position in the bank.
    pub name: String,
    pub tag: u32,
}

/// The default clip: the object's native idle, decoded as it has always been.
pub(super) fn load(
    manager: &PackageManager,
    resources: &[Vec<u8>],
    model: &Model,
) -> Result<Option<Animation>, String> {
    let Some(bank) = bank(manager, resources, Some(model))? else {
        return Ok(None);
    };
    let mut budget = Budget::default();
    let mut failure = None;
    for &tag in &bank.tags {
        let Some(read) = budget.read(manager, tag) else {
            return Err(OVER_BUDGET.into());
        };
        let Ok(bytes) = read else {
            continue;
        };
        // Native FNV-1 identifier for "idle", not a guessed first animation.
        if u32_at(&bytes, 0x120).ok() == Some(IDLE_NAME) {
            match decode(tag, &bytes, bank.skeleton, bank.data).and_then(|animation| {
                skinned(model, &animation)?;
                Ok(animation)
            }) {
                Ok(animation) => return Ok(Some(animation)),
                Err(error) => failure = Some(error),
            }
        }
    }
    Err(failure.unwrap_or_else(|| "No native idle clip was found in this animation bank.".into()))
}

/// Every clip the object exposes, in a stable order. The default clip, when one exists, is
/// first; the rest keep the bank's own order.
///
/// A clip the preview cannot decode, because of its codec or its bone maps, is left out
/// rather than failing the walk, so one unsupported clip does not hide the others. An object
/// with no skeleton, no bank or an unreadable one enumerates as nothing to choose from.
pub(crate) fn clips(manager: &PackageManager, resources: &[Vec<u8>]) -> Vec<Clip> {
    let Ok(Some(bank)) = bank(manager, resources, None) else {
        return Vec::new();
    };
    let mut budget = Budget::default();
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for (index, &tag) in bank.tags.iter().enumerate() {
        if !seen.insert(tag) {
            continue;
        }
        let Some(read) = budget.read(manager, tag) else {
            break;
        };
        let Ok(bytes) = read else {
            continue;
        };
        let Ok(hash) = u32_at(&bytes, 0x120) else {
            continue;
        };
        if decode(tag, &bytes, bank.skeleton, bank.data).is_err() {
            continue;
        }
        let name = manager
            .get_tag_name(tag)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| label(hash, index));
        found.push((hash == IDLE_NAME, Clip { name, tag }));
    }
    order(found)
}

/// Loads one clip the caller chose from [`clips`], with the same reasons as [`load`] when it
/// cannot be played.
pub(crate) fn load_clip(
    manager: &PackageManager,
    resources: &[Vec<u8>],
    model: &Model,
    tag: u32,
) -> Result<Animation, String> {
    let Some(bank) = bank(manager, resources, Some(model))? else {
        return Err("This object has no animation skeleton.".into());
    };
    if !bank.tags.contains(&tag) {
        return Err("This clip is not in this object's animation bank.".into());
    }
    let mut budget = Budget::default();
    let Some(read) = budget.read(manager, tag) else {
        return Err(OVER_BUDGET.into());
    };
    let bytes = read?;
    let animation = decode(tag, &bytes, bank.skeleton, bank.data)?;
    skinned(model, &animation)?;
    Ok(animation)
}

/// Native FNV-1 identifier for "idle", the clip the preview plays when nothing is chosen.
const IDLE_NAME: u32 = 0x6FB7_60FF;
const OVER_BUDGET: &str = "Animation bank exceeds the preview read budget.";
/// One walk over a bank reads whole clip resources, so the walk is capped rather than the
/// individual read.
const MAX_BANK_BYTES: u64 = 32 * 1024 * 1024;

/// The skeleton an object animates with, and the clips its bank lists.
struct Bank<'a> {
    skeleton: &'a [u8],
    data: usize,
    tags: Vec<u32>,
}

/// Locates the animation bank. `model` is checked only for a caller that means to play a clip
/// on it, because enumerating what an object carries does not need one.
fn bank<'a>(
    manager: &PackageManager,
    resources: &'a [Vec<u8>],
    model: Option<&Model>,
) -> Result<Option<Bank<'a>>, String> {
    let Some((skeleton, data)) = component(resources, 0x8080_8546)? else {
        return Ok(None);
    };
    let Some((definition, definition_data)) = component(resources, 0x8080_344B)? else {
        return Err("This skeleton has no supported animation bank.".into());
    };
    if model.is_some_and(|model| model.tags.len() != 1) {
        return Err("Animation for objects with multiple models is not supported yet.".into());
    }
    let bytes = checked(
        manager,
        u32_at(definition, definition_data + 0x90)?,
        0x8080_36F6,
    )?;
    let (count, rows) = array(&bytes, 8, 0x8080_8F48, 4, 512)?;
    let tags = (0..count)
        .map(|index| u32_at(&bytes, rows + index * 4))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Bank {
        skeleton,
        data,
        tags,
    }))
}

/// The first resource carrying a component of `class`, with the offset of its data.
fn component(resources: &[Vec<u8>], class: u32) -> Result<Option<(&[u8], usize)>, String> {
    for bytes in resources {
        let data = pointer(bytes, 0x18)?;
        if data >= 4 && u32_at(bytes, data - 4)? == class {
            return Ok(Some((bytes.as_slice(), data)));
        }
    }
    Ok(None)
}

/// The read budget for one walk over a bank, so a large bank cannot stall the preview.
#[derive(Default)]
struct Budget(u64);

impl Budget {
    /// Reads one clip, or `None` once the budget is spent and the walk has to stop.
    fn read(&mut self, manager: &PackageManager, tag: u32) -> Option<Result<Vec<u8>, String>> {
        let Some(entry) = manager.get_entry(tag) else {
            return Some(Err("Animation clip is missing.".into()));
        };
        self.0 += entry.file_size as u64;
        if self.0 > MAX_BANK_BYTES {
            return None;
        }
        Some(checked(manager, tag, 0x8080_8F49))
    }
}

/// A clip stores only the FNV-1 hash of its name, never the name, so a readable label needs a
/// hash whose name is already known. `idle` is the one verified against native data; the rest
/// stay positional, because offering a vocabulary at these hashes would be guessing at names
/// rather than reading them.
fn label(name: u32, index: usize) -> String {
    if name == IDLE_NAME {
        return "Idle".into();
    }
    format!("Clip {}", index + 1)
}

/// The default clip leads the list; everything else keeps the bank's order.
fn order(found: Vec<(bool, Clip)>) -> Vec<Clip> {
    let default = found.iter().position(|(default, _)| *default);
    let mut result: Vec<Clip> = found.into_iter().map(|(_, clip)| clip).collect();
    if let Some(index) = default {
        let clip = result.remove(index);
        result.insert(0, clip);
    }
    result
}

/// The model's skin weights have to address bones the clip's skeleton actually has.
fn skinned(model: &Model, animation: &Animation) -> Result<(), String> {
    if model.weights.len() != model.vertices.len()
        || model.weights.iter().any(|w| {
            w.as_ref().is_none_or(|w| {
                w.bones
                    .iter()
                    .zip(w.values)
                    .any(|(&b, v)| v != 0 && b as usize >= animation.parents.len())
            })
        })
    {
        return Err("This model uses unsupported skin weights.".into());
    }
    Ok(())
}

fn float(bytes: &[u8], offset: usize) -> Result<f32, String> {
    let value = f32::from_bits(u32_at(bytes, offset)?);
    if !value.is_finite() {
        return Err("Animation contains a non-finite value.".into());
    }
    Ok(value)
}

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod tests;
