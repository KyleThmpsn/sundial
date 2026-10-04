use super::super::fixtures::{Package, array, floats, layouts, put};
use super::*;

pub(crate) struct Fixture {
    directory: tempfile::TempDir,
    pub entity: u32,
    pub clips: Vec<(String, u32)>,
    pub invalid: Vec<(String, u32)>,
}
impl Fixture {
    pub fn manager(&self) -> PackageManager {
        PackageManager::new(
            self.directory.path(),
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            Some(tiger_pkg::PackagePlatform::Win64),
        )
        .unwrap()
    }
}
fn words(bytes: &mut Vec<u8>, at: usize, values: &[u16]) -> usize {
    array(
        bytes,
        at,
        0x8080_000A,
        &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        2,
    )
}
fn signed(bytes: &mut Vec<u8>, at: usize, values: &[i16]) -> usize {
    array(
        bytes,
        at,
        0x8080_0006,
        &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        2,
    )
}
fn reals(bytes: &mut Vec<u8>, at: usize, class: u32, values: &[f32], width: usize) -> usize {
    array(
        bytes,
        at,
        class,
        &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        width * 4,
    )
}
fn pointer(bytes: &mut [u8], at: usize, target: usize) {
    put(bytes, at, &((target as i64) - (at as i64)).to_le_bytes());
}
fn resource(class: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0x280];
    pointer(&mut bytes, 0x18, 0x80);
    put(&mut bytes, 0x7C, &class.to_le_bytes());
    bytes
}

fn clip(kind: &str) -> Vec<u8> {
    let fixed_only = kind.starts_with("static");
    let mut bytes = vec![0; 0x380];
    put(
        &mut bytes,
        0x13C,
        &(if kind == "static-single" { 1u16 } else { 3 }).to_le_bytes(),
    );
    put(&mut bytes, 0x13E, &2u16.to_le_bytes());
    put(&mut bytes, 0xA0, &1u32.to_le_bytes());
    put(&mut bytes, 0xA4, &1u32.to_le_bytes());
    for at in [0xA8, 0xB8, 0xC8] {
        words(&mut bytes, at, if fixed_only { &[1] } else { &[] });
    }
    for at in [0xD8, 0xE8, 0xF8] {
        words(&mut bytes, at, if fixed_only { &[] } else { &[1] });
    }
    pointer(&mut bytes, 0x10, 0x200);
    put(&mut bytes, 0x1FC, &0x8080_8F6Fu32.to_le_bytes());
    put(&mut bytes, 0x200, &3u16.to_le_bytes());
    put(&mut bytes, 0x210, &1u32.to_le_bytes());
    floats(&mut bytes, 0x214, &[1.0, 2.0]);
    floats(&mut bytes, 0x228, &[1.0, 0.0, 0.0]);
    if fixed_only {
        for at in [0x202, 0x204, 0x206] {
            put(&mut bytes, at, &1u16.to_le_bytes());
        }
        words(&mut bytes, 0x238, &[0, 32768, 32768, 32768, 65535, 0, 0, 0]);
        return bytes;
    }
    words(&mut bytes, 0x238, &[]);
    let d = 0x280;
    pointer(&mut bytes, 0x18, d);
    let (class, id) = match kind {
        "interval" => (0x8080_8F71u32, 2u16),
        "quantized" => (0x8080_8F6F, 3),
        "float" => (0x8080_8F73, 0),
        "curve" => (0x8080_8F72, 1),
        _ => unreachable!(),
    };
    put(&mut bytes, d - 4, &class.to_le_bytes());
    put(&mut bytes, d, &id.to_le_bytes());
    for at in [2, 4, 6] {
        put(&mut bytes, d + at, &1u16.to_le_bytes());
    }
    let q = std::f32::consts::FRAC_1_SQRT_2;
    let rotations = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, q, q, 0.0, 0.0, 1.0, 0.0];
    let packed = |v: f32| ((v + 1.0) * 32767.5).round() as u16;
    if kind == "curve" {
        floats(&mut bytes, d + 8, &[2.0, 2.0]);
        signed(&mut bytes, d + 0x10, &[]);
        // One segment over two frame intervals. Stored quaternion keys use axis/angle packing.
        signed(
            &mut bytes,
            d + 0x20,
            &[
                0, 0, 1, 0, 32767, 0, 1, 1, 0, 0, 32767, 0, 0, 16384, 0, 5, 1, 0, 0, 0, 0, 32767, 0,
            ],
        );
        array(&mut bytes, d + 0x30, 0x8080_0009, &[2], 1);
        array(
            &mut bytes,
            d + 0x40,
            0x8080_0009,
            &[0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0xE0, 0x77],
            1,
        );
        reals(&mut bytes, d + 0x50, 0x8080_000F, &[2.0, 2.0], 1);
        reals(&mut bytes, d + 0x60, 0x8080_000F, &[2.0, 0.0], 1);
        signed(&mut bytes, d + 0x70, &[1, 6, 15, 0]);
        // Position x is a normalized half, transformed by the scalar range.
        let (_, rows) = super::super::super::array(&bytes, d + 0x20, 0x8080_0006, 2, 100).unwrap();
        for i in [17, 20] {
            put(&mut bytes, rows + i * 2, &16384i16.to_le_bytes());
        }
    } else {
        put(&mut bytes, d + 0x10, &3u32.to_le_bytes());
        if kind == "float" {
            reals(&mut bytes, d + 0x18, 0x8080_000F, &[2.0, 3.0, 4.0], 1);
            reals(&mut bytes, d + 0x28, 0x8080_0096, &rotations, 4);
            reals(
                &mut bytes,
                d + 0x38,
                0x8080_0091,
                &[1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0],
                4,
            );
        } else {
            let mut samples = vec![0, 32768, 65535];
            samples.extend(rotations.into_iter().map(packed));
            samples.extend([0, 0, 0, 0, 32768, 0, 0, 65535, 0]);
            if kind == "quantized" {
                floats(
                    &mut bytes,
                    d + 0x14,
                    &[2.0, 2.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.0],
                );
                words(&mut bytes, d + 0x38, &samples);
            } else {
                words(&mut bytes, d + 0x18, &samples);
                reals(
                    &mut bytes,
                    d + 0x28,
                    0x8080_000F,
                    &[2.0, 2.0, 2.0, 2.0, 2.0, 0.0, 2.0, 0.0],
                    1,
                );
                reals(
                    &mut bytes,
                    d + 0x38,
                    0x8080_000F,
                    &[2.0, -1.0, -1.0, -1.0, -1.0, 1.0, 0.0, 0.0],
                    1,
                );
            }
        }
    }
    bytes
}

