//! A shader already created in the library remains selectable and survives the next build.
use super::*;

fn sockets(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, 1600.0),
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

fn tap(ctx: &egui::Context, app: &mut PackageAuthoringApp, donor: &WeaponDonor, point: egui::Pos2) {
    for events in crate::test_support::driver::tap(point) {
        sockets(ctx, app, donor, events);
    }
}

#[test]
#[ignore = "Requires SUNDIAL_INSTALL and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn a_created_shader_is_selected_saved_and_built_with_its_weapon() {
    let packages = crate::test_support::install().join("packages");
    let output = crate::test_support::artifact_dir("shader-socket");
    let output = sundial::package_authoring::resolve_path_for_comparison(&output).unwrap();
    let install =
        sundial::package_authoring::resolve_path_for_comparison(packages.parent().unwrap())
            .unwrap();
    assert!(!sundial::package_authoring::path_is_within(
        &output, &install
    ));
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    let clean = output.join("clean");
    package_view(&packages, &[], &clean);
    let catalog = crate::test_support::catalog(&clean).unwrap();
    let base = catalog
        .shader_donors()
        .into_iter()
        .find(|item| item.collection_backed)
        .unwrap();
    let library = RecipeLibrary::open(output.join("recipes")).unwrap();
    let mut shader = WeaponRecipe::new_unbound_kind(ItemKind::Shader).unwrap();
    shader.set_donor(base.hash, &base.name);
    shader.rename_authored_item("Socket Color Shader").unwrap();
    shader.overrides.icon_from_dyes = false;
    shader.overrides.dye_edits.push(DyeEdit {
        color: Some([17, 33, 201]),
        ..DyeEdit::new(None, DyeChannel::Armor, DyeSurface::Primary)
    });
    let shader_hash = shader.identity.item_hash.parse_u32().unwrap();
    let shader_path = library.save_new(&shader).unwrap();
    let first = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: clean.join("packages"),
        staging_root: output.join("first-build"),
        ignore_installed_authored_overlays: true,
        recipes: vec![shader.clone()],
    })
    .unwrap();
    let first = crate::build_and_stage_snapshot_with_progress(&first, |_| {}).unwrap();
    let installed = output.join("installed-view");
    staged_view(&clean.join("packages"), &first, &installed);
    let catalog = crate::test_support::catalog(&installed).unwrap();
    let donor = catalog
        .weapon_donor(crate::recipe::ARC_LOGIC_DONOR_HASH)
        .unwrap();
    let choices = catalog
        .supported_plug_sets(donor.summary.hash, &[])
        .unwrap();
    let socket = choices
        .iter()
        .position(|choices| choices.plug_hashes.contains(&shader_hash))
        .unwrap();
    let stock = donor.sockets[socket].native_default.unwrap();
    let label = catalog.plug_label(stock, false);
    let weapon = WeaponRecipe::new_named_weapon_for_donor(
        "Shader Socket Weapon",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let path = library.save_new(&weapon).unwrap();
    let mut app = PackageAuthoringApp {
        packages: installed.join("packages"),
        staging: output.join("second-build").display().to_string(),
        recipe_library: Some(library.clone()),
        enabled_recipe_paths: BTreeSet::from([path.clone()]),
        ignore_installed: true,
        perk_workbench: Workbench::offline(),
        ..Default::default()
    };
    app.install_catalog(catalog);
    app.recipe_entries.clear();
    assert!(app.open_recipe_path(&path));
    let ctx = context();
    sockets(&ctx, &mut app, &donor, vec![]);
    let frame = sockets(&ctx, &mut app, &donor, vec![]);
    let point = find(&frame, "the shader socket", |text, _| text == label);
    tap(&ctx, &mut app, &donor, point);
    sockets(
        &ctx,
        &mut app,
        &donor,
        vec![egui::Event::Text(format!("0x{shader_hash:08X}"))],
    );
    let frame = sockets(&ctx, &mut app, &donor, vec![]);
    let point = find(&frame, "the created shader", |text, _| {
        text.contains(&shader.name)
    });
    tap(&ctx, &mut app, &donor, point);
    let frame = sockets(&ctx, &mut app, &donor, vec![]);
    if let Some((_, rect)) = texts(&frame)
        .into_iter()
        .find(|(text, _)| text == "Apply Choice")
    {
        tap(&ctx, &mut app, &donor, rect.center());
    }
    assert_eq!(
        recipe_socket_choices(&app.recipe, socket, &[]).unwrap(),
        [shader_hash]
    );
    assert!(app.recipe.overrides.socket_plug_variants.is_empty());
    capture::write(
        &ctx,
        &sockets(&ctx, &mut app, &donor, vec![]),
        "shader-socket-applied",
    );
    app.save_edits_for_build().unwrap();
    assert!(app.open_recipe_path(&path));
    assert_eq!(
        recipe_socket_choices(&app.recipe, socket, &[]).unwrap(),
        [shader_hash]
    );

    let mut second_weapon = app.recipe.clone();
    second_weapon
        .rename_authored_item("Second Shader Socket Weapon")
        .unwrap();
    let second_path = library.save_new(&second_weapon).unwrap();
    app.enabled_recipe_paths.insert(second_path);
    let selected = app.enabled_recipe_paths.clone();
    let snapshot = app.snapshot().unwrap();
    verify_dependency_selection(&snapshot, &selected, &shader_path);

    let duplicate = library.root().join("duplicate-shader.parhelion.json");
    fs::copy(&shader_path, &duplicate).unwrap();
    let duplicate_error = app.batch_request().unwrap_err();
    assert!(duplicate_error.contains("More than one shader recipe"));
    fs::remove_file(&duplicate).unwrap();

    let mut changed = shader.clone();
    changed.flavor = "Changed after the first snapshot".into();
    library.save_existing(&shader_path, &changed).unwrap();
    assert_ne!(snapshot.fingerprint, app.snapshot().unwrap().fingerprint);
    library.save_existing(&shader_path, &shader).unwrap();
    assert!(app.open_recipe_path(&shader_path));
    library.save_existing(&shader_path, &changed).unwrap();
    assert!(app.batch_request().unwrap_err().contains("changed on disk"));
    library.save_existing(&shader_path, &shader).unwrap();
    assert!(app.open_recipe_path(&path));
    let snapshot = app.snapshot().unwrap();
    let built = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let readback = output.join("readback-view");
    staged_view(&clean.join("packages"), &built, &readback);
    let staged = crate::test_support::catalog(&readback).unwrap();
    let bindings = [app.recipe.clone(), second_weapon]
        .into_iter()
        .map(|recipe| {
            let hash = recipe.identity.item_hash.parse_u32().unwrap();
            let item = staged.weapon_donor(hash).unwrap();
            assert_eq!(item.sockets[socket].native_default, Some(shader_hash));
            serde_json::json!({"item":hash,"socket":socket,"shader":shader_hash})
        })
        .collect::<Vec<_>>();
    let materials = check_custom_dyes(
        &readback.join("packages"),
        &shader.name,
        &shader_view::stock_rows(&staged, shader_hash),
        &shader_view::stock_rows(&staged, base.hash),
        DyeChannel::Armor,
        (&shader.overrides.dye_edits, &[]),
    );
    fs::write(
        output.join("verified-shader-socket.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "bindings":bindings,"materials":materials,"shader_recipe":shader_path,
            "shader_automatically_included":true,"dependency_deduplicated":true,
            "duplicate_identity_rejected":duplicate_error,
            "dependency_changes_invalidate_snapshot":true,"concurrent_edits_preserved":true,
            "gameplay_verified":false
        }))
        .unwrap(),
    )
    .unwrap();
}

fn verify_dependency_selection(
    snapshot: &crate::BatchBuildSnapshot,
    selected: &BTreeSet<PathBuf>,
    shader_path: &Path,
) {
    assert_eq!(snapshot.request.recipes.len(), 3);
    assert_eq!(
        snapshot
            .request
            .recipes
            .iter()
            .filter(|r| r.kind == ItemKind::Shader)
            .count(),
        1
    );
    assert!(!selected.contains(shader_path));
}
