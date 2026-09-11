use super::*;

#[test]
fn source_refresh_reports_current_errors_without_reusing_editor_errors_or_stale_entries() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Saved Source".into();
    let entry = library.save(&perk, None).unwrap();
    let mut workbench = Workbench {
        library: Some(library),
        error: Some("An unrelated editor error".into()),
        ..Default::default()
    };
    assert!(workbench.scan_library().is_empty());
    assert_eq!(workbench.entries.len(), 1);
    std::fs::write(&entry.path, "invalid JSON").unwrap();
    let warnings = workbench.scan_library();
    assert_eq!(warnings.len(), 1);
    assert!(workbench.entries.is_empty());
    assert_eq!(
        workbench.error.as_deref(),
        Some("An unrelated editor error")
    );
    std::fs::write(&entry.path, &entry.baseline).unwrap();
    assert!(workbench.scan_library().is_empty());
    assert_eq!(workbench.entries.len(), 1);
}

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                workbench_style(ui);
                let donor = app.current_donor().unwrap();
                app.draw_socket_columns_panel(ui, Some(&donor));
            });
            app.draw_perk_workbench(ctx);
        },
    )
}

fn label(output: &egui::FullOutput, name: &str, last: bool) -> egui::Rect {
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    *if last { labels.last() } else { labels.first() }.unwrap_or_else(|| panic!("Missing {name}"))
}