fn traveling_clip() -> Vec<u8> {
    let mut bytes = clip("float");
    for (at, bones) in [(0xA8, [0]), (0xB8, [0]), (0xC8, [1]), (0xF8, [0])] {
        words(&mut bytes, at, &bones);
    }
    for at in [0x202, 0x204, 0x206] {
        put(&mut bytes, at, &1u16.to_le_bytes());
    }
    floats(&mut bytes, 0x218, &[1.0]);
    words(&mut bytes, 0x238, &[0, 32768, 32768, 32768, 65535, 0, 0, 0]);
    reals(
        &mut bytes,
        0x2B8,
        0x8080_0091,
        &[
            0.0, 0.0, 0.0, 0.0, 20.0, 30.0, 40.0, 0.0, 40.0, 60.0, 80.0, 0.0,
        ],
        4,
    );
    bytes
}

fn turning_clip() -> Vec<u8> {
    let mut bytes = clip("float");
    for at in [0xA8, 0xB8, 0xC8] {
        words(&mut bytes, at, &[1]);
    }
    for at in [0xD8, 0xE8, 0xF8] {
        words(&mut bytes, at, &[0]);
    }
    for at in [0x202, 0x204, 0x206] {
        put(&mut bytes, at, &1u16.to_le_bytes());
    }
    words(&mut bytes, 0x238, &[0, 32768, 32768, 32768, 65535, 0, 0, 0]);
    reals(&mut bytes, 0x298, 0x8080_000F, &[1.0; 3], 1);
    let q = std::f32::consts::FRAC_1_SQRT_2;
    reals(
        &mut bytes,
        0x2A8,
        0x8080_0096,
        &[0.0, 0.0, 0.0, 1.0, 0.0, q, 0.0, q, 0.0, 1.0, 0.0, 0.0],
        4,
    );
    reals(
        &mut bytes,
        0x2B8,
        0x8080_0091,
        &[
            0.0, 0.0, 0.0, 0.0, 20.0, 30.0, 40.0, 0.0, 40.0, 60.0, 80.0, 0.0,
        ],
        4,
    );
    bytes
}

