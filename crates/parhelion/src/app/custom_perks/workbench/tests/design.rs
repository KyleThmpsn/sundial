use super::*;
use sundial::package_authoring::{sandbox_perk::dependencies, tft};

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    if let Some(install) = std::env::var_os("PARHELION_UI_FONT_INSTALL") {
        sundial::investment::configure_authoring_fonts(&ctx, std::path::Path::new(&install))
            .expect("capture fonts must be available");
    }
    ctx
}

fn render(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(ctx, workbench, true, size, vec![]);
    }
    output
}

fn click(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2, name: &str) {
    let output = render(ctx, workbench, size);
    let position = label(&output, name)
        .unwrap_or_else(|| panic!("Missing {name}"))
        .center();
    for pressed in [true, false] {
        frame(
            ctx,
            workbench,
            true,
            size,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

#[test]
fn workbench_descriptions_and_properties_use_readable_text_without_hiding_actions() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1320.0, 900.0)] {
        for page in [Page::Basics, Page::Effects] {
            let mut recipe = PerkRecipe::new();
            recipe.description = "A custom perk with a reusable effect.".into();
            recipe.effects.push(program::new_effect(1178));
            let mut workbench = Workbench {
                initialized: true,
                open: true,
                drafts_writable: true,
                page,
                documents: vec![Document::new(recipe, None)],
                ..Default::default()
            };
            let ctx = context();
            let output = render(&ctx, &mut workbench, size);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for name in ["Save to Library", "Description"] {
                if let Some(rect) = label(&output, name) {
                    assert!(screen.contains_rect(rect), "{name} clipped at {size:?}");
                }
            }
            assert!(label(&output, "Save to Library").is_some());
            capture::write(
                &ctx,
                &output,
                &format!(
                    "workbench-{}-{}",
                    if page == Page::Basics {
                        "description"
                    } else {
                        "effects"
                    },
                    size.x
                ),
            );
        }
        let mut workbench = Workbench::default();
        let mut parameter_editor = editor(fixture());
        parameter_editor.entity_source = Some(0x8152_82E1);
        workbench.set_test_editor(parameter_editor);
        workbench.drafts_writable = true;
        let ctx = context();
        let output = render(&ctx, &mut workbench, size);
        for name in ["Apply and Back", "Back"] {
            assert!(label(&output, name).is_some(), "Missing {name}");
        }
        capture::write(&ctx, &output, &format!("workbench-properties-{}", size.x));
    }
}

