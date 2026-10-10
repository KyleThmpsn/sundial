//! Emblem authoring and independent staged nameplate readback.

use super::*;

/// An emblem from the New menu with another emblem's banner picked in its page's Nameplate
/// section, the base's own overlay and a picture of its own as its background. A file dialog
/// cannot be driven here, so the picture goes into the recipe the way an import puts it.
pub(super) struct AuthoredEmblem {
    pub(super) recipe: WeaponRecipe,
    pub(super) base: u32,
    pub(super) banner: u32,
    pub(super) banner_picture: EmbeddedImage,
    pub(super) background: EmbeddedImage,
}

/// The emblem page: base, icon and rarity as on other gear, then a tile for each nameplate image
/// naming its size, `sizes`, and the editor of the selected one, the banner at first, in place of
/// sockets and stats. No model preview.
pub(super) fn check_emblem_page(output: &egui::FullOutput, sizes: [(u32, u32); 3]) {
    let drawn = texts(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    for label in [
        "Base Emblem",
        "Inventory Icon",
        "Rarity",
        "Text Presentation",
        "Nameplate",
        "Banner",
        "Overlay",
        "Background",
        "Import Image…",
        "Export PNG…",
    ] {
        assert!(
            drawn.iter().any(|text| text == label),
            "Emblem page lacks {label}"
        );
    }
    for (part, (width, height)) in NameplatePart::ALL.into_iter().zip(sizes) {
        let size = format!("({width} × {height} px)");
        assert!(
            drawn.contains(&size),
            "the {} tile names its size, {size}",
            part.label()
        );
    }
    assert_eq!(
        drawn.iter().filter(|text| *text == "Banner").count(),
        2,
        "the banner's tile starts selected, with its editor below"
    );
    assert!(
        !drawn.iter().any(|text| text == "Perks & Sockets"),
        "Emblem page shows sockets"
    );
    assert!(
        accessible(output, "Model Preview").is_none(),
        "Emblem page shows a model preview"
    );
}

/// A nameplate container's banner, overlay and background layers, where it has them.
pub(super) fn nameplate_layers(manager: &PackageManager, container: u32) -> [Option<u32>; 3] {
    let payload = manager.read_tag(container).unwrap();
    NameplatePart::ALL.map(|part| {
        let offset = part.layer_offset();
        let layer = u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
        (layer != u32::MAX).then_some(layer)
    })
}

/// A layer's texture header and size.
pub(super) fn layer_texture(manager: &PackageManager, layer: u32) -> (TagHash, (u32, u32)) {
    let layer_tag = TagHash(layer);
    let payload = manager.read_tag(layer_tag).unwrap();
    let header = crate::icon_edit::texture_reference_offsets(&payload, layer_tag).unwrap()[0].1;
    let header_payload = manager.read_tag(header).unwrap();
    let side = |offset: usize| {
        u32::from(u16::from_le_bytes([
            header_payload[offset],
            header_payload[offset + 1],
        ]))
    };
    (header, (side(0x0E), side(0x10)))
}

/// Frames until the page has loaded each of these emblems' images.
pub(super) fn settle_nameplate(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    images: [(u32, NameplatePart); 3],
) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        frame(ctx, app, Vec::new());
        let catalog = app.catalog.as_ref().unwrap();
        let loaded = images.iter().all(|&(emblem, part)| {
            catalog
                .nameplate_texture(ctx, emblem, part.layer_offset())
                .is_some()
        });
        if loaded {
            return settle(ctx, app);
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the nameplate images did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A picture with detail everywhere, so a crop or a resize shows in the built texture.
pub(super) fn background_picture() -> EmbeddedImage {
    EmbeddedImage::from_rgba(image::RgbaImage::from_fn(1200, 120, |x, y| {
        image::Rgba([(x % 256) as u8, (y * 2) as u8, ((x / 5) % 256) as u8, 255])
    }))
    .unwrap()
}

pub(super) fn author_emblem(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredEmblem {
    new_from_menu(ctx, app, ItemKind::Emblem);
    let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
    let catalog = app.catalog.as_ref().unwrap();
    // A base with all three images, so a picture has a background to take the place of.
    let emblems = app.gear_donors[&ItemKind::Emblem]
        .iter()
        .filter(|emblem| {
            emblem.collection_backed
                && catalog
                    .nameplate_container(emblem.hash)
                    .is_some_and(|container| {
                        nameplate_layers(&manager, container)
                            .iter()
                            .all(Option::is_some)
                    })
        })
        .collect::<Vec<_>>();
    let base = emblems
        .first()
        .expect("a Collections emblem with all three images");
    // A banner that differs from the base's, so the one the build carries is the one picked.
    let target = emblems
        .iter()
        .find(|emblem| {
            catalog.nameplate_container(emblem.hash) != catalog.nameplate_container(base.hash)
        })
        .expect("two emblems with different nameplates");
    let (base_hash, base_name) = (base.hash, base.name.clone());
    let (target_hash, target_name) = (target.hash, target.name.clone());
    let base_layers = nameplate_layers(&manager, catalog.nameplate_container(base_hash).unwrap());
    let size = |layer: Option<u32>| layer_texture(&manager, layer.unwrap()).1;
    app.recipe.set_donor(base_hash, base_name);
    app.recipe
        .rename_authored_item("Parhelion Test Emblem")
        .unwrap();

    assert_page_fits(ctx, app, "Emblem");
    let output = settle_nameplate(ctx, app, NameplatePart::ALL.map(|part| (base_hash, part)));
    check_emblem_page(&output, base_layers.map(size));
    capture::write(ctx, &output, "gear-emblem");

    // An emblem has no lore tab, so its text offers none.
    let opened = toggle_text_presentation(ctx, app, &output);
    find(&opened, "Custom Item-Type Label", |text, _| {
        text == "Custom Item-Type Label"
    });
    assert!(
        !texts(&opened)
            .iter()
            .any(|(text, _)| text.contains("Lore Tab")),
        "an emblem page offers no lore tab"
    );
    let output = toggle_text_presentation(ctx, app, &opened);

    // Every image starts on its base's. The selected banner's picker reads Base after the three
    // tiles that read it too, and its search takes focus as it opens, so the reader types the
    // emblem's name.
    let picker = texts(&output)
        .into_iter()
        .rev()
        .find(|(text, _)| text == "Base")
        .map(|(_, rect)| rect.center())
        .expect("the banner's picker");
    click(ctx, app, picker);
    frame(ctx, app, vec![egui::Event::Text(target_name.clone())]);
    let output = settle(ctx, app);
    capture::write(ctx, &output, "gear-emblem-picker");
    // The search box holds the bare name. A row names the emblem with its hash after it.
    let row_label = format!("{target_name}  (");
    let row = texts(&output)
        .into_iter()
        .find(|(text, _)| text.starts_with(&row_label))
        .map(|(_, rect)| rect.center())
        .unwrap_or_else(|| panic!("{target_name} is not in the nameplate list"));
    click(ctx, app, row);
    assert_eq!(
        app.recipe
            .overrides
            .nameplate
            .as_ref()
            .and_then(|nameplate| nameplate.part(NameplatePart::Banner)),
        Some(&NameplateImage::Emblem {
            item_hash: target_hash.into(),
        }),
        "the banner's picker picks {target_name}"
    );

    let banner_picture = EmbeddedImage::from_rgba(
        crate::emblem::layer_pixels(
            &manager,
            TagHash(
                app.catalog
                    .as_ref()
                    .unwrap()
                    .nameplate_container(target_hash)
                    .unwrap(),
            ),
            NameplatePart::Banner,
        )
        .unwrap(),
    )
    .unwrap();
    settle_nameplate(
        ctx,
        app,
        [
            (target_hash, NameplatePart::Banner),
            (base_hash, NameplatePart::Overlay),
            (base_hash, NameplatePart::Background),
        ],
    );
    for name in ["Edit Artwork…", "Flip Horizontally", "Apply Artwork"] {
        let output = settle(ctx, app);
        click(ctx, app, find(&output, name, |text, _| text == name));
        if name == "Flip Horizontally" {
            let output = settle(ctx, app);
            capture::write(ctx, &output, "gear-emblem-banner-editor");
        }
    }

    // The background's tile selects it, and the picture goes in as an import puts it.
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "Background tile", |text, _| text == "Background"),
    );
    settle(ctx, app);
    assert_eq!(
        app.emblem_page.selected,
        NameplatePart::Background,
        "the background's tile selects it"
    );
    let background = background_picture();
    app.recipe
        .overrides
        .nameplate
        .get_or_insert_with(Default::default)
        .set(
            NameplatePart::Background,
            Some(NameplateImage::Image {
                image: background.clone(),
            }),
        );
    // Edited artwork and the imported picture name the base's native canvas sizes.
    let output = settle_nameplate(
        ctx,
        app,
        [
            (target_hash, NameplatePart::Banner),
            (base_hash, NameplatePart::Overlay),
            (base_hash, NameplatePart::Background),
        ],
    );
    let drawn = texts(&output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    for label in ["Edited Artwork", "Base", "Picture"] {
        assert!(
            drawn.iter().any(|text| text == label),
            "the nameplate shows {label}"
        );
    }
    for (part, layer) in [
        (NameplatePart::Banner, base_layers[0]),
        (NameplatePart::Overlay, base_layers[1]),
        (NameplatePart::Background, base_layers[2]),
    ] {
        let (width, height) = size(layer);
        let label = format!("({width} × {height} px)");
        assert!(
            drawn.contains(&label),
            "the {} tile names {label}",
            part.label()
        );
    }
    capture::write(ctx, &output, "gear-emblem-nameplate");
    for name in ["Edit Artwork…", "Colors", "Invert Colors", "Apply Artwork"] {
        let output = settle(ctx, app);
        click(ctx, app, find(&output, name, |text, _| text == name));
        if name == "Invert Colors" {
            let output = settle(ctx, app);
            capture::write(ctx, &output, "gear-emblem-background-editor");
        }
    }
    // The reader saves before starting the next item, so New starts without a prompt.
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredEmblem {
        recipe: app.recipe.clone(),
        base: base_hash,
        banner: target_hash,
        banner_picture,
        background,
    }
}

/// The staged emblem keeps its base's bucket and rarity, and sits on the runtime's page under
/// Emblems and in its badge. Its strings and its presentation row name a nameplate container of
/// its own, with a privately flipped donor banner, the base's overlay and a privately inverted
/// background. The donor's banner colors survive editing. The page's export writes the edited
/// background into `artifacts`.
pub(super) fn read_back_emblem(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    emblem: &AuthoredEmblem,
    (brand, packages): (&str, &Path),
    artifacts: &Path,
) -> serde_json::Value {
    let name = &emblem.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Emblem);
    let donor = staged
        .gear_donor(report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as an emblem"));
    let base = staged.gear_donor(emblem.base).unwrap();
    assert_eq!(
        ItemKind::from_bucket_hash(donor.summary.bucket_hash),
        Some(ItemKind::Emblem)
    );
    assert_eq!(&donor.summary.name, name);
    assert_eq!(donor.summary.rarity, base.summary.rarity);
    assert!(donor.sockets.is_empty(), "{name} has no sockets");
    check_collections(
        staged,
        ItemKind::Emblem,
        emblem.base,
        name,
        report.item_hash,
        brand,
    );

    let container = staged
        .nameplate_container(report.item_hash)
        .unwrap_or_else(|| panic!("{name} names a nameplate in its strings"));
    let base_container = staged.nameplate_container(emblem.base).unwrap();
    assert_ne!(
        container, base_container,
        "{name} has a nameplate container of its own"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let dense = manager
        .read_tag(
            investment_globals_table_tag(&globals, GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT)
                .unwrap(),
        )
        .unwrap();
    let fields = crate::item::dense_field_tags(&dense, usize::from(report.item_index)).unwrap();
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == crate::emblem::NAMEPLATE_FIELD_TYPE)
            .map(|(_, tags)| tags.clone()),
        Some(vec![container]),
        "{name}'s presentation row shows its own nameplate"
    );
    let nameplate = read_back_nameplate(
        &manager,
        emblem,
        (
            container,
            base_container,
            staged.nameplate_container(emblem.banner).unwrap(),
        ),
        packages,
        artifacts,
    );
    serde_json::json!({
        "kind": ItemKind::Emblem,
        "name": donor.summary.name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.summary.name, emblem.base),
        "rarity": format!("{:?}", donor.summary.rarity),
        "nameplate": nameplate,
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

pub(super) fn read_back_nameplate(
    manager: &PackageManager,
    emblem: &AuthoredEmblem,
    (container, base_container, donor_container): (u32, u32, u32),
    packages: &Path,
    artifacts: &Path,
) -> serde_json::Value {
    let name = &emblem.recipe.name;
    let layers = nameplate_layers(manager, container);
    let base_layers = nameplate_layers(manager, base_container);
    let picked = nameplate_layers(manager, donor_container);
    assert_ne!(layers[0], picked[0], "{name} has a privately edited banner");
    let (_, banner_size) = layer_texture(manager, layers[0].unwrap());
    let banner =
        crate::emblem::layer_pixels(manager, TagHash(container), NameplatePart::Banner).unwrap();
    assert_eq!(
        banner,
        image::imageops::flip_horizontal(&crate::image_import::cover(
            emblem.banner_picture.pixels(),
            banner_size.0,
            banner_size.1
        ))
    );
    assert_eq!(
        manager.read_tag(container).unwrap()[0x30..0x50],
        manager.read_tag(donor_container).unwrap()[0x30..0x50],
        "editing the donor banner preserves its native colors"
    );
    assert_eq!(layers[1], base_layers[1], "{name} keeps the base's overlay");
    let background = layers[2].expect("a background layer");
    assert_ne!(
        Some(background),
        base_layers[2],
        "{name}'s background is a layer of its own"
    );
    let (header, (width, height)) = layer_texture(manager, background);
    let data = manager
        .read_tag(TagHash(manager.get_entry(header).unwrap().reference))
        .unwrap();
    let mut picture = crate::image_import::cover(emblem.background.pixels(), width, height);
    assert_eq!(data.len(), picture.as_raw().len());
    for pixel in picture.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = 255 - *channel;
        }
    }
    assert!(
        data.iter()
            .zip(picture.as_raw())
            .all(|(&actual, &expected)| actual.abs_diff(expected) <= 1),
        "{name}'s background texture holds the picture covering {width}x{height}"
    );
    let export = artifacts.join("emblem-background.png");
    emblem_view::write_png(
        &export,
        packages,
        NameplatePart::Background,
        emblem_view::Export::Layer(container),
    )
    .unwrap();
    assert!(
        image::open(&export).unwrap().into_rgba8().as_raw() == &data,
        "{name}'s background exports as the picture covering {width}x{height}"
    );
    let tag = |layer: Option<u32>| layer.map(|layer| format!("0x{layer:08X}"));
    serde_json::json!({
        "container": format!("0x{container:08X}"),
        "banner": tag(layers[0]),
        "banner_from": format!("0x{:08X}", emblem.banner),
        "overlay": tag(layers[1]),
        "background": tag(layers[2]),
        "background_size": format!("{width}x{height}"),
        "background_export": "emblem-background.png",
    })
}
