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

fn drag(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    from: egui::Pos2,
    to: egui::Pos2,
) {
    frame(ctx, app, donor, vec![egui::Event::PointerMoved(from)]);
    frame(
        ctx,
        app,
        donor,
        vec![
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    // Cross the drag threshold in steps, the way a pointer actually travels.
    for step in 1..=6 {
        let position = from + (to - from) * (step as f32 / 6.0);
        frame(ctx, app, donor, vec![egui::Event::PointerMoved(position)]);
    }
    frame(
        ctx,
        app,
        donor,
        vec![
            egui::Event::PointerMoved(to),
            egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(ctx, app, donor, vec![]);
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only socket reorder UI check"]
fn dragging_a_choice_grip_reorders_its_socket() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let mut app = PackageAuthoringApp {
        catalog: Some(catalog),
        recipe: WeaponRecipe::new_named_weapon_for_donor(
            "Reorder Test",
            donor.summary.hash,
            &donor.summary.name,
        )
        .unwrap(),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let grips = text_origins(&output, egui_phosphor::regular::DOTS_SIX_VERTICAL);
    assert!(grips.len() >= 2, "no drag grips were rendered");
    // Two grips sharing a row belong to the same socket, so this is a reorder and not a copy.
    let (first, second) = grips
        .iter()
        .zip(grips.iter().skip(1))
        .find(|(left, right)| (left.y - right.y).abs() <= 1.0)
        .map(|(left, right)| (*left, *right))
        .expect("a socket with two choices on one row");
    let before = app.recipe.clone();
    drag(
        &ctx,
        &mut app,
        &donor,
        first + egui::vec2(3.0, 6.0),
        second + egui::vec2(3.0, 6.0),
    );
    assert_ne!(
        app.recipe, before,
        "dragging a grip onto the next choice changed nothing"
    );
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only socket UI check"]
fn extra_choice_context_menu_promotes_its_private_definition() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let temporary = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &temporary.path().join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let source = donor.sockets[0].native_default.unwrap();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.default-choice-ui",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    socket_editor::set_recipe_socket_column(
        &mut recipe,
        donor.sockets.len(),
        0,
        &[source],
        vec![0xDD5C_B37A, source],
        None,
    );
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
            replace_effects: false,
            investment_stats: vec![],
            socket_index: 0,
            choice_index: 1,
            source_plug_hash: source.into(),
            name: Some("Alternate Choice".into()),
            icon: None,
            classification_donor_hash: None,
            description: None,
            additional_sandbox_perks: vec![],
            sandbox_perks: vec![],
        });
    let mut app = PackageAuthoringApp {
        catalog: Some(catalog),
        recipe,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let pos = text_origin(&output, "Alternate Choice") + egui::vec2(8.0, 6.0);
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            &donor,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let output = frame(&ctx, &mut app, &donor, vec![]);
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Make Default") + egui::vec2(8.0, 6.0),
    );
    assert_eq!(
        socket_editor::recipe_socket_choices(&app.recipe, 0, &[]).unwrap(),
        vec![source, 0xDD5C_B37A]
    );
    assert_eq!(app.recipe.overrides.socket_plug_variants[0].choice_index, 0);
    assert_eq!(
        app.recipe.overrides.socket_plug_variants[0].name.as_deref(),
        Some("Alternate Choice")
    );
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
        // Compatible keeps the native pool, and an added socket type has none, so its picker
        // is empty by design. This is the pool the supported plug sets report for one.
        plug_selection_mode: PlugSelectionMode::SocketAndGearType,
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
        text_origin(&output, "Remove Socket") + egui::vec2(8.0, 6.0),
    );
    assert_eq!(app.recipe, original);
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only socket removal UI check"]
fn base_socket_menu_removes_and_restores_choices() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let mut app = PackageAuthoringApp {
        catalog: Some(catalog),
        recipe: WeaponRecipe::new_named_weapon_for_donor(
            "Removed Socket Test",
            donor.summary.hash,
            &donor.summary.name,
        )
        .unwrap(),
        ..Default::default()
    };
    let original = app.recipe.clone();
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    let options = text_origins(&output, "…")[0];
    click(&ctx, &mut app, &donor, options + egui::vec2(5.0, 6.0));
    frame(&ctx, &mut app, &donor, vec![]);
    let output = frame(&ctx, &mut app, &donor, vec![]);
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Remove Socket") + egui::vec2(8.0, 6.0),
    );
    let output = frame(&ctx, &mut app, &donor, vec![]);
    assert!(text(&output).contains("Socket 1 Removed"));
    assert_eq!(
        app.recipe.overrides.socket_columns.len(),
        donor.sockets.len()
    );
    assert!(
        app.recipe.overrides.socket_columns[1..]
            .iter()
            .all(Option::is_none)
    );
    click(
        &ctx,
        &mut app,
        &donor,
        text_origin(&output, "Restore Socket") + egui::vec2(8.0, 6.0),
    );
    assert_eq!(app.recipe, original);
}
