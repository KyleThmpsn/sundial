//! Installed investment appearance assignments, separate from gameplay entities.
use super::*;
use crate::package_authoring::resolve_live_named_tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub arrangement: u16,
    /// Effective channel/reference pairs after default, custom, shader and locked precedence.
    pub dyes: Vec<(i8, u16)>,
}

pub(crate) fn load(packages: &Path, appearance: &Appearance) -> Result<Model, String> {
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
    for entity in entities {
        let model = match load_with_manager(&manager, entity) {
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
        result
            .triangles
            .extend(model.triangles.into_iter().map(|t| t.map(|v| v + base)));
        result.triangle_textures.extend(
            model
                .triangle_textures
                .into_iter()
                .map(|t| t.and_then(|i| texture_map[i])),
        );
        result.triangle_dyes.extend(model.triangle_dyes);
        result.triangle_clip.extend(model.triangle_clip);
        result.triangle_constant.extend(model.triangle_constant);
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
        result.vertices.extend(model.vertices);
        result.uvs.extend(model.uvs);
        result.tags.extend(model.tags);
        result.notices.extend(model.notices);
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
    Ok(result)
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
    let (count, rows) = array(&channels, 8, 0x8080_5BE2, 4, 4096)?;
    let indices: Vec<_> = appearance.dyes.iter().map(|row| row.1).collect();
    let colors = crate::weapon_dyes::material::load(manager, &indices)?;
    let mut slots = [None; 6];
    for &(channel, reference) in &appearance.dyes {
        if channel < 0 || channel as usize >= count {
            continue;
        }
        let hash = u32_at(&channels, rows + channel as usize * 4)?;
        let Some(slot) = [1667433279, 1667433278, 1667433277]
            .iter()
            .position(|&h| h == hash)
        else {
            continue;
        };
        model.dye_animations.retain(|(s, _)| *s != slot);
        match colors.get(&reference) {
            None => continue,
            Some(Ok(material)) => {
                let detail = match &material.detail {
                    Ok(Some(tag)) => load_detail(manager, *tag, model),
                    Ok(None) => None,
                    Err(error) => {
                        model.notices.push(error.clone());
                        None
                    }
                };
                let normal = match &material.normal {
                    Ok(Some(tag)) => load_detail(manager, *tag, model),
                    Ok(None) => None,
                    Err(error) => {
                        model.notices.push(error.clone());
                        None
                    }
                };
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
    model.notices.push("Shader preview includes dye masks, detail color, wear, roughness, metal and emission. Lighting is approximate. Normal maps and supported native material animations are included. Iridescence, transparency and game-driven shader inputs are not shown.".into());
    Ok(())
}

fn load_detail(manager: &PackageManager, tag: u32, model: &mut Model) -> Option<usize> {
    if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
        return Some(index);
    }
    let loaded = if model.textures.len() >= MAX_TEXTURES {
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
    #[ignore = "requires installed Shadowkeep packages"]
    fn installed_normal_maps_and_shader_animations() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let catalog =
            crate::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
                .unwrap();
        let summary = catalog
            .weapon_donors()
            .into_iter()
            .find(|d| d.name == "Better Devils")
            .unwrap();
        let mut appearance = Appearance {
            arrangement: 930,
            dyes: vec![(4, 1702), (5, 1703), (6, 1704)],
        };
        let mut model = load(&packages, &appearance).unwrap();
        assert!(model.triangle_normals.iter().all(Option::is_some));
        assert!(model.dyes.iter().flatten().any(|d| d.normal.is_some()));
        let render =
            |m: &Model, t| render::animated_image(m, render::Camera::default(), [500, 500], t);
        let save = |name: &str, image: &eframe::egui::ColorImage| {
            let mut bytes = b"P6\n500 500\n255\n".to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::create_dir_all("docs/model-preview/motion").unwrap();
            std::fs::write(format!("docs/model-preview/motion/{name}.ppm"), bytes).unwrap();
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
            .weapon_supported_plug_sets(summary.hash)
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
                    [300, 300],
                    0.0,
                    render::Style::Solid
                )
                .pixels,
                render::styled_image(
                    &model,
                    render::Camera::default(),
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
    #[ignore = "requires installed Shadowkeep packages"]
    #[expect(
        clippy::cognitive_complexity,
        reason = "One installed-package walk checks several appearances in sequence"
    )]
    fn installed_weapon_appearances() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
        let catalog =
            crate::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
                .unwrap();
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
            assert!(!model.triangles.is_empty());
            println!(
                "dye slots {:?}, notices {:?}",
                model.triangle_dyes.iter().collect::<BTreeSet<_>>(),
                model.notices
            );
            let image = render::image(&model, render::Camera::default(), [500, 500]);
            let mut bytes = b"P6\n500 500\n255\n".to_vec();
            bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            std::fs::create_dir_all("docs/model-preview/weapons").unwrap();
            std::fs::write(format!("docs/model-preview/weapons/{name}.ppm"), bytes).unwrap();
            if name == "Sunshot" {
                let ornaments = catalog.weapon_ornaments(summary.hash);
                let ornament = ornaments.iter().find(|o| o.name == "Red Dwarf").unwrap();
                let appearance = Appearance {
                    arrangement: ornament.art_arrangements[0].arrangement,
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
                std::fs::write("docs/model-preview/weapons/Red Dwarf.ppm", bytes).unwrap();
            }
            if name == "Better Devils" {
                let shaders: Vec<_> = catalog
                    .weapon_supported_plug_sets(summary.hash)
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
                assert_eq!(
                    shaders.len(),
                    8,
                    "Need eight installed shaders to verify material coverage"
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
                    std::fs::write(
                        format!("docs/model-preview/weapons/shader-{hash:08X}.ppm"),
                        bytes,
                    )
                    .unwrap();
                }
                assert!(
                    details_visible > 0,
                    "native detail maps must visibly affect at least one shader"
                );
                std::fs::write(
                    "docs/model-preview/weapons/shader-coverage.json",
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }
}
