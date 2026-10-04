//! Native track maps and the four stored clip codecs, surveyed across Shadowkeep packages.
use super::*;
mod curve;

const MAX_BONES: usize = 512;
const MAX_POSES: usize = 500_000;

#[derive(Clone, Copy)]
enum Channel {
    Scale,
    Rotation,
    Position,
}
impl Channel {
    fn width(self) -> usize {
        match self {
            Self::Scale => 1,
            Self::Rotation => 4,
            Self::Position => 3,
        }
    }
    fn store(self, transform: &mut Transform, value: &[f32]) {
        match self {
            Self::Scale => transform.scale = value[0],
            Self::Rotation => transform.rotation.copy_from_slice(&value[..4]),
            Self::Position => transform.translation.copy_from_slice(&value[..3]),
        }
    }
}

struct Tracks([Vec<usize>; 3]);
impl Tracks {
    fn read(bytes: &[u8], at: usize, bones: usize) -> Result<Self, String> {
        let mut groups = [Vec::new(), Vec::new(), Vec::new()];
        for (group, offset) in groups.iter_mut().zip([at, at + 0x10, at + 0x20]) {
            let (count, rows) = table(bytes, offset, 0x8080_000A, 2, bones)?;
            for i in 0..count {
                group.push(u16_at(bytes, rows + i * 2)? as usize);
            }
            if group.iter().any(|&v| v >= bones)
                || group.iter().collect::<BTreeSet<_>>().len() != count
            {
                return Err("Animation has an invalid bone map.".into());
            }
        }
        Ok(Self(groups))
    }
    fn groups(&self) -> [(Channel, &[usize]); 3] {
        [
            (Channel::Scale, &self.0[0]),
            (Channel::Rotation, &self.0[1]),
            (Channel::Position, &self.0[2]),
        ]
    }
    fn count(&self) -> usize {
        self.0.iter().map(Vec::len).sum()
    }
    fn channels(&self) -> usize {
        self.groups().iter().map(|(c, b)| c.width() * b.len()).sum()
    }
}

fn table(
    bytes: &[u8],
    at: usize,
    class: u32,
    stride: usize,
    limit: usize,
) -> Result<(usize, usize), String> {
    // Empty optional tables have no samples to address and may have a null relative pointer.
    if u64_at(bytes, at)? == 0 {
        return Ok((0, 0));
    }
    let (count, _, rows, actual) = native_array_at(bytes, at)?;
    if actual != class
        || count > limit
        || count
            .checked_mul(stride)
            .and_then(|n| rows.checked_add(n))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(format!("Invalid animation array at {at:#x}."));
    }
    Ok((count, rows))
}

fn exact(
    bytes: &[u8],
    at: usize,
    class: u32,
    stride: usize,
    count: usize,
) -> Result<usize, String> {
    let (actual, rows) = table(bytes, at, class, stride, count)?;
    if actual != count {
        return Err("Animation sample counts do not match the tracks.".into());
    }
    Ok(rows)
}

pub(super) fn decode(
    tag: u32,
    bytes: &[u8],
    skeleton: &[u8],
    data: usize,
) -> Result<Animation, String> {
    if u64_at(bytes, 0)? != bytes.len() as u64 {
        return Err("Animation resource is truncated.".into());
    }
    let (bones, hierarchy) = table(skeleton, data + 0x80, 0x8080_8A08, 16, MAX_BONES)?;
    if bones == 0 {
        return Err("Animation skeleton is empty.".into());
    }
    if u16_at(bytes, 0x13E)? as usize != bones {
        return Err("Animation and skeleton bone counts differ.".into());
    }
    let frames = u16_at(bytes, 0x13C)? as usize;
    if !(1..=3600).contains(&frames) || frames * bones > MAX_POSES {
        return Err("Animation exceeds the preview frame budget.".into());
    }
    let inverse_rows = exact(skeleton, data + 0xA0, 0x8080_9F75, 32, bones)?;
    let mut parents = Vec::with_capacity(bones);
    let mut inverse = Vec::with_capacity(bones);
    for i in 0..bones {
        let parent = i32_at(skeleton, hierarchy + i * 16 + 4)?;
        if parent < -1 || parent >= i as i32 {
            return Err("Animation skeleton has an invalid hierarchy.".into());
        }
        parents.push((parent >= 0).then_some(parent as usize));
        inverse.push(Transform::read(skeleton, inverse_rows + i * 32)?);
    }
    // Inverting the inverse bind table recovers the object's rest pose. The parent inverse
    // then converts it to local space, preserving joints omitted by a partial clip.
    let mut bind = Vec::with_capacity(bones);
    for (i, parent) in parents.iter().enumerate() {
        let object = inverse[i].inverse()?;
        bind.push(parent.map_or(object, |p| inverse[p].compose(object)));
    }
    let fixed = Tracks::read(bytes, 0xA8, bones)?;
    let dynamic = Tracks::read(bytes, 0xD8, bones)?;
    let mut coverage = None;
    for (a, b) in fixed.0.iter().zip(&dynamic.0) {
        let union: BTreeSet<_> = a.iter().chain(b).copied().collect();
        if union.len() != a.len() + b.len()
            || coverage.as_ref().is_some_and(|prior| prior != &union)
        {
            return Err("Animation tracks have overlapping or inconsistent bone maps.".into());
        }
        coverage = Some(union);
    }
    apply(bytes, 0x10, &fixed, std::slice::from_mut(&mut bind))?;
    let mut poses = vec![bind; frames];
    apply(bytes, 0x18, &dynamic, &mut poses)?;
    for pose in &mut poses {
        let mut world = Vec::<Transform>::with_capacity(bones);
        for (i, transform) in pose.iter_mut().enumerate() {
            transform.normalize()?;
            let mut object = parents[i].map_or(*transform, |p| world[p].compose(*transform));
            object.normalize()?;
            let mut skin = object.compose(inverse[i]);
            skin.normalize()?;
            world.push(object);
        }
    }
    // 0xA0 and 0xA4 are bone bounds, not a frame rate. Native clips contain frame samples
    // without an independently validated playback-rate field, so the viewer uses 30 Hz.
    Ok(Animation {
        tag,
        frames,
        fps: 30.0,
        parents,
        inverse,
        poses,
    })
}

