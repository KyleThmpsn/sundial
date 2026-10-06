//! Declared mip images and reciprocal native samplers, prepared before sampling changes.
use super::*;
use effects::native::paint::emit;
use effects::native::{instruction, register, shader_stage, source};
use fixtures::{Package, array, floats, put};

pub(super) const EDGE: usize = 512;
pub(super) const LEVELS: usize = 10;

pub(super) fn pixel(slot: usize, level: usize, hdr: bool) -> [f32; 4] {
    if hdr && slot == 0 {
        return [if level % 2 == 0 { 2.0 } else { 0.5 }, 0.25, 1.0, 1.0];
    }
    let value = match slot {
        0 => [40 + level as u8 * 17, 176 - level as u8 * 11, 96, 255],
        1 => [
            96 + level as u8 * 9,
            168 - level as u8 * 7,
            48 + level as u8 * 19,
            255,
        ],
        2 => [255, 220, 0, 255],
        3 => [88 + level as u8 * 11, 160 - level as u8 * 9, 136, 255],
        4 => [
            168 - level as u8 * 7,
            112 + level as u8 * 9,
            200 - level as u8 * 13,
            255,
        ],
        _ => unreachable!(),
    };
    value.map(|v| f32::from(v) / 255.0)
}

fn image(package: &mut Package, slot: usize, case: u8) -> u32 {
    let hdr = case == 9 && slot == 0;
    let mut levels = Vec::new();
    let count = if case == 10 { 1 } else { LEVELS };
    for level in 0..count {
        let pixel = pixel(slot, level, hdr);
        let bytes: Vec<_> = if hdr {
            pixel
                .into_iter()
                .flat_map(|v| {
                    let bits: u16 = if v == 2.0 {
                        0x4000
                    } else if v == 0.5 {
                        0x3800
                    } else if v == 0.25 {
                        0x3400
                    } else {
                        0x3C00
                    };
                    bits.to_le_bytes()
                })
                .collect()
        } else {
            pixel.map(|v| (v * 255.0).round() as u8).to_vec()
        };
        levels.push(bytes.repeat((EDGE >> level).pow(2)));
    }
    let mut header = vec![0; 40];
    put(
        &mut header,
        4,
        &(if hdr {
            10u32
        } else if matches!(slot, 0 | 3) {
            29
        } else {
            28
        })
        .to_le_bytes(),
    );
    for (at, value) in [(14, EDGE as u16), (16, EDGE as u16), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    header[23] = count as u8;
    let (large, tail) = if case == 8 {
        (
            Some(package.raw(0, 0, 0, levels[..2].concat())),
            levels[2..].concat(),
        )
    } else {
        (None, levels.concat())
    };
    let payload = package.raw(0, 0, 0, tail);
    put(&mut header, 36, &large.unwrap_or(u32::MAX).to_le_bytes());
    package.raw(payload, 32, 1, header)
}

pub(super) fn settings(case: u8, index: usize) -> (u32, f32, [f32; 2]) {
    let filter = match case {
        1 if index >= 2 => 0x55,
        3 => 0x14,
        5 => 0,
        _ => 0x15,
    };
    let bias = if case == 2 {
        0.75 - index as f32 * 0.25
    } else {
        -0.5
    };
    let range = if case == 4 {
        [1.0, 1.0]
    } else {
        [0.0, f32::MAX]
    };
    (filter, bias, range)
}

fn samplers(package: &mut Package, material: &mut Vec<u8>, case: u8) {
    let mut rows = Vec::new();
    for index in 0..5 {
        let (filter, bias, range) = settings(case, index);
        let mut descriptor = vec![0; 52];
        let address = if case == 10 { 4 } else { 1 };
        for (at, value) in [
            (0, filter),
            (4, address),
            (8, address),
            (12, 1),
            (20, 4),
            (24, 1),
        ] {
            put(&mut descriptor, at, &value.to_le_bytes());
        }
        floats(&mut descriptor, 16, &[bias]);
        floats(&mut descriptor, 44, &range);
        let data = package.raw(0, 42, 1, descriptor);
        let header = package.raw(data, 34, 1, vec![0; 8]);
        package.set_reference(data, header);
        rows.extend(header.to_le_bytes());
        rows.extend([0; 12]);
    }
    array(material, 0x308, 0x8080_73F3, &rows, 16);
}

fn pixel_shader(package: &mut Package, case: u8) -> u32 {
    if case == 10 {
        return shader_stage(
            package,
            &legacy_normal::fixture::code(7, 0),
            0,
            &[
                ("TEXCOORD", 0),
                ("TEXCOORD", 1),
                ("TEXCOORD", 2),
                ("TEXCOORD", 3),
            ],
            &[("SV_TARGET", 1)],
        );
    }
    let mut prefix = Vec::new();
    for slot in [0, 3] {
        prefix.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
    for sampler in [1, 3] {
        prefix.extend(instruction(90, &[&[0x0010_6000, sampler]]));
    }
    let mut body = Vec::new();
    emit(
        &mut body,
        50,
        &[
            register(0, 0, 3).to_vec(),
            source(1, 3, 0xEE).to_vec(),
            vec![0x0020_8446, 7, 0],
            vec![0x0020_8EE6, 7, 0],
        ],
    );
    for (slot, sampler, coordinates) in [(0, 3, source(1, 3, 0x44)), (3, 1, source(0, 0, 0x44))] {
        emit(
            &mut body,
            69,
            &[
                register(0, 1, 15).to_vec(),
                coordinates.to_vec(),
                source(7, slot, 0xE4).to_vec(),
                vec![0x0010_6000, sampler],
            ],
        );
    }
    let mut program = legacy_normal::fixture::code(7, 0);
    let mut executable = 0;
    while matches!(program[executable] & 0x7FF, 88..=90 | 95..=106) {
        executable += ((program[executable] >> 24) & 0x7F) as usize;
    }
    program.splice(executable..executable, body);
    prefix.extend(program);
    shader_stage(
        package,
        &prefix,
        0,
        &[
            ("TEXCOORD", 0),
            ("TEXCOORD", 1),
            ("TEXCOORD", 2),
            ("TEXCOORD", 3),
        ],
        &[("SV_TARGET", 1)],
    )
}

pub(super) fn build(case: u8) -> (tempfile::TempDir, u32, [u32; 2]) {
    let mut details = [0; 2];
    let (directory, tag) =
        plates::fixture_material([0, 0, 8, 8], [8, 8], false, 0, |package, _| {
            let shader = pixel_shader(package, case);
            let mut material = vec![0; 0x400];
            put(&mut material, 0x2C8, &shader.to_le_bytes());
            let images: Vec<_> = (0..5).map(|slot| image(package, slot, case)).collect();
            details.copy_from_slice(&images[3..5]);
            let bindings: Vec<_> = images
                .into_iter()
                .enumerate()
                .flat_map(|(slot, tag)| [slot as u32, tag])
                .flat_map(u32::to_le_bytes)
                .collect();
            array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
            samplers(package, &mut material, case);
            let mut constants = vec![0; 24 * 16];
            floats(&mut constants, 23 * 16, &[255.0 / 128.0, -1.0, 0.4, 0.0]);
            array(&mut material, 0x318, 0x8080_0090, &constants, 16);
            package.add(0x8080_71E8, material)
        });
    (directory, tag, details)
}
