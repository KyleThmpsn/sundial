//! UI selection, history, persistence and captures over disposable account files.
use crate::app::tests::driver::*;
use crate::app::*;

#[test]
fn appearance_sidebar_follows_the_launch_runtime() {
    use crate::package_runtime::installation::RuntimeLocation;
    let directory = TestDirectory::new("appearance runtime navigation");
    std::fs::create_dir_all(directory.0.join("bin/x64")).unwrap();
    std::fs::write(directory.0.join("steam_api64.dll"), b"root runtime fixture").unwrap();
    std::fs::write(
        directory.0.join("bin/x64/steam_api64.dll"),
        b"bin runtime fixture",
    )
    .unwrap();
    let mut app = app(directory.0.clone());
    // Runtime resource recognition is covered by the installation suite. This workflow
    // supplies the detected identities and exercises navigation against launch precedence.
    app.runtime_choice.inspection.copies[1].version = Some("0.6".into());
    let original = app.document.clone();
    let context = egui::Context::default();
    context.enable_accesskit();
    let mut observations = Vec::new();
    for (name, version, dawn, visible) in [
        ("unknown", None, false, false),
        ("older", Some("0.5.99"), false, false),
        ("sunrise-0-6", Some("0.6"), false, true),
        ("sunrise-padded", Some("0.6.0.0"), false, true),
        ("sunrise-later", Some("0.7.1"), false, true),
        ("sunrise-major", Some("1.0"), false, true),
        ("dawn", Some("6.0"), true, false),
        ("unrecognized", Some("unknown"), false, false),
    ] {
        observations.push(check_runtime(
            &mut app, &context, name, version, dawn, visible,
        ));
        assert_eq!(app.document, original);
        assert!(app.undo_history.is_empty());
    }
    app.runtime_choice
        .inspection
        .copies
        .retain(|copy| copy.location == RuntimeLocation::BinX64);
    let frame = draw(&mut app, &context, Vec::new(), egui::vec2(1100.0, 760.0));
    assert!(button(&frame, "Appearance").1);
    app.runtime_choice.inspection.copies.clear();
    app.view_mode = ViewMode::Appearance;
    let frame = draw(&mut app, &context, Vec::new(), egui::vec2(1100.0, 760.0));
    assert!(!contains_text(&frame, "Appearance"));
    assert!(matches!(app.view_mode, ViewMode::Characters));
    assert_eq!(app.document, original);
    assert!(!app.settings_path.exists());
    if let Some(path) = crate::test_support::artifacts("appearance-runtime-navigation.json") {
        std::fs::write(path, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
}

fn check_runtime(
    app: &mut SundialApp,
    context: &egui::Context,
    name: &str,
    version: Option<&str>,
    dawn: bool,
    visible: bool,
) -> serde_json::Value {
    let root = app
        .runtime_choice
        .inspection
        .copies
        .iter_mut()
        .find(|copy| copy.location == crate::package_runtime::installation::RuntimeLocation::Root)
        .unwrap();
    root.version = version.map(str::to_owned);
    root.dawn = dawn;
    root.dawn_runtime = dawn.then(|| crate::game_settings::dawn::Runtime::inspect(&root.dll_path));
    app.view_mode = if visible {
        ViewMode::Characters
    } else {
        ViewMode::Appearance
    };
    let frame = draw(app, context, Vec::new(), egui::vec2(1100.0, 760.0));
    let shown = frame
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|(_, node)| node.label() == Some("Appearance"));
    assert_eq!(
        shown, visible,
        "{name}, with an inactive Sunrise 0.6 copy installed"
    );
    assert!(matches!(app.view_mode, ViewMode::Characters));
    if visible {
        let (position, enabled) = button(&frame, "Appearance");
        assert!(enabled);
        draw(
            app,
            context,
            crate::test_support::primary_press(position, true),
            egui::vec2(1100.0, 760.0),
        );
        draw(
            app,
            context,
            crate::test_support::primary_press(position, false),
            egui::vec2(1100.0, 760.0),
        );
        assert!(matches!(app.view_mode, ViewMode::Appearance));
    }
    crate::test_support::capture::write(context, &frame, &format!("appearance-runtime-{name}"));
    serde_json::json!({"runtime":name,"version":version,"dawn":dawn,"tab_visible":shown})
}

#[test]
fn character_appearance_ui_edit_undo_save_and_reload() {
    let Some(packages) = std::env::var_os("SUNDIAL_APPEARANCE_PACKAGES") else {
        return;
    };
    let packages = PathBuf::from(packages);
    let native = crate::character::Catalog::load(&packages).unwrap();
    let initial = native.default_appearance(0, 0).unwrap();
    let directory = TestDirectory::new("character appearance ui");
    let database = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::versions::fixture(&database, "v3-fresh");
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute(
        "UPDATE characters SET race=0,gender=0,appearance_header=?,appearance_header_valid=1",
        [initial.header()],
    )
    .unwrap();
    std::fs::write(
        directory.0.join("steam_api64.dll"),
        b"appearance runtime fixture",
    )
    .unwrap();
    let mut app = app(directory.0.clone());
    app.runtime_choice.inspection.copies[0].version = Some("0.6".into());
    app.install_path = packages.parent().unwrap().to_owned();
    app.manifest = Manifest::for_test_with_packages(app.install_path.clone());
    app.document =
        WorkspaceDocument::load(serde_json::json!({"version":18}), &app.settings_path, false);
    app.persisted_document = app.document.clone();
    assert!(
        app.document.native_account().is_some(),
        "Appearance fixture must load as a native account"
    );
    let context = egui::Context::default();
    context.enable_accesskit();
    app.select_view(ViewMode::Appearance);
    wait_for_appearance(&mut app, &context);
    let face = native
        .identity(0, 0)
        .unwrap()
        .groups
        .iter()
        .find(|g| g.field == crate::character::Field::Face)
        .unwrap();
    wait_for_icons(&mut app, &context, face);
    let mut frame = draw(&mut app, &context, Vec::new(), egui::vec2(1360.0, 900.0));
    let (position, enabled) = button(&frame, "Face Option 2");
    assert!(enabled);
    draw(
        &mut app,
        &context,
        crate::test_support::primary_press(position, true),
        egui::vec2(1360.0, 900.0),
    );
    draw(
        &mut app,
        &context,
        crate::test_support::primary_press(position, false),
        egui::vec2(1360.0, 900.0),
    );
    let initial = app
        .document
        .native_account()
        .unwrap()
        .appearance(0)
        .unwrap()
        .unwrap();
    // Shadowkeep's second Human Male face is art arrangement 36, independent of its icon row.
    assert_eq!(
        u16::from_le_bytes(initial.header()[..2].try_into().unwrap()),
        36
    );
    app.undo();
    assert_eq!(
        app.document
            .native_account()
            .unwrap()
            .appearance(0)
            .unwrap()
            .unwrap()
            .value(crate::character::Field::Face)[0],
        35
    );
    app.redo();
    app.document.save_account(|| Ok(())).unwrap();
    app.mark_document_saved();
    let saved_header: Vec<u8> = db
        .query_row(
            "SELECT appearance_header FROM characters WHERE slot=0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        u16::from_le_bytes(saved_header[..2].try_into().unwrap()),
        36
    );
    frame = wait_for_preview(&mut app, &context);
    crate::test_support::capture::write(&context, &frame, "character-appearance-human-icons");
    if let Some(output) = crate::test_support::artifacts("appearance-icons-human.sqlite3") {
        db.backup(rusqlite::MAIN_DB, &output, rusqlite::backup::NO_PROGRESS)
            .unwrap();
    }
    write_option_icons(&native);
    let before = app.document.clone();
    let position = crate::test_support::text_position(&frame, "Exo").expect("Visible Exo control");
    draw(
        &mut app,
        &context,
        crate::test_support::primary_press(position, true),
        egui::vec2(1360.0, 900.0),
    );
    draw(
        &mut app,
        &context,
        crate::test_support::primary_press(position, false),
        egui::vec2(1360.0, 900.0),
    );
    assert_eq!(
        account::character_metadata(&app.document, 0).unwrap().race,
        2
    );
    assert!(app.has_unsaved_changes());
    app.undo();
    assert_eq!(app.document, before);
    app.redo();
    let edited = app
        .document
        .native_account()
        .unwrap()
        .appearance(0)
        .unwrap()
        .unwrap();
    app.document.save_account(|| Ok(())).unwrap();
    app.mark_document_saved();
    let reloaded =
        WorkspaceDocument::load(serde_json::json!({"version":18}), &app.settings_path, false);
    assert_eq!(
        reloaded.native_account().unwrap().appearance(0).unwrap(),
        Some(edited.clone())
    );
    assert_eq!(
        db.query_row(
            "SELECT race,gender,appearance_header FROM characters WHERE slot=0",
            [],
            |r| Ok((
                r.get::<_, u8>(0)?,
                r.get::<_, u8>(1)?,
                r.get::<_, Vec<u8>>(2)?
            ))
        )
        .unwrap(),
        (2, 0, edited.header().to_vec())
    );
    app.undo();
    app.document.save_account(|| Ok(())).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT race,appearance_header FROM characters WHERE slot=0",
            [],
            |r| Ok((r.get::<_, u8>(0)?, r.get::<_, Vec<u8>>(1)?))
        )
        .unwrap(),
        (0, initial.header().to_vec())
    );
    app.redo();
    app.document.save_account(|| Ok(())).unwrap();
    app.mark_document_saved();
    // Capture actual preview output after its worker has had time to finish, without a render
    // comparison or an assertion about a particular shading implementation.
    let exo_face = native
        .identity(2, 0)
        .unwrap()
        .groups
        .iter()
        .find(|g| g.field == crate::character::Field::Face)
        .unwrap();
    wait_for_icons(&mut app, &context, exo_face);
    frame = wait_for_preview(&mut app, &context);
    check_preview_controls(&mut app, &context, frame);
    if let Some(output) = crate::test_support::artifacts("appearance-ui.sqlite3") {
        db.backup(rusqlite::MAIN_DB, &output, rusqlite::backup::NO_PROGRESS)
            .unwrap();
    }
}

