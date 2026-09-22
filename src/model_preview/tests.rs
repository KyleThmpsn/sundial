use super::*;

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
    };
    let solid = render::styled_image(&model, camera, [128, 128], 0.0, render::Style::Solid);
    let wire = render::styled_image(&model, camera, [128, 128], 0.0, render::Style::Wireframe);
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
            let image =
                render::styled_image(model, render::Camera::default(), [400, 400], 0.0, style);
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
    };
    let first = render::image(&model, camera, [128, 128]);
    let wireframe = render::styled_image(&model, camera, [128, 128], 0.0, render::Style::Wireframe);
    model.triangles.reverse();
    assert_eq!(first, render::image(&model, camera, [128, 128]));
    assert_eq!(
        wireframe,
        render::styled_image(&model, camera, [128, 128], 0.0, render::Style::Wireframe)
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
    assert!(model.vertices.len() > 100);
    assert!(model.triangles.len() > 100);
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

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
#[expect(
    clippy::cognitive_complexity,
    reason = "A probe that dumps every intermediate of one render in one place"
)]
fn textured_preview_dumps_frames_samples_and_timings() {
    use std::time::Instant;
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let catalog =
        crate::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
            .unwrap();
    let mut report = String::new();
    {
        let manager = crate::investment::discovery::open_packages(&packages).unwrap();
        let sets = manager.get_all_by_reference(0x8080_6B99);
        report.push_str(&format!(
            "global texture sets: {}
",
            sets.len()
        ));
        for (tag, _) in &sets {
            let bytes = manager.read_tag(*tag).unwrap();
            let slots: Vec<String> = (0..4)
                .map(|i| format!("{:08X}", u32_at(&bytes, 8 + i * 4).unwrap_or(0)))
                .collect();
            report.push_str(&format!(
                "  set {:08X} len {} slots {slots:?}
",
                tag.0,
                bytes.len()
            ));
            for i in 0..4 {
                let texture = u32_at(&bytes, 8 + i * 4).unwrap_or(0);
                if let Some(entry) = manager.get_entry(texture) {
                    let header = manager.read_tag(texture).unwrap();
                    report.push_str(&format!(
                        "    texture {texture:08X} type {} sub {} format {} size {}x{} depth {} array {} large {:08X}: {:?}
",
                        entry.file_type, entry.file_subtype, u32_at(&header, 4).unwrap(), u16_at(&header, 0x0E).unwrap(),
                        u16_at(&header, 0x10).unwrap(), u16_at(&header, 0x12).unwrap(), u16_at(&header, 0x14).unwrap(),
                        u32_at(&header, 0x24).unwrap(), texture::load(&manager, texture).map(|t| t.size)
                    ));
                }
            }
        }
    }
    for name in ["Age-Old Bond", "Better Devils"] {
        let donor = catalog
            .weapon_donors()
            .into_iter()
            .find(|d| d.name == name)
            .unwrap();
        let loadout = catalog.preview_loadout(donor.hash).unwrap();
        let appearance = catalog.preview_appearance(&loadout);
        report.push_str(&format!(
            "{name}: arrangement {} dyes {:?}
",
            appearance.arrangement, appearance.dyes
        ));
        let slug = name.to_lowercase().replace(' ', "-");
        {
            let manager = crate::investment::discovery::open_packages(&packages).unwrap();
            let indices: Vec<u16> = appearance.dyes.iter().map(|d| d.1).collect();
            let materials = crate::weapon_dyes::material::load(&manager, &indices).unwrap();
            for (index, material) in &materials {
                if let Ok(material) = material {
                    for (i, v) in material.vectors.iter().enumerate() {
                        report.push_str(&format!(
                            "  dye {index} vector {i:2}: {v:?}
"
                        ));
                    }
                }
            }
        }
        let started = Instant::now();
        let model = weapon::load(&packages, &appearance).unwrap();
        report.push_str(&format!("{name}: load {:?}, {} vertices, {} triangles, {} textures, animation {}, shader animation {}\n",
            started.elapsed(), model.vertices.len(), model.triangles.len(), model.textures.len(), model.animation.is_some(), model.has_shader_animation()));
        for (i, t) in model.textures.iter().enumerate() {
            report.push_str(&format!(
                "  texture {i}: tag {:08X} {}x{}\n",
                t.tag, t.size[0], t.size[1]
            ));
        }
        let mut slots = std::collections::BTreeMap::new();
        for &slot in &model.triangle_dyes {
            *slots.entry(slot).or_insert(0usize) += 1;
        }
        report.push_str(&format!(
            "  part dye slots (slot: triangles) {slots:?}
"
        ));
        for (i, dye) in model.dyes.iter().enumerate() {
            if let Some(d) = dye {
                let s = &d.surface;
                report.push_str(&format!("  dye slot {i}: albedo {:?} worn {:?} params {:?} worn_params {:?} wear {:?} rough {:?} emissive {:?} detail {:?} normal {:?}
",
                    s.albedo, s.worn_albedo, s.params, s.worn_params, s.wear, s.roughness, s.emissive, d.detail, d.normal));
            } else {
                report.push_str(&format!(
                    "  dye slot {i}: none
"
                ));
            }
        }
        if let Some(Some(g)) = model
            .triangle_gearstacks
            .first()
            .map(|g| g.map(|i| &model.textures[i]))
        {
            let mut bands = [0usize; 8];
            for px in g.rgba.chunks_exact(4) {
                bands[(px[3] / 32) as usize] += 1;
            }
            report.push_str(&format!(
                "  gearstack alpha histogram (32-wide bands) {bands:?}
"
            ));
        }
        // Per slot: gearstack alpha and blue histograms over the texels each triangle touches.
        for want in 0u8..6 {
            let mut alpha = [0usize; 8];
            let mut blue = [0usize; 8];
            let mut seen = 0usize;
            for (tri, &slot) in model.triangle_dyes.iter().enumerate() {
                if slot != want {
                    continue;
                }
                let Some(Some(g)) = model.triangle_gearstacks.get(tri) else {
                    continue;
                };
                let g = &model.textures[*g];
                let t = model.triangles[tri];
                for vertex in t {
                    let uv = model.uvs[vertex as usize];
                    let px = g.sample_rgba(uv);
                    alpha[(px[3] as usize / 32).min(7)] += 1;
                    blue[(px[2] as usize / 32).min(7)] += 1;
                    seen += 1;
                }
            }
            if seen > 0 {
                report.push_str(&format!(
                    "  slot {want}: {seen} samples, alpha bands {alpha:?}, blue bands {blue:?}
"
                ));
            }
        }
        let dyes = shader::dyes(&model, 0.0);
        for want in [0u8, 1, 2, 4] {
            let Some(tri) = model.triangle_dyes.iter().position(|&s| s == want) else {
                continue;
            };
            let t = model.triangles[tri];
            let uv: [f32; 2] = std::array::from_fn(|i| {
                (0..3).map(|k| model.uvs[t[k] as usize][i]).sum::<f32>() / 3.0
            });
            let b = shader::Bindings::new(&model, tri, &dyes);
            let albedo = b.albedo.map(|a| a.sample_rgba(uv));
            let gear = model.triangle_gearstacks[tri].map(|i| model.textures[i].sample_rgba(uv));
            let norm = model.triangle_normals[tri].map(|i| model.textures[i].sample_rgba(uv));
            let shaded = b.shade(uv, [0.0, 0.0, -1.0], None);
            report.push_str(&format!("  slot {want} tri {tri} uv {uv:?}: albedo {albedo:?} gearstack {gear:?} normal {norm:?} -> {shaded:?}
"));
        }
        report.push_str(&format!(
            "  constant emissive triangles {}
",
            model
                .triangle_constant
                .iter()
                .filter(|c| c.is_some())
                .count()
        ));
        {
            // Channel histograms over the texels the emissive panels cover.
            let mut bands = [[0usize; 8]; 12];
            let mut samples = 0usize;
            for (tri, constant) in model.triangle_constant.iter().enumerate() {
                if constant.is_none() {
                    continue;
                }
                let t = model.triangles[tri];
                let uv = |k: usize| model.uvs[t[k] as usize];
                for a in 0..8 {
                    for b in 0..(8 - a) {
                        let (fa, fb) = (a as f32 / 8.0 + 0.05, b as f32 / 8.0 + 0.05);
                        let fc = 1.0 - fa - fb;
                        let p: [f32; 2] =
                            std::array::from_fn(|i| fa * uv(0)[i] + fb * uv(1)[i] + fc * uv(2)[i]);
                        let sources = [
                            model.triangle_textures[tri],
                            model.triangle_gearstacks[tri],
                            model.triangle_normals[tri],
                        ];
                        for (which, index) in sources.iter().enumerate() {
                            if let Some(index) = index {
                                let px = model.textures[*index].sample_rgba(p);
                                for c in 0..4 {
                                    bands[which * 4 + c][(px[c] as usize / 32).min(7)] += 1;
                                }
                            }
                        }
                        samples += 1;
                    }
                }
            }
            let names = [
                "albedo.r", "albedo.g", "albedo.b", "albedo.a", "gear.r", "gear.g", "gear.b",
                "gear.a", "normal.r", "normal.g", "normal.b", "normal.a",
            ];
            report.push_str(&format!(
                "  panel samples {samples}
"
            ));
            for (name, band) in names.iter().zip(bands) {
                report.push_str(&format!(
                    "    {name}: {band:?}
"
                ));
            }
        }
        let with = |v: &Vec<Option<usize>>| v.iter().filter(|t| t.is_some()).count();
        report.push_str(&format!(
            "  triangles with albedo {}, gearstack {}, normal {}\n",
            with(&model.triangle_textures),
            with(&model.triangle_gearstacks),
            with(&model.triangle_normals)
        ));
        for n in &model.notices {
            report.push_str(&format!("  notice: {n}\n"));
        }
        for (label, size, style) in [
            (
                "textured-640",
                [640usize, 480usize],
                render::Style::Textured,
            ),
            ("textured-320", [320, 240], render::Style::Textured),
            ("solid-640", [640, 480], render::Style::Solid),
        ] {
            let started = Instant::now();
            let image = render::styled_image(&model, render::Camera::default(), size, 0.0, style);
            let elapsed = started.elapsed();
            let started = Instant::now();
            let _ = render::styled_image(&model, render::Camera::default(), size, 0.5, style);
            report.push_str(&format!(
                "  {label}: {elapsed:?} then {:?}\n",
                started.elapsed()
            ));
            let mut bytes = format!("P6\n{} {}\n255\n", size[0], size[1]).into_bytes();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::write(out.join(format!("{slug}-{label}.ppm")), bytes).unwrap();
        }
        // Flat colour per dye slot so geometry-to-slot assignment can be compared with the game.
        let mut flat = model;
        let colors = [
            [1.0, 0.1, 0.1],
            [0.1, 1.0, 0.1],
            [0.1, 0.1, 1.0],
            [1.0, 1.0, 0.1],
            [1.0, 0.1, 1.0],
            [0.1, 1.0, 1.0],
        ];
        for (i, dye) in flat.dyes.iter_mut().enumerate() {
            if let Some(d) = dye {
                d.surface.albedo = colors[i];
                d.surface.worn_albedo = colors[i];
                d.surface.emissive = [0.0; 3];
                d.detail = None;
            }
        }
        for (label, yaw) in [
            ("slots-front", -0.65f32),
            ("slots-back", -0.65 + std::f32::consts::PI),
        ] {
            let camera = render::Camera {
                yaw,
                pitch: 0.25,
                zoom: 1.0,
            };
            let image =
                render::styled_image(&flat, camera, [640, 480], 0.0, render::Style::Textured);
            let mut bytes = b"P6
640 480
255
"
            .to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::write(out.join(format!("{slug}-{label}.ppm")), bytes).unwrap();
        }
        let model = flat;
        if let Some(t) = &model.iridescence {
            report.push_str(&format!(
                "  iridescence lookup {}x{}
",
                t.size[0], t.size[1]
            ));
            let mut bytes = format!(
                "P6
{} {}
255
",
                t.size[0], t.size[1]
            )
            .into_bytes();
            bytes.extend(t.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(out.join(format!("{slug}-iridescence.ppm")), bytes).unwrap();
        }
        for (i, t) in model.textures.iter().enumerate() {
            let mut bytes = format!("P6\n{} {}\n255\n", t.size[0], t.size[1]).into_bytes();
            bytes.extend(t.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(out.join(format!("{slug}-texture-{i}.ppm")), bytes).unwrap();
        }
    }
    std::fs::write(out.join("report.txt"), report).unwrap();
}

/// Loads a random sample of installed weapons through the preview and reports any failure.
#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
fn sample_weapons_load_in_the_preview() {
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let catalog =
        crate::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
            .unwrap();
    let donors = catalog.weapon_donors();
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
            None => "no preview loadout".to_owned(),
            Some(loadout) => {
                let appearance = catalog.preview_appearance(&loadout);
                match weapon::load(&packages, &appearance) {
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

/// Dumps the resources of named objects the preview cannot load, to find what to support.
#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
fn probe_unsupported_objects() {
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let manager = crate::investment::discovery::open_packages(&packages).unwrap();
    let catalog = crate::sandbox_perk::projectile::catalog::cached_only(&packages)
        .unwrap()
        .expect("cached effects catalog");
    let wanted = ["alpha_strike", "harpy", "ritual_seeker", "seeker"];
    let mut report = String::new();
    let mut seen = 0;
    for entry in &catalog.entries {
        let text = format!("{:?} {:?}", entry.native_name, entry.native_paths).to_lowercase();
        if !wanted.iter().any(|w| text.contains(w)) {
            continue;
        }
        seen += 1;
        if seen > 12 {
            break;
        }
        report.push_str(&format!(
            "\n== {:08X} {:?} {:?} {:?}\n",
            entry.graph, entry.kind, entry.native_name, entry.native_paths
        ));
        match load(&packages, entry.graph) {
            Ok(model) => {
                report.push_str(&format!("  loads: {} triangles\n", model.triangles.len()))
            }
            Err(error) => report.push_str(&format!("  FAILS: {error}\n")),
        }
        let Some(header) = manager.get_entry(entry.graph) else {
            continue;
        };
        report.push_str(&format!(
            "  class {:08X} type {}/{} size {}\n",
            header.reference, header.file_type, header.file_subtype, header.file_size
        ));
        if header.reference != ENTITY {
            continue;
        }
        let Ok(entity) = manager.read_tag(entry.graph) else {
            continue;
        };
        let Ok((count, rows)) = array(&entity, 0x10, 0x8080_9C04, 12, 4096) else {
            continue;
        };
        for index in 0..count {
            let resource = u32_at(&entity, rows + index * 12).unwrap_or(0);
            let Some(res_entry) = manager.get_entry(resource) else {
                report.push_str(&format!("  resource {resource:08X}: missing\n"));
                continue;
            };
            let Ok(bytes) = manager.read_tag(resource) else {
                continue;
            };
            let class_at = |p: usize| -> String {
                pointer(&bytes, p)
                    .ok()
                    .and_then(|o| o.checked_sub(4))
                    .and_then(|o| u32_at(&bytes, o).ok())
                    .map_or("-".into(), |c| format!("{c:08X}"))
            };
            report.push_str(&format!(
                "  resource {resource:08X} class {:08X} size {} header {} data {}\n",
                res_entry.reference,
                bytes.len(),
                class_at(0x10),
                class_at(0x18)
            ));
            // Children candidates: dump the arrays at the data struct's known offsets.
            if let Ok(data) = pointer(&bytes, 0x18) {
                for offset in [0x78usize, 0x88, 0x100, 0x168] {
                    if let Ok((n, _, r, class)) = native_array_at(&bytes, data + offset)
                        && n > 0
                        && n < 512
                        && r < bytes.len()
                    {
                        let head: Vec<String> = (0..6)
                            .map(|i| {
                                u32_at(&bytes, r + i * 4).map_or("?".into(), |v| format!("{v:08X}"))
                            })
                            .collect();
                        report.push_str(&format!("    array at data+{offset:#x}: {n} rows class {class:08X} first words {head:?}\n"));
                    }
                }
            }
        }
    }
    std::fs::write(out.join("objects.txt"), &report).unwrap();
    println!("{report}");
}

/// Survey: across the whole effects catalog, which objects have no direct model, and what
/// would rescue them (child entities with models, transparent-only stages, nothing).
#[test]
#[ignore]
fn survey_model_coverage() {
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    let manager = crate::investment::discovery::open_packages(&packages).unwrap();
    let catalog = crate::sandbox_perk::projectile::catalog::cached(&packages, &manager).unwrap();
    let has_model = |tag: u32| -> Option<bool> {
        let entity = manager.read_tag(tag).ok()?;
        let (count, rows) = array(&entity, 0x10, 0x8080_9C04, 12, 4096).ok()?;
        for index in 0..count {
            let resource = u32_at(&entity, rows + index * 12).ok()?;
            let Ok(bytes) = manager.read_tag(resource) else {
                continue;
            };
            let header = pointer(&bytes, 0x10)
                .ok()
                .and_then(|o| o.checked_sub(4))
                .and_then(|o| u32_at(&bytes, o).ok());
            if header == Some(0x8080_72B8) {
                return Some(true);
            }
        }
        Some(false)
    };
    let child_entities = |tag: u32| -> Vec<u32> {
        let mut found = Vec::new();
        let Ok(entity) = manager.read_tag(tag) else {
            return found;
        };
        let Ok((count, rows)) = array(&entity, 0x10, 0x8080_9C04, 12, 4096) else {
            return found;
        };
        for index in 0..count {
            let Ok(resource) = u32_at(&entity, rows + index * 12) else {
                continue;
            };
            let Ok(bytes) = manager.read_tag(resource) else {
                continue;
            };
            for offset in (0..bytes.len().saturating_sub(3)).step_by(4) {
                let word = u32_at(&bytes, offset).unwrap();
                if word & 0xFF00_0000 != 0x8000_0000 || word == tag || word == resource {
                    continue;
                }
                if manager
                    .get_entry(word)
                    .is_some_and(|e| e.reference == ENTITY)
                    && !found.contains(&word)
                {
                    found.push(word);
                }
            }
        }
        found
    };
    let mut total = 0;
    let mut with_model = 0;
    let mut not_entity = 0;
    let mut rescued_by_child = 0;
    let mut child_examples = Vec::new();
    let mut orphan_kinds = std::collections::BTreeMap::new();
    let mut orphan_examples = Vec::new();
    for entry in &catalog.entries {
        total += 1;
        let Some(header) = manager.get_entry(entry.graph) else {
            continue;
        };
        if header.reference != ENTITY {
            not_entity += 1;
            continue;
        }
        if has_model(entry.graph) == Some(true) {
            with_model += 1;
            continue;
        }
        let mut visited = BTreeSet::new();
        let mut queue = vec![(entry.graph, 0usize)];
        let mut rescued = None;
        while let Some((tag, depth)) = queue.pop() {
            if !visited.insert(tag) || depth > 3 {
                continue;
            }
            if tag != entry.graph && has_model(tag) == Some(true) {
                rescued = Some((tag, depth));
                break;
            }
            for child in child_entities(tag) {
                queue.push((child, depth + 1));
            }
        }
        if let Some((tag, depth)) = rescued {
            rescued_by_child += 1;
            if child_examples.len() < 15 {
                child_examples.push(format!(
                    "{:08X} {:?} {:?} -> child {tag:08X} at depth {depth}",
                    entry.graph,
                    entry.kind,
                    entry.native_paths.first()
                ));
            }
        } else {
            *orphan_kinds
                .entry(format!("{:?}", entry.kind))
                .or_insert(0usize) += 1;
            if orphan_examples.len() < 15 {
                orphan_examples.push(format!(
                    "{:08X} {:?} {:?}",
                    entry.graph,
                    entry.kind,
                    entry.native_paths.first()
                ));
            }
        }
    }
    let report = format!(
        "total {total} not_entity {not_entity} with_model {with_model} rescued_by_child {rescued_by_child} orphans {}
orphan kinds {orphan_kinds:?}
child examples:
{}
orphan examples:
{}
",
        total - not_entity - with_model - rescued_by_child,
        child_examples.join("
"),
        orphan_examples.join("
")
    );
    std::fs::write(out.join("survey.txt"), &report).unwrap();
    println!("{report}");
}
