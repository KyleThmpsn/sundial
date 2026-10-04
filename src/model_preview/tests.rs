use super::*;

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
fn model_less_emitter_reaches_sound_bank_events() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let mut inventory = Inventory::default();
    inventory.scan_sound_bank(&manager, 0x80BD_0809);
    assert!(inventory.sounds.contains(&0x80BD_07BB));
    assert!(inventory.sounds.contains(&0x80BD_0808));
    let model = load(Path::new(&packages), 0x80BD_059F).unwrap();
    assert_eq!(model.assets.sounds.len(), inventory.sounds.len());
    assert!(
        model
            .assets
            .sounds
            .iter()
            .any(|sound| !sound.clips.is_empty())
    );
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
fn model_less_emitter_reaches_shadowing_light() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let packages = Path::new(&packages);
    let emitter = load(packages, 0x80FD_E2FB).unwrap();
    assert!(
        emitter
            .assets
            .lights
            .iter()
            .any(|light| light.tag == 0x80FD_E2F9)
    );
    assert!(emitter.light_geometry);
    let light = load(packages, 0x80FD_E2F9).unwrap();
    assert!(light.light_geometry);
    let image = render::styled_image(
        &light,
        render::Camera::default(),
        render::Scene::default(),
        [480, 360],
        0.0,
        render::Style::Textured,
    );
    let background = image.pixels[0];
    assert!(
        image
            .pixels
            .iter()
            .filter(|&&pixel| pixel != background)
            .count()
            > 100
    );
    if let Some(output) = std::env::var_os("SUNDIAL_PROBE_OUT") {
        let rgba = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect::<Vec<_>>();
        let png = export::png(&rgba, 480, 360).unwrap();
        std::fs::write(Path::new(&output).join("shadowing-light-preview.png"), png).unwrap();
    }
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "One installed effect sequence is checked node by node against the packages"
)]
fn air_weak_fx_sequence_opens_particles_and_sounds() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let packages = Path::new(&packages);
    let model = load(packages, 0x80BC_12EB).unwrap();
    assert_eq!(model.assets.particles.len(), 1);
    assert_eq!(model.assets.sounds.len(), 4);
    assert_eq!(model.assets.effect_nodes.len(), 7);
    assert_eq!(model.assets.effect_nodes[0].target, Some(0x80EF_5769));
    assert_eq!(model.assets.effect_nodes[3].target, Some(0x80BB_E621));
    let particle_timing = model.assets.effect_nodes[0].timing.unwrap();
    assert_eq!(particle_timing.start, 0.0);
    assert_eq!(particle_timing.duration, 0.0001);
    assert_eq!(model.assets.effect_nodes[3].timing.unwrap().start, 0.0);
    assert_eq!(model.assets.particles[0].definition, Some(0x80BC_130B));
    assert_eq!(
        model.assets.particles[0]
            .material_textures
            .iter()
            .map(|(slot, _)| *slot)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        model.assets.particles[0].material_textures[2].1.size,
        [256, 1]
    );
    assert_eq!(model.assets.particles[0].material_slot_omissions, 0);
    assert_eq!(
        model.assets.particles[0]
            .material_samplers
            .iter()
            .map(|sampler| sampler.u)
            .collect::<Vec<_>>(),
        [
            super::texture::AddressMode::Wrap,
            super::texture::AddressMode::Border,
            super::texture::AddressMode::Clamp,
        ]
    );
    assert!(model.assets.particles[0].compute_passes.is_empty());
    assert_eq!(
        model.assets.particles[0].pixel_kind,
        Some(assets::PixelKind::DualMaskRamp)
    );
    let program = model.assets.particles[0]
        .program
        .as_ref()
        .expect("validated particle program");
    assert_eq!(program.sections, [0, 31, 8, 32, 288, 0, 10, 0]);
    assert_eq!(program.bytecode.len(), 369);
    assert_eq!(program.constants.len(), 60);
    assert_eq!(
        program.routes[7].map(|route| (route.bank, route.scalar)),
        Some((1, 12))
    );
    assert_eq!(
        program.routes[6].map(|route| (route.bank, route.scalar)),
        Some((1, 16))
    );
    assert_eq!(program.default_for(5), Some(0.85));
    assert_eq!(program.lifetime_default(), Some(0.85));
    assert_eq!(program.lifetime_ceiling, 0.85_f32 + 0.05);
    let [first, second] = program.state_routes().expect("native state routes");
    assert_eq!((first.0.scalar, first.1.scalar), (8, 12));
    assert_eq!((second.0.scalar, second.1.scalar), (12, 16));
    let state = program.section(3).expect("state initialization section");
    assert_eq!(state.len(), 32);
    assert!(state.windows(4).any(|window| window == [0x3F, 1, 3, 1]));
    assert!(state.windows(4).any(|window| window == [0x3F, 1, 4, 1]));
    let mut registers = assets::Registers::new(program);
    program
        .evaluate_section_with_inputs(1, &mut registers, &[[0.0; 4], [0.0; 4]])
        .unwrap();
    assert_eq!(registers.output(program, 32), Some(1.0));
    program
        .evaluate_section_with_inputs(1, &mut registers, &[[0.0; 4], [3.0; 4]])
        .unwrap();
    let gradient_z = registers.output(program, 32).unwrap();
    assert!((gradient_z - 0.928429).abs() < 1e-5);
    let state_a = [1.0, 2.0, 3.0, 0.5];
    let state_b = [4.0, 5.0, 6.0, 7.0];
    registers.set(2, 2, state_a).unwrap();
    registers.set(2, 3, state_b).unwrap();
    program.evaluate_section(3, &mut registers).unwrap();
    assert_eq!(registers.get(1, 3), Some(state_a));
    assert_eq!(registers.get(1, 4), Some(state_b));
    assert_eq!(registers.output(program, 7), Some(state_a[0]));
    assert_eq!(registers.output(program, 6), Some(state_b[0]));
    assert_eq!(registers.output(program, 0), Some(state_a[3]));
    assert_eq!(registers.output(program, 27), Some(state_b[3]));
    program
        .evaluate_section_with_runtime(6, &mut registers, &[], &[[0.2; 4], [0.8; 4]])
        .unwrap();
    assert_eq!(registers.get(1, 6).unwrap()[3], 0.2);
    assert_eq!(registers.get(1, 7).unwrap()[0], 0.8);
    program.evaluate_section(4, &mut registers).unwrap();
    assert_eq!(registers.get(1, 3), Some(state_a));
    assert_eq!(registers.get(1, 4), Some(state_b));
    for slot in 0..=2 {
        assert!(
            registers
                .get(1, slot)
                .unwrap()
                .iter()
                .all(|value| value.is_finite())
        );
    }
    let writes = program
        .register_writes()
        .expect("bounded particle instructions");
    assert_eq!(writes.iter().filter(|write| write.section == 1).count(), 2);
    assert!(
        writes
            .iter()
            .any(|write| write.section == 1 && write.bank == 5 && write.slot == 1)
    );
    assert_eq!(writes.iter().filter(|write| write.section == 3).count(), 4);
    assert_eq!(writes.iter().filter(|write| write.section == 6).count(), 2);
    assert_eq!(model.assets.particles[0].emitter, Some(0x80EF_5768));
    assert_eq!(model.assets.particles[0].emitter_model, Some(0x80EF_5767));
    assert!(
        model.assets.particles[0].texture.is_some(),
        "particle material texture: {:?}",
        model.assets.particles[0].notice
    );
    assert_eq!(
        model.assets.particles[0].gradient.as_ref().map(|t| t.size),
        Some([256, 1])
    );
    assert!(
        model
            .assets
            .sounds
            .iter()
            .all(|sound| sound.clips.len() >= 3)
    );
    assert!(model.particle_geometry);
    assert!(!model.has_object_mesh());
    assert!(model.particle_sources.is_empty());
    assert!(model.uvs.iter().any(|uv| uv[0] > 0.8 && uv[1] < 0.2));
    assert!(model.normals.iter().any(|normal| normal[2] < -0.9));
    let z_extent = model
        .vertices
        .iter()
        .map(|position| position[2])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), z| {
            (low.min(z), high.max(z))
        });
    assert!(z_extent.1 - z_extent.0 < 2.0);
    assert!(model.triangle_emitter.iter().any(|&emitter| emitter));
    assert!(!model.triangles.is_empty());
    println!(
        "{} particle triangles, {} sound clips",
        model.triangles.len(),
        model
            .assets
            .sounds
            .iter()
            .map(|sound| sound.clips.len())
            .sum::<usize>()
    );
    let manager = crate::investment::discovery::open_packages(packages).unwrap();
    let bytes = manager.read_tag(0x80C7_ACBF).unwrap();
    let wave = assets::decoded_wave(&bytes).unwrap();
    assert_eq!(&wave[..4], b"RIFF");
    assert_eq!(&wave[8..12], b"WAVE");
    assert!(wave.len() > bytes.len());
    if let Some(output) = std::env::var_os("SUNDIAL_PROBE_OUT") {
        std::fs::write(Path::new(&output).join("air-weak-sample.wav"), wave).unwrap();
        if !model.triangles.is_empty() {
            let image = render::styled_image(
                &model,
                render::Camera::default(),
                render::Scene::default(),
                [512, 512],
                0.0,
                render::Style::Textured,
            );
            let rgba: Vec<_> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            let png = export::png(&rgba, image.width(), image.height()).unwrap();
            std::fs::write(Path::new(&output).join("air-weak-preview.png"), png).unwrap();
            for (seconds, name) in [
                (0.17, "air-weak-preview-0.17.png"),
                (0.34, "air-weak-preview-0.34.png"),
            ] {
                let image = render::styled_image(
                    &model,
                    render::Camera::default(),
                    render::Scene {
                        particle_study: true,
                        ..Default::default()
                    },
                    [512, 512],
                    seconds,
                    render::Style::Textured,
                );
                if seconds == 0.17 {
                    let warm = image
                        .pixels
                        .iter()
                        .filter(|pixel| pixel.r() > pixel.b().saturating_add(8) && pixel.r() > 50)
                        .count();
                    assert!(
                        warm > 500,
                        "native mask should produce a visible warm effect"
                    );
                }
                let rgba: Vec<_> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                let png = export::png(&rgba, image.width(), image.height()).unwrap();
                std::fs::write(Path::new(&output).join(name), png).unwrap();
            }
            for (style, name) in [
                (render::Style::Solid, "air-weak-emitter-solid.png"),
                (render::Style::Wireframe, "air-weak-emitter-wireframe.png"),
            ] {
                let image = render::styled_image(
                    &model,
                    render::Camera::default(),
                    render::Scene::default(),
                    [512, 512],
                    0.0,
                    style,
                );
                let rgba: Vec<_> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                let png = export::png(&rgba, image.width(), image.height()).unwrap();
                std::fs::write(Path::new(&output).join(name), png).unwrap();
            }
        }
        let texture = model.assets.particles[0].texture.as_ref().unwrap();
        let png = export::png(&texture.rgba, texture.size[0], texture.size[1]).unwrap();
        std::fs::write(Path::new(&output).join("air-weak-particle.png"), png).unwrap();
        for (slot, texture) in &model.assets.particles[0].material_textures {
            let png = export::png(&texture.rgba, texture.size[0], texture.size[1]).unwrap();
            std::fs::write(
                Path::new(&output).join(format!("air-weak-material-slot-{slot}.png")),
                png,
            )
            .unwrap();
        }
    }
}

