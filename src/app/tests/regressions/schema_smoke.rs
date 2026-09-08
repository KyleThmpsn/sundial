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
        for size in [
            egui::vec2(640.0, 480.0),
            egui::vec2(900.0, 600.0),
            egui::vec2(1280.0, 800.0),
            egui::vec2(1920.0, 1080.0),
        ] {
            for dark in [false, true] {
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
    }
    eprintln!(
        "Validated {frames} real-fixture UI frames across v2, v3, v4, v5, v6, v8, v13, v15, and v16"
    );
}

#[test]
fn committed_historical_defaults_validate_without_repair_or_schema_upgrade() {
    for fixture in FIXTURES.into_iter().chain(HISTORICAL_FIXTURES) {
        let original: Value = serde_json::from_str(fixture).unwrap();
        let document = original.clone();
        settings::validate_document(&document)
            .unwrap_or_else(|error| panic!("schema {}: {error}", document["version"]));
        let encoded = settings::encode_settings(&document).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), original);
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
