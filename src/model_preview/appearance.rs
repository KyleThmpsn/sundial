//! Installed investment appearance assignments, separate from gameplay entities.
use super::*;
use crate::package_authoring::resolve_live_named_tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub arrangement: u16,
    /// Effective channel/reference pairs after default, custom, shader and locked precedence.
    pub dyes: Vec<(i8, u16)>,
    /// Detail textures an editor gives dyes it has not built yet.
    pub dye_textures: Vec<DyeTextureOverride>,
}

/// The detail textures one dye draws with instead of its own, by texture tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DyeTextureOverride {
    /// The dye's channel within the item's gear type, from 0 to 2.
    pub channel: usize,
    pub detail: Option<u32>,
    pub normal: Option<u32>,
}

/// The composed appearance with no progress reporting, for tests.
#[cfg(test)]
pub(crate) fn load(packages: &Path, appearance: &Appearance) -> Result<Model, String> {
    load_reported(packages, appearance, &super::Load::default())
}

pub(crate) fn load_reported(
    packages: &Path,
    appearance: &Appearance,
    cancel: &super::Load,
) -> Result<Model, String> {
    cancel.say("Opening packages", 0, 0);
    let manager = crate::investment::discovery::open_packages(packages)?;
    let globals = manager.read_tag(resolve_live_named_tag(
        &manager,
        "investment_globals",
        None,
    )?)?;
    let table = checked(&manager, u32_at(&globals, 0x430)?, 0x8080_5DF5)?;
    let assignments = assignment_hashes(&table, appearance.arrangement)?;
    let assets = manager.read_tag(resolve_live_named_tag(&manager, "investment_assets", None)?)?;
    let map = checked(&manager, u32_at(&assets, 0x20)?, 0x8080_56EA)?;
    let (count, rows) = array(&map, 8, 0x8080_56EC, 8, 100_000)?;
    let mut entities = BTreeSet::new();
    for hash in assignments {
        for index in 0..count {
            let row = rows + index * 8;
            if u32_at(&map, row)? != hash {
                continue;
            }
            let relation = checked(&manager, u32_at(&map, row + 4)?, 0x8080_744A)?;
            entities.insert(u32_at(&relation, 0x10)?);
            break;
        }
    }
    if entities.is_empty() {
        return Err("This appearance has no installed model assignment".into());
    }
    if entities.len() > 64 {
        return Err("This appearance exceeds the preview object budget".into());
    }
    let mut result = Model::default();
    let total = entities.len();
    for (done, entity) in entities.into_iter().enumerate() {
        if cancel.stopped() {
            return Err(super::CANCELLED.into());
        }
        cancel.say(
            if total > 1 {
                format!("Reading part {} of {total}", done + 1)
            } else {
                "Reading weapon".to_owned()
            },
            done,
            total,
        );
        let model = match load_with_manager(&manager, entity, cancel, None) {
            Ok(model) => model,
            Err(error) => {
                result
                    .notices
                    .push(format!("Object 0x{entity:08X}: {error}"));
                continue;
            }
        };
        let base = result.vertices.len() as u32;
        let texture_map: Vec<_> = model
            .textures
            .into_iter()
            .map(|texture| {
                if let Some(index) = result.textures.iter().position(|t| t.tag == texture.tag) {
                    return Some(index);
                }
                if result.textures.len() >= MAX_TEXTURES {
                    return None;
                }
                result.textures.push(texture);
                Some(result.textures.len() - 1)
            })
            .collect();
        if result.vertices.len() + model.vertices.len() > MAX_VERTICES
            || result.triangles.len() + model.triangles.len() > MAX_TRIANGLES
        {
            return Err("This appearance exceeds the preview geometry budget".into());
        }
        let first_triangle = result.triangles.len();
        let effect_base = result.effects.len();
        for mut material in model.effects {
            material.remap_textures(&texture_map);
            result.effects.push(material);
        }
        let part_triangles = model.triangles.len();
        result
            .triangles
            .extend(model.triangles.into_iter().map(|t| t.map(|v| v + base)));
        result.triangle_light.resize(first_triangle, false);
        result.triangle_light.extend(
            (0..part_triangles)
                .map(|index| model.triangle_light.get(index).copied().unwrap_or(false)),
        );
        result.triangle_emitter.resize(first_triangle, false);
        result.triangle_emitter.extend(
            (0..part_triangles)
                .map(|index| model.triangle_emitter.get(index).copied().unwrap_or(false)),
        );
        for mut source in model.particle_sources {
            let Some(texture) = texture_map.get(source.texture).copied().flatten() else {
                continue;
            };
            source.texture = texture;
            source.gradient = source
                .gradient
                .and_then(|index| texture_map.get(index).copied().flatten());
            result.particle_sources.push(source);
        }
        result.particle_geometry |= model.particle_geometry;
        result.light_geometry |= model.light_geometry;
        result.triangle_textures.extend(
            model
                .triangle_textures
                .into_iter()
                .map(|t| t.and_then(|i| texture_map[i])),
        );
        result.triangle_dyes.extend(model.triangle_dyes);
        result.triangle_dye_maps.resize(first_triangle, None);
        result
            .triangle_dye_maps
            .extend((0..part_triangles).map(|index| {
                model
                    .triangle_dye_maps
                    .get(index)
                    .copied()
                    .flatten()
                    .and_then(|mut map| {
                        map.texture = texture_map[map.texture]?;
                        Some(map)
                    })
            }));
        result.triangle_clip.extend(model.triangle_clip);
        result.triangle_constant.extend(model.triangle_constant);
        result.triangle_effects.resize(first_triangle, None);
        result
            .triangle_effects
            .extend((0..part_triangles).map(|index| {
                model
                    .triangle_effects
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|index| effect_base + index)
            }));
        result.triangle_gearstacks.extend(
            model
                .triangle_gearstacks
                .into_iter()
                .map(|t| t.and_then(|i| texture_map[i])),
        );
        result.triangle_normals.extend(
            model
                .triangle_normals
                .into_iter()
                .map(|t| t.and_then(|i| texture_map[i])),
        );
        result.normals.extend(model.normals);
        result.weights.extend(model.weights);
        for mut motion in model.motions {
            motion.vertices =
                motion.vertices.start + base as usize..motion.vertices.end + base as usize;
            result.motions.push(motion);
        }
        result
            .tangents
            .resize(result.vertices.len(), [0.0, 0.0, 0.0, 1.0]);
        result.colors.resize(result.vertices.len(), [1.0; 4]);
        result.tangents.extend((0..model.vertices.len()).map(|i| {
            model
                .tangents
                .get(i)
                .copied()
                .unwrap_or([0.0, 0.0, 0.0, 1.0])
        }));
        result.colors.extend(
            (0..model.vertices.len()).map(|i| model.colors.get(i).copied().unwrap_or([1.0; 4])),
        );
        result.vertices.extend(model.vertices);
        result.uvs.extend(model.uvs);
        result.detail_uvs.resize(base as usize, [0.0; 2]);
        result.detail_uvs.extend(model.detail_uvs);
        result.tags.extend(model.tags);
        result.notices.extend(model.notices);
        result.assets.particles.extend(model.assets.particles);
        result.assets.sounds.extend(model.assets.sounds);
        result.assets.lights.extend(model.assets.lights);
        result.assets.children.extend(model.assets.children);
        result.assets.components.extend(model.assets.components);
        result.assets.effect_nodes.extend(model.assets.effect_nodes);
        result.assets.references.extend(model.assets.references);
        if result.assets.image.is_none() {
            result.assets.image = model.assets.image;
        }
    }
    if result.triangles.is_empty() {
        return Err(format!(
            "This appearance has no supported geometry. {}",
            result.notices.join(" ")
        ));
    }
    apply_colors(&manager, &globals, appearance, &mut result)?;
    if !appearance.dyes.is_empty() {
        result.iridescence = texture::iridescence(&manager);
    }
    result.notices.sort();
    result.notices.dedup();
    let mut seen = BTreeSet::new();
    result
        .assets
        .particles
        .retain(|particle| seen.insert(particle.tag));
    seen.clear();
    result.assets.sounds.retain(|sound| seen.insert(sound.tag));
    seen.clear();
    result.assets.lights.retain(|light| seen.insert(light.tag));
    seen.clear();
    result.assets.children.retain(|tag| seen.insert(*tag));
    seen.clear();
    result
        .assets
        .components
        .retain(|component| seen.insert(component.tag));
    seen.clear();
    result
        .assets
        .references
        .retain(|reference| seen.insert(reference.tag));
    let mut nodes = BTreeSet::new();
    result
        .assets
        .effect_nodes
        .retain(|node| nodes.insert((node.source, node.index)));
    Ok(result)
}