#[test]
fn catalog_tabs_show_populated_content_and_readable_navigation() {
    let path = "test_content/projectiles/sample_projectile.tft".to_owned();
    let names = tft::Index {
        paths: vec![tft::ContentPath {
            source: 0x80800001,
            offset: 24,
            path: path.clone(),
        }],
        references: vec![tft::Reference {
            source: 0x80800001,
            source_class: 0x8080_40B5,
            offset: 32,
            target: 0x80800002,
            target_class: 0x8080_3B73,
            path,
        }],
        ..Default::default()
    };
    let mut workbench = Workbench {
        initialized: true,
        ..Default::default()
    };
    workbench.discovery.data = Some(discovery::Data {
        names: Arc::new(names),
        effects: Arc::new(projectile::catalog::Catalog {
            entries: vec![projectile::catalog::Entry {
                graph: 0x80800002,
                kind: projectile::Kind::Projectile,
                object_type: 0,
                owners: vec![0x80800004],
                package: "test_content.pkg".into(),
                native_name: Some("sample_projectile".into()),
                native_paths: vec!["test_content/projectiles/sample_projectile.tft".into()],
                contexts: vec![],
                perk_indices: vec![],
                source_hint: None,
            }],
            owners: std::collections::BTreeMap::from([(
                0x80800004,
                projectile::catalog::Owner {
                    package: "test_content.pkg".into(),
                    names: vec![],
                    components: vec![projectile::catalog::OwnerComponent {
                        binding: 0,
                        class: 0x80803B73,
                    }],
                },
            )]),
            ..Default::default()
        }),
        asset_choices: vec![sundial::investment::discovery::AssetChoice {
            index: 0,
            name: "sample_projectile".into(),
            search: "sample_projectile 80800002".into(),
        }],
        perks: Arc::new(dependencies::Index {
            patterns: vec![],
            caster: None,
            perks: vec![
                dependencies::Perk {
                    index: 0,
                    hash: 0x80800003,
                    runtime_key: 0,
                    action: Some(0x80800001),
                    graphs: vec![],
                    error: None,
                    behavior: Some(sample_behavior("On weapon kill, adjust grenade energy")),
                },
                dependencies::Perk {
                    index: 1,
                    hash: 0x80800005,
                    runtime_key: 0,
                    action: Some(0x80800006),
                    graphs: vec![],
                    error: None,
                    behavior: Some(sample_behavior("On weapon kill, adjust melee energy")),
                },
            ],
        }),
        perk_assets: vec![
            dependencies::content::PerkAssets {
                perk_index: 0,
                action: vec![0],
                graphs: vec![],
                components: vec![],
            },
            dependencies::content::PerkAssets {
                perk_index: 1,
                action: vec![],
                graphs: vec![],
                components: vec![],
            },
        ],
        pattern_items: Default::default(),
        abilities: vec![],
        perk_search: Default::default(),
        scripts: Arc::new(vec![]),
    });
    workbench.open_engine_catalog();
    let ctx = context();
    let size = egui::vec2(1100.0, 800.0);
    for (tab, expected, name) in [
        ("Kinds", "Installed Effect Entries", "kinds"),
        ("Objects and Effects", "Technical Details", "objects"),
        ("Perk References", "Action References (1)", "perks"),
        ("TFT Paths", "Containing Resource", "paths"),
        ("TFT References", "Copy Target Tag", "references"),
    ] {
        click(&ctx, &mut workbench, size, tab);
        let output = render(&ctx, &mut workbench, size);
        assert!(
            label(&output, expected).is_some(),
            "Missing {expected} on {tab}"
        );
        capture::write(&ctx, &output, &format!("catalog-tab-{name}"));
        if name == "perks" {
            // A perk search must not hide unrelated paths after changing tabs.
            click(
                &ctx,
                &mut workbench,
                size,
                "Search Perks, Effect Numbers, or Paths",
            );
            frame(
                &ctx,
                &mut workbench,
                true,
                size,
                vec![egui::Event::Text("Effect 0".into())],
            );
        }
    }
    check_catalog_details(&ctx, &mut workbench, size);
    check_catalog_links(&ctx, &mut workbench, size);
}

/// A reference page links to the kinds it uses, and a kind's example opens its reference.
fn check_catalog_links(_: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) {
    // A fresh context forgets the narrowed window from the large-text check.
    let ctx = &context();
    workbench.engine.scan_details = false;
    click(ctx, workbench, size, "Perk References");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Similar Effects · 1 same kinds, 0 related").is_some());
    click(ctx, workbench, size, "Timer");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Installed Effect Entries").is_some());
    assert!(
        label(
            &output,
            "Waits for a fixed number of seconds. Used for both effect duration and cooldown."
        )
        .is_some()
    );
    capture::write(ctx, &output, "catalog-kind-from-reference");
    click(ctx, workbench, size, "On weapon kill, adjust melee energy");
    click(ctx, workbench, size, "Open Reference");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Effect 1").is_some());
    assert!(label(&output, "On weapon kill, adjust melee energy").is_some());
    assert!(label(&output, "Similar Effects · 1 same kinds, 0 related").is_some());
    capture::write(ctx, &output, "catalog-reference-from-kind");
}

