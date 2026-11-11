use super::driver::*;
use crate::app::*;
use crate::test_support::{artifact, capture, click_button};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

fn frame(
    app: &mut SundialApp,
    ctx: &egui::Context,
    close: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 720.0),
        )),
        events,
        ..Default::default()
    };
    if close {
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
    }
    let output = ctx.run_ui(input, |ui| {
        app.handle_native_close_request(ui.ctx());
        app.draw_save_review_confirmation(ui.ctx());
        app.draw_exit_confirmation(ui.ctx());
    });
    capture::record(&output);
    output
}

fn cancels_close(output: &egui::FullOutput) -> bool {
    output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .iter()
        .any(|command| matches!(command, egui::ViewportCommand::CancelClose))
}

#[test]
fn repeated_close_keeps_save_and_exit_review_until_cancelled() {
    let directory = TestDirectory::new("close-save-review");
    let original = FIXTURES[2].as_bytes();
    let mut app = with_document(
        directory.0.clone(),
        serde_json::from_slice(original).unwrap(),
    );
    fs::write(&app.settings_path, original).unwrap();
    app.preferences.review_changes_before_saving = true;
    app.document.json_mut()["steam"]["user"]["persona_name"] = json!("Review fixture");
    app.record_edit("Changed Persona Name");

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let exit = frame(&mut app, &ctx, true, vec![]);
    assert!(cancels_close(&exit));
    let exit = frame(&mut app, &ctx, false, vec![]);
    frame(
        &mut app,
        &ctx,
        false,
        vec![click_button(&exit, "Save and Exit")],
    );
    let review = frame(&mut app, &ctx, true, vec![]);
    assert!(cancels_close(&review));
    let heading = "Review Changes Before Saving and Exiting";
    let review = frame(&mut app, &ctx, false, vec![]);
    assert!(contains_text(&review, heading));
    assert!(contains_text(&review, "Review fixture"));
    capture::write(&ctx, &review, "close-save-and-exit-review");

    frame(&mut app, &ctx, false, vec![click_button(&review, "Cancel")]);
    let cancelled = frame(&mut app, &ctx, false, vec![]);
    assert!(!contains_text(&cancelled, heading));
    assert!(app.has_unsaved_changes());
    assert_eq!(
        app.document.json()["steam"]["user"]["persona_name"],
        "Review fixture"
    );
    let saved = fs::read(&app.settings_path).unwrap();
    assert_eq!(saved, original);
    artifact(
        "close-save-and-exit-review.json",
        &json!({
            "fixture": "sunrise-v16-1120748-defaults.json",
            "retained_heading": heading,
            "draft_persona_name_after_cancel": app.document.json()["steam"]["user"]["persona_name"],
            "input_sha256": hex::encode(Sha256::digest(original)),
            "disk_sha256_after_cancel": hex::encode(Sha256::digest(&saved)),
        }),
    );
}
