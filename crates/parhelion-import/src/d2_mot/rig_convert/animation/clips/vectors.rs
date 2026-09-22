//! Decode affine variable vec4 tracks into the native uniform vec4 codec.
use super::*;
use crate::d2_mot::rig_convert::write_array;

struct Tracks<'a> {
    source: &'a Payload,
    frames: usize,
    indices: Vec<usize>,
    scales: Vec<usize>,
    biases: Vec<usize>,
    dense: Vec<usize>,
    sparse: Vec<usize>,
    deltas: Vec<usize>,
    tangents: Vec<usize>,
}

impl<'a> Tracks<'a> {
    fn read(source: &'a Payload, start: usize) -> Result<Self> {
        Ok(Self {
            source,
            frames: usize::from(source.u16(0x140)?),
            indices: source.array(start + 0x70, 2, Some(0x80800006))?,
            scales: source.array(start + 0x50, 4, Some(0x8080000F))?,
            biases: source.array(start + 0x60, 4, Some(0x8080000F))?,
            dense: source.array(start + 0x10, 2, Some(0x80800006))?,
            sparse: source.array(start + 0x20, 2, Some(0x80800006))?,
            deltas: source.array(start + 0x30, 1, Some(0x80800009))?,
            tangents: source.array(start + 0x40, 1, Some(0x80800009))?,
        })
    }

    fn sample(&self, track: usize, time: f32) -> Result<[f32; 4]> {
        let source = self.source;
        ensure!(
            self.frames > 0 && time.is_finite() && time >= 0. && time <= (self.frames - 1) as f32,
            "vec4 sample outside clip"
        );
        let index = i32::from(source.i16(*self.indices.get(track).context("vec4 track index")?)?);
        ensure!(index != 0, "vec4 track has no encoded samples");
        let scale = source.f32(*self.scales.get(track).context("vec4 scale")?)?;
        let bias = source.f32(*self.biases.get(track).context("vec4 bias")?)?;
        let dense = index < 0;
        let index = index.unsigned_abs() as usize - 1;
        let decode = |values: &[usize], key: usize, axis: usize| -> Result<f32> {
            Ok(
                f32::from(source.i16(*values.get(key + axis).context("vec4 key sample")?)?)
                    * (1.0 / 32767.0)
                    * scale
                    + bias,
            )
        };
        let mut output = [0.; 4];
        if dense {
            let values = &self.dense;
            let first = time.floor() as usize;
            let second = (first + 1).min(self.frames - 1);
            let blend = time - first as f32;
            for (axis, out) in output.iter_mut().enumerate() {
                let a = decode(values, index + first * 4, axis)?;
                let b = decode(values, index + second * 4, axis)?;
                *out = a + (b - a) * blend;
            }
        } else {
            let values = &self.sparse;
            let deltas = &self.deltas;
            let tangents = &self.tangents;
            let header = |off: usize| -> Result<usize> {
                Ok(usize::try_from(source.i16(
                    *values.get(index + off).context("vec4 segment header")?,
                )?)?)
            };
            let delta_offset = header(0)?;
            let tangent_offset = header(1)?;
            let count = header(2)?;
            ensure!(count > 0, "vec4 curve has no segments");
            let mut begin = 0usize;
            let mut selected = None;
            for segment in 0..count {
                let duration = usize::from(
                    source.u8(*deltas
                        .get(delta_offset + segment)
                        .context("vec4 segment duration")?)?,
                );
                ensure!(duration > 0, "vec4 segment has zero duration");
                if time <= (begin + duration) as f32 {
                    selected = Some((segment, begin, duration));
                    break;
                }
                begin += duration;
            }
            let (segment, begin, duration) =
                selected.context("vec4 curve does not cover the clip")?;
            let t = (time - begin as f32) / duration as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            for (axis, out) in output.iter_mut().enumerate() {
                let a = decode(values, index + 3 + segment * 4, axis)?;
                let b = decode(values, index + 3 + (segment + 1) * 4, axis)?;
                let packed = source.u8(*tangents
                    .get(tangent_offset + segment * 4 + axis)
                    .context("vec4 tangent")?)?;
                let slope = |n: u8| {
                    let v = (f32::from(n) - 7.) / 7.;
                    b - a + v.abs() * v * 0.3
                };
                *out = (2. * t3 - 3. * t2 + 1.) * a
                    + (t3 - 2. * t2 + t) * slope(packed >> 4)
                    + (-2. * t3 + 3. * t2) * b
                    + (t3 - t2) * slope(packed & 15);
            }
        }
        ensure!(
            output.iter().all(|v| v.is_finite()),
            "vec4 decode produced a nonfinite value"
        );
        Ok(output)
    }
}

