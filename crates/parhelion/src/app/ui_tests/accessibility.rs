use super::*;

#[test]
fn core_fields_and_build_selection_expose_accessible_names() {
    let mut app = PackageAuthoringApp::default();
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_definition_panel(ui, None);
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    for label in ["Weapon Name", "Flavor Text", "Rarity", "Power Cap"] {
        let ids: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label) || node.value() == Some(label))
            .map(|(id, _)| *id)
            .collect();
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.labelled_by().iter().any(|id| ids.contains(id))),
            "{label} must label its input"
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    app.recipe_entries = library.scan().unwrap().entries;
    app.open_build_selection();
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_library_windows(ui.ctx());
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Search recipes"))
    );
    let checkboxes: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::CheckBox)
        .collect();
    assert!(!checkboxes.is_empty());
    assert!(checkboxes.iter().all(|(_, node)| {
        node.label().is_some_and(|label| {
            label.starts_with("Include default Parhelion weapons") || label.starts_with("Select ")
        })
    }));
}

#[test]
fn build_selection_is_keyboard_operable_without_duplicate_row_tab_stops() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = name.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 640.0),
        )),
        ..Default::default()
    };
    let frame = |app: &mut PackageAuthoringApp, key: Option<egui::Key>| {
        let mut input = input.clone();
        if let Some(key) = key {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        ctx.run(input, |ctx| app.draw_library_windows(ctx))
    };
    let focused_label = |output: &egui::FullOutput| {
        let tree = output.platform_output.accesskit_update.as_ref().unwrap();
        tree.nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .and_then(|(_, node)| node.label())
            .unwrap_or_default()
            .to_owned()
    };
    for _ in 0..3 {
        frame(&mut app, None);
    }
    let mut reached = false;
    for _ in 0..8 {
        let output = frame(&mut app, Some(egui::Key::Tab));
        if focused_label(&output).starts_with(&format!("Select {name} ·")) {
            reached = true;
            break;
        }
    }
    assert!(reached, "the inclusion checkbox must be keyboard reachable");
    frame(&mut app, Some(egui::Key::Space));
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([path])
    );
    let output = frame(&mut app, Some(egui::Key::Tab));
    assert_eq!(
        focused_label(&output),
        "Apply Selection",
        "one tab stop per weapon"
    );
    frame(&mut app, Some(egui::Key::Escape));
    assert!(app.build_selection_draft.is_none());
    assert!(app.enabled_recipe_paths.is_empty());
}