/// The dye keys of each gear type's armor, cloth and suit channels: armor, weapons, ships,
/// Sparrows and Ghost Shells.
const GEAR_KEYS: [[i8; 3]; 5] = [[0, 1, 2], [4, 5, 6], [7, 8, 9], [10, 11, 12], [13, 14, 15]];

/// The keys that paint this appearance. An item's own dyes name only its gear type's keys. A
/// shader names every gear type's, and a shader's dyes alone preview on a weapon.
fn gear_keys(dyes: &[(i8, u16)]) -> [i8; 3] {
    let named = GEAR_KEYS
        .iter()
        .filter(|keys| dyes.iter().any(|(key, _)| keys.contains(key)))
        .collect::<Vec<_>>();
    match named.as_slice() {
        [only] => **only,
        _ => GEAR_KEYS[1],
    }
}

fn apply_colors(
    manager: &PackageManager,
    globals: &[u8],
    appearance: &Appearance,
    model: &mut Model,
) -> Result<(), String> {
    if appearance.dyes.is_empty() {
        return Ok(());
    }
    let channels = checked(manager, u32_at(globals, 0x420)?, 0x8080_5BDE)?;
    let (count, _) = array(&channels, 8, 0x8080_5BE2, 4, 4096)?;
    let indices: Vec<_> = appearance.dyes.iter().map(|row| row.1).collect();
    let colors = crate::dyes::material::load(manager, &indices)?;
    let keys = gear_keys(&appearance.dyes);
    let mut slots = [None; 6];
    for &(channel, reference) in &appearance.dyes {
        if channel < 0 || channel as usize >= count {
            continue;
        }
        let Some(slot) = keys.iter().position(|&key| key == channel) else {
            continue;
        };
        model.dye_animations.retain(|(s, _)| *s != slot);
        match colors.get(&reference) {
            None => continue,
            Some(Ok(material)) => {
                let replaced = appearance
                    .dye_textures
                    .iter()
                    .find(|texture| texture.channel == slot);
                let mut load =
                    |replacement: Option<u32>, own: &Result<Option<u32>, String>| match replacement
                        .map(|tag| Ok(Some(tag)))
                        .unwrap_or_else(|| own.clone())
                    {
                        Ok(Some(tag)) => load_detail(manager, tag, model),
                        Ok(None) => None,
                        Err(error) => {
                            model.notices.push(error);
                            None
                        }
                    };
                let detail = load(
                    replaced.and_then(|texture| texture.detail),
                    &material.detail,
                );
                let normal = load(
                    replaced.and_then(|texture| texture.normal),
                    &material.normal,
                );
                match &material.animation {
                    Ok(Some(animation)) => model.dye_animations.push((slot, animation.clone())),
                    Ok(None) => {}
                    Err(error) => model.notices.push(format!(
                        "Dye {reference}: {error}. Showing its static material."
                    )),
                }
                // A part's dye index counts channel pairs: armor primary, armor secondary,
                // cloth primary, cloth secondary, suit primary, suit secondary.
                for index in 0..2 {
                    slots[slot * 2 + index] = Some(shader::Dye {
                        surface: material.surfaces[index],
                        detail,
                        normal,
                        normal_transform: material.normal_transform,
                        transform: material.detail_transform,
                        vectors: material.vectors,
                    });
                }
            }
            Some(Err(error)) => model.notices.push(format!("Dye {reference}: {error}")),
        }
    }
    model.dyes = slots;
    if model.triangle_gearstacks.iter().any(Option::is_none) {
        model
            .notices
            .push("Parts without material masks use a basic color preview.".into());
    }
    model.notices.push("Shader preview includes dye masks, detail color, wear, roughness, metal, emission, iridescence and supported transparent effects. Lighting is approximate. Gameplay-driven effects use their stored initial values.".into());
    Ok(())
}

