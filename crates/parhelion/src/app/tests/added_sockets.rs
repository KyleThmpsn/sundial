use super::*;
use crate::app::custom_perks::workbench::tests::capture;

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                app.draw_socket_columns_panel(ui, Some(donor));
            });
        },
    );
    capture::record(&output);
    output
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, donor: &WeaponDonor, pos: egui::Pos2) {
    for events in crate::test_support::driver::tap(pos) {
        frame(ctx, app, donor, events);
    }
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL; read-only package-backed socket UI check"]
fn added_socket_menu_appends_reloads_and_removes_a_real_row() {
    let packages = crate::test_support::install().join("packages");
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donor = catalog
        .weapon_donors()
        .iter()
        .filter_map(|summary| catalog.weapon_donor(summary.hash))
        .find(|donor| {
            !donor.sockets.is_empty()
                && donor.sockets.len() < sundial::investment::MAX_WEAPON_SOCKETS
        })
        .expect("An installed weapon with space for another socket");
    let role = catalog
        .socket_type_choices(donor.summary.hash)
        .unwrap()
        .into_iter()
        .find(|choice| choice.socket_type == 92)
        .expect("Installed Trait role");
    let mut app = PackageAuthoringApp {
        catalog: Some(catalog),
        recipe: WeaponRecipe::new_named_weapon_for_donor(
            "Added Socket Test",
            donor.summary.hash,
            &donor.summary.name,
        )
        .unwrap(),
        show_experimental_options: false,
        // Compatible keeps the native pool, and an added socket type has none, so its picker
        // is empty by design. This is the pool the supported plug sets report for one.
        plug_selection_mode: PlugSelectionMode::SocketAndGearType,
        ..Default::default()
    };
    let original = app.recipe.clone();
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert_eq!(app.recipe, original);
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "+ Add Socket") + egui::vec2(8.0, 6.0),
    );
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let role_pos = *text_origins(&output, &role.label)
        .last()
        .expect("Add Socket role option");
    click(&ctx, &mut app, &donor, role_pos + egui::vec2(8.0, 6.0));
    let index = donor.sockets.len();
    assert_eq!(app.recipe.overrides.socket_columns.len(), index + 1);
    let added = app.recipe.overrides.socket_columns[index].as_ref().unwrap();
    assert_eq!(added.socket_type, Some(92));
    assert!(added.choices.is_empty());
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(text(&output).contains("+ Set Plug"));
    // An empty or reusable-only socket must stay reachable when starting a roll. Switching
    // views must also preserve the row and its saved choices after the roll has been authored.
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Randomized") + egui::vec2(8.0, 6.0),
    );
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(text(&output).contains("Set a reusable plug first."));
    capture::write(&ctx, &output, "randomized-empty-socket");
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Reusable") + egui::vec2(8.0, 6.0),
    );
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let (plug, plug_label, randomized, randomized_label) =
        set_socket_plugs(&ctx, &mut app, &donor, index, output);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("rolled-socket.json");
    std::fs::write(&path, app.recipe.to_json_pretty().unwrap()).unwrap();
    app.recipe = WeaponRecipe::load_json(&path).unwrap();
    let rolled = app.recipe.overrides.socket_columns[index].as_ref().unwrap();
    assert_eq!(rolled.reusable_choices, Some(1));
    assert_eq!(
        recipe_socket_choices(&app.recipe, index, &[]).unwrap(),
        vec![plug, randomized]
    );
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(text(&output).contains(&randomized_label));
    capture::write(&ctx, &output, "randomized-authored-socket");
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Reusable") + egui::vec2(8.0, 6.0),
    );
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(text(&output).contains(&plug_label));
    let saved = app.recipe.clone();
    assert_added_socket_fits(&mut app, &donor, &saved);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let options = *text_origins(&output, crate::app::style::MORE)
        .last()
        .expect("Added socket options");
    click(&ctx, &mut app, &donor, options + egui::vec2(5.0, 6.0));
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(!text(&output).contains("Reset Choices & Role"));
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Remove Socket") + egui::vec2(8.0, 6.0),
    );
    assert_eq!(app.recipe, original);
    crate::test_support::artifact(
        "randomized-socket.json",
        &serde_json::json!({
            "donor": donor.summary.hash,
            "socket": index,
            "default": plug,
            "randomized": randomized,
            "reloaded_recipe": saved,
            "removed_recipe": app.recipe,
        }),
    );
}

fn assert_added_socket_fits(
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    saved: &WeaponRecipe,
) {
    for width in [480.0, 900.0, 1320.0] {
        let (output, overflow) = render(width, |ui| app.draw_socket_columns_panel(ui, Some(donor)));
        assert!(text(&output).contains("Added"));
        assert!(
            overflow < 1.0,
            "Added socket overflow at {width}: {overflow}"
        );
        assert_eq!(&app.recipe, saved);
    }
}

fn set_socket_plugs(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    index: usize,
    output: egui::FullOutput,
) -> (u32, String, u32, String) {
    let socket_types = app
        .recipe
        .overrides
        .socket_columns
        .iter()
        .map(|column| column.as_ref().and_then(|column| column.socket_type))
        .collect::<Vec<_>>();
    let sets = app
        .catalog
        .as_ref()
        .unwrap()
        .supported_plug_sets(donor.summary.hash, &socket_types)
        .unwrap();
    let plug = sets[index].plug_hashes[0];
    let plug_label = app.catalog.as_ref().unwrap().plug_label(plug, false);
    app.plug_queries[index].insert(0, format!("0x{plug:08X}"));
    click(
        ctx,
        app,
        donor,
        text_origin(&output, "+ Set Plug") + egui::vec2(8.0, 6.0),
    );
    frame(ctx, app, donor, vec![]);
    let output = frame(ctx, app, donor, vec![]);
    click(
        ctx,
        app,
        donor,
        text_origin(&output, &plug_label) + egui::vec2(8.0, 6.0),
    );
    assert_eq!(
        recipe_socket_choices(&app.recipe, index, &[]).unwrap(),
        vec![plug]
    );
    let output = frame(ctx, app, donor, vec![]);
    click(
        ctx,
        app,
        donor,
        text_origin(&output, "Randomized") + egui::vec2(8.0, 6.0),
    );
    let output = frame(ctx, app, donor, vec![]);
    let randomized = sets[index]
        .plug_hashes
        .iter()
        .copied()
        .find(|hash| *hash != plug)
        .expect("The Trait role offers another plug to roll");
    let randomized_label = app.catalog.as_ref().unwrap().plug_label(randomized, false);
    app.plug_queries[index].insert(1, format!("0x{randomized:08X}"));
    click(
        ctx,
        app,
        donor,
        *text_origins(&output, "+ Add Choice").last().unwrap() + egui::vec2(8.0, 6.0),
    );
    frame(ctx, app, donor, vec![]);
    let output = frame(ctx, app, donor, vec![]);
    click(
        ctx,
        app,
        donor,
        text_origin(&output, &randomized_label) + egui::vec2(8.0, 6.0),
    );
    (plug, plug_label, randomized, randomized_label)
}
