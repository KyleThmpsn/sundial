use super::super::authoring::{
    read_icon_layer, read_primary_layer_tag, read_rgba8_texture_pair, texture_reference_offsets,
};
use super::super::preview::DecodedIconImage;
use super::super::preview::{
    composite_icon, load_bundled_preview_watermark, load_primary_preview_layer,
};
use super::super::{ImportedIcon, build_weapon_icon_edit_plan};
use super::*;
use crate::tag_payload::{read_u16, read_u32};
use sundial::package_authoring::icon_schema::ICON_BACKGROUND_LAYER_OFFSET;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn native_rarity_backgrounds_match_stock_and_preserve_exotic_artwork() {
    use crate::AuthoredWeaponRarity as R;
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let exotic = TagHash(0x8132_36D9); // Cerberus+1
    let original = manager.read_tag(exotic).unwrap();
    let original_primary = load_primary_preview_layer(&manager, &original, exotic).unwrap();
    let mut previews = Vec::new();
    for (rarity, stock_icon) in [
        (R::Common, 0x8132_57FA),    // Khvostov 7G-02
        (R::Uncommon, 0x8132_57E6),  // Cydonia-AR1
        (R::Rare, 0x8132_57C9),      // Cuboid ARu
        (R::Legendary, 0x8132_57A7), // Age-Old Bond
        (R::Exotic, 0x8132_36D9),    // Cerberus+1
    ] {
        let stock_icon = TagHash(stock_icon);
        let stock = manager.read_tag(stock_icon).unwrap();
        assert_eq!(
            read_u32(&stock, ICON_BACKGROUND_LAYER_OFFSET).unwrap(),
            rarity.icon_background_layer().0
        );
        let preview = load_icon_preview(&manager, exotic, rarity, false).unwrap();
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
        assert_eq!(preview.primary.rgba, original_primary.rgba);
        let stock_preview = load_icon_preview(&manager, stock_icon, rarity, false).unwrap();
        assert_eq!(
            preview.background.as_ref().unwrap().rgba,
            stock_preview.background.as_ref().unwrap().rgba
        );
        let rendered = preview.render(&WeaponIconEdit::default()).unwrap();
        assert!(
            !previews.contains(&rendered),
            "Each rarity must visibly differ"
        );
        previews.push(rendered);
    }
    assert_eq!(manager.read_tag(exotic).unwrap(), original);
}

