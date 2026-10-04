//! Signed-short curve tracks with byte time deltas and two four-bit tangents per component.
//! The axis/angle quaternion and Hermite tangent math follows TagTool's CurveCodec:
//! <https://github.com/TheGuardians/TagTool/blob/ce2dc6ac13072b160752af618ea1dc143e2727ac/TagTool/Animations/Codecs/CurveCodec.cs>
//! The package tables and bone maps are Shadowkeep-specific and checked independently.
use super::*;

struct Values<'a> {
    bytes: &'a [u8],
    rows: usize,
    count: usize,
}
impl<'a> Values<'a> {
    fn read(bytes: &'a [u8], at: usize, limit: usize) -> Result<Self, String> {
        let (count, rows) = table(bytes, at, 0x8080_0006, 2, limit)?;
        Ok(Self { bytes, rows, count })
    }
    fn get(&self, index: usize) -> Result<i16, String> {
        if index >= self.count {
            return Err("Animation curve addresses a missing sample.".into());
        }
        Ok(i16::from_le_bytes(bytes_at(
            self.bytes,
            self.rows + index * 2,
        )?))
    }
    fn offset(&self, index: usize) -> Result<usize, String> {
        usize::try_from(self.get(index)?)
            .map_err(|_| "Animation curve has a negative table offset.".into())
    }
    fn point(&self, at: usize, channel: Channel) -> Result<[f32; 4], String> {
        let mut point = [0.0; 4];
        let width = if matches!(channel, Channel::Rotation) {
            3
        } else {
            channel.width()
        };
        for (axis, value) in point.iter_mut().enumerate().take(width) {
            *value = f32::from(self.get(at + axis)?) / 32767.0;
        }
        if matches!(channel, Channel::Rotation) {
            let [i, j, encoded, _] = point;
            if i * i + j * j > 1.001 || encoded.abs() > 1.001 {
                return Err("Animation curve contains an invalid packed rotation.".into());
            }
            let z = (1.0 - i * i - j * j).max(0.0).sqrt() * if encoded < 0.0 { -1.0 } else { 1.0 };
            let w = encoded.abs().min(1.0) * 2.0 - 1.0;
            let radius = (1.0 - w * w).max(0.0).sqrt();
            point = [i * radius, j * radius, z * radius, w];
        }
        Ok(point)
    }
}

struct Data<'a> {
    raw: Values<'a>,
    compressed: Values<'a>,
    times: &'a [u8],
    tangents: &'a [u8],
}
impl Data<'_> {
    fn samples(&self, map: i16, channel: Channel, count: usize) -> Result<Vec<[f32; 4]>, String> {
        let width = if matches!(channel, Channel::Rotation) {
            3
        } else {
            channel.width()
        };
        if map < 0 {
            let start = (-i32::from(map) - 1) as usize;
            return (0..count)
                .map(|frame| self.raw.point(start + frame * width, channel))
                .collect();
        }
        if map == 0 {
            return Err("Animation curve is missing its track offset.".into());
        }
        let start = map as usize - 1;
        let time_at = self.compressed.offset(start)?;
        let tangent_at = self.compressed.offset(start + 1)?;
        let segments = self.compressed.offset(start + 2)?;
        if segments == 0
            || segments >= count
            || time_at + segments > self.times.len()
            || tangent_at + segments * channel.width() > self.tangents.len()
        {
            return Err("Invalid animation curve segment bounds.".into());
        }
        let mut keys = Vec::with_capacity(segments + 1);
        let mut frames = Vec::with_capacity(segments + 1);
        frames.push(0usize);
        for segment in 0..=segments {
            keys.push(
                self.compressed
                    .point(start + 3 + segment * width, channel)?,
            );
            if segment < segments {
                let delta = usize::from(self.times[time_at + segment]);
                if delta == 0 {
                    return Err("Animation curve contains an empty time interval.".into());
                }
                frames.push(frames[segment] + delta);
            }
        }
        if frames[segments] != count - 1 {
            return Err("Animation curve timing does not match the clip.".into());
        }
        let tangents = &self.tangents[tangent_at..tangent_at + segments * channel.width()];
        Ok(evaluate(&keys, &frames, tangents, channel.width(), count))
    }
}