fn settle(ctx: &egui::Context, app: &mut PackageAuthoringApp, width: f32) -> egui::FullOutput {
    frame(ctx, app, width, vec![]);
    frame(ctx, app, width, vec![])
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, width: f32, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            width,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

fn open_from_plug(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    trigger: &str,
    expect_native_reset: bool,
) {
    let output = settle(ctx, app, 900.0);
    click(ctx, app, 900.0, label(&output, trigger, false).center());
    let output = settle(ctx, app, 900.0);
    let action = label(&output, "Use Custom Perk…", true);
    if expect_native_reset {
        let donor = app.current_donor().unwrap();
        let native_default = donor.sockets[0].native_default.unwrap();
        let native_label = app
            .catalog
            .as_ref()
            .unwrap()
            .plug_label(native_default, true);
        let reset = label(
            &output,
            &format!("Reset to Native Default: {native_label}"),
            false,
        );
        assert!(
            action.bottom() < reset.top(),
            "Custom perks belong immediately above the native reset action"
        );
    } else {
        assert!(
            action.top() > label(&output, "None", true).bottom(),
            "Custom perks belong at the picker footer when native reset is unavailable"
        );
    }
    click(ctx, app, 900.0, action.center());
    let output = settle(ctx, app, 900.0);
    label(&output, "Select Custom Perk", false);
    assert!(!ctx.memory(|memory| memory.any_popup_open()));
}

fn open_context_workbench(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    trigger: &str,
    choice: usize,
) {
    let before = app.recipe.clone();
    let output = settle(ctx, app, 900.0);
    let pos = label(&output, trigger, false).center();
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            900.0,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Secondary,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let output = settle(ctx, app, 900.0);
    click(
        ctx,
        app,
        900.0,
        label(&output, "Open in Custom Perk Workbench", false).center(),
    );
    let output = settle(ctx, app, 900.0);
    label(&output, "Custom Perk Workbench", false);
    let donor = app.current_donor().unwrap();
    let document = &app.perk_workbench.documents[app.perk_workbench.selected];
    assert_eq!(
        document.target,
        Some(Target::capture(&before, &donor, 0, choice).unwrap())
    );
    assert_eq!(
        app.recipe, before,
        "Opening the workbench must not change the weapon"
    );
    assert!(app.perk_workbench.picker.is_none());
    app.perk_workbench.open = false;
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES for the native plug picker"]
fn native_custom_picker_uses_uninstalled_perks_for_exact_choices_and_supports_creation() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let temporary = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &temporary.path().join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let mut weapon = WeaponRecipe::new_named_weapon_for_donor(
        "Picker Test",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    weapon.overrides.socket_columns = vec![None; donor.sockets.len()];
    weapon.overrides.socket_columns[0] = Some(crate::WeaponSocketColumnRecipe {
        socket_type: Some(92),
        choices: vec![crate::perk::DEFAULT_PLUG_LAYOUT.into(); 2],
        ..Default::default()
    });
    let mut current = PerkRecipe::new();
    current.name = "Existing Choice".into();
    weapon
        .overrides
        .socket_plug_variants
        .push(current.at_socket(0, 1));
    let library = Library::open(temporary.path().join("perks")).unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Uninstalled Test Perk".into();
    perk.description = "Saved locally and never installed.".into();
    perk.effects.push(PerkRecipe::effect(405));
    perk.stats.push(WeaponStatOverride {
        definition_index: 255,
        value: 10,
    });
    let saved = library.save(&perk, None).unwrap();
    let mut app = PackageAuthoringApp {
        recipe: weapon,
        catalog: Some(catalog),
        perk_workbench: Workbench {
            initialized: true,
            library: Some(library),
            ..Default::default()
        },
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let before = app.recipe.clone();
    let native_socket_type = donor.sockets[0].socket_type;
    let socket_type = app.recipe.overrides.socket_columns[0]
        .as_ref()
        .unwrap()
        .socket_type;
    app.recipe.overrides.socket_columns[0]
        .as_mut()
        .unwrap()
        .socket_type = Some(native_socket_type);
    open_from_plug(&ctx, &mut app, "Existing Choice", true);
    app.perk_workbench.picker = None;
    app.perk_workbench.open = false;
    app.recipe.overrides.socket_columns[0]
        .as_mut()
        .unwrap()
        .socket_type = socket_type;

    open_from_plug(&ctx, &mut app, "Existing Choice", false);
    let output = settle(&ctx, &mut app, 900.0);
    label(&output, "Uninstalled Test Perk", false);
    let picker = app.perk_workbench.picker.as_ref().unwrap();
    assert_eq!(
        picker.choices.len(),
        2,
        "Include My Perks and local weapon variants"
    );
    assert_eq!(
        picker.target,
        Target::capture(&before, &donor, 0, 1).unwrap()
    );
    assert_eq!(app.recipe, before);
    click(
        &ctx,
        &mut app,
        900.0,
        label(&output, "Cancel", false).center(),
    );
    assert!(app.perk_workbench.picker.is_none());
    assert_eq!(app.recipe, before);

    open_from_plug(&ctx, &mut app, "Existing Choice", false);
    let output = settle(&ctx, &mut app, 900.0);
    click(
        &ctx,
        &mut app,
        900.0,
        label(&output, "Uninstalled Test Perk", false).center(),
    );
    assert!(app.perk_workbench.picker.is_none());
    assert_eq!(
        app.recipe.overrides.socket_plug_variants,
        vec![perk.at_socket(0, 1)]
    );
    assert_eq!(
        app.recipe.overrides.socket_columns,
        before.overrides.socket_columns
    );
    assert_eq!(
        WeaponRecipe::from_json_str(&app.recipe.to_json_pretty().unwrap()).unwrap(),
        app.recipe
    );
    assert_eq!(std::fs::read(&saved.path).unwrap(), saved.baseline);

    open_context_workbench(&ctx, &mut app, "Uninstalled Test Perk", 1);
    assert_eq!(
        app.perk_workbench.documents[app.perk_workbench.selected]
            .recipe
            .effects,
        perk.effects
    );
    let stock_name = app
        .catalog
        .as_ref()
        .unwrap()
        .plug_label(crate::perk::DEFAULT_PLUG_LAYOUT, false);
    open_context_workbench(&ctx, &mut app, &stock_name, 0);

    verify_creation_and_stale_destination(&ctx, &mut app, &donor);
    verify_empty_picker(&mut app, &donor);
}

fn verify_creation_and_stale_destination(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
) {
    open_from_plug(ctx, app, "+ Add Choice", false);
    assert_eq!(
        app.perk_workbench.picker.as_ref().unwrap().target,
        Target::capture(&app.recipe, donor, 0, 2).unwrap()
    );
    let output = settle(ctx, app, 900.0);
    let unchanged = app.recipe.clone();
    click(
        ctx,
        app,
        900.0,
        label(&output, "Create Custom Perk…", false).center(),
    );
    assert!(app.perk_workbench.open);
    assert_eq!(
        app.perk_workbench.documents[app.perk_workbench.selected].target,
        Some(Target::capture(&unchanged, donor, 0, 2).unwrap())
    );
    assert_eq!(app.recipe, unchanged);
    app.perk_workbench.open = false;

    let target = Target::capture(&app.recipe, donor, 0, 2).unwrap();
    app.perk_workbench
        .open_picker(target, app.catalog.as_ref().unwrap(), None, &app.recipe);
    app.recipe.overrides.socket_columns[0]
        .as_mut()
        .unwrap()
        .choices
        .push(0xDD5C_B37A.into());
    let changed = app.recipe.clone();
    let output = settle(ctx, app, 900.0);
    click(
        ctx,
        app,
        900.0,
        label(&output, "Uninstalled Test Perk", false).center(),
    );
    assert_eq!(
        app.recipe, changed,
        "A stale picker must not overwrite a changed destination"
    );
}

fn verify_empty_picker(app: &mut PackageAuthoringApp, donor: &WeaponDonor) {
    app.perk_workbench.documents.clear();
    app.perk_workbench.library = None;
    app.perk_workbench.entries.clear();
    app.recipe.overrides.socket_plug_variants.clear();
    let target = Target::capture(&app.recipe, donor, 0, 2).unwrap();
    app.perk_workbench
        .open_picker(target, app.catalog.as_ref().unwrap(), None, &app.recipe);
    for width in [480.0, 900.0] {
        let ctx = egui::Context::default();
        let output = settle(&ctx, app, width);
        let create = label(&output, "Create Custom Perk…", false);
        assert!(create.left() >= 0.0 && create.right() <= width && create.bottom() <= 760.0);
        label(
            &output,
            "No custom perks yet. Create one to use it in this choice.",
            false,
        );
        // Capture just the picker window so package icons in the background are not needed.
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 760.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.draw_perk_workbench(ctx);
            },
        );
        crate::app::ui_tests::build_flow::capture(
            &ctx,
            output,
            &format!("custom-perk-picker-{width}"),
            width,
        );
    }
}
