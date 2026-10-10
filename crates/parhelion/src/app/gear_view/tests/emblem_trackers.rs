//! Tracker choices survive recipe saving and reach the staged native emblem.
use super::*;

fn panel(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.draw_emblem_trackers(ui);
            });
        },
    );
    capture::record(&output);
    output
}

fn choose(ctx: &egui::Context, app: &mut PackageAuthoringApp, label: &str) {
    let output = panel(ctx, app, vec![]);
    let point = find(&output, label, |text, _| text == label);
    for events in crate::test_support::driver::tap(point) {
        panel(ctx, app, events);
    }
}

/// Independent consumer of the item's reachable metric-category array.
fn categories(bytes: &[u8]) -> Vec<u16> {
    let relative = i64::from_le_bytes(bytes[0x38..0x40].try_into().unwrap());
    if relative == 0 {
        return vec![];
    }
    let block = (0x38_i64 + relative) as usize;
    assert_eq!(
        u32::from_le_bytes(bytes[block - 4..block].try_into().unwrap()),
        0x80802C9A
    );
    let count = u64::from_le_bytes(bytes[block..block + 8].try_into().unwrap()) as usize;
    if count == 0 {
        return vec![];
    }
    let header =
        (block as i64 + 8 + i64::from_le_bytes(bytes[block + 8..block + 16].try_into().unwrap()))
            as usize;
    assert_eq!(
        u64::from_le_bytes(bytes[header..header + 8].try_into().unwrap()) as usize,
        count
    );
    assert_eq!(
        u32::from_le_bytes(bytes[header + 8..header + 12].try_into().unwrap()),
        0x80802CA9
    );
    (0..count)
        .map(|i| {
            u16::from_le_bytes(
                bytes[header + 16 + i * 2..header + 18 + i * 2]
                    .try_into()
                    .unwrap(),
            )
        })
        .collect()
}

