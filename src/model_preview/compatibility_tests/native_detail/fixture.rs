//! Native vertex declaration, placement and sparse draw inputs, authored before recovery.
use super::*;
use effects::native::{instruction, register, shader_stage, source};
use fixtures::{Package, array, floats, put};

pub(super) const PLACEMENT: [f32; 4] = [0.5, 0.25, 0.125, 0.375];
pub(super) const UV: [[f32; 2]; 3] = [[0.25, 0.5], [0.75, 0.5], [0.25, 0.25]];
pub(super) const AUX: [[f32; 2]; 3] = [[3.0, 0.5], [-0.5, 1.5], [0.0, 2.0]];

pub(super) fn auxiliary(mode: u8, panel: usize, corner: usize) -> [f32; 2] {
    if mode == 10 && panel == 0 {
        [if matches!(corner, 1 | 2) { 64.0 } else { 0.0 }, 0.5]
    } else {
        AUX[panel]
    }
}

fn declaration(package: &mut Package, mode: u8) {
    let mut elements = vec![0; 0x18];
    let sets = array(&mut elements, 8, 0x8080_72AF, &[0; 32], 16);
    array(
        &mut elements,
        sets,
        0x8080_72B2,
        &[0, 0, 3, 5, 0, 2, 3, 0, 3, 6, 0, 4],
        3,
    );
    array(
        &mut elements,
        sets + 16,
        0x8080_72B2,
        &[5, if mode == 7 { 9 } else { 2 }, 12],
        3,
    );
    let elements = package.add(0x8080_72AD, elements);
    let mut mapping = vec![0; 0x18];
    let row = array(&mut mapping, 8, 0x8080_72AC, &[0; 28], 28);
    mapping[row] = 77;
    for (at, value) in [(8, 0u32), (12, 1), (16, u32::MAX), (20, u32::MAX)] {
        put(&mut mapping, row + at, &value.to_le_bytes());
    }
    let mapping = package.add(0x8080_72A9, mapping);
    let mut root = vec![0; 0x30];
    put(&mut root, 0xC, &elements.to_le_bytes());
    put(&mut root, 0x28, &mapping.to_le_bytes());
    package.add(0x8080_72A6, root);
}

fn shader(package: &mut Package, mode: u8) -> u32 {
    let (uv, aux, temp, output) = if mode == 1 {
        (6, 9, 11, 7)
    } else {
        (1, 2, 5, 3)
    };
    let cb = |swizzle: u32| {
        [
            0x0020_8006 | swizzle << 4,
            11,
            if mode == 4 { 7 } else { 6 },
        ]
    };
    let mut code = instruction(104, &[&[12]]);
    for (slot, rows) in [(11, 24), (12, 14)] {
        code.extend(instruction(89, &[&[0x0020_8000, slot, rows]]));
    }
    for index in [uv, aux] {
        code.extend(instruction(95, &[&register(1, index, 3)]));
    }
    code.extend(instruction(101, &[&register(2, output, 15)]));
    let raw = source(1, uv, 0x44);
    let scale = cb(0x44);
    code.extend(instruction(
        50,
        &[
            &register(0, temp, 3),
            if mode == 2 { &scale } else { &raw },
            if mode == 2 { &raw } else { &scale },
            &cb(0xEE),
        ],
    ));
    code.extend(instruction(
        54,
        &[
            &register(2, output, 3),
            &source(
                if mode == 5 { 1 } else { 0 },
                if mode == 5 { uv } else { temp },
                0x44,
            ),
        ],
    ));
    let primary = source(0, temp, 0x44);
    let auxiliary = source(1, aux, if mode == 3 { 0x11 } else { 0x44 });
    code.extend(instruction(
        56,
        &[
            &register(2, output, 12),
            if mode == 2 { &auxiliary } else { &primary },
            if mode == 2 { &primary } else { &auxiliary },
        ],
    ));
    code.extend(instruction(62, &[]));
    let tag = shader_stage(
        package,
        &code,
        1,
        &[("TEXCOORD", uv), ("TEXCOORD", aux)],
        &[("TEXCOORD", output)],
    );
    let payload = package.reference(tag);
    let bytes = package.payload_mut(payload);
    let inputs = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize + 8;
    put(bytes, inputs + 12, &0u32.to_le_bytes());
    put(
        bytes,
        inputs + 36,
        &(if mode == 6 { 1u32 } else { 2 }).to_le_bytes(),
    );
    let outputs = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize + 8;
    put(bytes, outputs + 12, &3u32.to_le_bytes());
    tag
}