fn evaluate(
    keys: &[[f32; 4]],
    frames: &[usize],
    tangents: &[u8],
    width: usize,
    count: usize,
) -> Vec<[f32; 4]> {
    let mut segment = 0;
    let mut result = Vec::with_capacity(count);
    for frame in 0..count {
        while segment + 2 < frames.len() && frame > frames[segment + 1] {
            segment += 1;
        }
        let t = (frame - frames[segment]) as f32 / (frames[segment + 1] - frames[segment]) as f32;
        let mut point = [0.0; 4];
        for axis in 0..width {
            point[axis] = hermite(
                keys[segment][axis],
                keys[segment + 1][axis],
                tangents[segment * width + axis],
                t,
            );
        }
        result.push(point);
    }
    result
}

fn byte_table(bytes: &[u8], at: usize, limit: usize) -> Result<&[u8], String> {
    let (count, rows) = table(bytes, at, 0x8080_0009, 1, limit)?;
    Ok(&bytes[rows..rows + count])
}

pub(super) fn apply(
    bytes: &[u8],
    data: usize,
    tracks: &Tracks,
    poses: &mut [Vec<Transform>],
) -> Result<(), String> {
    let frame_count = poses.len();
    let limit = tracks.channels() * frame_count;
    let curves = Data {
        raw: Values::read(bytes, data + 0x10, limit)?,
        compressed: Values::read(bytes, data + 0x20, limit + tracks.count() * 3)?,
        times: byte_table(bytes, data + 0x30, limit)?,
        tangents: byte_table(bytes, data + 0x40, limit)?,
    };
    let intervals = tracks.0[0].len() + tracks.0[2].len();
    let ranges = exact(bytes, data + 0x50, 0x8080_000F, 4, intervals)?;
    let biases = exact(bytes, data + 0x60, 0x8080_000F, 4, intervals)?;
    let mappings = Values::read(bytes, data + 0x70, tracks.count() + 1)?;
    if mappings.count != tracks.count() + 1 {
        return Err("Invalid animation curve track count.".into());
    }
    let mut mapped = 0;
    for (channel, bones) in tracks.groups() {
        for (track, &bone) in bones.iter().enumerate() {
            let map = mappings.get(mapped)?;
            mapped += 1;
            let (range, bias) = match channel {
                Channel::Scale | Channel::Position => {
                    let index = track
                        + if matches!(channel, Channel::Position) {
                            tracks.0[0].len()
                        } else {
                            0
                        };
                    (
                        float(bytes, ranges + index * 4)?,
                        float(bytes, biases + index * 4)?,
                    )
                }
                Channel::Rotation => (1.0, 0.0),
            };
            for (pose, point) in poses
                .iter_mut()
                .zip(curves.samples(map, channel, frame_count)?)
            {
                channel.store(&mut pose[bone], &transform(point, channel, range, bias)?);
            }
        }
    }
    Ok(())
}

fn transform(point: [f32; 4], channel: Channel, range: f32, bias: f32) -> Result<[f32; 4], String> {
    if !matches!(channel, Channel::Rotation) {
        return Ok(point.map(|v| v * range + bias));
    }
    let length = point.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !length.is_finite() || length < 1e-8 {
        return Err("Animation curve contains a singular rotation.".into());
    }
    Ok(point.map(|v| v / length))
}

fn hermite(first: f32, second: f32, code: u8, t: f32) -> f32 {
    let delta = second - first;
    let tangent = |n: u8| {
        let value = (f32::from(n) - 7.0) / 7.0;
        delta + 0.3 * value.abs() * value
    };
    let t2 = t * t;
    let t3 = t2 * t;
    (2.0 * t3 - 3.0 * t2 + 1.0) * first
        + (t3 - 2.0 * t2 + t) * tangent(code >> 4)
        + (-2.0 * t3 + 3.0 * t2) * second
        + (t3 - t2) * tangent(code & 15)
}
