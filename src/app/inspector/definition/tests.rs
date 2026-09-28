use super::*;

/// Renders the inspector for one representative definition of each kind from an installed
/// catalog and writes captures when `PARHELION_UI_CAPTURE_DIR` is set.
#[test]
#[ignore = "Requires SUNDIAL_PROGRESSION_INSTALL and SUNDIAL_INSPECTOR_CATALOG"]
fn installed_inspector_pages_render() {
    let install = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_PROGRESSION_INSTALL").expect("install path"),
    );
    let cache = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_INSPECTOR_CATALOG").expect("catalog cache copy"),
    );
    let catalog =
        Catalog::load_or_scan_with_progress(&install, cache, false, |_| {}).expect("catalog");
    let targets = capture_targets(&catalog);
    assert!(targets.len() >= 6, "too few capture targets: {targets:?}");
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Dark);
    crate::app::ui::configure_contrast(&ctx);
    crate::app::preferences::configure_destiny_symbol_fonts(&ctx, &install).ok();
    warm_icons(&ctx, &catalog, targets[0].1);
    capture_search(&ctx, &catalog, targets[0].1);
    for (name, hash) in targets {
        let mut state = HashInspectionState::default();
        state.open(hash);
        let mut output = with_icons(&ctx, &catalog, &mut state);
        crate::app::tests::capture::write(&ctx, &output, &format!("inspector-{name}"));
        if name != "weapon" {
            continue;
        }
        for tab in [
            "Appearance",
            "Sockets",
            "Runtime",
            "Technical",
            "Related Records",
        ] {
            let Some(position) = text_center(&output, tab) else {
                continue;
            };
            let click = |pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frames(
                &ctx,
                &catalog,
                &mut state,
                vec![egui::Event::PointerMoved(position), click(true)],
                1,
            );
            frames(&ctx, &catalog, &mut state, vec![click(false)], 1);
            output = frames(&ctx, &catalog, &mut state, Vec::new(), 4);
            let slug = tab.to_ascii_lowercase().replace(' ', "-");
            crate::app::tests::capture::write(&ctx, &output, &format!("inspector-weapon-{slug}"));
        }
    }
}

/// The home page empty and with a query, and the toolbar dropdown over a definition page.
fn capture_search(ctx: &egui::Context, catalog: &Catalog, hash: u64) {
    let typed = || vec![egui::Event::Text("ace".into())];
    let mut state = HashInspectionState::default();
    state.open(hash);
    frames(ctx, catalog, &mut state, Vec::new(), 2);
    state.go_home();
    let output = frames(ctx, catalog, &mut state, Vec::new(), 4);
    crate::app::tests::capture::write(ctx, &output, "inspector-home");
    frames(ctx, catalog, &mut state, typed(), 1);
    let output = with_icons(ctx, catalog, &mut state);
    crate::app::tests::capture::write(ctx, &output, "inspector-home-search");

    let mut state = HashInspectionState::default();
    state.open(hash);
    frames(ctx, catalog, &mut state, Vec::new(), 2);
    state.search.focus = true;
    frames(ctx, catalog, &mut state, Vec::new(), 2);
    frames(ctx, catalog, &mut state, typed(), 1);
    let output = with_icons(ctx, catalog, &mut state);
    crate::app::tests::capture::write(ctx, &output, "inspector-toolbar-search");
    state.close();
    frames(ctx, catalog, &mut state, Vec::new(), 1);
}

fn frames(
    ctx: &egui::Context,
    catalog: &Catalog,
    state: &mut HashInspectionState,
    events: Vec<egui::Event>,
    count: usize,
) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    let mut events = Some(events);
    for _ in 0..count {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1180.0, 860.0),
                )),
                events: events.take().unwrap_or_default(),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |_| {});
                draw_catalog_hash_window(ctx, catalog, None, false, state, "capture");
            },
        );
        crate::app::tests::capture::record(&output);
    }
    output
}