pub(super) fn convert(
    source: &Payload,
    native: &mut Payload,
    field: usize,
    start: usize,
) -> Result<Value> {
    let tracks = usize::from(source.u16(start + 2)?);
    let frames = usize::from(source.u16(0x140)?);
    ensure!(
        tracks > 0 && tracks < 0x4000 && frames > 0,
        "unsupported vec4 dimensions"
    );
    ensure!(tracks * frames <= 1_000_000, "vec4 sample budget exceeded");
    let source_tracks = Tracks::read(source, start)?;
    ensure!(
        source_tracks.indices.len() == tracks + 1
            && source_tracks.scales.len() == tracks
            && source_tracks.biases.len() == tracks,
        "vec4 track descriptor counts differ"
    );
    ensure!(
        usize::try_from(source.i16(source_tracks.indices[tracks])?)? == source_tracks.sparse.len(),
        "vec4 sparse terminator differs"
    );
    let mut packed = Vec::new();
    let mut ranges = Vec::new();
    let mut biases = Vec::new();
    let mut indices = Vec::new();
    let mut max_error = 0f32;
    let mut curve_error = 0f32;
    for track in 0..tracks {
        let samples = (0..frames)
            .map(|frame| source_tracks.sample(track, frame as f32))
            .collect::<Result<Vec<_>>>()?;
        let mut lo = [f32::INFINITY; 4];
        let mut hi = [f32::NEG_INFINITY; 4];
        for value in &samples {
            for axis in 0..4 {
                lo[axis] = lo[axis].min(value[axis]);
                hi[axis] = hi[axis].max(value[axis]);
            }
        }
        let scale = std::array::from_fn::<_, 4, _>(|axis| hi[axis] - lo[axis]);
        ensure!(
            scale.iter().all(|v| v.is_finite()),
            "vec4 quantization range overflow"
        );
        let mut decoded = Vec::new();
        for value in &samples {
            let mut result = [0.; 4];
            for axis in 0..4 {
                let q = if scale[axis] == 0. {
                    0
                } else {
                    ((value[axis] - lo[axis]) / scale[axis] * 65535.)
                        .round()
                        .clamp(0., 65535.) as u16
                };
                packed.extend_from_slice(&q.to_le_bytes());
                result[axis] = f32::from(q) * (scale[axis] / 65535.) + lo[axis];
                max_error = max_error.max((result[axis] - value[axis]).abs());
            }
            decoded.push(result);
        }
        // The native codec interpolates uniformly sampled values linearly.
        // Measure loss from the source Hermite segments at subframe positions.
        for frame in 0..frames.saturating_sub(1) {
            for fraction in [0.25, 0.5, 0.75] {
                let actual = source_tracks.sample(track, frame as f32 + fraction)?;
                for (axis, actual) in actual.iter().enumerate() {
                    let unquantized = samples[frame][axis]
                        + (samples[frame + 1][axis] - samples[frame][axis]) * fraction;
                    curve_error = curve_error.max((actual - unquantized).abs());
                    let linear = decoded[frame][axis]
                        + (decoded[frame + 1][axis] - decoded[frame][axis]) * fraction;
                    max_error = max_error.max((actual - linear).abs());
                }
            }
        }
        ranges.extend(scale.into_iter().flat_map(f32::to_le_bytes));
        biases.extend(lo.into_iter().flat_map(f32::to_le_bytes));
        indices.extend_from_slice(&(0x4000u16 | track as u16).to_le_bytes());
    }
    ensure!(
        curve_error <= 0.001,
        "vec4 curve resampling error {curve_error} exceeds tolerance"
    );
    let target = (native.0.len() + 11) & !7;
    native.0.resize(target - 4, 0);
    native.0.extend_from_slice(&0x80808F77u32.to_le_bytes());
    native.0.resize(target + 0x50, 0);
    native.0[field..field + 8].copy_from_slice(&(target as i64 - field as i64).to_le_bytes());
    native.0[target..target + 2].copy_from_slice(&2u16.to_le_bytes());
    native.0[target + 2..target + 4].copy_from_slice(&(tracks as u16).to_le_bytes());
    word(native, target + 12, frames as u32);
    write_array(&mut native.0, target + 0x10, 0x80808F82, tracks, &indices)?;
    write_array(
        &mut native.0,
        target + 0x20,
        0x8080000A,
        packed.len() / 2,
        &packed,
    )?;
    write_array(
        &mut native.0,
        target + 0x30,
        0x8080000F,
        tracks * 4,
        &ranges,
    )?;
    write_array(
        &mut native.0,
        target + 0x40,
        0x8080000F,
        tracks * 4,
        &biases,
    )?;
    Ok(
        json!({"field":field,"source_class":"80808B48","native_class":"80808F77",
        "encoded_tracks_preserved":false,"frames":frames,"tracks":tracks,
        "max_sample_error":max_error,"max_curve_error":curve_error}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: usize = 0x198;

    fn fixture(sparse: bool, frames: u16, scale: f32, bias: f32) -> Payload {
        let mut data = vec![0; START + 0x80];
        data[0x28..0x30].copy_from_slice(&(START as i64 - 0x28).to_le_bytes());
        data[START - 4..START].copy_from_slice(&0x80808B48u32.to_le_bytes());
        data[START..START + 4].copy_from_slice(&0x0001_0001u32.to_le_bytes());
        data[0x140..0x142].copy_from_slice(&frames.to_le_bytes());
        let mut array = |offset: usize, class: u32, stride: usize, bytes: &[u8]| {
            write_array(
                &mut data,
                START + offset,
                class,
                bytes.len() / stride,
                bytes,
            )
            .unwrap();
            let header = Payload(data.clone()).pointer(START + offset + 8).unwrap();
            data[header - 4..header].copy_from_slice(&0x80809FB8u32.to_le_bytes());
        };
        array(0x50, 0x8080000F, 4, &scale.to_le_bytes());
        array(0x60, 0x8080000F, 4, &bias.to_le_bytes());
        array(
            0x70,
            0x80800006,
            2,
            &if sparse {
                [1u8, 0, 11, 0]
            } else {
                [255, 255, 0, 0]
            },
        );
        if sparse {
            let keys: Vec<u8> = [0i16, 0, 1, 0, 0, 0, 0, 32767, 16384, -32767, 0]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect();
            array(0x20, 0x80800006, 2, &keys);
            array(0x30, 0x80800009, 1, &[(frames - 1) as u8]);
            array(0x40, 0x80800009, 1, &[0x87; 4]);
        } else {
            let keys: Vec<u8> = (0..frames)
                .flat_map(|f| {
                    [f as i16 * 1000 - 1500, 32767, -32767, 0]
                        .into_iter()
                        .flat_map(i16::to_le_bytes)
                })
                .collect();
            array(0x10, 0x80800006, 2, &keys);
        }
        let size = data.len() as u64;
        data[..8].copy_from_slice(&size.to_le_bytes());
        Payload(data)
    }

    fn decoded(native: &Payload, frame: usize) -> [f32; 4] {
        let start = native.pointer(0x28).unwrap();
        assert_eq!(native.u32(start - 4).unwrap(), 0x80808F77);
        let values = native.array(start + 0x20, 2, Some(0x8080000A)).unwrap();
        let ranges = native.array(start + 0x30, 4, Some(0x8080000F)).unwrap();
        let biases = native.array(start + 0x40, 4, Some(0x8080000F)).unwrap();
        std::array::from_fn(|axis| {
            f32::from(native.u16(values[frame * 4 + axis]).unwrap()) / 65535.
                * native.f32(ranges[axis]).unwrap()
                + native.f32(biases[axis]).unwrap()
        })
    }

    #[test]
    fn dense_affine_tracks_keep_wide_ranges_and_constant_axes() {
        let source = fixture(false, 4, 180., -90.);
        let original = source.0.clone();
        let (native, report) = super::super::convert(&source.0).unwrap();
        let tracks = Tracks::read(&source, START).unwrap();
        for frame in 0..4 {
            let expected = tracks.sample(0, frame as f32).unwrap();
            let actual = decoded(&native, frame);
            for axis in 0..4 {
                assert!((actual[axis] - expected[axis]).abs() < 0.001);
            }
        }
        assert_eq!(source.0, original);
        assert_eq!(native.u64(0).unwrap(), native.0.len() as u64);
        assert_eq!(report["encoded_tracks_preserved"], false);
        let start = native.pointer(0x28).unwrap();
        let indices = native.array(start + 0x10, 2, Some(0x80808F82)).unwrap();
        assert_eq!(native.u16(indices[0]).unwrap(), 0x4000);
    }

    #[test]
    fn sparse_tangents_interpolate_and_excessive_curvature_is_rejected() {
        let source = fixture(true, 9, 1., 3.);
        let tracks = Tracks::read(&source, START).unwrap();
        let midpoint = tracks.sample(0, 4.).unwrap();
        // Hermite at t=0.5, start tangent = delta + 0.3/49, end = delta.
        assert!((midpoint[0] - (3.5 + 0.125 * 0.3 / 49.)).abs() < 0.000001);
        let (native, _) = super::super::convert(&source.0).unwrap();
        assert!((decoded(&native, 4)[0] - midpoint[0]).abs() < 0.00002);
        let mut steep = fixture(true, 2, 1., 3.);
        let tangent = steep.array(START + 0x40, 1, None).unwrap()[0];
        steep.0[tangent..tangent + 4].fill(0xF0);
        assert!(
            format!("{:#}", super::super::convert(&steep.0).err().unwrap())
                .contains("resampling error")
        );
    }

    #[test]
    fn malformed_tracks_fail_without_mutating_the_destination() {
        let source = fixture(true, 9, 1., 3.);
        for (descriptor, bytes) in [
            (0x30, vec![0]),
            (0x70, vec![0, 0]),
            (0x70, vec![0xFF, 0x7F]),
        ] {
            let mut invalid = source.clone();
            let at = invalid
                .array(START + descriptor, bytes.len(), None)
                .unwrap()[0];
            invalid.0[at..at + bytes.len()].copy_from_slice(&bytes);
            let mut native = source.clone();
            assert!(convert(&invalid, &mut native, 0x28, START).is_err());
            assert_eq!(native.0, source.0);
        }
        let mut truncated = fixture(false, 4, 2., 1.);
        truncated.0.pop();
        assert!(Tracks::read(&truncated, START).is_err());
    }
}