/// Compare several installed effects against the Air Weak particle preview.
#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
fn sample_other_effects_render() {
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mut report = String::new();
    let mut failures = Vec::new();
    for tag in [0x80B3_BDC5, 0x80BB_AEFC, 0x80C6_611E, 0x80F8_321E] {
        match load(&packages, tag) {
            Ok(model) => {
                let image = render::animated_image(
                    &model,
                    render::Camera::default(),
                    render::Scene {
                        particle_study: true,
                        ..Default::default()
                    },
                    [512, 512],
                    0.36,
                );
                let background = eframe::egui::Color32::from_rgb(24, 28, 35);
                let visible = image.pixels.iter().filter(|&&p| p != background).count();
                if tag == 0x80C6_611E {
                    assert_eq!(model.assets.particles[0].compute_passes.len(), 3);
                    assert_eq!(
                        model.assets.particles[0]
                            .compute_passes
                            .iter()
                            .map(|pass| pass.phase)
                            .collect::<Vec<_>>(),
                        ["Spawn", "Motion", "Appearance"]
                    );
                } else {
                    assert!(
                        !model.particle_sources.is_empty() || model.particle_geometry,
                        "{tag:08X} has no particle preview"
                    );
                    assert!(visible > 100, "{tag:08X} is blank");
                }
                let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let png = export::png(&rgba, image.width(), image.height()).unwrap();
                std::fs::write(out.join(format!("effect-{tag:08X}.png")), png).unwrap();
                report.push_str(&format!(
                    "{tag:08X}: {} triangles, {} particle systems, {} compute passes, {} sprite sources, {} sounds, {} visible pixels, notices {:?}\n",
                    model.triangles.len(),
                    model.assets.particles.len(),
                    model.assets.particles.iter().map(|particle| particle.compute_passes.len()).sum::<usize>(),
                    model.particle_sources.len(),
                    model.assets.sounds.len(),
                    visible,
                    model.notices,
                ));
                for (system, texture) in
                    model
                        .assets
                        .particles
                        .iter()
                        .enumerate()
                        .filter_map(|(index, particle)| {
                            particle.texture.as_ref().map(|texture| (index, texture))
                        })
                {
                    let transparent = texture
                        .rgba
                        .chunks_exact(4)
                        .filter(|pixel| pixel[3] == 0)
                        .count();
                    let texture_png =
                        export::png(&texture.rgba, texture.size[0], texture.size[1]).unwrap();
                    std::fs::write(
                        out.join(format!("effect-{tag:08X}-texture-{system}.png")),
                        texture_png,
                    )
                    .unwrap();
                    report.push_str(&format!(
                        "  system {system} texture {:08X} {}x{}, {} zero-alpha pixels\n",
                        texture.tag, texture.size[0], texture.size[1], transparent
                    ));
                }
            }
            Err(error) => {
                report.push_str(&format!("{tag:08X}: load failed: {error}\n"));
                failures.push(format!("{tag:08X}: {error}"));
            }
        }
    }
    std::fs::write(out.join("effect-sample.txt"), &report).unwrap();
    println!("{report}");
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn solid_and_wireframe_modes_ignore_texture_color() {
    let model = Model {
        vertices: vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 0.0, 1.0]],
        triangles: vec![[0, 1, 2]],
        uvs: vec![[0.0; 2]; 3],
        triangle_textures: vec![Some(0)],
        textures: vec![texture::Texture {
            tag: 0,
            size: [1, 1],
            rgba: vec![255, 0, 0, 255],
        }],
        ..Default::default()
    };
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        zoom: 1.0,
        pan: [0.0, 0.0],
    };
    let solid = render::styled_image(
        &model,
        camera,
        render::Scene::default(),
        [128, 128],
        0.0,
        render::Style::Solid,
    );
    let wire = render::styled_image(
        &model,
        camera,
        render::Scene::default(),
        [128, 128],
        0.0,
        render::Style::Wireframe,
    );
    let colored = render::image(&model, camera, [128, 128]);
    let background = eframe::egui::Color32::from_rgb(24, 28, 35);
    let count = |image: &eframe::egui::ColorImage| {
        image.pixels.iter().filter(|&&p| p != background).count()
    };
    assert!(count(&wire) > 100 && count(&wire) < count(&solid) / 4);
    assert_ne!(solid, colored);
    assert_ne!(wire, solid);
}

