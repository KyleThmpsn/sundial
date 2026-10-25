//! Persisted curve clips through control sampling and rig extension, with float-stream readback.
use anyhow::{Result, ensure};
use parhelion_import::tiger::{animation::clip, payload::Payload};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

const REST: [f32; 8] = [0., 0., 0., 1., 20., 21., 22., 1.];

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

fn array(bytes: &mut Vec<u8>, at: usize, class: u32, stride: usize, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    // Native arrays keep their allocation marker in the four bytes before a 16-aligned header.
    let header = (bytes.len() + 19) & !15;
    bytes.resize(header + 16, 0);
    put(bytes, header - 4, &0x80809FBDu32.to_le_bytes());
    let count = (data.len() / stride) as u64;
    put(bytes, at, &count.to_le_bytes());
    put(
        bytes,
        at + 8,
        &(header as i64 - at as i64 - 8).to_le_bytes(),
    );
    put(bytes, header, &count.to_le_bytes());
    put(bytes, header + 8, &class.to_le_bytes());
    bytes.extend(data);
}

fn shorts(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn finish(mut bytes: Vec<u8>) -> Payload {
    let length = bytes.len() as u64;
    put(&mut bytes, 0, &length.to_le_bytes());
    Payload(bytes)
}

fn source(compressed_scale: bool, compressed_rotation: bool) -> Payload {
    let mut bytes = vec![0; 0x350];
    put(&mut bytes, 0x13C, &5u16.to_le_bytes());
    for at in [0x13E, 0x140] {
        put(&mut bytes, at, &3u16.to_le_bytes());
    }
    // Opaque retained action identity and timing. These are not decoded as a codec.
    put(&mut bytes, 0x120, &0x12345678u32.to_le_bytes());
    put(&mut bytes, 0x134, &0.125f32.to_le_bytes());
    put(&mut bytes, 0x10, &0x2F0i64.to_le_bytes());
    put(&mut bytes, 0x2FC, &0x80808F73u32.to_le_bytes());
    put(&mut bytes, 0x306, &1u16.to_le_bytes());
    put(&mut bytes, 0x310, &1u32.to_le_bytes());
    array(&mut bytes, 0xC8, 0x8080000A, 2, &shorts(&[2]));
    array(
        &mut bytes,
        0x338,
        0x80800091,
        16,
        &floats(&[5., 6., 7., 0.]),
    );
    let base = 0x200;
    put(&mut bytes, 0x18, &((base - 0x18) as i64).to_le_bytes());
    put(&mut bytes, base - 4, &0x80808F72u32.to_le_bytes());
    put(&mut bytes, base, &shorts(&[1, 1, 1, 1]));
    for (at, slot) in [(0xD8, 2), (0xE8, 0), (0xF8, 1)] {
        array(&mut bytes, at, 0x8080000A, 2, &shorts(&[slot]));
    }
    let mut raw = Vec::<i16>::new();
    let mut keys = Vec::<i16>::new();
    let mut maps = Vec::<i16>::new();
    let mut times = Vec::new();
    let mut tangents = Vec::new();
    for (curve, width, first, last) in [
        (compressed_scale, 1, vec![-32767], vec![32767]),
        (
            compressed_rotation,
            3,
            vec![0, 0, -16384],
            vec![0, 0, -16384],
        ),
        (true, 3, vec![-32767, 0, 32767], vec![32767, 0, -32767]),
    ] {
        if curve {
            maps.push(keys.len() as i16 + 1);
            keys.extend([times.len() as i16, tangents.len() as i16, 1]);
            keys.extend(first);
            keys.extend(last);
            times.push(4);
            tangents.extend(vec![0x77; if maps.len() == 2 { 4 } else { width }]);
            if maps.len() == 3 {
                let middle = tangents.len() - 2;
                tangents[middle] = 0xF0;
            }
        } else {
            maps.push(-(raw.len() as i16) - 1);
            for frame in 0..5 {
                if width == 1 {
                    raw.push([-32767, -16384, 0, 16384, 32767][frame]);
                } else {
                    raw.extend(&first);
                }
            }
        }
    }
    maps.push(keys.len() as i16);
    for (at, data) in [
        (0x10, shorts(&raw)),
        (0x20, shorts(&keys)),
        (0x70, shorts(&maps)),
    ] {
        array(&mut bytes, base + at, 0x80800006, 2, &data);
    }
    for (at, data) in [(0x30, times), (0x40, tangents)] {
        array(&mut bytes, base + at, 0x80800009, 1, &data);
    }
    array(&mut bytes, base + 0x50, 0x8080000F, 4, &floats(&[2., 4.]));
    array(&mut bytes, base + 0x60, 0x8080000F, 4, &floats(&[3., 10.]));
    finish(bytes)
}

fn controls() -> Payload {
    let mut bytes = vec![0; 0x120];
    put(&mut bytes, 24, &40i64.to_le_bytes());
    put(&mut bytes, 60, &0x80808F8Fu32.to_le_bytes());
    let mut group = 0x12345678u32.to_le_bytes().to_vec();
    group.extend(4u32.to_le_bytes());
    array(&mut bytes, 64 + 0x90, 0x8080907F, 8, &group);
    array(
        &mut bytes,
        64 + 0xB0,
        0x80809F75,
        32,
        &floats(&REST.repeat(4)),
    );
    finish(bytes)
}

fn close(actual: f32, expected: f32) -> Result<()> {
    ensure!(
        (actual - expected).abs() < 0.00002,
        "Expected {expected}, got {actual}"
    );
    Ok(())
}

fn within(actual: f32, expected: f32, tolerance: f32) -> Result<()> {
    ensure!(
        (actual - expected).abs() <= tolerance,
        "Expected {expected} within {tolerance}, got {actual}"
    );
    Ok(())
}

// Decode the emitted pose spans directly, without the production codec reader. A track that
// never moves is one float sample in the static span. A moving track is 16-bit codec 2 data,
// ordered by control, frame and component, with one range and bias per component. Each value
// comes back with the precision its storage promises.
fn emitted(bytes: &Payload, kind: usize, slot: u16, frame: usize) -> Result<Vec<(f32, f32)>> {
    let width = [1, 4, 3][kind];
    let fixed = bytes.array(0xA8 + kind * 16, 2, Some(0x8080000A))?;
    if let Some(track) = fixed
        .iter()
        .position(|&at| bytes.u16(at).ok() == Some(slot))
    {
        let base = bytes.pointer(0x10)?;
        ensure!(bytes.u32(base - 4)? == 0x80808F73 && bytes.u32(base + 0x10)? == 1);
        let rows = bytes.array(
            base + 0x18 + kind * 16,
            if kind == 0 { 4 } else { 16 },
            None,
        )?;
        return (0..width)
            .map(|axis| Ok((bytes.f32(rows[track] + axis * 4)?, 0.00002)))
            .collect();
    }
    let maps = bytes.array(0xD8 + kind * 16, 2, Some(0x8080000A))?;
    let track = maps
        .iter()
        .position(|&at| bytes.u16(at).ok() == Some(slot))
        .unwrap();
    let base = bytes.pointer(0x18)?;
    ensure!(bytes.u32(base - 4)? == 0x80808F71 && bytes.u16(base)? == 2);
    let frames = bytes.u32(base + 0x10)? as usize;
    let counts = [
        bytes.u16(base + 2)?,
        bytes.u16(base + 4)?,
        bytes.u16(base + 6)?,
    ];
    let component = (0..kind)
        .map(|k| usize::from(counts[k]) * [1, 4, 3][k])
        .sum::<usize>()
        + track * width;
    let samples = bytes.array(base + 0x18, 2, Some(0x8080000A))?;
    let ranges = bytes.array(base + 0x28, 4, Some(0x8080000F))?;
    let biases = bytes.array(base + 0x38, 4, Some(0x8080000F))?;
    (0..width)
        .map(|axis| {
            let range = bytes.f32(ranges[component + axis])?;
            let bias = bytes.f32(biases[component + axis])?;
            let sample = bytes.u16(samples[component * frames + frame * width + axis])?;
            Ok((
                f32::from(sample) / 65535. * range + bias,
                range / 131070. + 0.00002,
            ))
        })
        .collect()
}

#[test]
fn curve_controls_survive_persisted_extension_and_translation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut receipt = Vec::new();
    for (scales, rotations) in [(false, false), (true, false), (false, true), (true, true)] {
        let name = format!("scale-{scales}-rotation-{rotations}");
        let original = source(scales, rotations);
        let input = directory.path().join(format!("{name}-input.bin"));
        fs::write(&input, &original.0)?;
        let original = Payload(fs::read(&input)?);
        let before = original.0.clone();
        let poses = (0..5)
            .map(|frame| {
                let mut pose = REST;
                pose[4] = frame as f32;
                pose
            })
            .collect::<Vec<_>>();
        let extended = clip::extend(&original, 3, 4, &controls(), Some(&[poses]))?;
        let output = directory.path().join(format!("{name}-extended.bin"));
        fs::write(&output, &extended.0)?;
        let extended = Payload(fs::read(output)?);
        for frame in 0..5 {
            let sampled = clip::sample(&original, &[REST; 3], frame)?;
            let t = frame as f32 / 4.;
            let scale = if scales {
                1. + 4. * t
            } else {
                3. + 2. * f32::from([-32767i16, -16384, 0, 16384, 32767][frame]) / 32767.
            };
            close(sampled[2][7], scale)?;
            let (value, tolerance) = emitted(&extended, 0, 2, frame)?[0];
            within(value, scale, tolerance)?;
            let w = 2. * 16384. / 32767. - 1.;
            let expected_rotation = [0., 0., -(1f32 - w * w).sqrt(), w];
            for (actual, expected) in sampled[0][..4].iter().zip(expected_rotation) {
                close(*actual, expected)?;
            }
            for (&(actual, tolerance), expected) in emitted(&extended, 1, 0, frame)?
                .iter()
                .zip(expected_rotation)
            {
                within(actual, expected, tolerance)?;
            }
            let y = 10.
                + 4. * ((t * t * t - 2. * t * t + t) * (0.3 * 64. / 49.)
                    + (t * t * t - t * t) * -0.3);
            let expected_position = [6. + 8. * t, y, 14. - 8. * t];
            for (actual, expected) in sampled[1][4..7].iter().zip(expected_position) {
                close(*actual, expected)?;
            }
            for (&(actual, tolerance), expected) in emitted(&extended, 2, 1, frame)?
                .iter()
                .zip(expected_position)
            {
                within(actual, expected, tolerance)?;
            }
            ensure!(sampled[0][4..] == REST[4..]);
            ensure!(sampled[2][..4] == REST[..4]);
            ensure!(sampled[2][4..7] == [5., 6., 7.]);
            let (value, tolerance) = emitted(&extended, 2, 3, frame)?[0];
            within(value, frame as f32, tolerance)?;
        }
        let shifted = clip::translate(&original, 1, [0.; 3], [0., 0.1, 0.])?;
        for frame in 0..5 {
            let a = clip::sample(&original, &[REST; 3], frame)?;
            let b = clip::sample(&shifted, &[REST; 3], frame)?;
            ensure!((b[1][5] - a[1][5] - 0.1).abs() < 0.0001);
        }
        ensure!(original.0 == before);
        ensure!(extended.0[0x134..0x13E] == original.0[0x134..0x13E]);
        ensure!(extended.u32(0x120)? == original.u32(0x120)?);
        let fixed = extended.pointer(0x10)?;
        let positions = extended.array(fixed + 0x38, 16, Some(0x80800091))?;
        ensure!(positions.len() == 1);
        for (axis, expected) in [5., 6., 7.].into_iter().enumerate() {
            close(extended.f32(positions[0] + axis * 4)?, expected)?;
        }
        receipt.push(json!({"case":name,"frames":5,"controls":4,
            "input_sha256":hex::encode(Sha256::digest(&original.0)),
            "output_sha256":hex::encode(Sha256::digest(&extended.0))}));
    }
    fs::write(
        directory.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("Animation artifact: {}", directory.keep().display());
    Ok(())
}

#[test]
fn malformed_curve_tables_fail_before_an_extended_clip_is_returned() -> Result<()> {
    let original = source(true, true);
    let base = original.pointer(0x18)?;
    let maps = original.array(base + 0x70, 2, None)?;
    let keys = original.array(base + 0x20, 2, None)?;
    let times = original.array(base + 0x30, 1, None)?;
    let changes = [
        (maps[0], 0i16.to_le_bytes().to_vec()),
        (*maps.last().unwrap(), 0i16.to_le_bytes().to_vec()),
        (keys[0], (-1i16).to_le_bytes().to_vec()),
        (keys[1], i16::MAX.to_le_bytes().to_vec()),
        (keys[2], 0i16.to_le_bytes().to_vec()),
        (maps[0], i16::MIN.to_le_bytes().to_vec()),
        (times[0], vec![0]),
        (times[0], vec![3]),
        (times[0], vec![5]),
    ];
    for (at, value) in changes {
        let mut broken = original.clone();
        put(&mut broken.0, at, &value);
        let before = broken.0.clone();
        ensure!(clip::sample(&broken, &[REST; 3], 0).is_err());
        ensure!(clip::extend(&broken, 3, 4, &controls(), Some(&[vec![REST; 5]])).is_err());
        ensure!(broken.0 == before);
    }
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_CURVE_REFERENCE with independent native samples and fresh PARHELION_CURVE_OUTPUT"]
fn native_curves_match_independent_reference_samples() -> Result<()> {
    use anyhow::Context;
    use serde_json::Value;
    use std::{env, path::PathBuf};
    use sundial::package_authoring::animation_curve::{Channel, Decoder};

    let reference =
        PathBuf::from(env::var_os("PARHELION_CURVE_REFERENCE").context("Reference manifest")?);
    let output = PathBuf::from(env::var_os("PARHELION_CURVE_OUTPUT").context("Fresh output")?);
    ensure!(!output.exists(), "Use a fresh curve output directory");
    let manifest: Value = serde_json::from_slice(&fs::read(&reference)?)?;
    let cases = manifest["cases"]
        .as_array()
        .context("Required curve cases")?;
    ensure!(!cases.is_empty(), "Required curve corpus is empty");
    fs::create_dir_all(&output)?;
    let mut receipt = Vec::new();
    for case in cases {
        let name = case["name"].as_str().context("Curve identity")?;
        let bytes = fs::read(
            reference
                .parent()
                .unwrap()
                .join(case["input"].as_str().context("Input path")?),
        )?;
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == case["sha256"],
            "Source digest differs for {name}"
        );
        let payload = Payload(bytes);
        let base = payload.pointer(0x18)?;
        let counts = [
            payload.u16(base + 2)? as usize,
            payload.u16(base + 4)? as usize,
            payload.u16(base + 6)? as usize,
        ];
        let frames = usize::from(payload.u16(0x13C)?);
        let decoder =
            Decoder::read(&payload.0, base, counts, frames).map_err(anyhow::Error::msg)?;
        let expected = case["tracks"].as_array().context("Expected tracks")?;
        ensure!(expected.len() == counts.iter().sum::<usize>());
        let mut ordinal = 0;
        let mut maximum_error = [0f32; 3];
        let mut samples = Vec::new();
        for (kind, channel) in [Channel::Scale, Channel::Rotation, Channel::Position]
            .into_iter()
            .enumerate()
        {
            for track in 0..counts[kind] {
                let actual = decoder
                    .samples(channel, track)
                    .map_err(anyhow::Error::msg)?;
                let expected = &expected[ordinal];
                ensure!(expected["kind"] == kind);
                let values = expected["values"]
                    .as_array()
                    .context("Independent samples")?;
                ensure!(values.len() == frames);
                for (sample, values) in actual.iter().zip(values) {
                    let values = values.as_array().context("Independent components")?;
                    ensure!(values.len() == [1, 4, 3][kind]);
                    for (a, b) in sample.iter().zip(values) {
                        let b = b.as_f64().context("Independent scalar")? as f32;
                        let error = (a - b).abs();
                        maximum_error[kind] = maximum_error[kind].max(error);
                        ensure!(
                            error <= 0.0001 + b.abs() * 0.000002,
                            "Curve {name} track {ordinal}: {a} differs from reference {b}"
                        );
                    }
                }
                samples.push(json!({"kind":kind,"track":track,"values":actual}));
                ordinal += 1;
            }
        }
        let emitted = serde_json::to_vec(&samples)?;
        fs::write(output.join(format!("{name}.json")), &emitted)?;
        receipt.push(json!({"name":name,"frames":frames,"tracks":counts,
            "input_sha256":case["sha256"],"samples_sha256":hex::encode(Sha256::digest(&emitted)),
            "maximum_error":maximum_error}));
    }
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "source_build":manifest["source_build"],"reference":reference,
            "reference_sha256":hex::encode(Sha256::digest(fs::read(&reference)?)),"cases":receipt,
            "limits":"Stored curve samples only. No gameplay, rendering or runtime selection claim."
        }))?,
    )?;
    Ok(())
}