fn load_detail(manager: &PackageManager, tag: u32, model: &mut Model) -> Option<usize> {
    if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
        return Some(index);
    }
    // Object effects must leave room for a color and normal image per gear dye channel.
    let loaded = if model.textures.len() >= MAX_TEXTURES + GEAR_KEYS[0].len() * 2 {
        Err("The preview texture budget is full".into())
    } else {
        texture::load(manager, tag)
    };
    match loaded {
        Ok(texture) => {
            model.textures.push(texture);
            Some(model.textures.len() - 1)
        }
        Err(error) => {
            model
                .notices
                .push(format!("Detail Texture 0x{tag:08X}: {error}"));
            None
        }
    }
}

fn assignment_hashes(table: &[u8], index: u16) -> Result<BTreeSet<u32>, String> {
    let (count, rows) = array(table, 8, 0x8080_5DFB, 0x20, 65536)?;
    if usize::from(index) >= count {
        return Err("Appearance row is outside the installed table".into());
    }
    let row = rows + usize::from(index) * 0x20;
    let mut hashes = BTreeSet::new();
    if u64_at(table, row + 0x10)? == 0 {
        hashes.insert(u32_at(table, row + 8)?);
        hashes.insert(u32_at(table, row + 12)?);
    } else {
        let (count, rows) = array(table, row + 0x10, 0x8080_5DFE, 8, 4096)?;
        for index in 0..count {
            let resource = pointer(table, rows + index * 8)?;
            let (count, rows) = array(table, resource + 8, 0x8080_5E01, 4, 65536)?;
            // Each region lists alternatives, not simultaneous geometry. Use its base
            // attachment until socket-driven region selection is supported.
            if count != 0 {
                hashes.insert(u32_at(table, rows)?);
            }
        }
    }
    hashes.remove(&0);
    hashes.remove(&u32::MAX);
    hashes.remove(&0x811C_9DC5);
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
    fn installed_normal_maps_and_shader_animations() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let output =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap()).join("motion");
        std::fs::create_dir_all(&output).unwrap();
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
        let summary = catalog
            .weapon_donors()
            .into_iter()
            .find(|d| d.name == "Better Devils")
            .unwrap();
        let mut appearance = Appearance {
            arrangement: 930,
            dye_textures: Vec::new(),
            dyes: vec![(4, 1702), (5, 1703), (6, 1704)],
        };
        let mut model = load(&packages, &appearance).unwrap();
        assert!(model.triangle_normals.iter().all(Option::is_some));
        assert!(model.dyes.iter().flatten().any(|d| d.normal.is_some()));
        let render = |m: &Model, t| {
            render::animated_image(
                m,
                render::Camera::default(),
                render::Scene::default(),
                [500, 500],
                t,
            )
        };
        let save = |name: &str, image: &eframe::egui::ColorImage| {
            let mut bytes = b"P6\n500 500\n255\n".to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::write(output.join(format!("{name}.ppm")), bytes).unwrap();
        };
        let mapped = render(&model, 0.0);
        save("normal-mapped", &mapped);
        model.triangle_normals.clear();
        let flat = render(&model, 0.0);
        assert_ne!(
            mapped.pixels, flat.pixels,
            "native normal maps must affect lighting"
        );
        save("without-normal-maps", &flat);
        let hashes: Vec<_> = catalog
            .supported_plug_sets(summary.hash, &[])
            .unwrap()
            .into_iter()
            .flat_map(|s| s.plug_hashes)
            .collect();
        let mut failures = Vec::new();
        for name in ["Bergusian Night", "Blueshift Dreams", "Oiled Algae"] {
            let hash = *hashes
                .iter()
                .find(|&&h| catalog.plug_label(h, false) == name)
                .unwrap_or_else(|| panic!("Missing shader {name}"));
            appearance.dyes = catalog
                .item_render_dye_rows(hash)
                .iter()
                .flatten()
                .map(|r| (r.channel_index, r.dye_reference_index))
                .collect();
            let model = load(&packages, &appearance).unwrap();
            println!(
                "Animated {name}: {} native programs, notices {:?}",
                model.dye_animations.len(),
                model.notices
            );
            assert!(
                model.has_shader_animation(),
                "{name} must load native animation"
            );
            let first = render(&model, 0.0);
            let later = [1.137, 7.5, 23.719]
                .into_iter()
                .map(|t| render(&model, t))
                .find(|image| image.pixels != first.pixels);
            if later.is_none() {
                failures.push(name);
            }
            let later = later.unwrap_or_else(|| first.clone());
            assert_eq!(
                first.pixels,
                render(&model, 0.0).pixels,
                "restart must be deterministic"
            );
            assert_eq!(
                render::styled_image(
                    &model,
                    render::Camera::default(),
                    render::Scene::default(),
                    [300, 300],
                    0.0,
                    render::Style::Solid
                )
                .pixels,
                render::styled_image(
                    &model,
                    render::Camera::default(),
                    render::Scene::default(),
                    [300, 300],
                    7.5,
                    render::Style::Solid
                )
                .pixels
            );
            save(&format!("{name}-0"), &first);
            save(&format!("{name}-later"), &later);
            if name == "Bergusian Night" {
                let start = std::time::Instant::now();
                for i in 0..40 {
                    save(&format!("frame-{i:02}"), &render(&model, i as f32 * 0.25));
                }
                println!("500px animation render mean {:?}", start.elapsed() / 40);
            }
        }
        assert!(
            failures.is_empty(),
            "Shaders without visible animation: {failures:?}"
        );
    }

    #[test]
    #[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
    #[expect(
        clippy::cognitive_complexity,
        reason = "One installed-package walk checks several appearances in sequence"
    )]
    fn installed_weapon_appearances() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap())
            .join("weapons");
        std::fs::create_dir_all(&output).unwrap();
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
        let mut failures = Vec::new();
        for name in [
            "Better Devils",
            "Sunshot",
            "The Last Word",
            "Black Talon",
            "Hard Light",
        ] {
            let summary = catalog
                .weapon_donors()
                .into_iter()
                .find(|d| d.name == name)
                .unwrap();
            let donor = catalog.weapon_donor(summary.hash).unwrap();
            println!(
                "{name}: {:?} dyes {:?}",
                donor.art_arrangements, donor.render_dye_rows
            );
            let a = Appearance {
                arrangement: donor.art_arrangements[0].arrangement,
                dye_textures: Vec::new(),
                dyes: donor
                    .render_dye_rows
                    .iter()
                    .flatten()
                    .map(|r| (r.channel_index, r.dye_reference_index))
                    .collect(),
            };
            let model = match load(&packages, &a) {
                Ok(model) => model,
                Err(error) => {
                    println!("FAILED {name}: {error}");
                    failures.push((name, error));
                    continue;
                }
            };
            println!(
                "{} vertices, {} triangles, {} textures",
                model.vertices.len(),
                model.triangles.len(),
                model.textures.len()
            );
            println!(
                "{} particles, {} sounds, {} lights, {} effect nodes",
                model.assets.particles.len(),
                model.assets.sounds.len(),
                model.assets.lights.len(),
                model.assets.effect_nodes.len()
            );
            assert!(!model.triangles.is_empty());
            println!(
                "dye slots {:?}, notices {:?}",
                model.triangle_dyes.iter().collect::<BTreeSet<_>>(),
                model.notices
            );
            let image = render::image(&model, render::Camera::default(), [500, 500]);
            let mut bytes = b"P6\n500 500\n255\n".to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::write(output.join(format!("{name}.ppm")), bytes).unwrap();
            if name == "Black Talon" {
                for (index, yaw) in [0.0_f32, 0.7, 1.4, 2.1].into_iter().enumerate() {
                    let camera = render::Camera {
                        yaw,
                        ..render::Camera::default()
                    };
                    let image = render::image(&model, camera, [500, 500]);
                    let mut bytes = b"P6\n500 500\n255\n".to_vec();
                    bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                    std::fs::write(output.join(format!("Black Talon-{index}.ppm")), bytes).unwrap();
                }
            }
            if name == "Sunshot" {
                assert!(
                    model.triangle_effects.iter().any(Option::is_some),
                    "the transparent stage carries part of Sunshot's visible geometry"
                );
                let solid = render::styled_image(
                    &model,
                    render::Camera::default(),
                    render::Scene::default(),
                    [500, 500],
                    0.0,
                    render::Style::Solid,
                );
                let mut bytes = b"P6\n500 500\n255\n".to_vec();
                bytes.extend(solid.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                std::fs::write(output.join("Sunshot Solid.ppm"), bytes).unwrap();
                let ornaments = catalog.weapon_ornaments(summary.hash);
                let ornament = ornaments.iter().find(|o| o.name == "Red Dwarf").unwrap();
                let appearance = Appearance {
                    arrangement: ornament.art_arrangements[0].arrangement,
                    dye_textures: Vec::new(),
                    dyes: ornament
                        .render_dye_rows
                        .iter()
                        .flatten()
                        .map(|r| (r.channel_index, r.dye_reference_index))
                        .collect(),
                };
                let ornament = load(&packages, &appearance).unwrap();
                assert_ne!(
                    model.tags, ornament.tags,
                    "ornament must select its own model"
                );
                let image = render::image(&ornament, render::Camera::default(), [500, 500]);
                let mut bytes = b"P6\n500 500\n255\n".to_vec();
                bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                std::fs::write(output.join("Red Dwarf.ppm"), bytes).unwrap();
            }
            if name == "Better Devils" {
                let shaders: Vec<_> = catalog
                    .supported_plug_sets(summary.hash, &[])
                    .unwrap()
                    .into_iter()
                    .flat_map(|s| s.plug_hashes)
                    .filter(|&hash| catalog.item_type_name(hash).as_deref() == Some("Shader"))
                    .filter(|&hash| {
                        catalog
                            .item_render_dye_rows(hash)
                            .iter()
                            .flatten()
                            .any(|r| matches!(r.channel_index, 4..=6))
                    })
                    .step_by(13)
                    .take(8)
                    .collect();
                assert!(
                    !shaders.is_empty(),
                    "Need installed shaders to verify materials"
                );
                let mut details_visible = 0;
                let mut report = Vec::new();
                for hash in shaders {
                    let mut shader = a.clone();
                    shader.dyes = catalog
                        .item_render_dye_rows(hash)
                        .iter()
                        .flatten()
                        .map(|r| (r.channel_index, r.dye_reference_index))
                        .collect();
                    let mut model = load(&packages, &shader).unwrap();
                    assert!(
                        model.dyes.iter().all(Option::is_some),
                        "missing shader channels for {hash:08X}"
                    );
                    assert!(
                        model.dyes.iter().flatten().any(|d| d.detail.is_some()),
                        "missing detail textures for {hash:08X}"
                    );
                    assert!(model.triangle_gearstacks.iter().any(Option::is_some));
                    let shaded = render::image(&model, render::Camera::default(), [500, 500]);
                    let masks = std::mem::take(&mut model.triangle_gearstacks);
                    let tint_only = render::image(&model, render::Camera::default(), [500, 500]);
                    assert_ne!(
                        shaded.pixels, tint_only.pixels,
                        "material masks must affect the native render"
                    );
                    model.triangle_gearstacks = masks;
                    let dyes = model.dyes;
                    for dye in model.dyes.iter_mut().flatten() {
                        dye.detail = None;
                    }
                    let without_detail =
                        render::image(&model, render::Camera::default(), [500, 500]);
                    let detail_changes_pixels = shaded.pixels != without_detail.pixels;
                    details_visible += usize::from(detail_changes_pixels);
                    model.dyes = dyes;
                    report.push(serde_json::json!({
                        "hash": format!("{hash:08X}"),
                        "name": catalog.plug_label(hash, false),
                        "textures": model.textures.len(),
                        "masked_triangles": model.triangle_gearstacks.iter().filter(|m| m.is_some()).count(),
                        "detail_changes_pixels": detail_changes_pixels,
                        "notices": model.notices,
                    }));
                    assert_ne!(
                        image.pixels,
                        shaded.pixels,
                        "shader {} must change rendered colors",
                        catalog.plug_label(hash, false)
                    );
                    let mut bytes = b"P6\n500 500\n255\n".to_vec();
                    bytes.extend(shaded.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                    println!(
                        "Shader {} ({hash:08X}): {:?}, {} textures, notices {:?}",
                        catalog.plug_label(hash, false),
                        shader.dyes,
                        model.textures.len(),
                        model.notices
                    );
                    std::fs::write(output.join(format!("shader-{hash:08X}.ppm")), bytes).unwrap();
                }
                assert!(
                    details_visible > 0,
                    "native detail maps must visibly affect at least one shader"
                );
                std::fs::write(
                    output.join("shader-coverage.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    /// The first candidate whose own appearance loads with dyed parts.
    fn first_loadable(
        catalog: &crate::investment::InvestmentCatalog,
        packages: &Path,
        candidates: impl IntoIterator<Item = crate::investment::WeaponDonorSummary>,
    ) -> Option<crate::investment::WeaponDonorSummary> {
        let unshaded: [Vec<(i8, u16)>; 3] = Default::default();
        candidates.into_iter().take(40).find(|donor| {
            catalog
                .shader_preview_appearance(donor.hash, &unshaded)
                .filter(|appearance| !appearance.dyes.is_empty())
                .and_then(|appearance| load(packages, &appearance).ok())
                .is_some_and(|model| model.triangle_dyes.iter().any(|&slot| slot < 6))
        })
    }

    /// A shader's dye rows as an appearance composes them.
    fn shader_rows(
        catalog: &crate::investment::InvestmentCatalog,
        shader: u32,
    ) -> [Vec<(i8, u16)>; 3] {
        catalog.item_render_dye_rows(shader).map(|rows| {
            rows.into_iter()
                .map(|row| (row.channel_index, row.dye_reference_index))
                .collect()
        })
    }

    /// A lookup row the game leaves unauthored is filled with magenta.
    fn placeholder(lookup: &texture::Texture, id: usize) -> bool {
        let at = (id * lookup.size[0] + lookup.size[0] / 2) * 4;
        lookup.rgba.get(at..at + 3) == Some(&[255, 0, 255][..])
    }

    /// The dye slots a model's dyeable parts name.
    fn dyed_slots(model: &Model) -> BTreeSet<usize> {
        model
            .triangle_dyes
            .iter()
            .zip(&model.triangle_gearstacks)
            .filter(|(slot, gearstack)| **slot < 6 && gearstack.is_some())
            .map(|(slot, _)| usize::from(*slot))
            .collect()
    }

    /// The iridescence rows a model's dyes name.
    fn iridescence_ids(model: &Model) -> BTreeSet<usize> {
        model
            .dyes
            .iter()
            .flatten()
            .map(|dye| dye.surface.iridescence)
            .filter(|id| *id >= 0.0)
            .map(|id| id as usize)
            .collect()
    }

    /// Whether two appearances' dyes hold the same materials key for key, as when an item ships
    /// with a shader's dyes as its own under other references.
    fn same_materials(manager: &PackageManager, a: &Appearance, b: &Appearance) -> bool {
        let indices = a
            .dyes
            .iter()
            .chain(&b.dyes)
            .map(|(_, index)| *index)
            .collect::<Vec<_>>();
        let Ok(materials) = crate::dyes::material::load(manager, &indices) else {
            return false;
        };
        let material = |index: &u16| materials.get(index).and_then(|found| found.as_ref().ok());
        a.dyes.len() == b.dyes.len()
            && a.dyes
                .iter()
                .zip(&b.dyes)
                .all(|((key, first), (other_key, second))| {
                    key == other_key
                        && match (material(first), material(second)) {
                            (Some(first), Some(second)) => {
                                first.vectors == second.vectors
                                    && first.detail == second.detail
                                    && first.normal == second.normal
                            }
                            _ => false,
                        }
                })
    }

    /// What a dyed model gets wrong: with a shader, dyeable parts it leaves without a dye; always,
    /// dye, detail texture or texture budget notices, and iridescence without the lookup or on a
    /// placeholder row.
    fn shading_problems(model: &Model, label: &str, shaded: bool) -> Vec<String> {
        let mut problems = Vec::new();
        let undyed = dyed_slots(model)
            .into_iter()
            .filter(|slot| model.dyes[*slot].is_none())
            .collect::<Vec<_>>();
        if shaded && !undyed.is_empty() {
            problems.push(format!("{label}: undyed slots {undyed:?}"));
        }
        for notice in &model.notices {
            if notice.starts_with("Dye ")
                || notice.contains("budget")
                || notice.starts_with("Detail Texture")
            {
                problems.push(format!("{label}: {notice}"));
            }
        }
        let ids = iridescence_ids(model);
        match &model.iridescence {
            None if !ids.is_empty() => {
                problems.push(format!("{label}: iridescence {ids:?} without the lookup"));
            }
            Some(lookup) => {
                for id in ids.into_iter().filter(|id| placeholder(lookup, *id)) {
                    problems.push(format!("{label}: iridescence {id} is a placeholder row"));
                }
            }
            None => {}
        }
        problems
    }

    /// Stock shaders on every gear type, dyed the way the game dyes each one. The preview has
    /// drawn armor undyed because it knew only the weapon dye keys, dropped cloth and suit detail
    /// textures, lacked iridescence in software, and left five shaders' animations still because
    /// their programs read game inputs or an eight-stop gradient it did not run. Every available
    /// dye animation program is parsed. Each item is drawn in its own dyes and in six shaders, and
    /// one armor piece again with a custom color and iridescence. Writes a sheet per gear type, a
    /// column per shader, and `report.json` under `SUNDIAL_PROBE_OUT/shaders`.
    #[test]
    #[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
    #[expect(
        clippy::cognitive_complexity,
        reason = "One installed-package survey checks every item and shader in turn"
    )]
    fn installed_shaders_on_every_gear_type() {
        use crate::investment::{WeaponDonorSummary, WeaponRarity};
        const TILE: usize = 200;
        const ARMOR: [u64; 5] = [
            3_448_274_439,
            3_551_918_588,
            14_239_492,
            20_886_954,
            1_585_787_867,
        ];
        const SHIP: u64 = 284_967_655;
        const SPARROW: u64 = 2_025_709_351;
        const GHOST: u64 = 4_023_194_814;
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
        let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap())
            .join("shaders");
        std::fs::create_dir_all(&out).unwrap();
        let mut problems = Vec::new();
        // Parse each supported dye animation program in the art-dye table.
        let manager = crate::investment::discovery::open_packages(&packages).unwrap();
        let globals = manager
            .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
            .unwrap();
        let table = manager
            .read_tag(u32_at(&globals, 16 + 67 * 16).unwrap())
            .unwrap();
        let (count, _) = array(&table, 8, 0x8080_5DEC, 8, 100_000).unwrap();
        let every = (0..count).map(|index| index as u16).collect::<Vec<_>>();
        let mut animated = 0;
        for (index, material) in crate::dyes::material::load(&manager, &every).unwrap() {
            match material.map(|material| material.animation) {
                Ok(Ok(Some(_))) => animated += 1,
                Ok(Err(error)) => problems.push(format!("Dye {index}: {error}")),
                // Dyes in the later 12-vector layout are not shader dyes and have no program.
                Ok(Ok(None)) | Err(_) => {}
            }
        }
        println!("{animated} animated dyes of {count} parsed");
        let unshaded: [Vec<(i8, u16)>; 3] = Default::default();
        let armor = catalog.gear_donors(&ARMOR);
        let weapons = catalog.weapon_donors();
        let of = |donors: &[WeaponDonorSummary], matches: &dyn Fn(&WeaponDonorSummary) -> bool| {
            first_loadable(
                &catalog,
                &packages,
                donors.iter().filter(|donor| matches(donor)).cloned(),
            )
        };
        let mut sheets: Vec<(&str, Vec<WeaponDonorSummary>)> = vec![
            ("armor", Vec::new()),
            ("weapons", Vec::new()),
            ("vehicles", Vec::new()),
        ];
        for bucket in &ARMOR[..4] {
            sheets[0]
                .1
                .extend(of(&armor, &|donor| donor.bucket_hash == *bucket));
        }
        for class_item in ["Cloak", "Mark", "Bond"] {
            sheets[0].1.extend(of(&armor, &|donor| {
                donor.bucket_hash == ARMOR[4] && donor.type_name.contains(class_item)
            }));
        }
        sheets[0]
            .1
            .extend(of(&armor, &|donor| donor.rarity == WeaponRarity::Exotic));
        for kind in [
            "Auto Rifle",
            "Hand Cannon",
            "Sniper Rifle",
            "Rocket Launcher",
            "Sword",
            "Combat Bow",
        ] {
            sheets[1]
                .1
                .extend(of(&weapons, &|donor| donor.type_name == kind));
        }
        sheets[1]
            .1
            .extend(of(&weapons, &|donor| donor.rarity == WeaponRarity::Exotic));
        for bucket in [SHIP, SPARROW, GHOST] {
            sheets[2].1.extend(first_loadable(
                &catalog,
                &packages,
                catalog.gear_donors(&[bucket]),
            ));
        }
        assert_eq!(
            sheets
                .iter()
                .map(|(_, items)| items.len())
                .collect::<Vec<_>>(),
            [8, 7, 3],
            "An item of each surveyed kind loads"
        );
        let item_filter = std::env::var("SUNDIAL_PREVIEW_ITEM").ok();
        if let Some(name) = &item_filter {
            for (_, items) in &mut sheets {
                items.retain(|item| item.name.eq_ignore_ascii_case(name));
            }
            assert!(
                sheets.iter().any(|(_, items)| !items.is_empty()),
                "No surveyed item matches {name}"
            );
        }
        let stock = catalog.shader_donors();
        let shaders = [
            "Always North",
            "Amaranth Atrocity",
            "Arctic Pearl",
            "Metallic Sunrise",
            "Bergusian Night",
            "Iridescent Coral",
        ]
        .into_iter()
        .filter_map(|name| stock.iter().find(|shader| shader.name == name))
        .map(|shader| {
            (
                shader.name.clone(),
                shader_rows(&catalog, shader.hash),
                shader.hash,
            )
        })
        .collect::<Vec<_>>();
        assert_eq!(shaders.len(), 6, "Every surveyed shader is installed");
        let mut report = Vec::new();
        let start = std::time::Instant::now();
        for (gear, items) in &sheets {
            if items.is_empty() {
                continue;
            }
            let columns = shaders.len() + 1;
            let width = TILE * columns;
            let mut sheet = vec![0_u8; width * TILE * items.len() * 4];
            for (row, item) in items.iter().enumerate() {
                let mut own = None;
                let mut entries = Vec::new();
                for column in 0..columns {
                    let (shader, rows) = match column {
                        0 => ("Own Dyes", &unshaded),
                        _ => (shaders[column - 1].0.as_str(), &shaders[column - 1].1),
                    };
                    let label = format!("{} in {shader}", item.name);
                    let Some(appearance) = catalog.shader_preview_appearance(item.hash, rows)
                    else {
                        problems.push(format!("{label}: no appearance"));
                        continue;
                    };
                    // Exercise the equipped-plug path, not just the shader browser's filtered rows.
                    let appearance = if column > 0 {
                        let equipped =
                            catalog.preview_appearance(&crate::ui::model_preview::Loadout {
                                arrangement: appearance.arrangement,
                                dyes: catalog.item_render_dye_rows(item.hash).map(|rows| {
                                    rows.into_iter()
                                        .map(|row| (row.channel_index, row.dye_reference_index))
                                        .collect()
                                }),
                                plugs: vec![Some(shaders[column - 1].2)],
                            });
                        let first = match item.bucket_hash {
                            SHIP => 7,
                            SPARROW => 10,
                            GHOST => 13,
                            bucket if ARMOR.contains(&bucket) => 0,
                            _ => 4,
                        };
                        assert!(
                            !equipped.dyes.is_empty()
                                && equipped
                                    .dyes
                                    .iter()
                                    .all(|(key, _)| (first..first + 3).contains(key)),
                            "{label}: equipped colors must use this item's gear category: {:?}",
                            equipped.dyes
                        );
                        assert_eq!(
                            equipped, appearance,
                            "{label}: equipped and shader-browser appearances disagree"
                        );
                        equipped
                    } else {
                        appearance
                    };
                    let model = match load(&packages, &appearance) {
                        Ok(model) => model,
                        Err(error) => {
                            problems.push(format!("{label}: {error}"));
                            continue;
                        }
                    };
                    problems.extend(shading_problems(&model, &label, column > 0));
                    let (slots, ids) = (dyed_slots(&model), iridescence_ids(&model));
                    let image = render::image(&model, render::Camera::default(), [TILE, TILE]);
                    match &own {
                        None => own = Some(image.pixels.clone()),
                        Some(pixels) if *pixels == image.pixels => {
                            problems.push(format!("{label}: draws like the item's own dyes"));
                        }
                        Some(_) => {}
                    }
                    // A custom color and iridescence on armor primary change the first armor piece.
                    if *gear == "armor" && row == 0 && column == 1 {
                        let even = (0..128)
                            .step_by(2)
                            .find(|id| {
                                model
                                    .iridescence
                                    .as_ref()
                                    .is_some_and(|lookup| !placeholder(lookup, *id))
                            })
                            .unwrap_or(0);
                        // Armor primary's albedo is vector 9 and its iridescence row vector 11.
                        model.set_surface_overrides(&[SurfaceOverride {
                            slot: 0,
                            writes: vec![
                                (9, 0, 0.578),
                                (9, 1, 0.031),
                                (9, 2, 0.007),
                                (11, 0, even as f32),
                            ],
                        }]);
                        let edited = render::image(&model, render::Camera::default(), [TILE, TILE]);
                        if edited.pixels == image.pixels {
                            problems.push(format!("{label}: a custom surface changes nothing"));
                        }
                        let bytes = edited
                            .pixels
                            .iter()
                            .flat_map(|pixel| pixel.to_array())
                            .collect::<Vec<_>>();
                        std::fs::write(
                            out.join("armor-custom-surface.png"),
                            super::super::export::png(&bytes, TILE, TILE).unwrap(),
                        )
                        .unwrap();
                    }
                    for (y, line) in image.pixels.chunks(TILE).enumerate() {
                        let at = ((row * TILE + y) * width + column * TILE) * 4;
                        let line = line
                            .iter()
                            .flat_map(|pixel| pixel.to_array())
                            .collect::<Vec<_>>();
                        sheet[at..at + TILE * 4].copy_from_slice(&line);
                    }
                    entries.push(serde_json::json!({
                        "shader": shader,
                        "dyed_slots": slots,
                        "dyes": model.dyes.iter().filter(|dye| dye.is_some()).count(),
                        "details": model.dyes.iter().flatten().filter(|dye| dye.detail.is_some()).count(),
                        "iridescence": ids,
                        "animated": model.has_shader_animation(),
                    }));
                }
                println!(
                    "{gear}: {} ({}) {:?}",
                    item.name,
                    item.type_name,
                    start.elapsed()
                );
                report.push(serde_json::json!({
                    "gear": gear,
                    "item": item.name,
                    "hash": format!("0x{:08X}", item.hash),
                    "type": item.type_name,
                    "keys": catalog
                        .shader_preview_appearance(item.hash, &unshaded)
                        .map(|appearance| appearance.dyes),
                    "columns": entries,
                }));
            }
            std::fs::write(
                out.join(format!("{gear}.png")),
                super::super::export::png(&sheet, width, TILE * items.len()).unwrap(),
            )
            .unwrap();
        }
        let names = std::iter::once("Own Dyes")
            .chain(shaders.iter().map(|(name, _, _)| name.as_str()))
            .collect::<Vec<_>>();
        std::fs::write(
            out.join("report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "animated_dyes": animated,
                "dyes": count,
                "item_filter": item_filter,
                "columns": names,
                "items": report,
                "problems": problems,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// Every stock shader on an armor piece, a weapon and a Ghost Shell, with the survey's checks.
    /// A shader may draw like the item's own dyes only when the item ships with that shader's
    /// materials. Images worth a second look are flagged rather than failed: a model mostly blown
    /// out to white, one nearly black, or magenta from a placeholder. Writes pages of 60 tiles per
    /// item, its own dyes first and then the shaders in catalog order, and `report.json` naming
    /// every tile under `SUNDIAL_PROBE_OUT/shaders/all`.
    #[test]
    #[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
    fn installed_every_shader_on_three_gear_types() {
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        const TILE: usize = 160;
        const COLUMNS: usize = 10;
        const PAGE: usize = 60;
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
        let out = std::path::PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap())
            .join("shaders/all");
        std::fs::create_dir_all(&out).unwrap();
        let weapons = catalog
            .weapon_donors()
            .into_iter()
            .filter(|donor| donor.type_name == "Auto Rifle");
        let items = [
            (
                "armor",
                first_loadable(&catalog, &packages, catalog.gear_donors(&[14_239_492])),
            ),
            ("weapon", first_loadable(&catalog, &packages, weapons)),
            (
                "ghost",
                first_loadable(&catalog, &packages, catalog.gear_donors(&[4_023_194_814])),
            ),
        ]
        .map(|(gear, item)| (gear, item.unwrap_or_else(|| panic!("No {gear} loads"))));
        let shaders = catalog.shader_donors();
        assert!(
            !shaders.is_empty(),
            "Need installed shaders to verify materials"
        );
        let unshaded: [Vec<(i8, u16)>; 3] = Default::default();
        // Each item in its own dyes, then in every shader.
        let mut jobs = Vec::new();
        for (item, (_, donor)) in items.iter().enumerate() {
            for shader in std::iter::once(None).chain((0..shaders.len()).map(Some)) {
                let (name, rows) = match shader {
                    None => ("Own Dyes", unshaded.clone()),
                    Some(index) => (
                        shaders[index].name.as_str(),
                        shader_rows(&catalog, shaders[index].hash),
                    ),
                };
                let appearance = catalog.shader_preview_appearance(donor.hash, &rows);
                jobs.push((
                    item,
                    shader,
                    format!("{} in {name}", donor.name),
                    appearance,
                ));
            }
        }
        let next = AtomicUsize::new(0);
        let done = Mutex::new(Vec::new());
        let workers = std::thread::available_parallelism()
            .map_or(4, |count| count.get())
            .clamp(2, 8);
        let start = std::time::Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((_, shader, label, appearance)) = jobs.get(index) else {
                            break;
                        };
                        let result = match appearance {
                            None => Err(format!("{label}: no appearance")),
                            Some(appearance) => load(&packages, appearance)
                                .map(|model| {
                                    let image = render::image(
                                        &model,
                                        render::Camera::default(),
                                        [TILE, TILE],
                                    );
                                    (shading_problems(&model, label, shader.is_some()), image)
                                })
                                .map_err(|error| format!("{label}: {error}")),
                        };
                        if index % 100 == 0 {
                            println!("{index} of {} {:?}", jobs.len(), start.elapsed());
                        }
                        done.lock().unwrap().push((index, result));
                    }
                });
            }
        });
        let mut done = done.into_inner().unwrap();
        done.sort_by_key(|(index, _)| *index);
        let background = render::Scene::default().background;
        // Flags on the model's own pixels, the ones that are not the background.
        let flags = |image: &eframe::egui::ColorImage| {
            let model = image
                .pixels
                .iter()
                .map(|pixel| [pixel.r(), pixel.g(), pixel.b()])
                .filter(|rgb| *rgb != background)
                .collect::<Vec<_>>();
            let share = |matches: &dyn Fn(&[u8; 3]) -> bool| {
                model.iter().filter(|rgb| matches(rgb)).count() as f32 / model.len().max(1) as f32
            };
            let mut flags = Vec::new();
            if share(&|rgb| rgb.iter().all(|value| *value >= 250)) > 0.25 {
                flags.push("blown out");
            }
            if share(&|rgb| rgb.iter().map(|value| u32::from(*value)).sum::<u32>() < 36) > 0.9 {
                flags.push("dark");
            }
            if share(&|rgb| rgb[0] > 200 && rgb[1] < 60 && rgb[2] > 200) > 0.005 {
                flags.push("magenta");
            }
            flags
        };
        let manager = crate::investment::discovery::open_packages(&packages).unwrap();
        let mut problems = Vec::new();
        let mut flagged = Vec::new();
        let mut own_shaders = Vec::new();
        let per_item = shaders.len() + 1;
        for (item, (gear, donor)) in items.iter().enumerate() {
            let tiles = &done[item * per_item..(item + 1) * per_item];
            let own = match &tiles[0].1 {
                Ok((_, image)) => Some(image.pixels.clone()),
                Err(_) => None,
            };
            for (page, chunk) in tiles.chunks(PAGE).enumerate() {
                let rows = chunk.len().div_ceil(COLUMNS);
                let width = COLUMNS * TILE;
                let mut sheet = vec![0_u8; width * rows * TILE * 4];
                for (position, (index, result)) in chunk.iter().enumerate() {
                    let (_, shader, label, _) = &jobs[*index];
                    let image =
                        match result {
                            Ok((found, image)) => {
                                problems.extend(found.iter().cloned());
                                if shader.is_some() && own.as_ref() == Some(&image.pixels) {
                                    let (own, shaded) = (&jobs[item * per_item].3, &jobs[*index].3);
                                    if own.as_ref().zip(shaded.as_ref()).is_some_and(
                                        |(own, shaded)| same_materials(&manager, own, shaded),
                                    ) {
                                        own_shaders.push(label.clone());
                                    } else {
                                        problems.push(format!(
                                            "{label}: draws like the item's own dyes"
                                        ));
                                    }
                                }
                                let marks = flags(image);
                                if !marks.is_empty() {
                                    flagged.push(serde_json::json!({
                                        "tile": label,
                                        "page": format!("{gear}-{}.png", page + 1),
                                        "position": position + 1,
                                        "flags": marks,
                                    }));
                                }
                                image
                            }
                            Err(error) => {
                                problems.push(error.clone());
                                continue;
                            }
                        };
                    let (column, row) = (position % COLUMNS, position / COLUMNS);
                    for (y, line) in image.pixels.chunks(TILE).enumerate() {
                        let at = ((row * TILE + y) * width + column * TILE) * 4;
                        let line = line
                            .iter()
                            .flat_map(|pixel| pixel.to_array())
                            .collect::<Vec<_>>();
                        sheet[at..at + TILE * 4].copy_from_slice(&line);
                    }
                }
                std::fs::write(
                    out.join(format!("{gear}-{}.png", page + 1)),
                    super::super::export::png(&sheet, width, rows * TILE).unwrap(),
                )
                .unwrap();
            }
            println!("{gear}: {} ({})", donor.name, donor.type_name);
        }
        let tiles = std::iter::once("Own Dyes")
            .chain(shaders.iter().map(|shader| shader.name.as_str()))
            .collect::<Vec<_>>();
        std::fs::write(
            out.join("report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "items": items
                    .iter()
                    .map(|(gear, donor)| format!("{gear}: {} 0x{:08X}", donor.name, donor.hash))
                    .collect::<Vec<_>>(),
                "tiles_per_page": PAGE,
                "tiles": tiles,
                "own_shaders": own_shaders,
                "flagged": flagged,
                "problems": problems,
                "seconds": start.elapsed().as_secs(),
            }))
            .unwrap(),
        )
        .unwrap();
        println!(
            "{} images, {} flagged, {} problems in {:?}",
            jobs.len(),
            flagged.len(),
            problems.len(),
            start.elapsed()
        );
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
