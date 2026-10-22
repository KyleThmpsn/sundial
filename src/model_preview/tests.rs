use super::*;

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "One installed effect sequence is checked node by node against the packages"
)]
fn air_weak_fx_sequence_opens_particles_and_sounds() {
    let packages = crate::test_support::preview_packages();
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
    if let Some(output) =
        crate::test_support::artifacts("probes").map(std::path::PathBuf::into_os_string)
    {
        std::fs::write(Path::new(&output).join("air-weak-sample.wav"), wave).unwrap();
        if !model.triangles.is_empty() {
            let image = render::styled_image(
                &model,
                render::Camera::default(),
                render::Scene::unprocessed(),
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
                        filmic: false,
                        bloom: false,
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
                    render::Scene::unprocessed(),
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
