//! Shader authoring and independent native dye and pixel readback.

use super::*;
mod zoom;

/// A shader from the New menu that takes its cloth dyes from another stock shader, gives two
/// surfaces custom colors and iridescence on every gear type and one weapon surface more values of
/// its own, and gives the weapons' suit dye another shader's textures.
pub(super) struct AuthoredShader {
    pub(super) recipe: WeaponRecipe,
    pub(super) base: u32,
    pub(super) source: u32,
    /// The armor piece the page's preview showed the unbuilt shader on, and its image.
    pub(super) preview_item: u32,
    pub(super) preview: Arc<egui::ColorImage>,
    /// The weapon the page's preview showed first, and its image.
    pub(super) weapon_item: u32,
    pub(super) weapon_preview: Arc<egui::ColorImage>,
}

/// The dye a shader's rows give one key.
pub(super) fn dye(rows: &shader_view::DyeRows, key: i8) -> Option<u16> {
    rows.iter()
        .flatten()
        .find(|row| row.channel_index == key)
        .map(|row| row.dye_reference_index)
}

/// Whether `rows` carry `source`'s dyes for one channel on every gear type.
pub(super) fn channel_matches(
    rows: &shader_view::DyeRows,
    source: &shader_view::DyeRows,
    channel: i8,
) -> bool {
    crate::dye::GearType::ALL.iter().all(|gear| {
        let first = gear.first_key();
        dye(rows, first + channel) == dye(source, first + channel)
    })
}

/// A shader's rows as a model preview composes them.
pub(super) fn preview_rows(rows: &shader_view::DyeRows) -> [Vec<(i8, u16)>; 3] {
    rows.clone().map(|rows| {
        rows.iter()
            .map(|row| (row.channel_index, row.dye_reference_index))
            .collect()
    })
}

/// Pixels of two same-sized previews that differ by more than one level in a channel. The unbuilt
/// preview applies its edits as surface overrides while the built shader reads the same values
/// from its dye records. Once the preview framed models closer (2026-09-30), the two drew 172 of
/// 147,076 pixels one level apart in one channel and none further, which no eye sees. Anything
/// more is a real difference.
pub(super) fn differing_pixels(a: &egui::ColorImage, b: &egui::ColorImage) -> usize {
    assert_eq!(a.size, b.size, "the previews compared are the same size");
    a.pixels
        .iter()
        .zip(&b.pixels)
        .filter(|(a, b)| {
            a.to_array()
                .into_iter()
                .zip(b.to_array())
                .any(|(a, b)| a.abs_diff(b) > 1)
        })
        .count()
}

/// The model preview's image among a frame's texture uploads. The font atlas is the only other
/// upload this large, and it is not a color image.
pub(super) fn preview_image(output: &egui::FullOutput) -> Option<Arc<egui::ColorImage>> {
    output
        .textures_delta
        .set
        .values()
        .flat_map(|deltas| deltas.iter())
        .filter(|delta| delta.pos.is_none())
        .map(|delta| {
            let egui::ImageData::Color(image) = &delta.image;
            image.clone()
        })
        .filter(|image| image.width() >= 200 && image.height() >= 200)
        .max_by_key(|image| image.width() * image.height())
}

/// Runs frames until the preview has read what it was asked for and drawn it, and returns that
/// image. A software image arrives a frame or more after its model, so a page whose model loaded
/// during earlier frames is waited on until its next image, and an image of the model shown while
/// another loads does not count.
pub(super) fn await_preview(
    mut frame: impl FnMut() -> egui::FullOutput,
    what: &str,
) -> Arc<egui::ColorImage> {
    let start = Instant::now();
    loop {
        let output = frame();
        let drawn = texts(&output);
        if !drawn.iter().any(|(text, _)| text == "Loading Model")
            && let Some(image) = preview_image(&output)
        {
            return image;
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "{what} drew no preview. Text drawn: {:?}",
            drawn.iter().map(|(text, _)| text).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The model preview alone, drawing `appearance` at the size of an earlier image.
pub(super) fn draw_preview(
    packages: &Path,
    appearance: &Appearance,
    overrides: &[SurfaceOverride],
    like: &egui::ColorImage,
    what: &str,
) -> Arc<egui::ColorImage> {
    let ctx = context();
    let size = egui::vec2(like.width() as f32, like.height() as f32);
    await_preview(
        || {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        size + egui::vec2(64.0, 64.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        still::show(
                            ui,
                            egui::Id::new("read-back-preview"),
                            packages,
                            Some(appearance.clone()),
                            overrides,
                            size,
                        );
                    });
                },
            )
        },
        what,
    )
}

