//! Real profile-page actions exercise the render-to-commit boundary on both account formats.
use super::inventory_recovery::{button, contains_text};
use super::*;

fn frame(app: &mut SundialApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 760.0),
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
    for sqlite in [false, true] {
        for dismantle in [false, true] {
            let directory = TestDirectory::new("profile-action-history");
            let mut app = state_recovery::for_source(&directory, sqlite);
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