#[test]
#[ignore = "Requires SUNDIAL_INSTALL and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn tracker_choices_go_from_the_emblem_page_to_native_packages() {
    let packages = crate::test_support::install().join("packages");
    let output = crate::test_support::artifact_dir("gear");
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let definition = |hash| {
        manager
            .read_tag(TagHash(stock.item_definition_tag(hash).unwrap()))
            .unwrap()
    };
    let donors = stock.gear_donors(ItemKind::Emblem.bucket_hashes());
    let without = donors
        .iter()
        .find(|d| d.collection_backed && categories(&definition(d.hash)).is_empty())
        .unwrap();
    let with = donors
        .iter()
        .find(|d| d.collection_backed && !categories(&definition(d.hash)).is_empty())
        .unwrap();
    let original = definition(with.hash);
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        perk_workbench: Workbench::offline(),
        ..Default::default()
    };
    app.install_catalog(stock);
    app.recipe = WeaponRecipe::new_unbound_kind(ItemKind::Emblem).unwrap();
    app.recipe.set_donor(with.hash, &with.name);
    app.recipe
        .rename_authored_item("Inherited Trackers")
        .unwrap();
    let mut recipes = vec![app.recipe.clone()];
    let ctx = context();
    choose(&ctx, &mut app, "All Trackers");
    app.recipe
        .rename_authored_item("All Trackers Emblem")
        .unwrap();
    recipes.push(app.recipe.clone());
    choose(&ctx, &mut app, "Selected Categories");
    choose(&ctx, &mut app, "Select None");
    choose(&ctx, &mut app, "Crucible");
    choose(&ctx, &mut app, "Raids");
    app.recipe
        .rename_authored_item("Selected Trackers Emblem")
        .unwrap();
    recipes.push(app.recipe.clone());
    capture::write(
        &ctx,
        &panel(&ctx, &mut app, vec![]),
        "emblem-selected-trackers",
    );
    choose(&ctx, &mut app, "Select None");
    app.recipe
        .rename_authored_item("No Trackers Emblem")
        .unwrap();
    recipes.push(app.recipe.clone());
    app.recipe.set_donor(without.hash, &without.name);
    choose(&ctx, &mut app, "All Trackers");
    app.recipe
        .rename_authored_item("Trackers on a Base Without Trackers")
        .unwrap();
    recipes.push(app.recipe.clone());
    #[cfg(feature = "d2-model-importer")]
    {
        let modern = PathBuf::from(
            std::env::var_os("PARHELION_IMPORT_MODERN_PACKAGES")
                .expect("Configure modern packages for the import portion"),
        );
        let items = parhelion_import::d2_mot::service::scan(
            &modern,
            &packages,
            &output.join("source-catalog"),
        )
        .unwrap();
        let source = items
            .iter()
            .find(|item| item.weapon_type == "Emblem" && item.rarity == Some(4))
            .expect("A modern legendary emblem");
        let donors = serde_json::json!({"emblems":donors.iter().map(|d| serde_json::json!({"hash":d.hash,"name":d.name})).collect::<Vec<_>>()});
        let path = parhelion_import::d2_mot::service::prepare(
            source,
            &modern,
            &packages,
            &donors,
            &output.join("import"),
        )
        .unwrap();
        let imported = WeaponRecipe::load_json(path).unwrap();
        assert_eq!(
            imported.overrides.stat_trackers,
            Some(crate::emblem::StatTrackers::All)
        );
        recipes.push(imported);
    }
    let library = RecipeLibrary::open(output.join("recipes")).unwrap();
    for recipe in &mut recipes {
        let path = library.save_new(recipe).unwrap();
        *recipe = WeaponRecipe::load_json(&path).unwrap();
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = output.join("view");
    staged_view(&packages, &build, &view);
    let staged = crate::test_support::catalog(&view).unwrap();
    let manager = open_shadowkeep_package_manager(&view.join("packages")).unwrap();
    let options = sundial::investment::load_emblem_tracker_categories(&manager).unwrap();
    let mut receipt = vec![];
    for (i, recipe) in recipes.iter().enumerate() {
        let hash = recipe.identity.item_hash.parse_u32().unwrap();
        let data = manager
            .read_tag(TagHash(staged.item_definition_tag(hash).unwrap()))
            .unwrap();
        let actual = categories(&data);
        let expected = match i {
            0 => categories(&original),
            1 | 4 | 5 => options.iter().map(|o| o.index).collect(),
            2 => options
                .iter()
                .filter(|o| matches!(o.name.as_str(), "Crucible" | "Raids"))
                .map(|o| o.index)
                .collect(),
            3 => vec![],
            _ => unreachable!(),
        };
        assert_eq!(actual, expected, "{}", recipe.name);
        receipt.push(
            serde_json::json!({"name":recipe.name,"item_hash":hash,"category_indices":actual}),
        );
    }
    assert_eq!(
        manager
            .read_tag(TagHash(staged.item_definition_tag(with.hash).unwrap()))
            .unwrap(),
        original
    );
    // Every metric with a presentation parent is covered, including categories no donor allows.
    let globals = investment_globals(&manager);
    let root = manager
        .read_tag(crate::tag_payload::read_u32(&globals, 0x10).unwrap())
        .unwrap();
    let metrics = manager
        .read_tag(crate::tag_payload::read_u32(&root, 8 + 55 * 16).unwrap())
        .unwrap();
    let (count, _, rows, _) = crate::tag_payload::array_at(&metrics, 8).unwrap();
    let mut covered = 0;
    for i in 0..count {
        let (n, _, parent_rows, _) =
            crate::tag_payload::array_at(&metrics, rows + i * 72 + 24).unwrap();
        let parents = (0..n)
            .map(|j| crate::tag_payload::read_u16(&metrics, parent_rows + j * 2).unwrap())
            .collect::<Vec<_>>();
        if parents.is_empty() {
            continue;
        }
        assert!(options.iter().any(|o| parents.contains(&o.index)));
        covered += 1;
    }
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({"emblems":receipt,"displayable_metrics":covered,"gameplay_verified":false})).unwrap()).unwrap();
}
