//! File-to-native geometry checks authored before float cloth conversion.
//! The configured gear suite continues through portable recipes and packages.
use super::*;
use crate::d2_mot::{geometry, rig_convert, skinning};
use serde_json::json;
use std::collections::BTreeSet;

const POINTS: [[f32; 3]; 4] = [
    [-0.75, -0.5, 0.25],
    [0.6, -0.4, 0.1],
    [0.5, 0.7, -0.2],
    [-0.5, 0.6, 0.3],
];
const SKIN: [[u8; 8]; 4] = [
    [128, 127, 0, 0, 0, 2, 254, 254],
    [0, 255, 0, 0, 254, 1, 254, 254],
    [40, 80, 60, 75, 0, 1, 2, 0],
    [255, 0, 0, 0, 2, 254, 254, 254],
];

fn put(bytes: &mut [u8], at: usize, data: &[u8]) {
    bytes[at..at + data.len()].copy_from_slice(data);
}

fn array(bytes: &mut [u8], at: usize, header: usize, count: usize, class: u32) {
    put(bytes, at, &(count as u64).to_le_bytes());
    put(
        bytes,
        at + 8,
        &((header as i64) - (at + 8) as i64).to_le_bytes(),
    );
    put(bytes, header, &(count as u64).to_le_bytes());
    put(bytes, header + 8, &class.to_le_bytes());
}

fn stream(data: &[u8], stride: u16, modern: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&stride.to_le_bytes());
    bytes.extend_from_slice(&u16::from(modern).to_le_bytes());
    bytes.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    bytes
}

