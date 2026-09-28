//! Shadowkeep's uniform quantized clip codec (80808F71) and static pose codec
//! (80808F6F). Layout verified against the chicken's native idle, 80BC90D2.
//! Spline codecs, animation graphs, root motion and runtime IK are not evaluated.
//!
//! An object's bank lists every clip it can play. A clip identifies itself only by the FNV-1
//! hash of its name at 0x120, never by the name, so `clips` labels the hashes it knows and
//! numbers the rest.
use super::*;
mod pose;
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

impl Animation {
    pub fn duration(&self) -> f32 {
        (self.frames - 1) as f32 / self.fps
    }

    pub fn vertices(&self, model: &Model, seconds: f32) -> Vec<[f32; 3]> {
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
        model
            .vertices
            .iter()
            .zip(&model.weights)
            .map(|(point, weights)| {
                let Some(weights) = weights else {
                    return *point;
                };
                let mut result = [0.0; 3];
                let total: u32 = weights.values.iter().map(|v| *v as u32).sum();
                if total == 0 {
                    return *point;
                }
                for (&bone, &weight) in weights.bones.iter().zip(&weights.values) {
                    if weight == 0 {
                        continue;
                    }
                    let transformed = skin[bone as usize].point(*point);
                    for axis in 0..3 {
                        result[axis] += transformed[axis] * weight as f32 / total as f32;
                    }
                }
                result
            })
            .collect()
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
    let mut clip = None;
    for &tag in &bank.tags {
        let Some(read) = budget.read(manager, tag) else {
            return Err(OVER_BUDGET.into());
        };
        let bytes = read?;
        // Native FNV-1 identifier for "idle", not a guessed first animation.
        if u32_at(&bytes, 0x120)? == IDLE_NAME {
            clip = Some((tag, bytes));
            break;
        }
    }
    let Some((tag, bytes)) = clip else {
        return Err("No native idle clip was found in this animation bank.".into());
    };
    let animation = decode(tag, &bytes, bank.skeleton, bank.data)?;
    skinned(model, &animation)?;
    Ok(Some(animation))
}

/// Every clip the object exposes, in a stable order. The default clip, when one exists, is
/// first; the rest keep the bank's own order.
///
/// A clip the preview cannot decode, because of its codec or its skeleton remap, is left out
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
    // `decode` names the idle clip because the default path is the one that reports it. A clip
    // the caller chose is not that clip, so the same reason is given without the word.
    let animation = decode(tag, &bytes, bank.skeleton, bank.data)
        .map_err(|error| error.replace("This idle clip uses", "This clip uses"))?;
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

fn indices(bytes: &[u8], offset: usize, bones: usize) -> Result<Vec<usize>, String> {
    let (count, rows) = array(bytes, offset, 0x8080_000A, 2, bones)?;
    let result = (0..count)
        .map(|i| u16_at(bytes, rows + i * 2).map(usize::from))
        .collect::<Result<Vec<_>, _>>()?;
    if result.iter().any(|&v| v >= bones) || result.iter().collect::<BTreeSet<_>>().len() != count {
        return Err("Animation has an invalid bone map.".into());
    }
    Ok(result)
}

fn decode(tag: u32, bytes: &[u8], skeleton: &[u8], data: usize) -> Result<Animation, String> {
    if u64_at(bytes, 0)? != bytes.len() as u64 {
        return Err("Animation resource is truncated.".into());
    }
    let (bones, hierarchy) = array(skeleton, data + 0x80, 0x8080_8A08, 16, 256)?;
    if bones == 0 {
        return Err("Animation skeleton is empty.".into());
    }
    let (count, inverse_rows) = array(skeleton, data + 0xA0, 0x8080_9F75, 32, 256)?;
    if count != bones {
        return Err("Animation skeleton has mismatched transforms.".into());
    }
    let mut parents = Vec::new();
    let mut inverse = Vec::new();
    for i in 0..bones {
        let parent = i32_at(skeleton, hierarchy + i * 16 + 4)?;
        if parent < -1 || parent >= i as i32 {
            return Err("Animation skeleton has an invalid hierarchy.".into());
        }
        parents.push((parent >= 0).then_some(parent as usize));
        inverse.push(Transform::read(skeleton, inverse_rows + i * 32)?);
    }
    let mapping = indices(bytes, 0xA8, bones)?;
    if mapping != (0..bones).collect::<Vec<_>>() {
        return Err("This animation uses an unsupported skeleton remap.".into());
    }
    let static_rot = indices(bytes, 0xB8, bones)?;
    let static_pos = indices(bytes, 0xC8, bones)?;
    let animated_rot = indices(bytes, 0xE8, bones)?;
    let animated_pos = indices(bytes, 0xF8, bones)?;
    for (a, b) in [(&static_rot, &animated_rot), (&static_pos, &animated_pos)] {
        if a.len() + b.len() != bones || a.iter().chain(b).collect::<BTreeSet<_>>().len() != bones {
            return Err("Animation tracks do not cover the skeleton exactly once.".into());
        }
    }
    let frames = usize::from(u16_at(bytes, 0x13C)?);
    let fps = u32_at(bytes, 0xA4)? as f32;
    if !(2..=3600).contains(&frames) || !(1.0..=120.0).contains(&fps) || frames * bones > 250_000 {
        return Err("Animation exceeds the preview frame budget.".into());
    }
    let fixed = pointer(bytes, 0x10)?;
    let dynamic = pointer(bytes, 0x18)?;
    if fixed < 4
        || dynamic < 4
        || u32_at(bytes, fixed - 4)? != 0x8080_8F6F
        || u32_at(bytes, dynamic - 4)? != 0x8080_8F71
    {
        return Err("This idle clip uses an animation codec that is not supported yet.".into());
    }
    if u16_at(bytes, fixed)? != 3
        || u16_at(bytes, dynamic)? != 2
        || u16_at(bytes, dynamic + 2)? != 0
        || u16_at(bytes, 0x13E)? as usize != bones
        || u16_at(bytes, fixed + 2)? as usize != bones
        || u16_at(bytes, fixed + 4)? as usize != static_rot.len()
        || u16_at(bytes, fixed + 6)? as usize != static_pos.len()
        || u16_at(bytes, dynamic + 4)? as usize != animated_rot.len()
        || u16_at(bytes, dynamic + 6)? as usize != animated_pos.len()
        || u32_at(bytes, dynamic + 0x10)? as usize != frames
    {
        return Err("Animation codec counts do not match the clip.".into());
    }
    let words = bones + static_rot.len() * 4 + static_pos.len() * 3;
    let (count, samples) = array(bytes, fixed + 0x38, 0x8080_000A, 2, words)?;
    if count != words {
        return Err("Invalid static animation sample count.".into());
    }
    // The initial supported layout has constant unit bone scale.
    if (float(bytes, fixed + 0x14)? - 1.0).abs() > 0.0001
        || (0..bones).any(|i| u16_at(bytes, samples + i * 2) != Ok(0))
    {
        return Err("Animated bone scaling is not supported yet.".into());
    }
    let mut bind = vec![Transform::identity(); bones];
    let mut offset = samples + bones * 2;
    for &bone in &static_rot {
        for axis in 0..4 {
            bind[bone].rotation[axis] =
                u16_at(bytes, offset + axis * 2)? as f32 / 65535.0 * 2.0 - 1.0;
        }
        bind[bone].normalize()?;
        offset += 8;
    }
    for &bone in &static_pos {
        for axis in 0..3 {
            bind[bone].translation[axis] = u16_at(bytes, offset + axis * 2)? as f32 / 65535.0
                * float(bytes, fixed + 0x1C + axis * 4)?
                + float(bytes, fixed + 0x28 + axis * 4)?;
        }
        offset += 6;
    }
    let channels = animated_rot.len() * 4 + animated_pos.len() * 3;
    let (count, samples) = array(bytes, dynamic + 0x18, 0x8080_000A, 2, channels * frames)?;
    let (scale_count, scales) = array(bytes, dynamic + 0x28, 0x8080_000F, 4, channels)?;
    let (bias_count, biases) = array(bytes, dynamic + 0x38, 0x8080_000F, 4, channels)?;
    if count != channels * frames || scale_count != channels || bias_count != channels {
        return Err("Invalid animated sample counts.".into());
    }
    let mut poses = vec![bind; frames];
    let mut channel = 0;
    let mut sample = 0;
    for (indices, width) in [(&animated_rot, 4), (&animated_pos, 3)] {
        for &bone in indices {
            for pose in &mut poses {
                for axis in 0..width {
                    let value = u16_at(bytes, samples + (sample + axis) * 2)? as f32 / 65535.0
                        * float(bytes, scales + (channel + axis) * 4)?
                        + float(bytes, biases + (channel + axis) * 4)?;
                    if width == 4 {
                        pose[bone].rotation[axis] = value;
                    } else {
                        pose[bone].translation[axis] = value;
                    }
                }
                sample += width;
            }
            channel += width;
        }
    }
    for pose in &mut poses {
        for transform in pose {
            transform.normalize()?;
        }
    }
    Ok(Animation {
        tag,
        frames,
        fps,
        parents,
        inverse,
        poses,
    })
}

#[cfg(test)]
mod tests;