#[test]
fn linked_light_volume_does_not_shrink_a_mesh_preview() {
    let mut model = Model {
        vertices: vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    };
    let camera = render::Camera::default();
    let reference = render::image(&model, camera, [128, 128]);
    model.vertices.extend([
        [100.0, 100.0, 100.0],
        [200.0, 100.0, 100.0],
        [100.0, 200.0, 100.0],
    ]);
    model.triangles.push([3, 4, 5]);
    model.triangle_light = vec![false, true];
    assert_eq!(render::image(&model, camera, [128, 128]), reference);

    model.triangles.remove(0);
    model.triangle_light.remove(0);
    let light = render::image(&model, camera, [128, 128]);
    assert_ne!(
        light,
        eframe::egui::ColorImage::new([128, 128], eframe::egui::Color32::from_rgb(24, 28, 35))
    );
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
fn expanded_preview_reads_component_and_rocket_color_texture() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let packages = Path::new(&packages);
    let component = load(packages, 0x80EFB8CB).unwrap();
    assert_eq!(component.tags, vec![0x80EFB8CA]);
    assert_eq!(component.triangles.len(), 1260);
    let rocket = load(packages, 0x80FB9011).unwrap();
    assert!(rocket.textures.iter().any(|t| t.tag == 0x80C1D0E5));
    assert!(!rocket.textures.iter().any(|t| t.tag == 0x80C1B794));
    let snowball = load(packages, 0x80F4BF98).unwrap();
    assert!(!snowball.textures.is_empty());
    if let Some(output) = std::env::var_os("SUNDIAL_PROJECTILE_OUTPUT") {
        let output = Path::new(&output);
        std::fs::create_dir_all(output).unwrap();
        for (name, model, style) in [
            ("rocket-color", &rocket, render::Style::Textured),
            ("rocket-wireframe", &rocket, render::Style::Wireframe),
            ("snowball-color", &snowball, render::Style::Textured),
        ] {
            let image = render::styled_image(
                model,
                render::Camera::default(),
                render::Scene::default(),
                [400, 400],
                0.0,
                style,
            );
            let mut bytes = b"P6\n400 400\n255\n".to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::write(output.join(format!("{name}.ppm")), bytes).unwrap();
        }
    }
}