fn fixture(root: &Path) -> Value {
    let source = root.join("source");
    let native = root.join("native");
    fs::create_dir_all(source.join("raw")).unwrap();
    fs::create_dir_all(native.join("raw")).unwrap();
    let mut model = vec![0; 0x1D0];
    array(&mut model, 16, 0xA0, 1, 0x80806EC5);
    array(&mut model, 0xD0, 0x130, 4, 0x80806ECB);
    for (at, tag) in [(0xB0, 100u32), (0xB4, 102), (0xB8, 104), (0xC0, 106)] {
        put(&mut model, at, &tag.to_le_bytes());
    }
    put(&mut model, 0xC8, &u32::MAX.to_le_bytes());
    for (i, value) in [2f32, 2., 2., 0., 1., -2., 3., 2., 2., 0.5, 0.1, 0.2]
        .into_iter()
        .enumerate()
    {
        put(&mut model, 0x50 + 4 * i, &value.to_le_bytes());
    }
    for stage in 0..25 {
        let count = if stage == 0 {
            0u16
        } else if stage <= 3 {
            2
        } else {
            4
        };
        put(&mut model, 0xE0 + 2 * stage, &count.to_le_bytes());
    }
    model[0x112..0x12A].fill(13);
    for i in 0..4 {
        let at = 0x140 + 36 * i;
        put(&mut model, at, &(i as u32 + 1).to_le_bytes());
        put(&mut model, at + 4, &u16::MAX.to_le_bytes());
        put(&mut model, at + 6, &3u16.to_le_bytes());
        put(&mut model, at + 12, &6u32.to_le_bytes());
        put(&mut model, at + 16, &2u32.to_le_bytes());
        put(&mut model, at + 22, &(i as u16).to_le_bytes());
        put(
            &mut model,
            at + 24,
            &(if i % 2 == 0 { 0x4008u32 } else { 0 }).to_le_bytes(),
        );
        model[at + 28] = 2;
        model[at + 29] = if i % 2 == 0 { 3 } else { 0 };
        model[at + 30] = if i % 2 == 0 { 127 } else { 0 };
        model[at + 31] = 1;
        let mut material = vec![0; 1024];
        put(&mut material, 8, &1u32.to_le_bytes());
        // The fallback pair has the same original pixel program.
        put(&mut material, 0x2B0, &(50u32 + i as u32 / 2).to_le_bytes());
        fs::write(source.join(format!("raw/{:08X}.bin", i + 1)), material).unwrap();
    }
    fs::write(source.join("raw/00000020.bin"), &model).unwrap();
    let positions = POINTS
        .into_iter()
        .flat_map(|p| {
            [p[0], p[1], p[2], 1., 0., 0., 1., 0., 1., 0., 0., -1.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let uvs = [
        [-32767i16, -32767],
        [32767, -32767],
        [32767, 32767],
        [-32767, 32767],
    ]
    .into_iter()
    .flatten()
    .flat_map(i16::to_le_bytes)
    .collect::<Vec<_>>();
    let skin = SKIN.into_iter().flatten().collect::<Vec<_>>();
    let indices = [0u16, 1, 2, 0, 2, 3]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut manifest = json!({"tags":{}});
    for (tag, data, stride) in [
        (100u32, positions, 48),
        (102, uvs, 4),
        (104, skin, 8),
        (106, indices, 0),
    ] {
        let header = if stride == 0 {
            let mut header = vec![0; 24];
            put(&mut header, 8, &(data.len() as u32).to_le_bytes());
            header
        } else {
            stream(&data, stride, true)
        };
        fs::write(source.join(format!("raw/{tag:08X}.bin")), header).unwrap();
        fs::write(source.join(format!("raw/{:08X}.bin", tag + 1)), data).unwrap();
        manifest["tags"][format!("{tag:08X}")] = json!({"reference":tag+1});
    }
    write_json(&source.join("source-manifest.json"), &manifest).unwrap();
    write_json(
        &source.join("report.json"),
        &json!({"item_tag":"00000030","models":[{
            "model":"00000020","mesh_index":0,"has_texture_plates":false,"cloth":true
        }]}),
    )
    .unwrap();

    native_fixture(&native);
    manifest
}

fn native_fixture(native: &Path) {
    let mut target = vec![0; 0x190];
    array(&mut target, 16, 0xA0, 1, 0x80807378);
    array(&mut target, 0xC8, 0x140, 2, 0x8080737E);
    put(&mut target, 0xB0, &80u32.to_le_bytes());
    put(&mut target, 0xB4, &81u32.to_le_bytes());
    for stage in 0..24 {
        let count = if stage == 0 {
            0u16
        } else if stage <= 3 {
            1
        } else {
            2
        };
        put(&mut target, 0xD8 + 2 * stage, &count.to_le_bytes());
        if stage < 23 {
            put(
                &mut target,
                0x108 + 2 * stage,
                &(if stage == 0 || stage == 3 { 139i16 } else { -1 }).to_le_bytes(),
            );
        }
    }
    for i in 0..2 {
        put(&mut target, 0x150 + 32 * i, &90u32.to_le_bytes());
    }
    fs::write(native.join("raw/00000040.bin"), &target).unwrap();
    let buffers = [(80u32, 8u16), (81, 24)].into_iter().map(|(tag, stride)| {
        let bytes = stream(&[], stride, false);
        fs::write(native.join(format!("raw/{tag:08X}.bin")), &bytes).unwrap();
        json!({"offset":if tag==80{0}else{4},"header":format!("{tag:08X}"),"bytes":hex::encode(bytes)})
    }).collect::<Vec<_>>();
    let mut material = vec![0; 1024];
    put(&mut material, 8, &1u32.to_le_bytes());
    put(&mut material, 24, &(1u32 << 25).to_le_bytes());
    for tag in [90u32, 0x80EC270D, 0x80EC271D] {
        fs::write(native.join(format!("raw/{tag:08X}.bin")), &material).unwrap();
    }
    write_json(
        &native.join("template-report.json"),
        &json!({"models":[{
            "model":"00000040","meshes":[{"buffers":buffers}]
        }]}),
    )
    .unwrap();
}

fn check_pose(i: usize, positions: &Payload, weights: &[[u8; 8]], mapping: &[u16]) -> (f32, Value) {
    let mut max_error = 0f32;
    // Distinct bone translations expose dominant-only deformation.
    let translation = [[-3f32, 1., 2.], [2., 4., -1.], [5., -2., 3.]];
    let point = std::array::from_fn::<_, 3, _>(|axis| {
        positions.i16(i * 8 + axis * 2).unwrap() as f32 / 32767.
    });
    for (axis, value) in point.iter().enumerate() {
        max_error = max_error.max((value - POINTS[i][axis]).abs());
    }
    let mut expected = POINTS[i];
    let mut actual = point;
    for lane in 0..4 {
        let weight = SKIN[i][lane] as f32 / 255.;
        if weight != 0. {
            for (axis, value) in expected.iter_mut().enumerate() {
                *value += weight * translation[mapping[SKIN[i][lane + 4] as usize] as usize][axis];
            }
        }
        let decoded = weights[i][lane] as f32 / 255.;
        for (axis, value) in actual.iter_mut().enumerate() {
            *value += decoded * translation[weights[i][lane + 4] as usize][axis];
        }
    }
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.0001)
    );
    assert_eq!(&weights[i][..4], &SKIN[i][..4]);
    (
        max_error,
        json!({"source":POINTS[i],"expected":expected,"actual":actual,"skin":weights[i]}),
    )
}

#[test]
fn float_cloth_keeps_geometry_winding_and_all_skin_influences_through_native_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = fixture(dir.path());
    let source = dir.path().join("source");
    let raw = fs::read(source.join("raw/00000020.bin")).unwrap();
    let model = geometry::model(&source, 0x20).unwrap();
    let streams = geometry::streams(&source, &manifest, &model, 0xB0).unwrap();
    assert_eq!(
        rig_convert::used_bones(&source, &json!("00000030")).unwrap(),
        BTreeSet::from([0, 1, 2])
    );
    let mapping = [2, 0, 1];
    let converted = convert_mapped(
        &source,
        &dir.path().join("native"),
        &dir.path().join("out"),
        0,
        true,
        Some(&mapping),
    )
    .unwrap();
    let positions = Payload(fs::read(dir.path().join("out/positions.bin")).unwrap());
    let weights =
        skinning::native_vertices(&streams.positions, &streams.auxiliary, &mapping).unwrap();
    assert_eq!(weights.len(), 4);
    let mut max_error = 0f32;
    let mut poses = Vec::new();
    for i in 0..4 {
        let (error, pose) = check_pose(i, &positions, &weights, &mapping);
        max_error = max_error.max(error);
        poses.push(pose);
    }
    assert!(max_error <= 1. / 32767.);
    let target = Payload(fs::read(dir.path().join("out/model.unlinked.bin")).unwrap());
    let faces = fs::read(dir.path().join("out/indices.bin")).unwrap();
    let rows = target.array(0xC8, 32, Some(0x8080737E)).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "Simulated and fallback passes were both emitted"
    );
    for row in rows {
        assert_eq!(target.u16(row + 6).unwrap(), 5);
        let start = target.u32(row + 8).unwrap() as usize * 2;
        let count = target.u32(row + 12).unwrap() as usize * 2;
        let values = faces[start..start + count]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(values, [0, 1, 2, 65535, 0, 2, 3, 65535]);
    }
    assert_eq!(fs::read(source.join("raw/00000020.bin")).unwrap(), raw);
    if let Some(path) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        let out = std::path::PathBuf::from(path).join("cloth-geometry");
        fs::create_dir_all(&out).unwrap();
        for file in fs::read_dir(dir.path().join("out")).unwrap() {
            let file = file.unwrap();
            fs::copy(file.path(), out.join(file.file_name())).unwrap();
        }
        write_json(&out.join("readback.json"), &json!({"conversion":converted,"max_normalized_error":max_error,"poses":poses,"source_bytes_preserved":true})).unwrap();
    }
}