pub(crate) fn build() -> Fixture {
    let mut package = Package::default();
    layouts(&mut package);
    let mut vertices = Vec::new();
    for [x, y, z] in [[1.0f32, 0.0, 0.0], [3.0, 0.0, 0.0], [1.0, 0.0, 2.0]] {
        vertices.extend([x, y, z].into_iter().flat_map(f32::to_le_bytes));
        vertices.extend([1, 0, 255, 0]);
        vertices.extend(
            [0.0f32, -0.6, 0.8, 0.5, 0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
    }
    let vertex = package.vertex(36, vertices);
    let index = package.indices(false);
    let mut textures = Vec::new();
    for (depth, length) in [(6u16, 40usize), (1, 4), (1, 40)] {
        let pixels = package.raw(0, 0, 0, vec![64, 192, 96, 255]);
        let mut header = vec![0; length];
        if length >= 40 {
            put(&mut header, 4, &29u32.to_le_bytes());
            for (at, value) in [(0xE, 1u16), (0x10, 1), (0x12, depth), (0x14, 1)] {
                put(&mut header, at, &value.to_le_bytes());
            }
        }
        textures.push(package.raw(pixels, 32, 1, header));
    }
    let mut material = vec![0; 0x300];
    let bindings: Vec<_> = textures
        .into_iter()
        .enumerate()
        .flat_map(|(i, tag)| [i as u32, tag])
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let material = package.add(0x8080_71E8, material);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x50, &[1.0, 1.0, 1.0]);
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    put(&mut model, mesh, &vertex.to_le_bytes());
    put(&mut model, mesh + 0x10, &index.to_le_bytes());
    for stage in 1..24 {
        put(&mut model, mesh + 0x28 + stage * 2, &1i16.to_le_bytes());
    }
    put(&mut model, mesh + 0x58, &20u16.to_le_bytes());
    let mut part = [0; 0x20];
    put(&mut part, 0, &material.to_le_bytes());
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    part[0x1A] = 255;
    array(&mut model, mesh + 0x18, 0x8080_737E, &part, 0x20);
    let model = package.add(MODEL, model);
    let mut render = resource(0x8080_72BD);
    pointer(&mut render, 0x10, 0x40);
    put(&mut render, 0x3C, &0x8080_72B8u32.to_le_bytes());
    put(&mut render, 0x80 + 0x1DC, &model.to_le_bytes());
    let render = package.add(RESOURCE, render);
    let mut skeleton = resource(0x8080_8546);
    let hierarchy: Vec<_> = [0i32, -1, 1, -1, 1, 0, -1, -1]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    array(&mut skeleton, 0x100, 0x8080_8A08, &hierarchy, 16);
    reals(
        &mut skeleton,
        0x110,
        0x8080_9F75,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0,
        ],
        8,
    );
    reals(
        &mut skeleton,
        0x120,
        0x8080_9F75,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, -0.5, 0.0, 0.0, 0.5,
        ],
        8,
    );
    let skeleton = package.add(RESOURCE, skeleton);
    let mut clips = Vec::new();
    for name in [
        "static",
        "static-single",
        "interval",
        "quantized",
        "float",
        "curve",
        "root-motion",
        "root-turn",
    ] {
        let mut data = match name {
            "root-motion" => traveling_clip(),
            "root-turn" => turning_clip(),
            _ => clip(name),
        };
        put(
            &mut data,
            0x120,
            &(if name == "static" {
                0x6FB7_60FFu32
            } else {
                clips.len() as u32
            })
            .to_le_bytes(),
        );
        clips.push((name.into(), package.add(0x8080_8F49, data)));
    }
    let mut invalid = Vec::new();
    for name in [
        "duplicate-map",
        "overlapping-map",
        "codec-count",
        "curve-time",
        "curve-sample",
        "nonfinite",
        "truncated",
    ] {
        let mut data = clip(if name.starts_with("curve") {
            "curve"
        } else {
            "float"
        });
        match name {
            "duplicate-map" => {
                words(&mut data, 0xE8, &[1, 1]);
            }
            "overlapping-map" => {
                words(&mut data, 0xB8, &[1]);
            }
            "codec-count" => {
                put(&mut data, 0x284, &2u16.to_le_bytes());
            }
            "curve-time" => {
                array(&mut data, 0x2B0, 0x8080_0009, &[0], 1);
            }
            "curve-sample" => {
                signed(&mut data, 0x2F0, &[1, 6, 32767, 0]);
            }
            "nonfinite" => {
                reals(&mut data, 0x298, 0x8080_000F, &[2.0, f32::NAN, 4.0], 1);
            }
            "truncated" => {
                data.truncate(0x170);
            }
            _ => unreachable!(),
        }
        put(
            &mut data,
            0x120,
            &(100 + invalid.len() as u32).to_le_bytes(),
        );
        invalid.push((name.into(), package.add(0x8080_8F49, data)));
    }
    let mut bank = vec![0; 0x18];
    let mut tags = 0x8080_3FFEu32.to_le_bytes().to_vec();
    tags.extend(
        clips
            .iter()
            .chain(&invalid)
            .flat_map(|(_, tag)| tag.to_le_bytes()),
    );
    array(&mut bank, 8, 0x8080_8F48, &tags, 4);
    let bank = package.add(0x8080_36F6, bank);
    let mut definition = resource(0x8080_344B);
    put(&mut definition, 0x110, &bank.to_le_bytes());
    let definition = package.add(RESOURCE, definition);
    let mut entity = vec![0; 0x20];
    let rows: Vec<_> = [render, skeleton, definition]
        .into_iter()
        .flat_map(|tag| [tag, 0, 0])
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut entity, 0x10, 0x8080_9C04, &rows, 12);
    let entity = package.add(ENTITY, entity);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    Fixture {
        directory,
        entity,
        clips,
        invalid,
    }
}
