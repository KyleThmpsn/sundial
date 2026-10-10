//! Package-to-preview color landmarks, authored before the plate canvas correction.
use super::*;
use fixtures::{Package, array, floats, put};

const COLORS: [[u8; 4]; 3] = [[16, 48, 240, 255], [220, 208, 176, 255], [12, 12, 12, 255]];

pub(in crate::model_preview) fn camera(case: &serde_json::Value) -> render::Camera {
    let mut camera = render::Camera::default();
    for (name, field) in [
        ("yaw", &mut camera.yaw),
        ("pitch", &mut camera.pitch),
        ("zoom", &mut camera.zoom),
    ] {
        if let Some(value) = case["camera"][name].as_f64() {
            *field = value as f32;
            assert!(field.is_finite(), "Configured camera {name} is not finite");
        }
    }
    assert!(camera.pitch.abs() <= render::MAX_PITCH && camera.zoom > 0.0);
    camera
}

fn plate(package: &mut Package, rect: [u32; 4], channel: usize, size: [usize; 2]) -> (u32, u32) {
    let rgba = (0..size[1])
        .flat_map(|y| {
            let color = match channel {
                0 => {
                    COLORS[if y < 2 {
                        0
                    } else if y < 6 {
                        1
                    } else {
                        2
                    }]
                }
                1 => [128, 128, 255, 255],
                2 => [255, if y < 2 { 32 } else { 192 }, 0, 0],
                _ => [240, 16, 16, 255],
            };
            std::iter::repeat_n(color, size[0]).flatten()
        })
        .collect();
    let data = package.raw(0, 0, 0, rgba);
    let mut header = vec![0; 0x40];
    put(
        &mut header,
        0,
        &((size[0] * size[1] * 4) as u32).to_le_bytes(),
    );
    put(
        &mut header,
        4,
        &(if channel == 0 { 29u32 } else { 28 }).to_le_bytes(),
    );
    for (at, length) in [14, 16].into_iter().zip(size) {
        put(&mut header, at, &(length as u16).to_le_bytes());
    }
    for at in [18, 20] {
        put(&mut header, at, &1u16.to_le_bytes());
    }
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    let texture = package.raw(data, 32, 1, header);
    let mut bytes = vec![0; 0x20];
    let row: Vec<_> = [texture, rect[0], rect[1], rect[2], rect[3]]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut bytes, 0x10, 0x8080_9EBD, &row, 20);
    (package.add(0x8080_9EBB, bytes), texture)
}

pub(super) fn fixture(
    rect: [u32; 4],
    canvas: [usize; 2],
    explicit: bool,
) -> (tempfile::TempDir, u32) {
    fixture_material(rect, canvas, explicit, 0, |package, bindings| {
        if explicit {
            let mut bytes = vec![0; 0x400];
            array(&mut bytes, 0x2D0, 0x8080_7211, bindings, 8);
            package.add(0x8080_71E8, bytes)
        } else {
            0
        }
    })
}

