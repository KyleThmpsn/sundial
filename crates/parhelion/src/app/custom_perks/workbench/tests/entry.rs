use super::*;
use crate::app::custom_perks::editor::tests::{set_test_speed, test_speed};

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            app.draw_perk_workbench(ctx);
        },
    )
}

fn button(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()).center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing button: {label}"))
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
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

fn settle(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    frame(ctx, app, vec![]);
    frame(ctx, app, vec![])
}

#[test]
fn perk_window_waits_for_package_operations_without_consuming_its_draft_or_request() {
    for installing in [false, true] {
        let mut app = PackageAuthoringApp::default();
        app.perk_workbench.initialized = true;
        app.perk_workbench.open = true;
        app.perk_workbench
            .documents
            .push(Document::new(PerkRecipe::new(), None));
        app.perk_request = Some(Request::EditChoice {
            socket: 0,
            choice: 0,
        });
        if installing {
            app.install_receiver = Some(mpsc::channel().1);
        } else {
            app.build_receiver = Some(mpsc::channel().1);
        }
        let recipe = app.recipe.clone();
        let perk = app.perk_workbench.documents[0].recipe.clone();
        let output = settle(&egui::Context::default(), &mut app);
        assert!(output.shapes.is_empty());
        assert_eq!(
            app.perk_request,
            Some(Request::EditChoice {
                socket: 0,
                choice: 0
            })
        );
        assert_eq!(app.recipe, recipe);
        assert_eq!(app.perk_workbench.documents[0].recipe, perk);
        assert!(app.perk_workbench.open);
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES for native editor entry and private speed apply"]
fn native_micro_missile_entry_applies_speed_without_experimental_mode() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let install = std::env::var_os("PARHELION_PROJECTILE_CATALOG_INSTALL")
        .map(PathBuf::from)
        .unwrap_or_else(|| packages.parent().unwrap().to_path_buf());
    let catalog = InvestmentCatalog::load(&install, false, |_| {}).unwrap();
    let donor = catalog.weapon_donor(0x23DB_942F).unwrap();
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        "Speed Editor Entry",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let socket = donor.sockets.len();
    recipe.overrides.socket_columns = vec![None; socket];
    recipe
        .overrides
        .socket_columns
        .push(Some(crate::WeaponSocketColumnRecipe {
            socket_type: Some(92),
            choices: vec![0xDD5C_B37A.into()],
            ..Default::default()
        }));
    let mut app = PackageAuthoringApp {
        sandbox_perk_choices: catalog
            .weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition),
        catalog: Some(catalog),
        packages,
        recipe,
        perk_request: Some(Request::EditChoice { socket, choice: 0 }),
        show_experimental_options: false,
        ..Default::default()
    };
    app.perk_workbench.initialized = true;
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    let output = settle(&ctx, &mut app);
    click(&ctx, &mut app, button(&output, "Edit Behavior…"));
    let start = std::time::Instant::now();
    while app
        .perk_workbench
        .editor
        .as_ref()
        .expect("parameter editor opened")
        .is_loading()
    {
        assert!(
            // A fresh fixture must index the native asset names and projectiles first.
            start.elapsed() < std::time::Duration::from_secs(600),
            "native asset indexing exceeded the cold-cache test deadline"
        );
        frame(&ctx, &mut app, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let editor = app.perk_workbench.editor.as_mut().unwrap();
    assert_eq!(editor.key.source_perk_index, 1178);
    let loaded = editor.graph.clone().expect("stock perk decoded");
    set_test_speed(&loaded, &mut editor.draft, 1.5);
    assert!(editor.validation_errors().is_empty());
    assert_eq!(app.recipe, before);
    let output = settle(&ctx, &mut app);
    click(&ctx, &mut app, button(&output, "Apply and Back"));
    assert!(app.perk_workbench.editor.is_none());
    let output = settle(&ctx, &mut app);
    click(&ctx, &mut app, button(&output, "Apply to Weapon"));
    let variant = &app.recipe.overrides.socket_plug_variants[0];
    assert_eq!(usize::from(variant.socket_index), socket);
    let edits = &variant.sandbox_perks[0].runtime_values;
    assert_eq!(edits.len(), 2);
    assert_eq!(test_speed(&loaded, edits), 1.5);
    assert_eq!(
        WeaponRecipe::from_json_str(&app.recipe.to_json_pretty().unwrap()).unwrap(),
        app.recipe
    );
}