/// The icon loader opens the installed packages on its first request, which takes longer than
/// a capture waits, so load one icon before capturing.
fn warm_icons(ctx: &egui::Context, catalog: &Catalog, hash: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while catalog.icon_texture(ctx, hash).is_none() {
        if let Some(problem) = catalog.icon_diagnostic(hash) {
            eprintln!("icon for {hash:#010X} did not load: {problem}");
            return;
        }
        if std::time::Instant::now() > deadline {
            eprintln!("icon for {hash:#010X} did not load within a minute");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Runs frames until package icons stop arriving from their loader, at most five seconds.
fn with_icons(
    ctx: &egui::Context,
    catalog: &Catalog,
    state: &mut HashInspectionState,
) -> egui::FullOutput {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut output = frames(ctx, catalog, state, Vec::new(), 4);
    let mut quiet = 0;
    while quiet < 20 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(30));
        output = frames(ctx, catalog, state, Vec::new(), 1);
        quiet = if output.textures_delta.set.is_empty() {
            quiet + 1
        } else {
            0
        };
    }
    output
}

fn text_center(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        // The Related Records tab carries its count after the label.
        egui::Shape::Text(text) if text.galley.job.text.starts_with(label) => {
            Some(text.pos + text.galley.rect.center().to_vec2())
        }
        _ => None,
    })
}

/// One named definition of each kind, chosen from the catalog rather than by hash.
fn capture_targets(catalog: &Catalog) -> Vec<(&'static str, u64)> {
    let named = |hash: u64| {
        catalog
            .display_name(hash)
            .is_some_and(|name| !name.is_empty())
    };
    let mut targets = Vec::new();
    let weapon = catalog
        .items
        .iter()
        .filter(|item| is_authorable_weapon_item(item) && !item.name.is_empty())
        .max_by_key(|item| (item.sockets.len(), std::cmp::Reverse(item.hash)));
    if let Some(weapon) = weapon {
        targets.push(("weapon", weapon.hash));
        targets.push(("bucket", weapon.bucket_hash));
        if let Some(plug) = weapon
            .default_plugs
            .iter()
            .flatten()
            .filter_map(|plug| parse_hash_hex(plug))
            .find(|hash| named(*hash))
        {
            targets.push(("plug", plug));
        }
        if let Some(stat) = catalog
            .item_package_metadata(weapon.hash)
            .and_then(|metadata| metadata.investment_stats.first())
            .and_then(|stat| catalog.item_stat_definition(stat.definition_index))
        {
            targets.push(("stat", stat.hash));
        }
    }
    if let Some(collectible) = catalog
        .collectibles()
        .iter()
        .find(|collectible| named(collectible.item_hash) && !collectible.paths.is_empty())
    {
        targets.push(("collectible", collectible.hash));
    }
    if let Some(record) = catalog.records().and_then(|records| {
        records
            .iter()
            .filter(|record| !record.name.is_empty())
            .max_by_key(|record| (record.objectives.len(), std::cmp::Reverse(record.hash)))
    }) {
        targets.push(("record", record.hash));
        if let Some(objective) = record
            .objectives
            .first()
            .and_then(|index| catalog.objective_definition(*index))
        {
            targets.push(("objective", objective.hash));
        }
    }
    if let Some(progression) = catalog
        .progression_definitions()
        .iter()
        .filter(|definition| progression_display_name(definition).is_some())
        .max_by_key(|definition| (definition.steps.len(), std::cmp::Reverse(definition.hash)))
    {
        targets.push(("progression", progression.hash));
    }
    if let Some(flag) = catalog
        .unlock_flag_definitions()
        .iter()
        .find(|definition| definition.name.is_some() && !definition.tested_by.is_empty())
    {
        targets.push(("unlock-flag", flag.hash));
    }
    if let Some(material) = catalog
        .material_requirement_sets()
        .iter()
        .flat_map(|set| set.requirements.iter())
        .map(|requirement| requirement.item_hash)
        .find(|hash| named(*hash))
    {
        targets.push(("material", material));
    }
    if let Some(node) = catalog.presentation_nodes().iter().find(|node| {
        let children = catalog.presentation_node_children(node.hash);
        !node.name.is_empty()
            && !node.parents.is_empty()
            && (!children.collectibles.is_empty() || !children.records.is_empty())
    }) {
        targets.push(("presentation-node", node.hash));
    }
    targets
}