fn imported_edit() -> WeaponIconEdit {
    let source = image::RgbaImage::from_fn(96, 96, |x, y| {
        image::Rgba([
            if x < 48 { 240 } else { 20 },
            if y < 48 { 80 } else { 200 },
            100,
            128,
        ])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    source.write_to(&mut png, image::ImageFormat::Png).unwrap();
    WeaponIconEdit {
        imported_image: Some(ImportedIcon::from_bytes(png.get_ref()).unwrap()),
        ..Default::default()
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn multiple_replacements_match_compiled_artwork_and_keep_context_layers() {
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let container = TagHash(0x8132_5796);
    let mut edit = imported_edit();
    edit.color_replacements = vec![
        IconColorReplacement {
            source: [240, 80, 100],
            replacement: [255, 0, 0],
            range_percent: 0,
            hue_range_degrees: None,
        },
        IconColorReplacement {
            source: [20, 200, 100],
            replacement: [255, 128, 0],
            range_percent: 0,
            hue_range_degrees: None,
        },
    ];
    edit.hue_shift_degrees = 120;
    edit.green_balance = 25;
    edit.invert = true;
    let preview = load_icon_preview(
        &manager,
        container,
        crate::AuthoredWeaponRarity::Legendary,
        false,
    )
    .unwrap();
    let source = preview.source_primary(&edit);
    assert_eq!(
        color_selection::sample(&source, egui::vec2(0.1, 0.1)),
        Some([240, 80, 100])
    );
    let plan = build_weapon_icon_edit_plan(&manager, 0x0914, 5300, 0, container, &edit)
        .unwrap()
        .unwrap();
    let authored = DecodedIconImage {
        size: preview.primary.size,
        rgba: plan.new_tags[0].payload.clone(),
    };
    assert_eq!(&authored.rgba[..4], &[255, 0, 0, 128]);
    let second_replacement = (72 * preview.primary.size[0] + 72) * 4;
    assert_eq!(
        &authored.rgba[second_replacement..second_replacement + 4],
        &[255, 128, 0, 128]
    );
    assert_ne!(authored.rgba, source.rgba);
    assert_eq!(
        preview.render(&edit).unwrap(),
        composite_icon([
            preview.background.as_ref(),
            Some(&authored),
            Some(&preview.authored_watermark),
            preview.foreground.as_ref(),
        ])
    );
}

fn layer(color: [u8; 4]) -> DecodedIconImage {
    DecodedIconImage {
        size: [96, 96],
        rgba: color.repeat(96 * 96),
    }
}

#[test]
fn editor_pages_keep_controls_and_footer_inside_the_viewport_without_scrolling() {
    for size in [
        egui::vec2(900.0, 640.0),
        egui::vec2(900.0, 728.0),
        egui::vec2(1320.0, 900.0),
        egui::vec2(380.0, 420.0),
    ] {
        for tab in [
            IconEditorTab::Preview,
            IconEditorTab::Recolor,
            IconEditorTab::Adjust,
            IconEditorTab::Image,
        ] {
            let context = egui::Context::default();
            let mut editor = WeaponIconEditor {
                donor_hash: 1,
                donor_name: "A weapon with a deliberately long appearance donor name".into(),
                draft: WeaponIconEdit {
                    color_replacements: vec![
                        IconColorReplacement {
                            replacement: [255, 128, 0],
                            ..Default::default()
                        };
                        16
                    ],
                    ..Default::default()
                },
                preview: Ok(LoadedIconPreview {
                    background: Some(layer([60, 20, 90, 255])),
                    primary: layer([20, 80, 200, 255]),
                    authored_watermark: load_bundled_preview_watermark().unwrap(),
                    foreground: None,
                    warnings: Vec::new(),
                    plain: false,
                }),
                source_texture: None,
                edited_texture: None,
                rendered_edit: None,
                image_import: Default::default(),
                tab,
                color_page: 0,
                pick_target: PickTarget::default(),
            };
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for page in [0, 15] {
                editor.color_page = page;
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ui| {
                            editor.show(ui);
                        },
                    );
                }
                for label in ["Cancel", "Apply Icon Changes"] {
                    let rect = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == label => {
                                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("{label} missing at {size:?} on {tab:?}"));
                    assert!(
                        screen.contains_rect(rect),
                        "{label} outside {size:?} on {tab:?}: {rect:?}"
                    );
                }
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            screen.expand(1.0).contains_rect(rect),
                            "Text {:?} outside {size:?} on {tab:?}: {rect:?}",
                            text.galley.job.text
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn editor_reset_changes_the_draft_and_escape_returns_cancel() {
    let saved = imported_edit();
    let mut editor = WeaponIconEditor {
        donor_hash: 1,
        donor_name: "Test donor".to_owned(),
        draft: saved,
        preview: Err("No packages required for draft test".to_owned()),
        source_texture: None,
        edited_texture: None,
        rendered_edit: None,
        image_import: Default::default(),
        tab: IconEditorTab::default(),
        color_page: 0,
        pick_target: PickTarget::default(),
    };
    let context = egui::Context::default();
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(380.0, 420.0));
    let mut frame = |events| {
        let mut action = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(viewport),
                events,
                ..Default::default()
            },
            |ui| {
                action = editor.show(ui);
            },
        );
        (output, action)
    };
    frame(Vec::new());
    let (output, _) = frame(Vec::new());
    let reset = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Reset All" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("Reset All should be visible in the compact footer");
    assert!(viewport.contains(reset));
    frame(vec![
        egui::Event::PointerMoved(reset),
        egui::Event::PointerButton {
            pos: reset,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    frame(vec![egui::Event::PointerButton {
        pos: reset,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    let (_, action) = frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    assert_eq!(action, Some(WeaponIconEditorAction::Cancel));
    assert!(editor.draft.is_identity());
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_import_matches_preview_and_preserves_native_texture_graph() {
    let directory = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(Path::new(&directory)).unwrap();
    let container = TagHash::from(0x8132_5796); // Misfit's audited RGBA8 artwork.
    let edit = imported_edit();
    let donor_layer = read_primary_layer_tag(&manager, container).unwrap();
    let donor_payload = read_icon_layer(&manager, donor_layer, container).unwrap();
    let references = texture_reference_offsets(&donor_payload, donor_layer).unwrap();
    assert!(
        !references.is_empty(),
        "the configured icon has no texture references"
    );
    let plan = build_weapon_icon_edit_plan(&manager, 0x0914, 5300, 0, container, &edit)
        .unwrap()
        .unwrap();
    for (_, header_tag) in references {
        let (data_tag, donor_pixels, donor_header) =
            read_rgba8_texture_pair(&manager, donor_layer, header_tag).unwrap();
        let width = usize::from(read_u16(&donor_header, 0x0E).unwrap());
        let height = usize::from(read_u16(&donor_header, 0x10).unwrap());
        let authored_header = &plan
            .new_tags
            .iter()
            .find(|tag| tag.template_tag == header_tag)
            .unwrap()
            .payload;
        let authored_pixels = &plan
            .new_tags
            .iter()
            .find(|tag| tag.template_tag == data_tag)
            .unwrap()
            .payload;
        assert_eq!(&authored_header[4..], &donor_header[4..]);
        assert_eq!(authored_pixels.len(), donor_pixels.len());
        assert_eq!(
            authored_pixels,
            edit.imported_image
                .as_ref()
                .unwrap()
                .fit_to(width as u32, height as u32)
                .as_raw()
        );
        assert_eq!(manager.read_tag(data_tag).unwrap(), donor_pixels);
    }
    assert_eq!(manager.read_tag(donor_layer).unwrap(), donor_payload);
    let preview = load_icon_preview(
        &manager,
        container,
        crate::AuthoredWeaponRarity::Legendary,
        false,
    )
    .unwrap();
    let rendered = preview.render(&edit).unwrap();
    let primary = DecodedIconImage {
        size: preview.primary.size,
        rgba: plan.new_tags[0].payload.clone(),
    };
    assert_eq!(
        rendered,
        composite_icon([
            preview.background.as_ref(),
            Some(&primary),
            Some(&preview.authored_watermark),
            preview.foreground.as_ref(),
        ])
    );
    if let Some(directory) = crate::test_support::artifacts("icon-previews") {
        let rgba: Vec<u8> = rendered
            .pixels
            .iter()
            .flat_map(egui::Color32::to_srgba_unmultiplied)
            .collect();
        image::save_buffer(
            std::path::Path::new(&directory).join("imported-icon-preview.png"),
            &rgba,
            96,
            96,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
