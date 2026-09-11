//! Real-fixture smoke coverage for navigation and uncommon account workflows.
use super::*;
use serde_json::json;

pub(super) const FIXTURES: [&str; 3] = [
    include_str!("../../../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v16-1120748-defaults.json"),
];

const HISTORICAL_FIXTURES: [&str; 6] = [
    include_str!("../../../../tests/fixtures/sunrise-v2-052d6a48-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v3-86bd0a16-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v4-b2724889-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v5-161efee2-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"),
    include_str!("../../../../tests/fixtures/sunrise-v15-f84356c1-defaults.json"),
];

#[derive(Clone, Copy)]
enum Page {
    Character(usize),
    Inventory(usize),
    Profile,
    Settings(game_settings::Tab),
    Progression(ProgressionSection),
    Preferences(PreferencesTab),
    Json,
}

fn pages() -> Vec<Page> {
    let mut pages = vec![Page::Profile, Page::Json];
    for index in 0..3 {
        pages.extend([Page::Character(index), Page::Inventory(index)]);
    }
    pages.extend(
        [
            game_settings::Tab::Player,
            game_settings::Tab::Controls,
            game_settings::Tab::Audio,
            game_settings::Tab::Display,
            game_settings::Tab::Interface,
            game_settings::Tab::Social,
            game_settings::Tab::KeyBindings,
            game_settings::Tab::Sunrise,
        ]
        .map(Page::Settings),
    );
    pages.extend(
        [
            ProgressionSection::Collections,
            ProgressionSection::Unlocks,
            ProgressionSection::Investment,
        ]
        .map(Page::Progression),
    );
    pages.extend(PreferencesTab::ALL.map(Page::Preferences));
    pages
}

fn select(app: &mut SundialApp, page: Page) {
    app.view_mode = match page {
        Page::Character(index) => {
            app.selected_character = index;
            ViewMode::Characters
        }
        Page::Inventory(index) => {
            app.selected_character = index;
            ViewMode::CharacterInventory
        }
        Page::Profile => ViewMode::ProfileInventory,
        Page::Settings(tab) => {
            app.game_settings_tab = tab;
            ViewMode::GameSettings
        }
        Page::Progression(section) => {
            app.progression_section = section;
            ViewMode::Progression
        }
        Page::Preferences(tab) => {
            app.preferences_tab = tab;
            ViewMode::Preferences
        }
        Page::Json => ViewMode::AdvancedJson,
    };
}

pub(super) fn with_document(path: PathBuf, json: Value) -> SundialApp {
    let mut app = app(path);
    app.document = WorkspaceDocument::json_only(json.clone());
    app.persisted_document = app.document.clone();
    app.raw_json = serde_json::to_string_pretty(&json).unwrap();
    app.raw_json_document = json;
    app
}

fn draw(app: &mut SundialApp, ctx: &egui::Context, size: egui::Vec2) {
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ctx| {
            app.draw_app_chrome(ctx, None);
            app.draw_active_view(ctx);
        },
    );
    assert!(!output.shapes.is_empty());
    for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            assert!(mesh.vertices.iter().all(|vertex| vertex.pos.is_finite()));
        }
    }
}

#[test]
fn actual_schemas_all_pages_preserve_data_across_sizes_themes_and_navigation() {
    let mut frames = 0;
    for fixture in FIXTURES.into_iter().chain(HISTORICAL_FIXTURES) {
        let mut json: Value = serde_json::from_str(fixture).unwrap();
        json["future_extension"] = json!({"unicode":"玩家 🌅", "null":null,"nested":[false,42]});
        // Every schema gets navigation coverage. Layout and theme combinations
        // use the shipped v8 and current JSON v16 contracts.
        let mut views = vec![(egui::vec2(900.0, 600.0), true)];
        if matches!(json["version"].as_u64(), Some(8 | 16)) {
            views.extend([
                (egui::vec2(640.0, 480.0), false),
                (egui::vec2(1280.0, 800.0), true),
                (egui::vec2(1920.0, 1080.0), false),
            ]);
        }
        for (size, dark) in views {
            let directory = TestDirectory::new("schema-page-smoke");
            let mut app = with_document(directory.0.clone(), json.clone());
            let original = app.document.clone();
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for (page_index, page) in pages().into_iter().enumerate() {
                select(&mut app, page);
                for _ in 0..2 {
                    draw(&mut app, &ctx, size);
                    frames += 1;
                    assert_eq!(
                        app.document, original,
                        "schema {} page {page_index}",
                        json["version"]
                    );
                    assert!(!app.dirty, "schema {} page {page_index}", json["version"]);
                }
            }
            assert!(app.undo_history.is_empty());
            assert!(app.redo_history.is_empty());
            assert!(!app.settings_path.exists());
        }
    }
    eprintln!(
        "Validated {frames} real-fixture UI frames across v2, v3, v4, v5, v6, v8, v13, v15, and v16"
    );
}