#[test]
fn triangle_strips_restart_and_reject_missing_vertices() {
    let bytes = [0_u16, 1, 2, 3, u16::MAX, 4, 5, 6]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        decode::triangles(&bytes, 2, 0, 8, 5, 7).unwrap(),
        vec![[0, 1, 2], [2, 1, 3], [4, 5, 6]]
    );
    assert!(decode::triangles(&bytes, 2, 0, 8, 5, 6).is_err());
    assert!(decode::triangles(&bytes, 2, usize::MAX, 8, 5, 7).is_err());
    assert!(decode::triangles(&bytes, 2, 0, 9, 5, 7).is_err());
    assert!(decode::triangles(&bytes, 2, 0, 8, 3, 7).is_err());
}

#[test]
fn preview_depth_is_independent_of_triangle_submission_order() {
    let mut model = Model {
        vertices: vec![
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [0.0, 0.0, 1.0],
            [-1.0, 1.0, -1.0],
            [1.0, 2.0, -1.0],
            [0.0, 1.0, 1.0],
        ],
        triangles: vec![[0, 1, 2], [3, 4, 5]],
        tags: vec![],
        ..Default::default()
    };
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        zoom: 1.0,
        pan: [0.0, 0.0],
    };
    let first = render::image(&model, camera, [128, 128]);
    let wireframe = render::styled_image(
        &model,
        camera,
        render::Scene::default(),
        [128, 128],
        0.0,
        render::Style::Wireframe,
    );
    model.triangles.reverse();
    assert_eq!(first, render::image(&model, camera, [128, 128]));
    assert_eq!(
        wireframe,
        render::styled_image(
            &model,
            camera,
            render::Scene::default(),
            [128, 128],
            0.0,
            render::Style::Wireframe
        )
    );
    assert!(
        first
            .pixels
            .iter()
            .filter(|&&p| p != first.pixels[0])
            .count()
            > 500
    );
    assert_ne!(
        first,
        render::image(&model, render::Camera { yaw: 0.8, ..camera }, [128, 128])
    );
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
fn chicken_preview_from_installed_packages() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let model = load(Path::new(&packages), 0x80BC_90E3).unwrap();
    assert_eq!(model.tags, vec![0x80EF_B8CA]);
    assert_eq!(model.textures.len(), 1);
    assert_eq!(model.textures[0].tag, 0x80BC_90AF);
    assert!(model.notices.is_empty(), "{:?}", model.notices);
    assert!(model.triangle_textures.iter().all(Option::is_some));
    assert_eq!(model.uvs.len(), model.vertices.len());
    assert!(!model.vertices.is_empty());
    assert!(!model.triangles.is_empty());
    eprintln!(
        "Chicken: {} vertices, {} triangles, model {:08X}",
        model.vertices.len(),
        model.triangles.len(),
        model.tags[0]
    );
    if let Some(output) = std::env::var_os("SUNDIAL_PREVIEW_OUTPUT") {
        let image = render::image(&model, render::Camera::default(), [640, 640]);
        let mut bytes = b"P6\n640 640\n255\n".to_vec();
        bytes.extend(
            image
                .pixels
                .iter()
                .flat_map(|color| [color.r(), color.g(), color.b()]),
        );
        std::fs::write(output, bytes).unwrap();
    }
}