pub(super) fn save_image(image: &egui::ColorImage, path: &Path) {
    let bytes = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect::<Vec<_>>();
    ::image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes)
        .unwrap()
        .save(path)
        .unwrap();
}

/// The custom surfaces: armor primary takes a color and an iridescence row that tints the
/// color, and cloth secondary one that tints the highlight, on every gear type. The weapons' suit
/// primary takes a color, metalness, smoothness, glow and worn color of its own.
pub(super) fn custom_surfaces(packages: &Path) -> Vec<DyeEdit> {
    let lookup = sundial::package_authoring::load_iridescence_rows(packages).unwrap();
    let row = |parity: i16| {
        lookup
            .iter()
            .map(|row| row.id)
            .find(|id| id % 2 == parity)
            .expect("an authored iridescence row of each parity")
    };
    let value = |value: f32| DyeValue::new(value).unwrap();
    vec![
        DyeEdit {
            color: Some([0xC8, 0x32, 0x14]),
            iridescence: Some(row(0)),
            ..DyeEdit::new(None, DyeChannel::Armor, DyeSurface::Primary)
        },
        DyeEdit {
            iridescence: Some(row(1)),
            ..DyeEdit::new(None, DyeChannel::Cloth, DyeSurface::Secondary)
        },
        DyeEdit {
            color: Some([0x14, 0x64, 0xC8]),
            metalness: Some(value(1.0)),
            smoothness: Some([value(0.6), value(0.9)]),
            glow: Some([0x10, 0x20, 0x30]),
            worn_color: Some([0xEE, 0xDD, 0xCC]),
            ..DyeEdit::new(
                Some(GearType::Weapon),
                DyeChannel::Suit,
                DyeSurface::Primary,
            )
        },
    ]
}

/// The weapons' suit dye takes the detail textures of another stock shader's weapon suit dye,
/// repeated twice as often.
pub(super) fn custom_textures(
    app: &PackageAuthoringApp,
    base: &shader_view::DyeRows,
) -> Vec<DyeTextureEdit> {
    let key = GearType::Weapon.key(DyeChannel::Suit);
    let own = dye(base, key).expect("the base shader has a weapon suit dye");
    let catalog = app.catalog.as_ref().unwrap();
    let materials =
        |dyes: &[u16]| sundial::package_authoring::load_dye_materials(&app.packages, dyes).unwrap();
    let own_detail = materials(&[own])[&own].as_ref().unwrap().detail_tag;
    let mut candidates = app.gear_donors[&ItemKind::Shader]
        .iter()
        .filter_map(|shader| dye(&shader_view::stock_rows(catalog, shader.hash), key))
        .filter(|dye| *dye != own)
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    // Read a few at a time, since most stock shaders share a handful of textures.
    let (detail, normal) = candidates
        .chunks(16)
        .find_map(|chunk| {
            materials(chunk)
                .into_values()
                .flatten()
                .find_map(|material| {
                    (material.detail_tag.is_some() && material.detail_tag != own_detail)
                        .then_some((material.detail_tag, material.normal_tag))
                })
        })
        .expect("another shader's weapon suit dye with other textures");
    let value = |value: f32| DyeValue::new(value).unwrap();
    vec![DyeTextureEdit {
        detail,
        normal,
        detail_tiling: Some([value(2.0), value(2.0), value(0.0), value(0.0)]),
        ..DyeTextureEdit::new(Some(GearType::Weapon), DyeChannel::Suit)
    }]
}

/// Cloth is the middle channel of each gear type's three.
const CLOTH: i8 = 1;