pub(super) fn fixture_material(
    rect: [u32; 4],
    canvas: [usize; 2],
    explicit: bool,
    slot: u8,
    material: impl FnOnce(&mut Package, &[u8]) -> u32,
) -> (tempfile::TempDir, u32) {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    let mut plates = vec![0; 0x30];
    let mut bindings = Vec::new();
    let image_size = if explicit { [16, 8] } else { [8, 8] };
    for channel in 0..3 {
        let (own, texture) = plate(&mut package, rect, channel, image_size);
        let tag = if explicit {
            plate(&mut package, rect, 3, image_size).0
        } else {
            own
        };
        put(&mut plates, 0x24 + channel * 4, &tag.to_le_bytes());
        bindings.extend((channel as u32).to_le_bytes());
        bindings.extend(texture.to_le_bytes());
    }
    let plates = package.add(0x8080_72D2, plates);
    let material = material(&mut package, &bindings);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (panel, v) in [0.125f32, 0.5, 0.875].into_iter().enumerate() {
        let x = panel as f32 * 1.2;
        let uv = [
            (rect[0] as f32 + rect[2] as f32 * 0.5) / canvas[0] as f32,
            (rect[1] as f32 + rect[3] as f32 * v) / canvas[1] as f32,
        ];
        for position in [
            [x, 0.0, 0.0],
            [x + 1.0, 0.0, 0.0],
            [x + 1.0, 0.0, 1.0],
            [x, 0.0, 1.0],
        ] {
            vertices.extend(
                position
                    .into_iter()
                    .chain(uv)
                    .chain([0.0, -1.0, 0.0])
                    .flat_map(f32::to_le_bytes),
            );
        }
        indices.extend(
            [0u16, 1, 2, 0, 2, 3]
                .map(|i| i + panel as u16 * 4)
                .into_iter()
                .flat_map(u16::to_le_bytes),
        );
    }
    let vertices = package.vertex(32, vertices);
    let index_data = package.raw(0, 0, 0, indices);
    let mut header = vec![0; 16];
    put(&mut header, 8, &36u64.to_le_bytes());
    let indices = package.raw(index_data, 32, 6, header);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x50, &[1.0; 3]);
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    put(&mut model, mesh, &vertices.to_le_bytes());
    for at in [4, 8, 12] {
        put(&mut model, mesh + at, &u32::MAX.to_le_bytes());
    }
    put(&mut model, mesh + 0x10, &indices.to_le_bytes());
    for stage in 1..24 {
        put(&mut model, mesh + 0x28 + stage * 2, &1i16.to_le_bytes());
    }
    put(&mut model, mesh + 0x58, &13u16.to_le_bytes());
    let mut part = [0; 0x20];
    put(&mut part, 0, &material.to_le_bytes());
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &18u32.to_le_bytes());
    part[0x1A] = slot;
    array(&mut model, mesh + 0x18, 0x8080_737E, &part, 0x20);
    let model = package.add(MODEL, model);
    let mut component = vec![0; 0x400];
    put(&mut component, 0x10, &0x30i64.to_le_bytes());
    put(&mut component, 0x18, &0x68i64.to_le_bytes());
    put(&mut component, 0x3C, &0x8080_72B8u32.to_le_bytes());
    put(&mut component, 0x7C, &0x8080_72BDu32.to_le_bytes());
    put(&mut component, 0x80 + 0x1DC, &model.to_le_bytes());
    put(&mut component, 0x80 + 0x248, &plates.to_le_bytes());
    let component = package.add(RESOURCE, component);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    (directory, component)
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn canvas_cases() -> Vec<(String, Model)> {
    [
        ("wide", [0, 0, 16, 8], [16, 16], false),
        ("tall", [0, 0, 8, 16], [16, 16], false),
        ("square", [0, 0, 8, 8], [8, 8], false),
        ("offset", [8, 4, 8, 4], [16, 16], false),
        ("explicit", [0, 0, 16, 8], [16, 8], true),
    ]
    .into_iter()
    .map(|(name, rect, canvas, explicit)| {
        let (directory, tag) = fixture(rect, canvas, explicit);
        let manager = PackageManager::new(
            directory.path(),
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            Some(tiger_pkg::PackagePlatform::Win64),
        )
        .unwrap();
        let model = load_with_manager(&manager, tag, &Load::default(), None).unwrap();
        validate(&model);
        (format!("native-canvas-{name}"), model)
    })
    .collect()
}