fn apply(
    bytes: &[u8],
    pointer_at: usize,
    tracks: &Tracks,
    poses: &mut [Vec<Transform>],
) -> Result<(), String> {
    if i64_at(bytes, pointer_at)? == 0 {
        return if tracks.count() == 0 {
            Ok(())
        } else {
            Err("Animation is missing its track codec.".into())
        };
    }
    let data = pointer(bytes, pointer_at)?;
    let class = u32_at(
        bytes,
        data.checked_sub(4)
            .ok_or("Invalid animation codec pointer.")?,
    )?;
    for (index, bones) in tracks.0.iter().enumerate() {
        if u16_at(bytes, data + 2 + index * 2)? as usize != bones.len() {
            return Err("Animation codec counts do not match the clip.".into());
        }
    }
    match (class, u16_at(bytes, data)?) {
        (0x8080_8F6F, 3) => quantized(bytes, data, tracks, poses, false),
        (0x8080_8F71, 2) => quantized(bytes, data, tracks, poses, true),
        (0x8080_8F72, 1) => curve::apply(bytes, data, tracks, poses),
        (0x8080_8F73, 0) => raw(bytes, data, tracks, poses),
        _ => Err(format!(
            "Animation codec 0x{class:08X} is not supported yet."
        )),
    }
}

fn frames(bytes: &[u8], data: usize, count: usize) -> Result<(), String> {
    if u32_at(bytes, data + 0x10)? as usize != count {
        return Err("Animation codec frame count does not match the clip.".into());
    }
    Ok(())
}

fn quantized(
    bytes: &[u8],
    data: usize,
    tracks: &Tracks,
    poses: &mut [Vec<Transform>],
    intervals: bool,
) -> Result<(), String> {
    frames(bytes, data, poses.len())?;
    let channels = tracks.channels();
    let samples = exact(
        bytes,
        data + if intervals { 0x18 } else { 0x38 },
        0x8080_000A,
        2,
        channels * poses.len(),
    )?;
    let (ranges, biases) = if intervals {
        (
            exact(bytes, data + 0x28, 0x8080_000F, 4, channels)?,
            exact(bytes, data + 0x38, 0x8080_000F, 4, channels)?,
        )
    } else {
        (0, 0)
    };
    let mut channel_at = 0;
    let mut sample_at = 0;
    for (channel, bones) in tracks.groups() {
        let width = channel.width();
        for &bone in bones {
            let mut range = [0.0; 4];
            let mut bias = [0.0; 4];
            for axis in 0..width {
                (range[axis], bias[axis]) = if intervals {
                    (
                        float(bytes, ranges + (channel_at + axis) * 4)?,
                        float(bytes, biases + (channel_at + axis) * 4)?,
                    )
                } else {
                    match channel {
                        Channel::Scale => (float(bytes, data + 0x14)?, float(bytes, data + 0x18)?),
                        Channel::Rotation => (2.0, -1.0),
                        Channel::Position => (
                            float(bytes, data + 0x1C + axis * 4)?,
                            float(bytes, data + 0x28 + axis * 4)?,
                        ),
                    }
                };
            }
            for pose in poses.iter_mut() {
                let mut value = [0.0; 4];
                for axis in 0..width {
                    value[axis] = f32::from(u16_at(bytes, samples + (sample_at + axis) * 2)?)
                        / 65535.0
                        * range[axis]
                        + bias[axis];
                }
                channel.store(&mut pose[bone], &value);
                sample_at += width;
            }
            channel_at += width;
        }
    }
    Ok(())
}

fn raw(
    bytes: &[u8],
    data: usize,
    tracks: &Tracks,
    poses: &mut [Vec<Transform>],
) -> Result<(), String> {
    let frame_count = poses.len();
    frames(bytes, data, frame_count)?;
    for (index, (channel, bones)) in tracks.groups().into_iter().enumerate() {
        let (class, stride) = match channel {
            Channel::Scale => (0x8080_000F, 4),
            Channel::Rotation => (0x8080_0096, 16),
            Channel::Position => (0x8080_0091, 16),
        };
        let samples = exact(
            bytes,
            data + 0x18 + index * 16,
            class,
            stride,
            bones.len() * frame_count,
        )?;
        for (track, &bone) in bones.iter().enumerate() {
            for (frame, pose) in poses.iter_mut().enumerate() {
                let mut value = [0.0; 4];
                let at = samples + (track * frame_count + frame) * stride;
                for (axis, value) in value.iter_mut().enumerate().take(channel.width()) {
                    *value = float(bytes, at + axis * 4)?;
                }
                channel.store(&mut pose[bone], &value);
            }
        }
    }
    Ok(())
}
