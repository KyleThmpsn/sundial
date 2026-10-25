//! Codec 9 stores bounded Hermite segments and signed normalized key values.
use super::{Channels, Payload, Result, ensure, floats, unit};
use anyhow::Context;

struct Curve<'a> {
    payload: &'a Payload,
    cursor: usize,
    frames: usize,
    keys: Vec<usize>,
    dense: bool,
}

impl<'a> Curve<'a> {
    fn read(payload: &'a Payload, at: usize, frames: usize, parameters: usize) -> Result<Self> {
        let count = payload.u16(at + 2)? as usize;
        let flags = payload.u8(at + 4)?;
        ensure!(flags <= 1, "Unsupported animation curve flags");
        let dense = flags == 1;
        let mut cursor = at + 8 + parameters * 4;
        let mut keys = vec![0];
        if !dense {
            ensure!(
                count > 0 && count < frames,
                "Invalid animation curve key count"
            );
            for _ in 0..count {
                let step = payload.u8(cursor)? as usize;
                cursor += 1;
                ensure!(step > 0, "Repeated animation curve key");
                keys.push(keys.last().unwrap() + step);
            }
            ensure!(
                keys.last().copied() == Some(frames - 1),
                "Animation curve does not span its frames"
            );
        }
        Ok(Self {
            payload,
            cursor,
            frames,
            keys,
            dense,
        })
    }

    fn value<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut output = [0.; N];
        for value in &mut output {
            *value = f32::from(self.payload.i16(self.cursor)?) / 32767.;
            self.cursor += 2;
        }
        Ok(output)
    }

    fn samples<const N: usize, const STORED: usize>(
        &mut self,
        unpack: impl Fn([f32; STORED]) -> Result<[f32; N]>,
    ) -> Result<Vec<[f32; N]>> {
        if self.dense {
            return (0..self.frames).map(|_| unpack(self.value()?)).collect();
        }
        let mut output = Vec::with_capacity(self.frames);
        let mut left = unpack(self.value()?)?;
        for segment in 0..self.keys.len() - 1 {
            let tangent: [u8; N] = self.payload.bytes(self.cursor)?;
            self.cursor += N;
            let right = unpack(self.value()?)?;
            let start = self.keys[segment];
            let end = self.keys[segment + 1];
            for frame in start..end {
                let t = (frame - start) as f32 / (end - start) as f32;
                let t2 = t * t;
                let t3 = t2 * t;
                output.push(std::array::from_fn(|axis| {
                    let delta = right[axis] - left[axis];
                    let slope = |code: u8| {
                        let v = (f32::from(code) - 7.) / 7.;
                        delta + 0.3 * v * v.abs()
                    };
                    (2. * t3 - 3. * t2 + 1.) * left[axis]
                        + (t3 - 2. * t2 + t) * slope(tangent[axis] >> 4)
                        + (3. * t2 - 2. * t3) * right[axis]
                        + (t3 - t2) * slope(tangent[axis] & 15)
                }));
            }
            left = right;
        }
        output.push(left);
        ensure!(
            output.len() == self.frames,
            "Animation curve frame count differs"
        );
        Ok(output)
    }
}

fn quaternion([x, y, z]: [f32; 3]) -> Result<[f32; 4]> {
    let w = z.abs() * 2. - 1.;
    let radius = (1. - w * w).max(0.).sqrt();
    let axis_z = (1. - x * x - y * y).max(0.).sqrt() * if z < 0. { -1. } else { 1. };
    Ok([x * radius, y * radius, axis_z * radius, w])
}

pub(super) fn decode(p: &Payload, frames: usize) -> Result<Channels> {
    let base = p.u32(20)? as usize;
    let size = p.u32(24)? as usize;
    ensure!(
        size <= p.0.len() && base >= 32 && base <= size,
        "Animation curve payload extent is invalid"
    );
    let bounded = Payload(p.0[..size].to_vec());
    let p = &bounded;
    let counts = [p.u8(1)? as usize, p.u8(2)? as usize, p.u8(3)? as usize];
    let tables = [32, base + p.u32(12)? as usize, base + p.u32(16)? as usize];
    let mut output = Channels::default();
    for kind in 0..3 {
        for node in 0..counts[kind] {
            let at = base
                .checked_add(p.u32(tables[kind] + node * 4)? as usize)
                .context("Animation curve offset overflow")?;
            let mut curve = Curve::read(p, at, frames, [0, 4, 2][kind])?;
            match kind {
                0 => output.rotations.push(
                    curve
                        .samples(quaternion)?
                        .into_iter()
                        .map(unit)
                        .collect::<Result<_>>()?,
                ),
                1 => {
                    let [x, y, z, range] = floats(p, at + 8)?;
                    let values = curve
                        .samples::<3, 3>(Ok)?
                        .into_iter()
                        .map(|v| [v[0] * range + x, v[1] * range + y, v[2] * range + z])
                        .collect();
                    output.translations.push(values);
                }
                _ => {
                    let [bias, range] = floats(p, at + 8)?;
                    output.scales.push(
                        curve
                            .samples::<1, 1>(Ok)?
                            .into_iter()
                            .map(|v| v[0] * range + bias)
                            .collect(),
                    );
                }
            }
        }
    }
    Ok(output)
}