#[test]
fn invalid_float_cloth_is_rejected_before_native_files_are_written() {
    for corruption in [
        "weight_sum",
        "nonfinite",
        "unbounded",
        "missing_fallback",
        "incomplete_triangle",
    ] {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let source = dir.path().join("source");
        let (file, offset, value) = match corruption {
            "weight_sum" => ("00000069.bin", 0, vec![127]),
            "nonfinite" => ("00000065.bin", 0, f32::NAN.to_le_bytes().to_vec()),
            "unbounded" => ("00000065.bin", 0, 4f32.to_le_bytes().to_vec()),
            "missing_fallback" => ("00000020.bin", 0x164 + 12, 3u32.to_le_bytes().to_vec()),
            _ => ("00000020.bin", 0x164 + 12, 5u32.to_le_bytes().to_vec()),
        };
        let path = source.join("raw").join(file);
        let mut bytes = fs::read(&path).unwrap();
        put(&mut bytes, offset, &value);
        if corruption == "incomplete_triangle" {
            put(&mut bytes, 0x140 + 12, &5u32.to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
        let output = dir.path().join("out");
        assert!(
            convert_mapped(
                &source,
                &dir.path().join("native"),
                &output,
                0,
                true,
                Some(&[2, 0, 1])
            )
            .is_err(),
            "{corruption} was accepted"
        );
        assert!(
            !output.join("model.unlinked.bin").exists(),
            "{corruption} emitted native geometry"
        );
    }
}
