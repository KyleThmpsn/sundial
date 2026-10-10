//! Package-to-frame coverage for the independently witnessed particle pixel contract.
use super::*;
use eframe::egui::ColorImage;
use fixtures::{Package, array};

#[derive(Clone, Copy)]
pub(super) struct Input {
    pub red: u8,
    pub green: u8,
    pub brightness: f32,
    pub coverage: f32,
    pub u: f32,
}

fn image(package: &mut Package, format: u32, width: u16, pixels: Vec<u8>) -> u32 {
    let payload = package.raw(0, 40, 1, pixels);
    let mut header = vec![0; 40];
    put(&mut header, 4, &format.to_le_bytes());
    for (at, value) in [(14, width), (16, 1), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    package.raw(payload, 32, 1, header)
}

pub(super) fn material(package: &mut Package, input: Input) -> u32 {
    let distortion = image(package, 28, 1, vec![64, 128, 0, 255]);
    let mask = image(package, 28, 1, vec![input.red, input.green, 0, 255]);
    // Exact half-float endpoints [2, 0.25, 0.125, 1] and [0.125, 0.5, 3, 1].
    let ramp = image(
        package,
        10,
        2,
        [
            0x4000_u16, 0x3400, 0x3000, 0x3C00, 0x3000, 0x3800, 0x4200, 0x3C00,
        ]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect(),
    );
    let mut bytes = vec![0; 0x3A0];
    let bindings: Vec<_> = [distortion, mask, ramp]
        .into_iter()
        .enumerate()
        .flat_map(|(slot, tag)| [slot as u32, tag])
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut bytes, 0x2D0, 0x8080_7211, &bindings, 8);
    let mut samplers = Vec::new();
    for address in [1_u32, 4, 3] {
        let mut descriptor = vec![0; 52];
        for (at, value) in [
            (0, 0x15_u32),
            (4, address),
            (8, address),
            (12, 1),
            (20, 1),
            (24, 1),
        ] {
            put(&mut descriptor, at, &value.to_le_bytes());
        }
        floats(&mut descriptor, 44, &[0.0, f32::MAX]);
        let data = package.raw(0, 42, 1, descriptor);
        let header = package.raw(data, 34, 1, vec![0; 8]);
        package.set_reference(data, header);
        samplers.extend(header.to_le_bytes());
        samplers.extend([0; 12]);
    }
    array(&mut bytes, 0x308, 0x8080_73F3, &samplers, 16);
    package.add(0x8080_71E8, bytes)
}