fn image(package: &mut Package, color: [u8; 4], srgb: bool) -> u32 {
    let payload = package.raw(0, 0, 0, color.repeat(4));
    let mut header = vec![0; 40];
    put(
        &mut header,
        4,
        &(if srgb { 29u32 } else { 28 }).to_le_bytes(),
    );
    for (at, value) in [(14, 2u16), (16, 2), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    package.raw(payload, 32, 1, header)
}

pub(super) fn build(mode: u8) -> (tempfile::TempDir, u32) {
    let mut package = Package::default();
    declaration(&mut package, mode);
    let textures = [
        image(&mut package, [137, 137, 137, 255], true),
        image(&mut package, [128, 128, 255, 255], false),
        image(&mut package, [255, 64, 0, 255], false),
    ];
    let mut materials = Vec::new();
    let shader_mode = if mode == 10 { 0 } else { mode };
    for shader_mode in [shader_mode, if mode == 9 { 3 } else { shader_mode }] {
        let vertex = shader(&mut package, if shader_mode == 9 { 0 } else { shader_mode });
        let mut material = vec![0; 0x400];
        put(&mut material, 0x48, &vertex.to_le_bytes());
        put(&mut material, 0x2C8, &u32::MAX.to_le_bytes());
        let bindings: Vec<_> = textures
            .iter()
            .enumerate()
            .flat_map(|(slot, &tag)| [slot as u32, tag])
            .flat_map(u32::to_le_bytes)
            .collect();
        array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
        materials.push(package.add(0x8080_71E8, material));
    }
    let mut vertices = vec![0; 16 * 48];
    let mut auxiliary = vec![0; 16 * 4];
    let mut indices = Vec::new();
    for (panel, uv) in UV.iter().enumerate() {
        for (corner, (x, z)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            let index = panel * 4 + corner + 2;
            floats(
                &mut vertices,
                index * 48,
                &[
                    x + panel as f32 * 1.2,
                    0.0,
                    z,
                    uv[0],
                    uv[1],
                    0.0,
                    -1.0,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                ],
            );
            let bits = if mode == 10 && panel == 0 {
                [if matches!(corner, 1 | 2) { 0x5400 } else { 0 }, 0x3800]
            } else {
                [[0x4200u16, 0x3800], [0xB800, 0x3E00], [0, 0x4000]][panel]
            };
            for (lane, value) in bits.into_iter().enumerate() {
                put(&mut auxiliary, index * 4 + lane * 2, &value.to_le_bytes());
            }
        }
        indices.extend(
            [0u16, 1, 2, 0, 2, 3]
                .map(|i| i + panel as u16 * 4 + 2)
                .into_iter()
                .flat_map(u16::to_le_bytes),
        );
    }
    let vertices = package.vertex(48, vertices);
    let auxiliary = package.vertex(4, if mode == 8 { vec![0; 4] } else { auxiliary });
    let payload = package.raw(0, 0, 0, indices);
    let mut header = vec![0; 16];
    put(&mut header, 8, &36u64.to_le_bytes());
    let indices = package.raw(payload, 32, 6, header);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x50, &[1.0; 3]);
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &PLACEMENT);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    for (at, tag) in [
        (0, vertices),
        (4, auxiliary),
        (8, u32::MAX),
        (12, u32::MAX),
        (16, indices),
    ] {
        put(&mut model, mesh + at, &tag.to_le_bytes());
    }
    for stage in 1..24 {
        put(&mut model, mesh + 0x28 + stage * 2, &3i16.to_le_bytes());
    }
    put(&mut model, mesh + 0x58, &77u16.to_le_bytes());
    let mut parts = [0; 96];
    for panel in 0..3 {
        let row = &mut parts[panel * 32..panel * 32 + 32];
        put(
            row,
            0,
            &materials[usize::from(mode == 9 && panel == 1)].to_le_bytes(),
        );
        put(row, 4, &(-1i16).to_le_bytes());
        put(row, 6, &3u16.to_le_bytes());
        put(row, 8, &(panel as u32 * 6).to_le_bytes());
        put(row, 12, &6u32.to_le_bytes());
    }
    array(&mut model, mesh + 0x18, 0x8080_737E, &parts, 32);
    let model = package.add(MODEL, model);
    let mut component = vec![0; 0x400];
    put(&mut component, 0x10, &0x30i64.to_le_bytes());
    put(&mut component, 0x18, &0x68i64.to_le_bytes());
    put(&mut component, 0x3C, &0x8080_72B8u32.to_le_bytes());
    put(&mut component, 0x7C, &0x8080_72BDu32.to_le_bytes());
    put(&mut component, 0x80 + 0x1DC, &model.to_le_bytes());
    let component = package.add(RESOURCE, component);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    (directory, component)
}