#[test]
fn bc7_color_decoding_checks_block_size_and_crops_partial_blocks() {
    // BC7 mode 6, equal near-red endpoints and index zero for every pixel.
    let mut block = [0_u8; 16];
    let mut bit = 0;
    let mut write = |value: u32, width: usize| {
        for i in 0..width {
            block[bit / 8] |= (((value >> i) & 1) as u8) << (bit % 8);
            bit += 1;
        }
    };
    write(64, 7);
    for value in [127, 127, 0, 0, 0, 0, 127, 127] {
        write(value, 7);
    }
    write(3, 2);
    let pixels = texture::decode(&block, 99, 3, 2).unwrap();
    assert_eq!(pixels, [255, 1, 1, 255].repeat(6));
    assert!(texture::decode(&block[..15], 99, 3, 2).is_err());
    assert!(texture::decode(&block, 99, 5, 4).is_err());
    assert!(texture::decode(&block, 99, usize::MAX, 4).is_err());
}

#[test]
fn bilinear_texture_sampling_wraps_both_coordinates() {
    let texture = texture::Texture {
        tag: 0,
        size: [2, 2],
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    };
    assert_eq!(texture.sample([0.25, 0.25]), [255.0, 0.0, 0.0]);
    assert_eq!(texture.sample([-0.75, 1.25]), [255.0, 0.0, 0.0]);
    assert_eq!(texture.sample([0.5, 0.5]), [127.5; 3]);
}