fn write_option_icons(native: &crate::character::Catalog) {
    let mut icon_observations = Vec::new();
    for group in native
        .identity(0, 0)
        .unwrap()
        .groups
        .iter()
        .filter(|g| g.visible && !g.field.is_color())
    {
        for (option, choice) in group.options.iter().enumerate() {
            icon_observations.push(serde_json::json!({"group":group.label,"option":option,"value":choice.value[0],"container":choice.icon_container}));
        }
    }
    if let Some(output) = crate::test_support::artifacts("appearance-option-icons.json") {
        std::fs::write(
            output,
            serde_json::to_vec_pretty(&icon_observations).unwrap(),
        )
        .unwrap();
    }
}

fn check_preview_controls(
    app: &mut SundialApp,
    context: &egui::Context,
    mut frame: egui::FullOutput,
) {
    context.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let (preview, _) = button(&frame, "Model Preview");
    draw(
        app,
        context,
        vec![egui::Event::PointerMoved(preview)],
        egui::vec2(1360.0, 900.0),
    );
    frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    assert!(!contains_text(&frame, "Drag to rotate"));
    let (info, _) = button(&frame, "Preview Info");
    draw(
        app,
        context,
        vec![egui::Event::PointerMoved(info)],
        egui::vec2(1360.0, 900.0),
    );
    let tooltip_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
        if contains_text(&frame, "Drag to rotate") {
            break;
        }
        assert!(
            Instant::now() < tooltip_deadline,
            "The preview info hover did not open"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    crate::test_support::capture::write(context, &frame, "character-appearance-info");
    draw(
        app,
        context,
        vec![egui::Event::PointerMoved(egui::pos2(10.0, 10.0))],
        egui::vec2(1360.0, 900.0),
    );
    frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    let plus = crate::test_support::text_position(&frame, "+").expect("Visible Zoom In control");
    draw(
        app,
        context,
        crate::test_support::primary_press(plus, true),
        egui::vec2(1360.0, 900.0),
    );
    draw(
        app,
        context,
        crate::test_support::primary_press(plus, false),
        egui::vec2(1360.0, 900.0),
    );
    // The new percentage becomes visible on the next frame after the button changes the camera.
    frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    assert!(contains_text(&frame, "125%"));
    let fit = crate::test_support::text_position(&frame, "Fit").expect("Visible Fit control");
    draw(
        app,
        context,
        crate::test_support::primary_press(fit, true),
        egui::vec2(1360.0, 900.0),
    );
    draw(
        app,
        context,
        crate::test_support::primary_press(fit, false),
        egui::vec2(1360.0, 900.0),
    );
    frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    assert!(contains_text(&frame, "100%"));
    crate::test_support::capture::write(context, &frame, "character-appearance-wide");
    let frame = draw(app, context, Vec::new(), egui::vec2(820.0, 900.0));
    crate::test_support::capture::write(context, &frame, "character-appearance-compact");
}

fn wait_for_appearance(app: &mut SundialApp, context: &egui::Context) {
    let mut frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    let deadline = Instant::now() + Duration::from_secs(120);
    while !contains_text(&frame, "Face") {
        if Instant::now() >= deadline {
            crate::test_support::capture::write(context, &frame, "character-appearance-failed");
            panic!("Appearance UI did not load: {:?}", frame.shapes);
        }
        std::thread::sleep(Duration::from_millis(30));
        frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
    }
}

fn wait_for_icons(app: &mut SundialApp, context: &egui::Context, group: &crate::character::Group) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let loaded = group.options.iter().all(|choice| {
            app.manifest
                .customization_icon_texture(
                    context,
                    choice
                        .icon_container
                        .expect("Native face thumbnail reference"),
                )
                .is_some()
        });
        if loaded {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Native face thumbnails did not load"
        );
        draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn wait_for_preview(app: &mut SundialApp, context: &egui::Context) -> egui::FullOutput {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let frame = draw(app, context, Vec::new(), egui::vec2(1360.0, 900.0));
        if !contains_text(&frame, "Loading Model") && !contains_text(&frame, "Preparing Preview") {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "Character preview did not finish"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn draw(
    app: &mut SundialApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
    size: egui::Vec2,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_app_chrome(ui, None);
            app.draw_active_view(ui);
        },
    );
    crate::test_support::capture::record(&output);
    output
}
