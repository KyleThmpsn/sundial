//! Package-decoded sprites join the same linear image and output controls as mesh effects.
use super::*;
use eframe::egui::ColorImage;
use fixtures::Package;

fn texture(package: &mut Package, values: &[[u16; 4]]) -> u32 {
    let data = package.raw(
        0,
        40,
        1,
        values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    );
    let mut header = vec![0; 40];
    put(&mut header, 4, &10_u32.to_le_bytes());
    for (at, value) in [(14, values.len() as u16), (16, 1), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    package.raw(data, 32, 1, header)
}

fn source() -> crate::model_preview::particles::Source {
    crate::model_preview::particles::Source {
        position: [0.0; 3],
        drift: [0.0; 3],
        width: 0.25,
        phase: 0.0,
        period: 2.0,
        texture: 0,
        gradient: None,
    }
}

fn save(image: &ColorImage, output: &Path, name: &str) {
    let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join(format!("{name}.png")),
        export::png(&bytes, image.width(), image.height()).unwrap(),
    )
    .unwrap();
}

fn encoded(rgb: [f32; 3]) -> [u8; 3] {
    rgb.map(|v| {
        let value = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    })
}

fn center(image: &ColorImage) -> [u8; 3] {
    let pixel = image.pixels[image.height() / 2 * image.width() + image.width() / 2];
    [pixel.r(), pixel.g(), pixel.b()]
}

#[test]
fn sprite_study_composes_hdr_overlap_output_controls_and_rewind() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("particles").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("sprites");
    std::fs::create_dir_all(&output).unwrap();
    let mut package = Package::default();
    // Explicit half values: dim [1/16, 1/8, 1/4, 1/2], HDR [4, 2, 1, 1].
    let dim = texture(&mut package, &[[0x2C00, 0x3000, 0x3400, 0x3800]; 2]);
    let hdr = texture(&mut package, &[[0x4400, 0x4000, 0x3C00, 0x3C00]; 2]);
    let ramp = texture(
        &mut package,
        &[[0x3800, 0x3400, 0x4400, 0x3C00], [0, 0, 0, 0x3C00]],
    );
    package.write(&output);
    let manager = PackageManager::new(
        &output,
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    model
        .textures
        .push(crate::model_preview::texture::load(&manager, dim).unwrap());
    model.particle_sources = vec![source(), source()];
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let scene = render::Scene {
        background: [0; 3],
        particle_study: true,
        ..render::Scene::unprocessed()
    };
    let draw = |model: &Model, scene, seconds| {
        render::animated_image(model, camera, scene, [128, 128], seconds)
    };
    let mut receipt = Vec::new();
    for (name, seconds, exposure) in [
        ("overlap", 0.0, 1.0),
        ("aged", 1.0, 1.0),
        ("exposure", 0.0, 0.25),
    ] {
        let image = draw(&model, render::Scene { exposure, ..scene }, seconds);
        let age = seconds / 2.0;
        let strength = 2.0 * 0.5 * 0.7 * exposure * (1.0_f32 - age).powf(1.4);
        let expected = encoded([0.0625, 0.125, 0.25].map(|v| v * strength));
        let actual = center(&image);
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "{name}: {actual:?} != {expected:?}"
        );
        save(&image, &output, name);
        receipt.push(json!({"name":name,"actual":actual,"expected":expected}));
    }
    let original = draw(&model, scene, 0.0);
    draw(&model, scene, 1.0);
    let rewind = draw(&model, scene, 0.0);
    assert_eq!(original.pixels, rewind.pixels);
    save(&rewind, &output, "rewind");
    let ordinary = draw(
        &model,
        render::Scene {
            particle_study: false,
            ..scene
        },
        0.0,
    );
    assert!(
        ordinary
            .pixels
            .iter()
            .all(|p| p.to_array() == [0, 0, 0, 255])
    );
    save(&ordinary, &output, "ordinary");

    // Sprite coverage can extend beyond every mesh raster band even without bloom.
    model.vertices = vec![[-1.0, 0.0, -0.15], [1.0, 0.0, -0.15], [0.0, 0.0, 0.15]];
    model.triangles = vec![[0, 1, 2]];
    for source in &mut model.particle_sources {
        source.position[2] = 0.5;
    }
    let outside_mesh = draw(&model, scene, 0.0);
    let point = render::project(
        &model,
        render::drawn_bounds(&model, render::Style::Textured),
        camera,
        [128.0; 2],
        [0.0, 0.0, 0.5],
    );
    let actual = outside_mesh.pixels[point[1] as usize * 128 + point[0] as usize].to_array();
    let expected = encoded([0.0625, 0.125, 0.25].map(|v| v * 0.7));
    assert!(
        actual[..3]
            .iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    save(&outside_mesh, &output, "outside-mesh-bands");
    receipt.push(json!({"name":"outside-mesh-bands","actual":actual,"expected":expected}));
    model.vertices.clear();
    model.triangles.clear();
    for source in &mut model.particle_sources {
        source.position[2] = 0.0;
    }

    model.particle_sources.truncate(1);
    model
        .textures
        .push(crate::model_preview::texture::load(&manager, ramp).unwrap());
    model.particle_sources[0].gradient = Some(1);
    let gradient = draw(&model, scene, 0.0);
    let expected = encoded([0.5, 0.25, 4.0].map(|v| v * 0.35));
    assert!(
        center(&gradient)
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    save(&gradient, &output, "ramp");
    receipt.push(json!({"name":"ramp","actual":center(&gradient),"expected":expected}));

    model.textures[0] = crate::model_preview::texture::load(&manager, hdr).unwrap();
    model.particle_sources[0].gradient = None;
    let filmic = draw(
        &model,
        render::Scene {
            filmic: true,
            ..scene
        },
        0.0,
    );
    // An independently constructed emissive panel carries the same known linear energy.
    let control = Model {
        vertices: vec![
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 0.0, 1.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        triangle_constant: vec![Some([2.8, 1.4, 0.7]); 2],
        ..Default::default()
    };
    let panel = draw(
        &control,
        render::Scene {
            filmic: true,
            ..scene
        },
        0.0,
    );
    assert_eq!(center(&filmic), center(&panel));
    save(&filmic, &output, "hdr-filmic");
    save(&panel, &output, "hdr-panel-control");
    let plain = draw(&model, scene, 0.0);
    let bloom = draw(
        &model,
        render::Scene {
            bloom: true,
            ..scene
        },
        0.0,
    );
    let halo = bloom
        .pixels
        .iter()
        .zip(&plain.pixels)
        .filter(|(a, b)| b.to_array() == [0, 0, 0, 255] && a.to_array() != [0, 0, 0, 255])
        .count();
    assert!(halo > 30, "Missing sprite bloom halo: {halo}");
    save(&plain, &output, "hdr-plain");
    save(&bloom, &output, "hdr-bloom");
    // Tiny and entirely offscreen studies remain bounded, including with output processing.
    let small = render::animated_image(&model, camera, scene, [1, 1], 0.0);
    assert_eq!(small.size, [1, 1]);
    let offscreen = render::animated_image(
        &model,
        render::Camera {
            pan: [10.0, 10.0],
            ..camera
        },
        scene,
        [128, 128],
        0.0,
    );
    assert!(
        offscreen
            .pixels
            .iter()
            .all(|p| p.to_array() == [0, 0, 0, 255])
    );
    std::fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":receipt,"hdr_filmic":center(&filmic),"panel_control":center(&panel),
            "halo_pixels":halo,"rewind_exact":true,"native_particle_playback":false
        }))
        .unwrap(),
    )
    .unwrap();
}
