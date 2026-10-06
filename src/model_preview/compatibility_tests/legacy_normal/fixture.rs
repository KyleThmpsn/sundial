//! A renamed native 27-vector consumer in a temporary package.
use super::*;
use effects::native::paint::{emit, minus};
use effects::native::{instruction, literal, register, shader_stage, source};
use fixtures::{array, floats, put};

fn cb(slot: u32, row: u32, swizzle: u32) -> Vec<u32> {
    vec![0x0020_8006 | swizzle << 4, slot, row]
}
fn r(index: u32, mask: u32) -> Vec<u32> {
    register(0, index, mask).to_vec()
}
fn s(index: u32, swizzle: u32) -> Vec<u32> {
    source(0, index, swizzle).to_vec()
}
fn l(value: f32) -> Vec<u32> {
    literal(value).to_vec()
}
fn selection(code: &mut Vec<u32>, bank: u32, target: u32, row: u32, sat: bool) {
    let (a, b) = if sat {
        emit(code, 54 | 1 << 13, &[r(22, 15), cb(bank, row, 0xE4)]);
        emit(code, 54 | 1 << 13, &[r(23, 15), cb(bank, row + 4, 0xE4)]);
        (s(22, 0xE4), s(23, 0xE4))
    } else {
        (cb(bank, row, 0xE4), cb(bank, row + 4, 0xE4))
    };
    emit(code, 0, &[r(target, 15), b, minus(a.clone())]);
    emit(code, 50, &[r(target, 15), cb(0, 4, 0), s(target, 0xE4), a]);
}