pub(super) fn author_shader(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredShader {
    new_from_menu(ctx, app, ItemKind::Shader);
    app.recipe
        .rename_authored_item("Parhelion Test Shader")
        .unwrap();
    let base = app.recipe.donor.item_hash.parse_u32().unwrap();
    assert_page_fits(ctx, app, "Shader");
    let source = remix_cloth(ctx, app, base);
    edit_surfaces(ctx, app, base);
    let (weapon_item, weapon_preview) = preview_weapon(ctx, app);
    let (preview_item, preview) = preview_armor(ctx, app);
    check_weapon_view(ctx, app, weapon_item);
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredShader {
        recipe: app.recipe.clone(),
        base,
        source,
        preview_item,
        preview,
        weapon_item,
        weapon_preview,
    }
}

/// Runs frames until nothing on the page is still loading.
pub(super) fn settle_loaded(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    what: &str,
) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        if !texts(&output).iter().any(|(text, _)| text == "Loading…") {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{what} did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The new shader's page, then its cloth taken from another stock shader through the page's own
/// edit, once every surface has loaded. Returns the source shader.
pub(super) fn remix_cloth(ctx: &egui::Context, app: &mut PackageAuthoringApp, base: u32) -> u32 {
    let output = settle(ctx, app);
    for label in [
        "Dyes",
        "All Gear",
        "Armor Primary",
        "Cloth Secondary",
        "Suit Primary",
        "Copy from Shader…",
        "Preview",
        "Rarity",
    ] {
        find(&output, label, |text, _| text == label);
    }
    capture::write(ctx, &output, "gear-shader");
    // A shader has no lore tab, so its text offers none.
    let opened = toggle_text_presentation(ctx, app, &output);
    find(&opened, "Custom Item-Type Label", |text, _| {
        text == "Custom Item-Type Label"
    });
    assert!(
        !texts(&opened)
            .iter()
            .any(|(text, _)| text.contains("Lore Tab")),
        "a shader page offers no lore tab"
    );
    toggle_text_presentation(ctx, app, &opened);
    // The channel's shader list sets the channel through the page's own edit.
    let catalog = app.catalog.as_ref().unwrap();
    let base_rows = shader_view::stock_rows(catalog, base);
    let source = app.gear_donors[&ItemKind::Shader]
        .iter()
        .map(|shader| shader.hash)
        .find(|hash| {
            *hash != base
                && !channel_matches(&base_rows, &shader_view::stock_rows(catalog, *hash), CLOTH)
        })
        .expect("another shader with other cloth dyes");
    let source_rows = shader_view::stock_rows(catalog, source);
    shader_view::set_shader_channel(&mut app.recipe, &base_rows, &source_rows, CLOTH, None);
    let output = settle(ctx, app);
    find(&output, "Restore Base Dyes", |text, _| {
        text == "Restore Base Dyes"
    });
    // Each surface loads from the installed dyes in the background, then the inspector shows the
    // chosen one's values.
    let output = settle_loaded(ctx, app, "The dyes");
    assert!(
        !texts(&output).iter().any(|(text, _)| text == "Unavailable"),
        "every surface loads"
    );
    for label in [
        "Paint",
        "Color",
        "Iridescence",
        "Detail",
        "Textures",
        "Worn",
        "Glow",
    ] {
        find(&output, label, |text, _| text == label);
    }
    capture::write(ctx, &output, "gear-shader-remixed");
    source
}

/// Two surfaces on every gear type and one weapon surface take custom values, and the weapons'
/// suit dye another shader's textures, through the page's own edits. The values show in the
/// surfaces' tiles and inspector, and the icon draws from them once the dyes and iridescence ramps
/// have loaded.
pub(super) fn edit_surfaces(ctx: &egui::Context, app: &mut PackageAuthoringApp, base: u32) {
    let edits = custom_surfaces(&app.packages);
    for edit in &edits {
        shader_view::set_dye_edit(
            &mut app.recipe.overrides.dye_edits,
            edit.gear,
            edit.channel,
            edit.surface,
            |surface| *surface = *edit,
        );
    }
    assert_eq!(app.recipe.overrides.dye_edits, edits);
    let base_rows = shader_view::stock_rows(app.catalog.as_ref().unwrap(), base);
    let textures = custom_textures(app, &base_rows);
    for texture in &textures {
        shader_view::set_texture_edit(
            &mut app.recipe.overrides.dye_texture_edits,
            texture.gear,
            texture.channel,
            |each| *each = *texture,
        );
    }
    assert_eq!(app.recipe.overrides.dye_texture_edits, textures);
    let output = settle(ctx, app);
    find(&output, "the custom color", |text, _| text == "#C83214");
    // A surface's iridescence shows in the inspector once its tile is chosen.
    for edit in edits.iter().filter(|edit| edit.gear.is_none()) {
        let Some(id) = edit.iridescence else {
            continue;
        };
        let name = format!(
            "{} {}",
            edit.channel.name_on(edit.gear),
            edit.surface.label()
        );
        let output = settle(ctx, app);
        click(ctx, app, find(&output, &name, |text, _| text == name));
        assert_eq!(app.shader_surface, (edit.channel, edit.surface));
        let id = id.to_string();
        find(&settle(ctx, app), "the custom iridescence", |text, _| {
            text == id
        });
    }
    // A new shader draws its icon from its dyes, custom ones included, once the stock dyes and
    // iridescence ramps have loaded.
    assert!(
        app.recipe.overrides.icon_from_dyes,
        "a new shader draws its icon from its dyes"
    );
    let start = Instant::now();
    while app.recipe.overrides.icon_edit.imported_image.is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the page drew the shader's icon from its dyes"
        );
        std::thread::sleep(Duration::from_millis(50));
        let _output = settle(ctx, app);
    }
}

