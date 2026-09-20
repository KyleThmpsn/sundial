//! Real profile-page actions exercise the render-to-commit boundary on both account formats.
use super::inventory_recovery::{button, contains_text};
use super::*;

fn frame(app: &mut SundialApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_at_width(app, ctx, events, 1000.0)
}

fn frame_at_width(
    app: &mut SundialApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    width: f32,
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
            egui::CentralPanel::default().show(ctx, |ui| app.draw_profile_inventory_page(ui));
        },
    )
}

fn click(app: &mut SundialApp, ctx: &egui::Context, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn profile_and_dismantle_deletions_record_one_edit_and_undo_without_a_frame() {
    for runtime in 0..3 {
        let sqlite = runtime == 1;
        for dismantle in [false, true] {
            let directory = TestDirectory::new("profile-action-history");
            let mut app = profile_for_runtime(&directory, runtime);
            seed_profile(&mut app, dismantle);
            app.persisted_document = app.document.clone();
            let original = app.document.clone();
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            frame(&mut app, &ctx, vec![]);
            let output = frame(&mut app, &ctx, vec![]);
            if dismantle {
                click(&mut app, &ctx, button(&output, "Dismantle Rewards").0);
            }
            let output = frame(&mut app, &ctx, vec![]);
            let label = if dismantle {
                "Delete dismantle policy"
            } else {
                "Delete shared item"
            };
            let (position, enabled) = button(&output, label);
            assert!(enabled, "{sqlite} {dismantle}: {label}");
            assert!(contains_text(&output, "Invalid Item"));
            click(&mut app, &ctx, position);
            assert_eq!(
                app.undo_history.len(),
                1,
                "sqlite={sqlite}, dismantle={dismantle}, status={}",
                app.status
            );
            assert!(app.document_repaint_pending);
            assert!(app.dirty);
            assert_ne!(app.document, original);
            app.undo();
            assert_eq!(app.document, original);
            assert!(!app.dirty);
            app.redo();
            assert_ne!(app.document, original);
            assert_eq!(app.undo_history.len(), 1);
        }
    }
}

fn profile_for_runtime(directory: &TestDirectory, runtime: u8) -> SundialApp {
    let mut app = state_recovery::for_source(directory, runtime == 1);
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

fn seed_profile(app: &mut SundialApp, dismantle: bool) {
    // Keep one unknown item visible so deletion covers the recovery card too.
    if dismantle {
        for reward in account_workspace::dismantle_rewards(&app.document)
            .unwrap()
            .unwrap_or_default()
            .into_iter()
            .rev()
        {
            account_workspace::apply_dismantle_reward_action(
                &mut app.document,
                reward.location,
                inventory::DismantleRewardAction::Remove,
            )
            .unwrap();
        }
        account_workspace::add_dismantle_reward(&mut app.document, 999).unwrap();
    } else {
        for item in account_workspace::profile_items(&app.document)
            .unwrap()
            .unwrap_or_default()
            .into_iter()
            .rev()
        {
            account_workspace::apply_profile_item_action(
                &mut app.document,
                item.location,
                inventory::ProfileItemAction::Remove,
            )
            .unwrap();
        }
        account_workspace::add_profile_item(&mut app.document, 999, 2).unwrap();
    }
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
fn dawn_profile_surfaces_unfiltered_policies_and_the_delivery_ledger() {
    let directory = TestDirectory::new("dawn-profile-ledger-ui");
    let path = directory.0.join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&path);
    rusqlite::Connection::open(&path).unwrap().execute_batch(
        "INSERT INTO dismantle_rewards VALUES(0,3159615086,3);
         INSERT INTO reward_debts(debt_id,account_soid,character_soid,mission_hash,runtime_epoch,session_id,run_id,definition_hash,quantity,credited,delivered) VALUES
         (1,'9EAA300100100100','9EAA300100100101',100,'0000000000000001','0000000000000002','0000000000000003',3159615086,50,0,0),
         (2,'9EAA300100100100','9EAA300100100101',100,'0000000000000001','0000000000000002','0000000000000004',3159615086,50,30,1);"
    ).unwrap();
    let mut app = state_recovery::for_source(&directory, false);
    app.document = account_workspace::WorkspaceDocument::load(
        serde_json::json!({"version":6}),
        &directory.0.join("settings.json"),
        true,
    );
    let original = app.document.clone();
    app.manifest = reward_catalog();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, button(&output, "Dismantle Rewards").0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(contains_text(&output, "Dawn supports up to eight"));
    assert!(!contains_text(&output, "Rarity"));
    crate::app::tests::capture::write(&ctx, &output, "dawn-profile-dismantle");
    click(&mut app, &ctx, button(&output, "Reward Queue").0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(contains_text(&output, "Pending"));
    assert!(!contains_text(&output, "single-slot profile currencies"));
    assert!(!contains_text(&output, "reward_debts"));
    assert!(contains_text(&output, "Add to Queue"));
    assert!(!contains_text(&output, "Profile Currency"));
    assert!(!contains_text(&output, "Character"));
    crate::app::tests::capture::write(&ctx, &output, "dawn-queue-pending");
    click(&mut app, &ctx, button(&output, "Delivery History (1)").0);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(contains_text(&output, "Partial"));
    crate::app::tests::capture::write(&ctx, &output, "dawn-queue-history");
    assert_eq!(app.document, original);
}

#[test]
fn dawn_reward_queue_cancel_records_one_edit_and_supports_undo_redo() {
    let directory = TestDirectory::new("dawn-reward-cancel-ui");
    let mut app = profile_for_runtime(&directory, 2);
    app.manifest = reward_catalog();
    app.document
        .dawn_account_mut()
        .unwrap()
        .queue_currency(
            0,
            3159615086,
            50,
            &crate::catalog::InventoryMetadata {
                scope: crate::catalog::InventoryScope::Profile,
                native_bucket_id: 0,
                stackability: crate::catalog::ItemStackability::Stackable,
                max_stack_size: Some(250000),
                bucket_capacity: Some(1),
            },
        )
        .unwrap();
    app.persisted_document = app.document.clone();
    let original = app.document.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, button(&output, "Reward Queue").0);
    let output = frame(&mut app, &ctx, vec![]);
    let (position, enabled) = button(&output, "Cancel");
    assert!(enabled);
    click(&mut app, &ctx, position);
    assert!(app.document.dawn_account().unwrap().reward_debts()[0].delivered);
    assert!(app.dirty);
    assert_eq!(app.undo_history.len(), 1);
    app.undo();
    assert_eq!(app.document, original);
    assert!(!app.dirty);
    app.redo();
    assert!(app.document.dawn_account().unwrap().reward_debts()[0].delivered);
    assert_eq!(app.undo_history.len(), 1);
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
    crate::app::tests::capture::write(&ctx, &output, "dawn-queue-empty");
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
    crate::app::tests::capture::write(&ctx, &output, "dawn-queue-ready");
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

#[test]
fn dawn_reward_queue_form_fits_a_narrow_panel() {
    let directory = TestDirectory::new("dawn-reward-narrow-ui");
    let mut app = profile_for_runtime(&directory, 2);
    app.manifest = reward_catalog();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, button(&output, "Reward Queue").0);
    frame_at_width(&mut app, &ctx, vec![], 600.0);
    let output = frame_at_width(&mut app, &ctx, vec![], 600.0);
    let currency = button(&output, "Choose Currency").0;
    let add = button(&output, "Add to Queue").0;
    assert!(
        (currency.y - add.y).abs() < 1.0,
        "Currency and Add controls must align"
    );
    assert!(add.x + 56.0 <= 600.0, "Add must fit inside the panel");
    crate::app::tests::capture::write(&ctx, &output, "dawn-queue-narrow");
}