#[test]
fn v18_native_database_all_pages_preserve_data_across_sizes_and_themes() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1280.0, 800.0)] {
        for dark in [false, true] {
            let directory = TestDirectory::new("v18-page-smoke");
            let json: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
            ))
            .unwrap();
            let mut app = with_document(directory.0.clone(), json.clone());
            let database = crate::persistence::investment_path(&app.settings_path);
            std::fs::create_dir_all(database.parent().unwrap()).unwrap();
            let db = rusqlite::Connection::open(&database).unwrap();
            db.execute_batch("BEGIN").unwrap();
            for sql in [
                include_str!("../../../persistence/sqlite_account/fixtures/investment_schema.sql"),
                include_str!(
                    "../../../persistence/sqlite_account/fixtures/account_settings_schema.sql"
                ),
                include_str!(
                    "../../../persistence/sqlite_account/fixtures/investment_defaults.sql"
                ),
                include_str!(
                    "../../../persistence/sqlite_account/fixtures/account_settings_defaults.sql"
                ),
            ] {
                db.execute_batch(sql).unwrap();
            }
            db.execute_batch("COMMIT").unwrap();
            app.document = WorkspaceDocument::load(json, &app.settings_path);
            assert_eq!(
                app.document.source_info().kind,
                account::AccountSourceKind::Sqlite
            );
            app.persisted_document = app.document.clone();
            let original = app.document.clone();
            let native = crate::persistence::sqlite_account::package::read(&database).unwrap();
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for (index, page) in pages().into_iter().enumerate() {
                select(&mut app, page);
                for _ in 0..2 {
                    draw(&mut app, &ctx, size);
                    assert_eq!(app.document, original, "v18 page {index}");
                    assert!(!app.dirty, "v18 page {index}");
                }
            }
            assert_eq!(
                crate::persistence::sqlite_account::package::read(&database).unwrap(),
                native
            );
        }
    }
}

#[test]
fn missing_or_malformed_optional_account_sections_are_not_repaired_by_navigation() {
    for version in [6, 8, 16] {
        for state in [
            Value::Null,
            json!({}),
            json!({"characters":[],"account":{}}),
            json!({"characters":"opaque", "account":null}),
        ] {
            let directory = TestDirectory::new("schema-optional-smoke");
            let json = json!({"version":version,"state":state,"future":{"keep":true}});
            let mut app = with_document(directory.0.clone(), json);
            let original = app.document.clone();
            let ctx = egui::Context::default();
            for page in pages() {
                select(&mut app, page);
                draw(&mut app, &ctx, egui::vec2(900.0, 600.0));
                assert_eq!(app.document, original, "schema {version}");
                assert!(!app.dirty);
            }
            assert!(!app.settings_path.exists());
        }
    }
}

#[test]
fn preferences_preserve_data_and_keep_actions_visible() {
    for dark in [true, false] {
        for width in [560.0, 960.0] {
            for tab in PreferencesTab::ALL {
                let directory = TestDirectory::new("preferences-layout");
                let mut app = with_document(
                    directory.0.clone(),
                    serde_json::from_str(FIXTURES[2]).unwrap(),
                );
                app.preferences_tab = tab;
                let before = serde_json::to_value(&app.preferences).unwrap();
                let original = app.document.clone();
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 760.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .show(ctx, |ui| app.draw_preferences_page(ui, ctx));
                        },
                    );
                }
                assert_eq!(serde_json::to_value(&app.preferences).unwrap(), before);
                assert_eq!(app.document, original);
                assert!(!app.dirty);
                let reset = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text)
                            if text.galley.job.text == "Reset Preferences\u{2026}" =>
                        {
                            Some(text.pos)
                        }
                        _ => None,
                    })
                    .expect("reset control");
                assert!(
                    reset.y < 740.0 && reset.x < width - 20.0,
                    "{tab:?}: {reset:?}"
                );
                capture_preferences(
                    &ctx,
                    output,
                    &format!(
                        "sundial-{tab:?}-{}-{width}",
                        if dark { "dark" } else { "light" }
                    ),
                    width,
                );
            }
        }
    }
}

pub(super) fn capture_preferences(
    ctx: &egui::Context,
    output: egui::FullOutput,
    name: &str,
    width: f32,
) {
    let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    let atlas = ctx.fonts(|fonts| fonts.image());
    let pixels = atlas
        .srgba_pixels(None)
        .flat_map(|color| color.to_array())
        .collect::<Vec<_>>();
    std::fs::write(directory.join(format!("{name}-atlas.rgba")), &pixels).unwrap();
    let meshes = ctx.tessellate(output.shapes, 1.0).into_iter().filter_map(|primitive| {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { return None; };
        assert_eq!(mesh.texture_id, egui::TextureId::Managed(0));
        Some(serde_json::json!({
            "clip": [primitive.clip_rect.min.x, primitive.clip_rect.min.y, primitive.clip_rect.max.x, primitive.clip_rect.max.y],
            "indices": mesh.indices,
            "vertices": mesh.vertices.iter().map(|v| serde_json::json!([v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.color.to_array()])).collect::<Vec<_>>()
        }))
    }).collect::<Vec<_>>();
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!({"width": width, "height": 760, "atlas_size": atlas.size, "meshes": meshes}))
            .unwrap(),
    )
    .unwrap();
}
