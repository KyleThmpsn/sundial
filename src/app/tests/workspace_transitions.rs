use super::driver::*;
use crate::app::{background_tasks::PendingInstallLoad, *};
use crate::test_support::{artifact, capture, click_button};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

const ACCOUNT_FIXTURE: &str =
    include_str!("../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json");
const KINETIC: &str = "/state/characters/0/equipment/kinetic";

fn kinetic(app: &SundialApp) -> equipment::EquippedItemSnapshot {
    account::equipped_item_snapshots(&app.document, 0)
        .unwrap()
        .into_iter()
        .find(|item| item.slot == "kinetic")
        .unwrap()
}

fn delete_frame(
    app: &mut SundialApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 650.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_app_chrome(ui, None);
            app.draw_delete_equipment_confirmation(ui.ctx());
        },
    );
    capture::record(&output);
    output
}

#[test]
fn deleting_after_account_reload_requires_the_same_equipped_instance() {
    let directory = TestDirectory::new("delete-equipped-after-reload");
    let mut app = with_document(
        directory.0.clone(),
        serde_json::from_str(ACCOUNT_FIXTURE).unwrap(),
    );
    fs::write(&app.settings_path, ACCOUNT_FIXTURE).unwrap();
    let original = kinetic(&app);
    app.request_equipment_delete(0, &original, "Original Weapon");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    delete_frame(&mut app, &ctx, vec![]);
    assert!(contains_text(
        &delete_frame(&mut app, &ctx, vec![]),
        "Delete Original Weapon?"
    ));

    // A second copy of the same definition is equipped while the review is open.
    let mut refreshed: Value = serde_json::from_str(ACCOUNT_FIXTURE).unwrap();
    refreshed.pointer_mut(KINETIC).unwrap()["instance_soid"] = json!("0x4000000000000101");
    let refreshed_bytes = serde_json::to_vec(&refreshed).unwrap();
    fs::write(&app.settings_path, &refreshed_bytes).unwrap();
    assert!(app.reload());
    let replacement = kinetic(&app);
    assert_ne!(replacement.instance_soid, original.instance_soid);
    let dialog = delete_frame(&mut app, &ctx, vec![]);
    delete_frame(&mut app, &ctx, vec![click_button(&dialog, "Delete Item")]);
    assert_eq!(
        app.document.json().pointer(KINETIC),
        refreshed.pointer(KINETIC)
    );
    assert!(!app.has_unsaved_changes());
    let refused = delete_frame(&mut app, &ctx, vec![]);
    assert!(contains_text(&refused, "The equipped item changed"));
    capture::write(&ctx, &refused, "stale-equipped-delete");

    app.request_equipment_delete(0, &replacement, "Replacement Weapon");
    delete_frame(&mut app, &ctx, vec![]);
    let dialog = delete_frame(&mut app, &ctx, vec![]);
    assert!(contains_text(&dialog, "Delete Replacement Weapon?"));
    delete_frame(&mut app, &ctx, vec![click_button(&dialog, "Delete Item")]);
    assert!(app.document.json().pointer(KINETIC).unwrap().is_null());
    assert!(app.has_unsaved_changes());
    let disk = fs::read(&app.settings_path).unwrap();
    assert_eq!(disk, refreshed_bytes);
    artifact(
        "stale-equipped-delete.json",
        &json!({
            "fixture": "sunrise-v8-d0fe8886-defaults.json",
            "reviewed_instance": original.instance_soid_text,
            "replacement_instance": replacement.instance_soid_text,
            "refreshed_sha256": hex::encode(Sha256::digest(&refreshed_bytes)),
            "disk_sha256_after_both_confirmations": hex::encode(Sha256::digest(&disk)),
            "fresh_confirmation_emptied_edit_buffer": true,
        }),
    );
}

#[test]
fn catalog_completion_reads_current_settings_and_rechecks_future_schema_consent() {
    let original = TestDirectory::new("before-install-switch");
    let destination = TestDirectory::new("during-install-switch");
    let mut app = app(original.0.clone());
    let settings = destination.0.join("settings.json");
    let mut document: Value = serde_json::from_str(ACCOUNT_FIXTURE).unwrap();
    fs::write(&settings, serde_json::to_vec(&document).unwrap()).unwrap();
    let pending = |accepted_future_schema| PendingInstallLoad {
        install_path: destination.0.clone(),
        settings_path: settings.clone(),
        settings_layout: SettingsLayout::GameRoot,
        accepted_future_schema,
    };
    let catalog = || Manifest::for_test(Vec::new(), HashMap::new());
    let request = pending(None);
    document["steam"]["user"]["persona_name"] = json!("Changed during catalog scan");
    let current = serde_json::to_vec(&document).unwrap();
    fs::write(&settings, &current).unwrap();
    assert!(app.apply_install_load(request, catalog()));
    assert_eq!(app.settings_path, settings);
    assert_eq!(
        app.document.json()["steam"]["user"]["persona_name"],
        document["steam"]["user"]["persona_name"]
    );
    assert_eq!(app.document.json(), app.persisted_document.json());
    assert_eq!(fs::read(&settings).unwrap(), current);

    let other = TestDirectory::new("future-install-switch");
    let future_settings = other.0.join("settings.json");
    let future = || PendingInstallLoad {
        install_path: other.0.clone(),
        settings_path: future_settings.clone(),
        settings_layout: SettingsLayout::GameRoot,
        accepted_future_schema: Some(20),
    };
    fs::write(&future_settings, b"{\"version\":21}").unwrap();
    assert!(!app.apply_install_load(future(), catalog()));
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.draw_future_schema_confirmation(ui.ctx())
    });
    let warning = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.draw_future_schema_confirmation(ui.ctx())
    });
    assert!(contains_text(&warning, "uses version 21"));
    assert_eq!(app.settings_path, settings);
    fs::write(&future_settings, b"{").unwrap();
    assert!(!app.apply_install_load(future(), catalog()));
    assert!(app.status.contains("Invalid JSON"));
    assert_eq!(app.settings_path, settings);
    assert_eq!(
        app.document.json()["steam"]["user"]["persona_name"],
        "Changed during catalog scan"
    );
    artifact(
        "install-switch-settings.json",
        &json!({
            "fixture": "sunrise-v8-d0fe8886-defaults.json",
            "loaded_persona": app.document.json()["steam"]["user"]["persona_name"],
            "current_settings_sha256": hex::encode(Sha256::digest(&current)),
            "changed_future_schema_requested_confirmation": 21,
            "invalid_destination_kept_active_workspace": true,
        }),
    );
}
