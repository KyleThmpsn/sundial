//! Shadowkeep's uniform quantized clip codec (80808F71) and static pose codec
//! (80808F6F). Layout verified against the chicken's native idle, 80BC90D2.
//! Spline codecs, animation graphs, root motion and runtime IK are not evaluated.
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

pub(super) fn load(
    manager: &PackageManager,
    resources: &[Vec<u8>],
    model: &Model,
) -> Result<Option<Animation>, String> {
    let component = |class| -> Result<Option<(&[u8], usize)>, String> {
        for bytes in resources {
            let data = pointer(bytes, 0x18)?;
            if data >= 4 && u32_at(bytes, data - 4)? == class {
                return Ok(Some((bytes, data)));
            }
        }
        Ok(None)
    };
    let Some((skeleton, data)) = component(0x8080_8546)? else {
        return Ok(None);
    };
    let Some((definition, definition_data)) = component(0x8080_344B)? else {
        return Err("This skeleton has no supported animation bank.".into());
    };
    if model.tags.len() != 1 {
        return Err("Animation for objects with multiple models is not supported yet.".into());
    }
    let bank = checked(
        manager,
        u32_at(definition, definition_data + 0x90)?,
        0x8080_36F6,
    )?;
    let (count, rows) = array(&bank, 8, 0x8080_8F48, 4, 512)?;
    let mut clip = None;
    let mut scanned_bytes = 0_u64;
    for i in 0..count {
        let tag = u32_at(&bank, rows + i * 4)?;
        scanned_bytes += manager
            .get_entry(tag)
            .ok_or("Animation clip is missing.")?
            .file_size as u64;
        if scanned_bytes > 32 * 1024 * 1024 {
            return Err("Animation bank exceeds the preview read budget.".into());
        }
        let bytes = checked(manager, tag, 0x8080_8F49)?;
        // Native FNV-1 identifier for "idle", not a guessed first animation.
        if u32_at(&bytes, 0x120)? == 0x6FB7_60FF {
            clip = Some((tag, bytes));
            break;
        }
    }
    let Some((tag, bytes)) = clip else {
        return Err("No native idle clip was found in this animation bank.".into());
    };
    let animation = decode(tag, &bytes, skeleton, data)?;
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
    Ok(Some(animation))
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