fn definition(package: &mut Package, input: Input) -> u32 {
    let mut bytes = vec![0; 0x150];
    bytes[0x80..0xF0].fill(0xFF);
    bytes[0x8A..0x8C].copy_from_slice(&[6, 0]);
    floats(&mut bytes, 0x120, &[1.0_f32 + 0.05]);
    let defaults: Vec<_> = [1.0_f32, 0.0, 0.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    array(&mut bytes, 8, 0x8080_0090, &defaults, 16);
    let code: Vec<_> = [0_u8, 1, 2, 5, 6]
        .into_iter()
        .enumerate()
        .flat_map(|(constant, slot)| [0x34, constant as u8, 0x3F, 1, slot, 0])
        .collect();
    put(&mut bytes, 0x78, &(code.len() as u16).to_le_bytes());
    array(&mut bytes, 0x50, 0x8080_0009, &code, 1);
    let constants: Vec<_> = [
        [0.0_f32, 0.0, 0.5, 0.5],
        [0.0, 0.0, 0.5, 0.5],
        [0.0, 0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, input.brightness, input.coverage, 0.5],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect();
    array(&mut bytes, 0x60, 0x8080_0090, &constants, 16);
    package.add(0x8080_6E2C, bytes)
}

fn load(input: Input, output: &Path, name: &str) -> Model {
    let (mut package, _, models) = fixtures::cloth_entity();
    let vertices: Vec<_> = [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .flat_map(|p| [p[0], p[1], p[2], input.u, 0.5, 0.0, 1.0, 0.0])
        .flat_map(f32::to_le_bytes)
        .collect();
    let stream = package.vertex(32, vertices);
    let mesh = package.payload_mut(models[0]);
    let at = 0x28 + i64::from_le_bytes(mesh[0x18..0x20].try_into().unwrap()) as usize;
    put(mesh, at, &stream.to_le_bytes());
    put(mesh, at + 0x58, &13_u16.to_le_bytes());
    floats(mesh, 0x50, &[1.0; 3]);
    floats(mesh, 0x60, &[0.0; 3]);
    floats(mesh, 0x6C, &[1.0]);
    floats(mesh, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let definition = definition(&mut package, input);
    let material = material(&mut package, input);
    let mut emitter = vec![0; 0x48];
    put(&mut emitter, 0x40, &models[0].to_le_bytes());
    let emitter = package.add(0x8080_6E2E, emitter);
    let mut system = vec![0; 52];
    for (at, tag) in [(0, definition), (0x14, material), (0x18, emitter)] {
        put(&mut system, at, &tag.to_le_bytes());
    }
    let system = package.raw(PARTICLE_SYSTEM, 8, 0, system);
    let directory = output.join(name);
    std::fs::create_dir_all(&directory).unwrap();
    package.write(&directory);
    let mut model = fixtures::load(&directory, system).unwrap();
    assert_eq!(model.triangles.len(), 1);
    assert!(
        model
            .uvs
            .iter()
            .all(|uv| (uv[0] - input.u).abs() < 1e-6 && uv[1] == 0.5)
    );
    let [particle] = model.assets.particles.as_mut_slice() else {
        panic!("Missing particle")
    };
    assert!(particle.notice.is_none(), "{:?}", particle.notice);
    assert_eq!(particle.material_textures.len(), 3);
    assert_eq!(particle.material_samplers.len(), 3);
    // The native shader is witnessed separately. This generated fixture exercises its
    // chosen study contract without fabricating or bundling original game bytecode.
    particle.pixel_kind = Some(assets::PixelKind::DualMaskRamp);
    model
}

fn expected(input: Input, exposure: f32) -> [u8; 4] {
    let coordinate = (f32::from(input.red) / 255.0).powi(2);
    let weight = (coordinate * 2.0 - 0.5).clamp(0.0, 1.0);
    let ramp = [2.0, 0.25, 0.125].map(|a| a * (1.0 - weight));
    let second = [0.125, 0.5, 3.0].map(|b| b * weight);
    let shape = (1.0 - (input.u - 0.5).abs() * 2.222_222).max(0.0).powi(2);
    let coverage = (shape * input.coverage).clamp(0.0, 1.0);
    let rgb: [u8; 3] = std::array::from_fn(|i| {
        let value = (ramp[i] + second[i]) * coverage * input.brightness * exposure / 50.0;
        let encoded = if value <= 0.003_130_8 {
            12.92 * value
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
    });
    [rgb[0], rgb[1], rgb[2], 255]
}

fn draw(model: &Model, input: Input, output: &Path, name: &str, seconds: f32) -> ColorImage {
    let scene = render::Scene {
        background: [0; 3],
        exposure: 1.25,
        particle_study: true,
        ..render::Scene::unprocessed()
    };
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let image = render::animated_image(model, camera, scene, [128, 128], seconds);
    let expected = if seconds > 0.999 {
        [0, 0, 0, 255]
    } else {
        expected(input, scene.exposure)
    };
    let visible = image
        .pixels
        .iter()
        .filter(|p| p.to_array() != [0, 0, 0, 255])
        .count();
    if expected == [0, 0, 0, 255] {
        assert_eq!(visible, 0, "{name}")
    } else {
        assert!(visible > 1500, "{name}: {visible}")
    }
    for pixel in &image.pixels {
        if pixel.to_array() == [0, 0, 0, 255] {
            continue;
        }
        assert!(
            pixel
                .to_array()
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "{name}: {:?} != {expected:?}",
            pixel.to_array()
        );
    }
    save(&image, output, name);
    std::fs::write(
        output.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&json!({
            "red":input.red,"green":input.green,"brightness":input.brightness,
            "coverage":input.coverage,"u":input.u,"seconds":seconds,
            "exposure":scene.exposure,"study_brightness":0.02,"expected_pixel":expected,
            "visible_pixels":visible,"native_playback_verified":false
        }))
        .unwrap(),
    )
    .unwrap();
    image
}

fn save(image: &ColorImage, output: &Path, name: &str) {
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join(format!("{name}.png")),
        export::png(&rgba, 128, 128).unwrap(),
    )
    .unwrap();
}

#[test]
fn packaged_particle_study_preserves_mask_coverage_hdr_and_rewind() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("particles").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let base = Input {
        red: 153,
        green: 89,
        brightness: 8.0,
        coverage: 0.5,
        u: 0.5,
    };
    let mut model = load(base, output, "base-package");
    let first = draw(&model, base, output, "baseline", 0.0);
    let boundary = draw(&model, base, output, "age-boundary", 0.999);
    assert_eq!(first.pixels, boundary.pixels);
    draw(&model, base, output, "expired", 1.0);
    assert_eq!(
        first.pixels,
        draw(&model, base, output, "rewind", 0.0).pixels
    );
    for green in [0, 32, 230, 255] {
        let input = Input { green, ..base };
        let name = format!("green-{green}");
        let variant = load(input, output, &format!("{name}-package"));
        assert_eq!(
            first.pixels,
            draw(&variant, input, output, &name, 0.0).pixels
        );
    }
    for red in [0, 32, 230, 255] {
        let input = Input { red, ..base };
        let name = format!("red-{red}");
        let variant = load(input, output, &format!("{name}-package"));
        let changed = draw(&variant, input, output, &name, 0.0)
            .pixels
            .into_iter()
            .zip(&first.pixels)
            .filter(|(a, b)| a != *b)
            .count();
        assert!(changed > 1500, "{name}: {changed}");
    }
    for (name, coverage, u) in [
        ("negative", -1.0, 0.5),
        ("zero", 0.0, 0.5),
        ("saturated", 4.0, 0.3),
        ("fractional", 0.2, 0.3),
    ] {
        let input = Input {
            coverage,
            u,
            ..base
        };
        let variant = load(input, output, &format!("{name}-package"));
        draw(&variant, input, output, name, 0.0);
    }
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let ordinary = render::Scene::unprocessed();
    let early = render::animated_image(&model, camera, ordinary, [128, 128], 0.0);
    let late = render::animated_image(&model, camera, ordinary, [128, 128], 1.0);
    assert_eq!(early.pixels, late.pixels);
    assert!(
        early
            .pixels
            .iter()
            .any(|p| p.to_array()[..3] != ordinary.background)
    );
    save(&early, output, "ordinary-early");
    save(&late, output, "ordinary-late");
    model.assets.particles[0].pixel_kind = None;
    let fallback = render::animated_image(
        &model,
        camera,
        render::Scene {
            particle_study: true,
            ..ordinary
        },
        [128, 128],
        1.0,
    );
    assert_eq!(early.pixels, fallback.pixels);
    save(&fallback, output, "unknown-contract-fallback");
    std::fs::write(output.join("particle-material-receipt.json"), serde_json::to_vec_pretty(&json!({
        "frames":19,"source":"Generated Shadowkeep packages","shader_contract_selected_explicitly":true,
        "native_shader_identification_verified":false,"native_playback_verified":false,
        "green_invariant":true,"red_changes_image":true,"hdr_preserved":true,"rewind_restored":true,
        "ordinary_preview_unchanged":true
    })).unwrap()).unwrap();
}
