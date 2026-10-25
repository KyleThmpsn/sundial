//! Real profile-page actions exercise the render-to-commit boundary on both account formats.
use crate::app::tests::driver::*;
use crate::app::*;

fn frame(app: &mut SundialApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_profile_inventory_page(ui));
        },
    )
}

fn click(app: &mut SundialApp, ctx: &egui::Context, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            crate::test_support::primary_press(position, pressed),
        );
    }
}

fn profile_for_runtime(directory: &TestDirectory, runtime: u8) -> SundialApp {
    let mut app = for_source(directory, runtime == 1);
    if runtime == 2 {
        crate::persistence::dawn_account::tests::create_fixture(
            &directory.0.join("player-state.db"),
        );
        app.document = account_workspace::WorkspaceDocument::load(
            serde_json::json!({"version":6}),
            &directory.0.join("settings.json"),
            true,
        );
    }
    app
}

fn reward_catalog() -> Manifest {
    use crate::catalog::{InventoryMetadata, InventoryScope, ItemDef, ItemStackability};
    Manifest::for_test_with_inventory(
        vec![ItemDef {
            hash: 3159615086,
            name: "Glimmer".into(),
            type_name: "Currency".into(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        }],
        HashMap::new(),
        HashMap::from([(
            3159615086,
            InventoryMetadata {
                scope: InventoryScope::Profile,
                native_bucket_id: 0,
                stackability: ItemStackability::Stackable,
                max_stack_size: Some(250000),
                bucket_capacity: Some(1),
            },
        )]),
    )
}

#[test]
fn dawn_reward_queue_picker_adds_currency_and_records_one_edit() {
    let directory = TestDirectory::new("dawn-reward-add-ui");
    let mut app = profile_for_runtime(&directory, 2);
    app.manifest = reward_catalog();
    app.persisted_document = app.document.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, button(&output, "Reward Queue").0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(!button(&output, "Add to Queue").1);
    assert!(contains_text(&output, "No pending rewards"));
    crate::test_support::capture::write(&ctx, &output, "dawn-queue-empty");
    click(&mut app, &ctx, button(&output, "Choose Currency").0);
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    let label = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label())
        .find(|label| label.starts_with("Glimmer"))
        .expect("Glimmer picker entry");
    click(&mut app, &ctx, button(&output, label).0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(button(&output, "Add to Queue").1);
    crate::test_support::capture::write(&ctx, &output, "dawn-queue-ready");
    click(&mut app, &ctx, button(&output, "Add to Queue").0);
    let debts = app.document.dawn_account().unwrap().reward_debts();
    assert_eq!(debts.len(), 1);
    assert_eq!(
        (debts[0].definition_hash, debts[0].quantity),
        (3159615086, 1)
    );
    assert!(!debts[0].delivered);
    assert_eq!(app.undo_history.len(), 1);
    assert!(app.dirty);
}