pub(in crate::model_preview::compatibility_tests) fn code(bank: u32, malformed: u8) -> Vec<u32> {
    let mut code = instruction(104, &[&[24]]);
    for (slot, size) in [(0, 24), (bank, 27)] {
        code.extend(instruction(89, &[&[0x0020_8000, slot, size]]));
    }
    for slot in [1, 2, 4 + (7 - bank) * 2] {
        code.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
    for sampler in [2, 4, 5] {
        code.extend(instruction(90, &[&[0x0010_6000, sampler]]));
    }
    for input in 0..4 {
        code.extend(instruction(98, &[&register(1, input, 15)]));
    }
    code.extend(instruction(101, &[&register(2, 1, 15)]));
    let uv = source(1, 3, 0x44).to_vec();
    for (temp, slot, sampler) in [(17, 2, 5), (11, 1, 4)] {
        emit(
            &mut code,
            69,
            &[
                r(temp, 7 | if temp == 17 { 8 } else { 0 }),
                uv.clone(),
                source(7, slot, 0xE4).to_vec(),
                vec![0x0010_6000, sampler],
            ],
        );
    }
    emit(&mut code, 29, &[r(18, 1), s(17, 0xFF), l(40.0 / 255.0)]);
    emit(&mut code, 1, &[r(18, 1), s(18, 0), l(1.0)]);
    emit(&mut code, 0, &[r(19, 1), s(17, 0xFF), l(-48.0 / 255.0)]);
    emit(
        &mut code,
        56 | 1 << 13,
        &[r(19, 1), s(19, 0), l(255.0 / 207.0)],
    );
    selection(&mut code, bank, 5, 18, false);
    emit(
        &mut code,
        50 | 1 << 13,
        &[r(19, 1), s(5, 0x55), s(19, 0), s(5, 0)],
    );
    emit(
        &mut code,
        50 | 1 << 13,
        &[r(19, 1), s(5, 0xFF), s(19, 0), s(5, 0xAA)],
    );
    selection(&mut code, bank, 6, 10, malformed == 1);
    selection(&mut code, bank, 7, 20, true);
    emit(
        &mut code,
        50,
        &[
            r(10, 3),
            source(1, 3, if malformed == 3 { 0x44 } else { 0xEE }).to_vec(),
            cb(bank, 1, 0x44),
            cb(bank, 1, 0xEE),
        ],
    );
    emit(
        &mut code,
        69,
        &[
            r(10, 7),
            s(10, 0x44),
            source(
                7,
                if malformed == 2 {
                    2
                } else {
                    4 + (7 - bank) * 2
                },
                0xE4,
            )
            .to_vec(),
            vec![0x0010_6000, 2],
        ],
    );
    emit(
        &mut code,
        1 << 13,
        &[r(12, 1), s(10, 0xAA), cb(bank, 2, 0xAA)],
    );
    emit(&mut code, 0, &[r(12, 1), s(12, 0), l(-1.0)]);
    emit(&mut code, 50, &[r(13, 1), s(6, 0x55), s(12, 0), l(1.0)]);
    emit(&mut code, 50, &[r(12, 1), s(7, 0x55), s(12, 0), l(1.0)]);
    emit(&mut code, 0, &[r(13, 1), s(13, 0), minus(s(12, 0))]);
    emit(&mut code, 50, &[r(13, 1), s(19, 0), s(13, 0), s(12, 0)]);
    emit(
        &mut code,
        1 << 13,
        &[r(14, 1), s(11, 0xAA), cb(0, 23, 0xAA)],
    );
    emit(&mut code, 51, &[r(15, 1), s(14, 0), s(13, 0)]);
    emit(&mut code, 51, &[r(15, 1), s(15, 0), s(17, 0x55)]);
    emit(
        &mut code,
        50,
        &[
            r(15, 1),
            if malformed == 4 { l(1.0) } else { s(15, 0) },
            l(0.125),
            l(0.375),
        ],
    );
    emit(
        &mut code,
        50,
        &[r(10, 3), s(10, 0x44), cb(bank, 2, 0), cb(bank, 2, 0x55)],
    );
    if malformed == 7 {
        emit(&mut code, 54, &[r(11, 3), s(11, 0x44)]);
    } else {
        emit(
            &mut code,
            50,
            &[r(11, 3), s(11, 0x44), cb(0, 23, 0), cb(0, 23, 0x55)],
        );
    }
    emit(&mut code, 56, &[r(8, 3), s(7, 0x55), s(10, 0x44)]);
    if malformed == 6 {
        emit(&mut code, 54, &[r(9, 3), s(10, 0x44)]);
    } else {
        emit(
            &mut code,
            50,
            &[r(9, 3), s(10, 0x44), s(6, 0x55), minus(s(8, 0x44))],
        );
    }
    if malformed == 5 {
        emit(&mut code, 0, &[r(9, 3), s(9, 0x44), s(8, 0x44)]);
    } else {
        emit(&mut code, 50, &[r(9, 3), s(19, 0), s(9, 0x44), s(8, 0x44)]);
    }
    emit(
        &mut code,
        50,
        &[r(16, 3), s(18, 0), s(9, 0x44), s(11, 0x44)],
    );
    if malformed == 8 {
        emit(&mut code, 54, &[r(21, 1), s(16, 0)]);
    } else {
        emit(&mut code, 15, &[r(21, 1), s(16, 0x44), s(16, 0x44)]);
    }
    emit(&mut code, 0, &[r(21, 1), minus(s(21, 0)), l(1.0)]);
    emit(&mut code, 52, &[r(21, 1), s(21, 0), l(0.0)]);
    emit(&mut code, 75, &[r(16, 4), s(21, 0)]);
    emit(&mut code, 16, &[r(21, 1), s(16, 0xE4), s(16, 0xE4)]);
    emit(&mut code, 68, &[r(21, 1), s(21, 0)]);
    emit(&mut code, 56, &[r(16, 7), s(21, 0), s(16, 0xE4)]);
    emit(
        &mut code,
        56,
        &[r(20, 7), s(16, 0x55), source(1, 2, 0xE4).to_vec()],
    );
    emit(
        &mut code,
        50,
        &[r(20, 7), s(16, 0), source(1, 1, 0xE4).to_vec(), s(20, 0xE4)],
    );
    emit(
        &mut code,
        50,
        &[
            r(20, 7),
            s(16, 0xAA),
            source(1, 0, 0xE4).to_vec(),
            s(20, 0xE4),
        ],
    );
    emit(&mut code, 16, &[r(21, 1), s(20, 0xE4), s(20, 0xE4)]);
    emit(&mut code, 68, &[r(21, 1), s(21, 0)]);
    emit(&mut code, 56, &[r(20, 7), s(21, 0), s(20, 0xE4)]);
    emit(
        &mut code,
        50 | 1 << 13,
        &[register(2, 1, 7).to_vec(), s(20, 0xE4), s(15, 0), l(0.5)],
    );
    code.extend(instruction(62, &[]));
    code
}

pub(super) fn load(slot: u8, malformed: u8) -> Model {
    let bank = if slot / 2 == 1 { 6 } else { 7 };
    let (directory, tag) =
        plates::fixture_material([0, 0, 8, 8], [8, 8], false, slot, |package, _| {
            let shader = shader_stage(
                package,
                &code(bank, malformed),
                0,
                &[
                    ("TEXCOORD", 0),
                    ("TEXCOORD", 1),
                    ("TEXCOORD", 2),
                    ("TEXCOORD", 3),
                ],
                &[("SV_TARGET", 1)],
            );
            let mut material = vec![0; 0x400];
            put(&mut material, 0x2C8, &shader.to_le_bytes());
            let mut constants = vec![0; 24 * 16];
            floats(
                &mut constants,
                4 * 16,
                &[f32::from(slot % 2), 0.0, 0.0, 0.0],
            );
            floats(&mut constants, 23 * 16, &[255.0 / 128.0, -1.0, 0.4, 0.0]);
            array(&mut material, 0x318, 0x8080_0090, &constants, 16);
            // An animated blue offset distinguishes evaluated frame grouping and rewind.
            array(
                &mut material,
                0x2E8,
                0x8080_0009,
                &[0x3C, 1, 0, 0x34, 2, 0x03, 0x42, 23, 0x01, 0x43, 23],
                1,
            );
            let mut values = vec![0; 48];
            floats(&mut values, 0, &[0.2; 4]);
            floats(&mut values, 16, &[1.0; 4]);
            floats(&mut values, 32, &[0.0, 0.0, 0.2, 0.0]);
            array(&mut material, 0x2F8, 0x8080_0090, &values, 16);
            package.add(0x8080_71E8, material)
        });
    fixtures::load(directory.path(), tag).unwrap()
}
