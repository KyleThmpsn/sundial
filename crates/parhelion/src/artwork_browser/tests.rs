use super::*;

fn picker() -> Picker {
    let mut picker = Picker::default();
    picker.rows.extend((0..1200).map(|i| Row {
        icon: Icon::Texture {
            tag: (0x80B40000 + i).into(),
        },
        local: None,
        white: true,
        label: format!("Icon {i}"),
        search: format!("icon {i}"),
        source: 1,
        image: egui::ColorImage::new([64, 64], egui::Color32::WHITE),
    }));
    picker.attempted = true;
    picker
}

fn frame(
    ctx: &egui::Context,
    picker: &mut Picker,
    query: &mut String,
    width: f32,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Selection>) {
    let mut selection = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                selection = picker.draw(
                    ui,
                    query,
                    false,
                    ui.available_height(),
                    Browser {
                        packages: None,
                        catalog: None,
                        current: None,
                    },
                );
            });
        },
    );
    (output, selection)
}

fn label(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if node.label() != Some(label) {
                return None;
            }
            let bounds = node.bounds()?;
            Some(egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            ))
        })
}

#[test]
fn grid_keeps_actions_in_bottom_chin_and_only_uploads_visible_icons() {
    for width in [640.0, 1050.0] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut picker = picker();
        let mut query = String::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = frame(&ctx, &mut picker, &mut query, width, vec![]).0;
        }
        for name in [
            "Select Existing Perk",
            "Download destiny-icons",
            "Add Icon…",
        ] {
            let bounds = label(&output, name).unwrap_or_else(|| panic!("Missing {name}"));
            assert!(
                bounds.top() > 530.0 && bounds.bottom() <= 600.0,
                "{name}: {bounds:?}"
            );
            assert!(bounds.right() <= width, "{name}: {bounds:?}");
        }
        assert!(picker.textures.len() < 100);
        assert!(!picker.textures.is_empty());
        picker.downloaded = true;
        output = frame(&ctx, &mut picker, &mut query, width, vec![]).0;
        assert!(label(&output, "Download destiny-icons").is_none());
        assert!(label(&output, "Add Icon…").is_some());
        query = "previous search".into();
        picker.events().send(Event::RevealLocal(2)).unwrap();
        frame(&ctx, &mut picker, &mut query, width, vec![]);
        assert!(query.is_empty());
        assert_eq!(picker.source, 2);
    }
}

#[test]
fn search_can_select_an_icon_beyond_the_initial_grid() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut picker = picker();
    let mut query = "1199".to_owned();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(&ctx, &mut picker, &mut query, 800.0, vec![]).0;
    }
    let pos = label(&output, "Icon 1199").unwrap().center();
    let mut selected = None;
    for pressed in [true, false] {
        selected = frame(
            &ctx,
            &mut picker,
            &mut query,
            800.0,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        )
        .1
        .or(selected);
    }
    let Some(Selection::Icon(icon)) = selected else {
        panic!("Icon was not selected");
    };
    assert_eq!(icon, picker.rows[1199].icon);
}

#[test]
fn artwork_selection_embeds_full_source_before_the_library_can_disappear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.png");
    let mut pixels = image::RgbaImage::new(768, 256);
    pixels.put_pixel(384, 128, image::Rgba([255; 4]));
    pixels.save(&path).unwrap();
    for purpose in [Purpose::Badge, Purpose::Watermark] {
        let mut picker = Picker::for_purpose(purpose);
        let receiver = picker.artwork(
            Selection::Local(path.clone()),
            Path::new("unused"),
            None,
            &egui::Context::default(),
        );
        let artwork = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(artwork.pixels(), &pixels);
        let saved = serde_json::to_vec(&artwork).unwrap();
        let restored: crate::presentation::Artwork = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.pixels(), &pixels);
        assert_eq!(restored.render(96, 96).dimensions(), (96, 96));
        assert_eq!(restored.render(512, 312).dimensions(), (512, 312));
    }
}

#[test]
fn native_names_packages_and_hashes_are_searchable_together() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut picker = picker();
    picker.rows.clear();
    picker
        .events()
        .send(Event::Native(package_icons::Entry {
            tag: tiger_pkg::TagHash(0x813180C9),
            package: "w64_investment_0131_0.pkg".into(),
            name: "ui/perks/Outlaw_reload.dds\nui/perks/precision_kill.dds".into(),
            white: true,
            size: [96, 96],
            thumbnail: egui::ColorImage::new([64, 64], egui::Color32::WHITE),
        }))
        .unwrap();
    for search in [
        "OUTLAW",
        "precision kill",
        "outlaw investment",
        "0x813180c9",
        "outlaw 0131_0.pkg",
    ] {
        let mut query = search.to_owned();
        frame(&ctx, &mut picker, &mut query, 800.0, vec![]);
        assert_eq!(
            picker.textures.len(),
            1,
            "Missing named texture for {search}"
        );
    }
    let mut query = "unrelated".to_owned();
    frame(&ctx, &mut picker, &mut query, 800.0, vec![]);
    assert!(picker.textures.is_empty());
}

#[test]
fn all_colors_is_opt_in_and_keeps_the_existing_search() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut picker = picker();
    picker.rows.truncate(2);
    picker.rows[1].white = false;
    picker.rows[1].source = 2;
    let mut query = "icon".to_owned();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(&ctx, &mut picker, &mut query, 800.0, vec![]).0;
    }
    assert!(!picker.all_colors);
    assert_eq!(picker.textures.len(), 1);
    let pos = label(&output, "Show All Colors").unwrap().center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut picker,
            &mut query,
            800.0,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(&ctx, &mut picker, &mut query, 800.0, vec![]);
    assert!(picker.all_colors);
    assert_eq!(picker.textures.len(), 2);
    assert_eq!(query, "icon");
    assert_eq!(
        picker.rows.len(),
        2,
        "Changing the filter does not require rescanning packages"
    );
}