/// Until another item is chosen the page previews the first weapon, which the weapons' own surface
/// and textures change. Returns the weapon and the page's preview of it.
pub(super) fn preview_weapon(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> (u32, Arc<egui::ColorImage>) {
    let weapon = app.donor_summaries[0].hash;
    assert_eq!(app.shader_preview_item, None);
    let weapon_preview = await_preview(|| frame(ctx, app, Vec::new()), "The shader page");
    let catalog = app.catalog.as_ref().unwrap();
    let rows = preview_rows(app.recipe.overrides.render_dye_rows.as_ref().unwrap());
    let weapon_stock = draw_preview(
        &app.packages,
        &catalog.shader_preview_appearance(weapon, &rows).unwrap(),
        &[],
        &weapon_preview,
        "The weapon without custom dyes",
    );
    assert_ne!(
        weapon_stock.pixels, weapon_preview.pixels,
        "the custom dyes change the weapon's preview"
    );
    zoom::check(ctx, app, &weapon_preview);
    (weapon, weapon_preview)
}

/// The preview's item picker finds an armor piece by its hash, and the custom surfaces change what
/// the page draws on it. Returns the armor piece and the page's preview of it.
pub(super) fn preview_armor(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> (u32, Arc<egui::ColorImage>) {
    let catalog = app.catalog.as_ref().unwrap();
    let rows = preview_rows(app.recipe.overrides.render_dye_rows.as_ref().unwrap());
    let armor = app.gear_donors[&ItemKind::Armor]
        .iter()
        .map(|donor| donor.hash)
        .find(|hash| {
            catalog
                .shader_preview_appearance(*hash, &rows)
                .is_some_and(|appearance| {
                    !appearance.dyes.is_empty()
                        && appearance.dyes.iter().all(|(key, _)| (0..=2).contains(key))
                })
        })
        .expect("an armor piece with its own dyes");
    let appearance = catalog.shader_preview_appearance(armor, &rows).unwrap();
    let first = app.donor_summaries[0].name.clone();
    let output = settle(ctx, app);
    find(&output, "Preview", |text, _| text == "Preview");
    click(
        ctx,
        app,
        find(&output, "the preview's item", |text, _| text == first),
    );
    let hash = format!("0x{armor:08X}");
    frame(ctx, app, vec![egui::Event::Text(hash.clone())]);
    let output = settle(ctx, app);
    let row = format!("({hash})");
    click(
        ctx,
        app,
        find(&output, "the armor piece in the picker", |text, _| {
            text.ends_with(&row)
        }),
    );
    assert_eq!(app.shader_preview_item, Some(armor));
    let preview = await_preview(|| frame(ctx, app, Vec::new()), "The shader page");
    capture::write(ctx, &settle(ctx, app), "gear-shader-custom");
    // The custom surfaces change what the preview draws.
    let stock = draw_preview(
        &app.packages,
        &appearance,
        &[],
        &preview,
        "The preview without custom surfaces",
    );
    assert_ne!(
        stock.pixels, preview.pixels,
        "the custom surfaces change the preview"
    );
    (armor, preview)
}

/// The Weapons tab, marked for the weapons' own edits, shows those edits over the ones for every
/// gear type, names the weapons' channels by number as Bungie does, and moves the preview to a
/// weapon.
pub(super) fn check_weapon_view(ctx: &egui::Context, app: &mut PackageAuthoringApp, weapon: u32) {
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "the Weapons tab", |text, _| text == "Weapons •"),
    );
    assert_eq!(app.shader_dye_gear, Some(GearType::Weapon));
    assert_eq!(app.shader_preview_item, Some(weapon));
    await_preview(|| frame(ctx, app, Vec::new()), "The weapons' view");
    let output = settle_loaded(ctx, app, "The weapons' dyes");
    find(&output, "the weapons' suit color", |text, _| {
        text == "#1464C8"
    });
    find(&output, "the weapons' third channel", |text, _| {
        text == "Channel 3 Primary"
    });
    capture::write(ctx, &output, "gear-shader-weapons");
}