/// A decoded reading with one trigger and one action, so the reference page has content.
fn sample_behavior(headline: &str) -> dependencies::Behavior {
    let line = |kind: &str, fields: &[&str]| dependencies::DetailLine {
        text: kind.to_owned(),
        kind: kind.to_owned(),
        fields: fields.iter().map(|field| (*field).to_owned()).collect(),
        depth: 0,
        asset: None,
    };
    dependencies::Behavior {
        headline: headline.to_owned(),
        support: sundial::package_authoring::sandbox_perk::nodes::Support::Authorable,
        editable: true,
        program: None,
        condition_kinds: vec![1],
        effect_kinds: vec![8],
        details: vec![
            dependencies::DetailSection {
                group: "Effect".into(),
                heading: "Starts When".into(),
                lines: vec![line(
                    "Weapon Kill",
                    &["Weapon: This weapon", "Chance: 100%"],
                )],
            },
            dependencies::DetailSection {
                group: "Effect".into(),
                heading: "Then".into(),
                lines: vec![line(
                    "Component Value Adjustment",
                    &["Target: Grenade Energy", "Amount: 10%", "Duration: 5 s"],
                )],
            },
        ],
        notes: vec![],
    }
}

fn check_catalog_details(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) {
    click(ctx, workbench, size, "Asset Type: Projectile Movement");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Class 0x80803B73").is_some());
    capture::write(ctx, &output, "catalog-type-page");
    click(ctx, workbench, size, "sample_projectile.tft");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Copy Resource Tag").is_some());
    assert!(label(&output, "Used By (1)").is_some());
    capture::write(ctx, &output, "catalog-resource-page");
    click(ctx, workbench, size, "Graph");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Connection Depth").is_some());
    capture::write(ctx, &output, "catalog-resource-graph");
    click(
        ctx,
        workbench,
        size,
        "Unnamed Resource\nPerk Action\n0x80800001",
    );
    assert!(label(&render(ctx, workbench, size), "Referenced Assets (1)").is_some());
    click(ctx, workbench, size, "Back");
    assert!(label(&render(ctx, workbench, size), "Connection Depth").is_some());
    check_graph_large_text(ctx, workbench, size);

    click(ctx, workbench, size, "List");
    click(ctx, workbench, size, "Component Structure");
    let output = render(ctx, workbench, size);
    assert!(
        label(
            &output,
            "Choose a game installation to read component field values."
        )
        .is_some()
    );
    capture::write(ctx, &output, "catalog-component-structure");
    click(ctx, workbench, size, "Component Structure");
    click(ctx, workbench, size, "Back");
    assert!(label(&render(ctx, workbench, size), "Class 0x80803B73").is_some());
    click(ctx, workbench, size, "Back");
    assert!(label(&render(ctx, workbench, size), "Copy Target Tag").is_some());
    click(ctx, workbench, size, "Unnamed Resource · 0x80800001");
    assert!(label(&render(ctx, workbench, size), "Referenced Assets (1)").is_some());
    click(ctx, workbench, size, "Back to Results");
    assert!(label(&render(ctx, workbench, size), "Copy Target Tag").is_some());
    workbench.engine.scan_details = true;
    let output = render(ctx, workbench, size);
    assert!(label(&output, "Resources Scanned").is_some());
    capture::write(ctx, &output, "catalog-scan-details");
}

fn check_graph_large_text(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) {
    let original_style = (*ctx.style()).clone();
    let mut large_style = original_style.clone();
    large_style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(20.0));
    ctx.set_style(large_style);
    let narrow = egui::vec2(640.0, 900.0);
    let output = render(ctx, workbench, narrow);
    for name in ["Connection Depth", "Center on Selection"] {
        let rect = label(&output, name).expect(name);
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, narrow).contains_rect(rect),
            "{name} clipped"
        );
    }
    capture::write(ctx, &output, "catalog-graph-large-text");
    ctx.set_style(original_style);
    render(ctx, workbench, size);
}
