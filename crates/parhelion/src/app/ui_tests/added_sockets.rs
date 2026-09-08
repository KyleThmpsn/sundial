use super::*;

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                workbench_style(ui);
                app.draw_socket_columns_panel(ui, Some(donor));
            });
        },
    )
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, donor: &WeaponDonor, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            donor,
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
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only package-backed socket UI check"]
fn added_socket_menu_appends_reloads_and_removes_a_real_row() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
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
        .weapon_socket_type_choices(donor.summary.hash)
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
        plug_selection_mode: PlugSelectionMode::Supported,
        ..Default::default()
    };
    let original = app.recipe.clone();
    let ctx = egui::Context::default();
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
        .weapon_supported_plug_sets_with_socket_types(donor.summary.hash, &socket_types)
        .unwrap();
    let plug = sets[index].plug_hashes[0];
    let plug_label = app.catalog.as_ref().unwrap().plug_label(plug, true);
    app.plug_queries[index].insert(0, format!("0x{plug:08X}"));
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "+ Set Plug") + egui::vec2(8.0, 6.0),
    );
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, &plug_label) + egui::vec2(8.0, 6.0),
    );
    assert_eq!(
        recipe_socket_choices(&app.recipe, index, &[]).unwrap(),
        vec![plug]
    );
    app.recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&app.recipe).unwrap()).unwrap();
    let saved = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        let (output, overflow) =
            render(width, |ui| app.draw_socket_columns_panel(ui, Some(&donor)));
        assert!(text(&output).contains("Added"));
        assert!(!text(&output).contains("normalize the recipe"));
        assert!(
            overflow < 1.0,
            "Added socket overflow at {width}: {overflow}"
        );
        assert_eq!(app.recipe, saved);
    }
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let options = *text_origins(&output, "…")
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
        text_origin(&output, "Remove Added Socket") + egui::vec2(8.0, 6.0),
    );
    assert_eq!(app.recipe, original);
}