/// Each gear type's dye for `channel` in the staged shader. An edited dye is a custom one whose
/// vectors are the stock dye's with that gear type's edits written over them, every value, and
/// which binds the edits' textures. Any other dye keeps the stock one.
pub(super) fn check_custom_dyes(
    packages: &Path,
    name: &str,
    rows: &shader_view::DyeRows,
    stock: &shader_view::DyeRows,
    channel: DyeChannel,
    (edits, textures): (&[DyeEdit], &[DyeTextureEdit]),
) -> Vec<String> {
    let pairs = GearType::ALL
        .into_iter()
        .filter_map(|gear| {
            let key = gear.key(channel);
            Some((gear, key, dye(rows, key)?, dye(stock, key)?))
        })
        .collect::<Vec<_>>();
    assert!(!pairs.is_empty(), "{name} has no {channel:?} dyes");
    let indices = pairs
        .iter()
        .flat_map(|&(_, _, built, original)| [built, original])
        .collect::<Vec<_>>();
    let materials = sundial::package_authoring::load_dye_materials(packages, &indices).unwrap();
    pairs
        .into_iter()
        .map(|(gear, key, built, original)| {
            let surfaces = DyeSurface::ALL
                .into_iter()
                .filter_map(|surface| surface_edit(edits, gear, channel, surface))
                .collect::<Vec<_>>();
            let texture = texture_edit(textures, gear, channel);
            if surfaces.is_empty() && texture.is_none() {
                assert_eq!(built, original, "{name} keeps the stock dye for key {key}");
                return format!("key {key}: stock dye {built}");
            }
            assert_ne!(built, original, "{name} has a custom dye for key {key}");
            let built_material = materials[&built].as_ref().unwrap();
            let stock_material = materials[&original].as_ref().unwrap();
            let mut expected = stock_material.vectors;
            for edit in &surfaces {
                write_vectors(&mut expected, &edit.writes());
            }
            if let Some(texture) = texture {
                write_vectors(&mut expected, &texture.writes());
            }
            assert_eq!(
                built_material.vectors, expected,
                "{name} key {key} carries every edited value"
            );
            let bound = (
                texture
                    .and_then(|texture| texture.detail)
                    .or(stock_material.detail_tag),
                texture
                    .and_then(|texture| texture.normal)
                    .or(stock_material.normal_tag),
            );
            assert_eq!(
                (built_material.detail_tag, built_material.normal_tag),
                bound,
                "{name} key {key} binds its textures"
            );
            format!("key {key}: dye {built} from {original}, textures {bound:?}")
        })
        .collect()
}

