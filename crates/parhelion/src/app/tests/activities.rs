use super::*;
use crate::test_support::driver::{control, label, tap};

fn frame(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_library_windows(ui);
            app.draw_activity_editor(ui);
        },
    );
    sundial::test_support::capture::record(&output);
    output
}

fn click(app: &mut PackageAuthoringApp, ctx: &egui::Context, name: &str) {
    let mut output = frame(app, ctx, vec![]);
    for _ in 0..2 {
        output = frame(app, ctx, vec![]);
    }
    let at = control(&output, name)
        .or_else(|| label(&output, name))
        .unwrap_or_else(|| panic!("Missing {name}"))
        .center();
    for events in tap(at) {
        frame(app, ctx, events);
    }
}

fn enter(app: &mut PackageAuthoringApp, ctx: &egui::Context, name: &str, value: &str) {
    click(app, ctx, name);
    frame(
        app,
        ctx,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Text(value.into()),
        ],
    );
}

/// Scrolls the activity fields by `delta` points with the pointer over the text `over`, then lets
/// the scroll animation settle.
fn scroll(app: &mut PackageAuthoringApp, ctx: &egui::Context, over: &str, delta: f32) {
    let output = frame(app, ctx, vec![]);
    let at = label(&output, over)
        .unwrap_or_else(|| panic!("Missing {over}"))
        .center();
    frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ],
    );
    for _ in 0..30 {
        frame(app, ctx, vec![]);
    }
}

#[test]
#[allow(clippy::cognitive_complexity)]
fn activity_library_edit_save_selection_and_conflict_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let maps = directory.path().join("maps");
    std::fs::create_dir(&maps).unwrap();
    let path = maps.join("pvp_example.json");
    let source = serde_json::json!({"maps":"source-maps", "map":"example", "offset":[1,2,3], "mission_scripts":["start"]});
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({
        "schema":1, "name":"pvp_example", "title":"Example Activity", "description":"Original description",
        "donor":"pvp_greenhouse_2", "source":source
    })).unwrap()).unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let scan = library.scan().unwrap();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        recipe_entries: scan.entries,
        map_entries: scan.maps,
        library_open: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    click(&mut app, &ctx, "All Items");
    click(&mut app, &ctx, "Activities");
    click(&mut app, &ctx, "Example Activity");
    enter(&mut app, &ctx, "Activity Title", "Changed Activity");
    enter(&mut app, &ctx, "Activity Internal Name", "pvp_changed");
    enter(&mut app, &ctx, "Activity Destination", "Reach Arenas");
    click(&mut app, &ctx, "Automatic");
    click(&mut app, &ctx, "Blocked");
    // The switches sit below the fields' visible height.
    scroll(&mut app, &ctx, "Vehicle Summoning", -200.0);
    click(&mut app, &ctx, "Ambient Audio");
    click(&mut app, &ctx, "Death Boundaries");
    scroll(&mut app, &ctx, "Vehicle Summoning", 200.0);
    let pixels = image::RgbaImage::from_pixel(32, 18, image::Rgba([7, 37, 91, 255]));
    app.activity_editor.as_mut().unwrap().recipe.image =
        Some(crate::presentation::Artwork::from_source(pixels).unwrap());
    let output = frame(&mut app, &ctx, vec![]);
    sundial::test_support::capture::write(&ctx, &output, "activity-editor");
    match app.batch_request() {
        Err(error) => assert!(error.contains("activity editor"), "{error}"),
        Ok(_) => panic!("A build must not silently use an older saved activity"),
    }
    click(&mut app, &ctx, "Save Activity");
    let saved_bytes = std::fs::read(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&saved_bytes).unwrap();
    assert_eq!(saved["title"], "Changed Activity");
    assert_eq!(saved["name"], "pvp_changed");
    assert_eq!(saved["destination"], "Reach Arenas");
    assert_eq!(saved["source"], source);
    assert_eq!(saved["options"]["vehicle_summoning"], false);
    assert_eq!(saved["options"]["ambient_audio"], false);
    assert_eq!(saved["options"]["death_boundaries"], false);
    let reloaded = crate::map::load_recipe(&path).unwrap();
    assert_eq!(reloaded.destination.as_deref(), Some("Reach Arenas"));
    assert_eq!(
        reloaded.image.unwrap().pixels().get_pixel(0, 0).0,
        [7, 37, 91, 255]
    );
    click(&mut app, &ctx, "Close");
    app.open_build_selection();
    click(&mut app, &ctx, "Changed Activity");
    click(&mut app, &ctx, "Apply Selection");
    assert!(
        library
            .enabled_map_paths(&library.scan().unwrap().maps)
            .unwrap()
            .contains(&path)
    );
    app.open_activity(&path);
    enter(&mut app, &ctx, "Activity Internal Name", "Invalid name");
    click(&mut app, &ctx, "Save Activity");
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
    enter(&mut app, &ctx, "Activity Internal Name", "pvp_unsaved");
    let mut external = std::fs::read(&path).unwrap();
    external.push(b'\n');
    std::fs::write(&path, &external).unwrap();
    click(&mut app, &ctx, "Save Activity");
    assert_eq!(std::fs::read(&path).unwrap(), external);
    app.request_recipe_action(PendingRecipeAction::Close);
    click(&mut app, &ctx, "Keep Editing");
    assert!(!app.take_close_approved());
    assert!(app.activity_dirty());
    let output = frame(&mut app, &ctx, vec![]);
    sundial::test_support::capture::write(&ctx, &output, "activity-conflict");
    if let Some(root) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            PathBuf::from(root).join("activity-saved.json"),
            serde_json::to_vec_pretty(&saved).unwrap(),
        )
        .unwrap();
    }
}
