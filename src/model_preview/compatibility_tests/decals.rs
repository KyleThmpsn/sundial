//! Native gbuffer coverage is a shader contract, independent of the part's stage.
use super::*;
use effects::native::{instruction, literal, register, shader_stage, source};
use fixtures::{Package, array, floats, put};

fn image(package: &mut Package, rows: &[[u8; 4]]) -> u32 {
    let payload = package.raw(0, 0, 0, rows.iter().flatten().copied().collect());
    let mut header = vec![0; 0x28];
    put(&mut header, 4, &28u32.to_le_bytes());
    for (at, value) in [(14, 1u16), (16, rows.len() as u16), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    package.raw(payload, 32, 1, header)
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(crate) fn case(cutoff: f32) -> Model {
    case_at(cutoff, 0)
}

fn case_at(cutoff: f32, stage: usize) -> Model {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    let color = image(
        &mut package,
        &[[16, 48, 240, 255], [220, 208, 176, 255], [12, 12, 12, 255]],
    );
    let normal = image(&mut package, &[[128, 128, 255, 255]; 3]);
    let mask = image(&mut package, &[[0, 0, 0, 0], [0, 0, 16, 0], [0, 0, 32, 0]]);
    let mut code = Vec::new();
    code.extend(instruction(104, &[&[2]]));
    code.extend(instruction(89, &[&[0x0020_8000, 0, 4]]));
    code.extend(instruction(88 | 3 << 11, &[&[0x0010_7000, 2], &[0x5555]]));
    code.extend(instruction(90, &[&[0x0010_6000, 0]]));
    code.extend(instruction(
        69,
        &[
            &register(0, 0, 15),
            &source(1, 3, 0x44),
            &source(7, 2, 0xE4),
            &source(6, 0, 0xE4),
        ],
    ));
    code.extend(instruction(
        56 | 1 << 13,
        &[&register(0, 0, 4), &source(0, 0, 0xAA), &literal(7.96875)],
    ));
    // The native non-dithered branch loads its material-specific cutoff.
    code.extend(instruction(
        57,
        &[&register(0, 1, 1), &literal(0.0), &[0x0020_8006, 0, 3]],
    ));
    code.extend(instruction(31 | 1 << 18, &[&source(0, 1, 0)]));
    code.extend(instruction(54, &[&register(0, 1, 1), &literal(0.9)]));
    code.extend(instruction(18, &[]));
    code.extend(instruction(54, &[&register(0, 1, 1), &[0x0020_8006, 0, 2]]));
    code.extend(instruction(21, &[]));
    let negative = [0x8010_0006, 0x41, 1];
    code.extend(instruction(
        0,
        &[&register(0, 1, 1), &negative, &source(0, 0, 0xAA)],
    ));
    code.extend(instruction(
        49,
        &[&register(0, 1, 1), &source(0, 1, 0), &literal(0.0)],
    ));
    code.extend(instruction(13 | 1 << 18, &[&source(0, 1, 0)]));
    for output in 0..3 {
        code.extend(instruction(54, &[&register(2, output, 15), &literal(1.0)]));
    }
    code.extend(instruction(62, &[]));
    let pixel = shader_stage(
        &mut package,
        &code,
        0,
        &[("TEXCOORD", 3)],
        &[("SV_TARGET", 0), ("SV_TARGET", 1), ("SV_TARGET", 2)],
    );
    let mut material = vec![0; 0x400];
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let bindings: Vec<_> = [(0u32, color), (1, normal), (2, mask)]
        .into_iter()
        .flat_map(|(slot, tag)| [slot, tag])
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let mut constants = vec![0; 64];
    floats(&mut constants, 32, &[cutoff]);
    array(&mut material, 0x318, 0x8080_0090, &constants, 16);
    let material = package.add(0x8080_71E8, material);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for panel in 0..3 {
        let x = panel as f32 * 1.2;
        for point in [
            [x, 0.0, 0.0],
            [x + 1.0, 0.0, 0.0],
            [x + 1.0, 0.0, 1.0],
            [x, 0.0, 1.0],
        ] {
            vertices.extend(
                point
                    .into_iter()
                    .chain([0.5, (panel as f32 + 0.5) / 3.0])
                    .chain([0.0, -1.0, 0.0])
                    .flat_map(f32::to_le_bytes),
            );
        }
        indices.extend(
            [0u16, 1, 2, 0, 2, 3]
                .map(|i| i + panel * 4)
                .into_iter()
                .flat_map(u16::to_le_bytes),
        );
    }
    let vertices = package.vertex(32, vertices);
    let payload = package.raw(0, 0, 0, indices);
    let mut index_header = vec![0; 16];
    put(&mut index_header, 8, &36u64.to_le_bytes());
    let indices = package.raw(payload, 32, 6, index_header);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    put(&mut model, mesh, &vertices.to_le_bytes());
    put(&mut model, mesh + 0x10, &indices.to_le_bytes());
    put(&mut model, mesh + 0x58 + stage * 2, &13u16.to_le_bytes());
    for index in stage + 1..24 {
        put(&mut model, mesh + 0x28 + index * 2, &1i16.to_le_bytes());
    }
    let mut part = [0; 0x20];
    put(&mut part, 0, &material.to_le_bytes());
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &18u32.to_le_bytes());
    array(&mut model, mesh + 0x18, 0x8080_737E, &part, 0x20);
    let tag = package.add(MODEL, model);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    fixtures::load(directory.path(), tag).unwrap()
}

#[test]
fn native_body_cutouts_use_their_coverage_and_material_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (stage, cutoff) in [0, 1, 2, 6]
        .into_iter()
        .flat_map(|stage| [0.3f32, 0.5, 0.7].map(|cutoff| (stage, cutoff)))
    {
        let model = case_at(cutoff, stage);
        let image = render::animated_image(
            &model,
            render::Camera {
                yaw: 0.0,
                pitch: 0.0,
                zoom: 1.0,
                pan: [0.0; 2],
            },
            render::Scene {
                key: 0.0,
                fill: 1.0,
                ..Default::default()
            },
            [480, 240],
            0.0,
        );
        let blue = image
            .pixels
            .iter()
            .filter(|p| p.b() > 60 && p.b() > p.r().saturating_mul(2))
            .count();
        let cream = image
            .pixels
            .iter()
            .filter(|p| p.r() > 150 && p.g() > 140 && p.b() < p.r())
            .count();
        assert_eq!(blue, 0, "Zero coverage must leave no blue decal background");
        assert_eq!(
            cream > 100,
            cutoff <= 0.5,
            "Half coverage uses the material cutoff"
        );
        let name = format!("stage-{stage}-cutoff-{}", (cutoff * 100.0) as u32);
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 480, 240).unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join(format!("{name}.glb")),
            export::glb(&model, 0.0).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"stage":stage,"cutoff":cutoff,"blue_pixels":blue,"cream_pixels":cream,"notices":model.notices}));
    }
    std::fs::write(
        output.join("body-cutout-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