#[test]
fn gear_plate_landmarks_survive_package_loading_rendering_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("plates").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (name, rect, canvas, expected_size, explicit) in [
        ("wide", [0, 0, 16, 8], [16, 16], [16, 16], false),
        ("tall", [0, 0, 8, 16], [16, 16], [16, 16], false),
        ("square", [0, 0, 8, 8], [8, 8], [8, 8], false),
        ("offset", [8, 4, 8, 4], [16, 16], [16, 16], false),
        (
            "high-resolution",
            [0, 0, 4096, 1024],
            [4096, 4096],
            [4096, 4096],
            false,
        ),
        ("explicit", [0, 0, 16, 8], [16, 8], [16, 8], true),
    ] {
        let (directory, tag) = fixture(rect, canvas, explicit);
        let manager = PackageManager::new(
            directory.path(),
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            Some(tiger_pkg::PackagePlatform::Win64),
        )
        .unwrap();
        let model = load_with_manager(&manager, tag, &Load::default(), None).unwrap();
        validate(&model);
        for indices in [
            &model.triangle_textures,
            &model.triangle_normals,
            &model.triangle_gearstacks,
        ] {
            assert_eq!(
                model.textures[indices[0].unwrap()].size,
                expected_size,
                "{name}"
            );
        }
        let albedo = &model.textures[model.triangle_textures[0].unwrap()];
        let mask = &model.textures[model.triangle_gearstacks[0].unwrap()];
        for (panel, color) in COLORS.iter().enumerate() {
            let uv = model.uvs[panel * 4];
            assert_eq!(
                albedo.sample(uv),
                [color[0], color[1], color[2]].map(f32::from),
                "{name} panel {panel}"
            );
            assert_eq!(
                mask.sample_rgba(uv)[1],
                if panel == 0 { 32.0 } else { 192.0 }
            );
        }
        let camera = render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        };
        let image = render::image(&model, camera, [480, 240]);
        let blue = image
            .pixels
            .iter()
            .filter(|p| p.b() > 100 && p.b() > p.r().saturating_mul(2))
            .count();
        let cream = image
            .pixels
            .iter()
            .filter(|p| p.r() > 100 && p.g() > 100 && p.r() > p.b())
            .count();
        assert!(
            blue > 200 && cream > 200,
            "{name}: blue {blue}, cream {cream}"
        );
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
        receipt.push(json!({"case":name,"rectangle":rect,"canvas":canvas,"preview_size":expected_size,"blue_pixels":blue,"cream_pixels":cream}));
    }
    std::fs::write(
        output.join("plate-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES, SUNDIAL_PLATE_CASES and SUNDIAL_TEST_ARTIFACTS"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps each case's render and its independent checks together"
)]
fn installed_gear_plates_render_catalog_appearances() {
    let packages = crate::test_support::preview_packages();
    let output = crate::test_support::artifact_dir("plates");
    let input = std::env::var_os("SUNDIAL_PLATE_CASES").unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!(!cases.is_empty());
    std::fs::create_dir_all(&output).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let mut receipt = Vec::new();
    for case in cases {
        let camera = camera(&case);
        let hash = u32::from_str_radix(case["hash"].as_str().unwrap(), 16).unwrap();
        let appearance = catalog
            .shader_preview_appearance(hash, &Default::default())
            .unwrap();
        let model = appearance::load(&packages, &appearance).unwrap();
        validate(&model);
        // Retain material inputs before acceptance assertions, so a failed image remains
        // traceable to its loaded geometry, colors and unavailable native inputs.
        let surfaces: Vec<_> = model
            .dyes
            .iter()
            .enumerate()
            .map(|(slot, dye)| {
                dye.as_ref().map(|d| {
                    json!({"slot":slot,"vectors":d.vectors,
                "albedo":d.surface.albedo,"emissive":d.surface.emissive,
                "detail":d.detail,"normal":d.normal})
                })
            })
            .collect();
        let textures: Vec<_> = model.textures.iter().map(|t| {
            json!({"tag":format!("{:08X}",t.tag),"size":t.size,"linear":t.linear.is_some()})
        }).collect();
        let color_consumers: Vec<_> = model.effects.iter().enumerate().filter(|(_, m)| m.native.as_ref().is_some_and(|n| n.opaque())).map(|(index, m)| {
            let frames: Vec<_> = [0.0, 0.5, 1.0].into_iter().map(|seconds| {
                let frame = m.frame(seconds);
                json!({"seconds":seconds,"base_gain":frame.as_ref().and_then(|f| m.native.as_ref()?.base_gain(f)),"base_metal":frame.as_ref().and_then(|f| m.native.as_ref()?.base_metal(f)),"paint_smoothness":frame.as_ref().and_then(|f| m.native.as_ref()?.paint(f)),"normal_decode":frame.as_ref().and_then(|f| m.native.as_ref()?.normal(f)),"normal_grain":frame.as_ref().and_then(|f| m.native.as_ref()?.grain(f)),"constants":frame.map(|f| f[..m.constants.len()].to_vec())})
            }).collect();
            json!({"index":index,"local_uv":m.native.as_ref().and_then(|n| n.opaque_uv),"intensity":m.native.as_ref().is_some_and(|n| n.intensity),"ambient_power":m.native.as_ref().and_then(|n| n.ambient_power),"frames":frames})
        }).collect();
        let normal_consumers: Vec<_> = model.effects.iter().enumerate().filter_map(|(index, material)| {
            let normal = material.normal.as_ref()?;
            let frames: Vec<_> = [0.0, 0.5, 1.0].into_iter().map(|seconds| {
                json!({"seconds":seconds,"decode":material.frame(seconds).as_ref().and_then(|f| normal.frame(f))})
            }).collect();
            let triangles = model.triangle_effects.iter().filter(|&&effect| effect == Some(index)).count();
            Some(json!({"index":index,"triangles":triangles,"frames":frames}))
        }).collect();
        std::fs::write(
            output.join(format!("{hash:08X}-loaded.json")),
            serde_json::to_vec_pretty(&json!({"case":case,"arrangement":appearance.arrangement,
                "dyes":appearance.dyes,"surfaces":surfaces,"textures":textures,
                "tags":model.tags,"vertices":model.vertices.len(),"triangles":model.triangles.len(),
                "mapped_triangles":model.triangle_dye_maps.iter().filter(|m| m.is_some()).count(),
                "detail_triangles":model.triangle_detail_uv.iter().filter(|&&m| m).count(),
                "opaque_triangles":(0..model.triangles.len()).filter(|&i| !crate::model_preview::effects::transparent(&model,i)).count(),
                "opaque_color_consumers":color_consumers,
                "native_normal_consumers":normal_consumers,
                "motions":model.motions.len(),"notices":model.notices}))
            .unwrap(),
        )
        .unwrap();
        assert!(!model.triangles.is_empty());
        if let Some(minimum) = case["minimum_triangles"].as_u64() {
            assert!(
                model.triangles.len() >= minimum as usize,
                "{hash:08X}: only {} triangles loaded",
                model.triangles.len()
            );
        }
        assert_eq!(model.vertices.len(), model.uvs.len());
        assert!(model.vertices.iter().flatten().all(|v| v.is_finite()));
        let name = format!("{hash:08X}");
        let image = render::image(&model, camera, [800, 480]);
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        if case["verify_particle_study"] == true {
            let study = render::animated_image(
                &model,
                camera,
                render::Scene {
                    filmic: false,
                    bloom: false,
                    particle_study: true,
                    ..render::Scene::unit_exposure()
                },
                [800, 480],
                0.0,
            );
            let differences = image
                .pixels
                .iter()
                .zip(&study.pixels)
                .filter(|(normal, study)| normal != study)
                .count();
            assert!(differences > 10, "{name}: no visible opt-in study");
            let study_rgba: Vec<_> = study.pixels.iter().flat_map(|p| p.to_array()).collect();
            std::fs::write(
                output.join(format!("{name}-particle-study.png")),
                export::png(&study_rgba, 800, 480).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 800, 480).unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join(format!("{name}.glb")),
            export::glb(&model, 0.0).unwrap(),
        )
        .unwrap();
        let blue = image
            .pixels
            .iter()
            .filter(|p| p.b() > 60 && p.b() > p.r().saturating_mul(2))
            .count();
        if let Some(minimum) = case["minimum_blue_pixels"].as_u64() {
            assert!(blue >= minimum as usize, "{name}: {blue} blue pixels");
        }
        let textures: Vec<_> = model
            .textures
            .iter()
            .map(|t| json!({"tag":format!("{:08X}",t.tag),"size":t.size}))
            .collect();
        let mut motion = Vec::new();
        let mut maximum_move = 0.0f32;
        let initial_pose = model.pose(0.0);
        let initial_positions = initial_pose
            .as_ref()
            .map_or(model.vertices.as_slice(), |pose| pose.positions.as_slice());
        let mut initial_image = None;
        let mut initial_glb = None;
        // Compare visible opaque and effect copies of the same stored surface. A moving
        // glow alone does not establish that its underlying solid part follows it.
        let mut layers = std::collections::BTreeMap::<[u32; 6], (Vec<usize>, Vec<usize>)>::new();
        if case["verify_layer_motion"] == true {
            for (triangle, vertices) in model.triangles.iter().enumerate() {
                let effect = crate::model_preview::effects::transparent(&model, triangle);
                for &index in vertices {
                    let index = index as usize;
                    let key = std::array::from_fn(|lane| {
                        if lane < 3 {
                            model.vertices[index][lane].to_bits()
                        } else {
                            model.normals[index][lane - 3].to_bits()
                        }
                    });
                    let pair = layers.entry(key).or_default();
                    let indices = if effect { &mut pair.1 } else { &mut pair.0 };
                    if !indices.contains(&index) {
                        indices.push(index);
                    }
                }
            }
            layers.retain(|_, (opaque, effect)| !opaque.is_empty() && !effect.is_empty());
            assert!(
                !layers.is_empty(),
                "{name}: no corresponding material layers"
            );
        }
        let mut dye_uv_bounds = [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]];
        if case["verify_dye_uv_bounds"] == true {
            let mut mapped = 0;
            for (triangle, map) in model.triangle_dye_maps.iter().enumerate() {
                let Some(map) = map else { continue };
                for &index in &model.triangles[triangle] {
                    let uv = map.uv(model.uvs[index as usize]);
                    for lane in 0..2 {
                        dye_uv_bounds[0][lane] = dye_uv_bounds[0][lane].min(uv[lane]);
                        dye_uv_bounds[1][lane] = dye_uv_bounds[1][lane].max(uv[lane]);
                        assert!(
                            (-0.001..=1.001).contains(&uv[lane]),
                            "{name}: dye lookup outside its local image: {uv:?}"
                        );
                    }
                    mapped += 1;
                }
            }
            assert!(mapped > 0, "{name}: no mapped dye surfaces");
        }
        let mut moving_layer_pairs = 0;
        let mut maximum_layer_gap = 0.0f32;
        for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
            let pose = model.pose(seconds);
            let positions = pose
                .as_ref()
                .map_or(model.vertices.as_slice(), |p| p.positions.as_slice());
            for (opaque, effect) in layers.values() {
                for &a in opaque {
                    for &b in effect {
                        // Only pair layers that coincide at the initial pose. Separate
                        // authored effect offsets remain separate surfaces.
                        let initial_gap = (0..3)
                            .map(|i| (initial_positions[a][i] - initial_positions[b][i]).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        if initial_gap > 0.0001 {
                            continue;
                        }
                        let gap = (0..3)
                            .map(|i| (positions[a][i] - positions[b][i]).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        maximum_layer_gap = maximum_layer_gap.max(gap);
                        assert!(
                            gap < 0.001,
                            "{name}: material layers separated at {seconds}s by {gap}"
                        );
                        let movement = (0..3)
                            .map(|i| (positions[a][i] - initial_positions[a][i]).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        if movement > 0.01 {
                            moving_layer_pairs += 1;
                        }
                    }
                }
            }
            if let Some(pose) = &pose {
                for (a, b) in initial_positions.iter().zip(&pose.positions) {
                    maximum_move =
                        maximum_move.max((0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt());
                }
            }
            let image = render::animated_image(
                &model,
                camera,
                render::Scene::unprocessed(),
                [800, 480],
                seconds,
            );
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let cyan = image
                .pixels
                .iter()
                .filter(|p| {
                    p.g() > 70
                        && p.b() > 70
                        && p.g() > p.r().saturating_add(25)
                        && p.b() > p.r().saturating_add(25)
                })
                .count();
            let suffix = format!("{name}-{step}-{seconds:.1}");
            let glb = export::glb(&model, seconds).unwrap();
            if step == 0 {
                initial_image = Some(rgba.clone());
                initial_glb = Some(glb.clone());
            } else if step == 3 {
                assert_eq!(
                    initial_image.as_ref().unwrap(),
                    &rgba,
                    "{name}: rewind image"
                );
                assert_eq!(
                    initial_glb.as_ref().unwrap(),
                    &glb,
                    "{name}: rewind geometry"
                );
            }
            std::fs::write(
                output.join(format!("{suffix}.png")),
                export::png(&rgba, 800, 480).unwrap(),
            )
            .unwrap();
            std::fs::write(output.join(format!("{suffix}.glb")), glb).unwrap();
            if let Some(minimum) = case["minimum_cyan_pixels"].as_u64() {
                assert!(cyan >= minimum as usize, "{name}: {cyan} cyan pixels");
            }
            motion.push(json!({"step":step,"seconds":seconds,"cyan_pixels":cyan,"posed":pose.is_some(),"artifact":suffix}));
        }
        if case["require_motion"] == true {
            assert!(maximum_move > 0.01, "{name}: no visible part motion");
        }
        if case["verify_layer_motion"] == true {
            assert!(
                moving_layer_pairs > 0,
                "{name}: corresponding material layers stayed static"
            );
        }
        for plate in case["plates"].as_array().unwrap() {
            let tag = u32::from_str_radix(plate["tag"].as_str().unwrap(), 16).unwrap();
            let texture = model.textures.iter().find(|t| t.tag == tag).unwrap();
            assert_eq!(json!(texture.size), plate["size"], "{name} plate {tag:08X}");
        }
        receipt.push(json!({"case":case,"arrangement":appearance.arrangement,"dyes":appearance.dyes,
            "dye_mapped_triangles":model.triangle_dye_maps.iter().filter(|m| m.is_some()).count(),
            "vertices":model.vertices.len(),"triangles":model.triangles.len(),"blue_pixels":blue,"textures":textures,"notices":model.notices,"motion":motion,"maximum_move":maximum_move,
            "moving_layer_pairs":moving_layer_pairs,"maximum_layer_gap":maximum_layer_gap,
            "dye_uv_bounds": if case["verify_dye_uv_bounds"] == true { Some(dye_uv_bounds) } else { None }}));
    }
    std::fs::write(
        output.join("installed-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