/// The staged shader keeps the stock shape and its base's dyes, with cloth from the source and
/// custom dyes for the edited surfaces. Its preview matches the page's preview of the unbuilt
/// edits pixel for pixel.
pub(super) fn read_back_shader(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    shader: &AuthoredShader,
    brand: &str,
    packages: &Path,
    artifacts: &Path,
) -> serde_json::Value {
    let name = &shader.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Shader);
    assert_eq!(
        staged.item_display_name(report.item_hash),
        Some(name.as_str())
    );
    assert!(
        staged.is_shader(report.item_hash),
        "{name} keeps the stock shader shape"
    );
    let rows = shader_view::stock_rows(staged, report.item_hash);
    let base = shader_view::stock_rows(staged, shader.base);
    let source = shader_view::stock_rows(staged, shader.source);
    let edits = &shader.recipe.overrides.dye_edits;
    let textures = &shader.recipe.overrides.dye_texture_edits;
    let custom = DyeChannel::ALL
        .into_iter()
        .flat_map(|channel| {
            // Cloth came from the source shader, and the other channels from the base.
            let stock = if channel.offset() == CLOTH {
                &source
            } else {
                &base
            };
            check_custom_dyes(packages, name, &rows, stock, channel, (edits, textures))
        })
        .collect::<Vec<_>>();
    let built = draw_preview(
        packages,
        &staged
            .shader_preview_appearance(shader.preview_item, &preview_rows(&rows))
            .unwrap(),
        &[],
        &shader.preview,
        "The built shader",
    );
    save_image(
        &shader.preview,
        &artifacts.join("shader-preview-unbuilt.png"),
    );
    save_image(&built, &artifacts.join("shader-preview-built.png"));
    assert_eq!(
        differing_pixels(&built, &shader.preview),
        0,
        "{name} built draws like its unbuilt preview on 0x{:08X}",
        shader.preview_item
    );
    // On a weapon too, with the weapons' own surface and textures.
    let built_weapon = draw_preview(
        packages,
        &staged
            .shader_preview_appearance(shader.weapon_item, &preview_rows(&rows))
            .unwrap(),
        &[],
        &shader.weapon_preview,
        "The built shader on a weapon",
    );
    save_image(
        &shader.weapon_preview,
        &artifacts.join("shader-weapon-unbuilt.png"),
    );
    save_image(&built_weapon, &artifacts.join("shader-weapon-built.png"));
    assert_eq!(
        differing_pixels(&built_weapon, &shader.weapon_preview),
        0,
        "{name} built draws like its unbuilt preview on the weapon 0x{:08X}",
        shader.weapon_item
    );
    check_collections(
        staged,
        ItemKind::Shader,
        shader.base,
        name,
        report.item_hash,
        brand,
    );
    serde_json::json!({
        "kind": ItemKind::Shader,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!(
            "{} 0x{:08X}",
            staged.item_display_name(shader.base).unwrap_or_default(),
            shader.base
        ),
        "cloth_from": format!(
            "{} 0x{:08X}",
            staged.item_display_name(shader.source).unwrap_or_default(),
            shader.source
        ),
        "dye_rows": rows[0]
            .iter()
            .map(|row| format!("{}: {}", row.channel_index, row.dye_reference_index))
            .collect::<Vec<_>>(),
        "dye_edits": edits,
        "dye_texture_edits": textures,
        "custom_dyes": custom,
        "preview": {
            "item": format!(
                "{} 0x{:08X}",
                staged.item_display_name(shader.preview_item).unwrap_or_default(),
                shader.preview_item
            ),
            "size": shader.preview.size,
            "matches_built": true,
        },
        "weapon_preview": {
            "item": format!(
                "{} 0x{:08X}",
                staged.item_display_name(shader.weapon_item).unwrap_or_default(),
                shader.weapon_item
            ),
            "matches_built": true,
        },
        "collections": staged.item_collection_paths(report.item_hash),
    })
}