#[test]
fn each_triangle_uses_its_own_material_texture() {
    let model = Model {
        vertices: vec![
            [-2.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [2.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
        ],
        triangles: vec![[0, 1, 2], [3, 4, 5]],
        uvs: vec![[0.5, 0.5]; 6],
        triangle_textures: vec![Some(0), Some(1)],
        textures: vec![
            texture::Texture {
                tag: 1,
                size: [1, 1],
                rgba: vec![255, 0, 0, 255],
            },
            texture::Texture {
                tag: 2,
                size: [1, 1],
                rgba: vec![0, 0, 255, 255],
            },
        ],
        ..Default::default()
    };
    let image = render::image(
        &model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            zoom: 1.0,
            pan: [0.0, 0.0],
        },
        [128, 128],
    );
    assert!(
        image
            .pixels
            .iter()
            .filter(|p| p.r() > 50 && p.b() == 0)
            .count()
            > 100
    );
    assert!(
        image
            .pixels
            .iter()
            .filter(|p| p.b() > 50 && p.r() == 0)
            .count()
            > 100
    );
}

/// Loads a random sample of installed weapons through the preview and reports any failure.
#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
fn sample_weapons_load_in_the_preview() {
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    assert!(!donors.is_empty(), "Need weapon donors to verify previews");
    let mut picked: Vec<usize> = donors
        .iter()
        .enumerate()
        .filter(|(_, d)| d.name == "Ghost Primus")
        .map(|(i, _)| i)
        .collect();
    // Fixed-seed linear congruential sample so a rerun checks the same weapons.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    while picked.len() < 21 && picked.len() < donors.len() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let index = (state >> 33) as usize % donors.len();
        if !picked.contains(&index) {
            picked.push(index);
        }
    }
    let mut report = String::new();
    let mut failures = 0;
    for index in picked {
        let donor = &donors[index];
        let line = match catalog.preview_loadout(donor.hash) {
            None => {
                failures += 1;
                "no preview loadout".to_owned()
            }
            Some(loadout) => {
                let appearance = catalog.preview_appearance(&loadout);
                match appearance::load_reported(
                    &packages,
                    &appearance,
                    &crate::model_preview::Load::default(),
                ) {
                    Ok(model) => format!(
                        "ok: {} triangles, {} textures, {} notices",
                        model.triangles.len(),
                        model.textures.len(),
                        model.notices.len()
                    ),
                    Err(error) => {
                        failures += 1;
                        format!("FAILED: {error}")
                    }
                }
            }
        };
        report.push_str(&format!(
            "{} ({:08X}) [{}]: {line}\n",
            donor.name, donor.hash, donor.type_name
        ));
    }
    std::fs::write(out.join("sample.txt"), &report).unwrap();
    assert_eq!(failures, 0, "{report}");
}
