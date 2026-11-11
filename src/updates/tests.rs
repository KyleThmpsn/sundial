use super::*;
use crate::test_support::{TestDirectory, artifact, button_node, capture, click_button};
use fs2::FileExt;
use serde_json::json;

fn frame(
    updater: &mut UpdateCheck,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 720.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            updater.poll();
            updater.draw(ui.ctx(), None, false);
        },
    );
    capture::record(&output);
    output
}

#[test]
fn cancel_download_waits_for_the_worker_to_release_its_update_lock() {
    let directory = TestDirectory::new("update-cancel-retry");
    let target = directory.0.join("sundial");
    let workspace = tempfile::tempdir_in(&directory.0).unwrap();
    let workspace_path = workspace.path().to_owned();
    let lock = files::open_lock(&target).unwrap();
    FileExt::try_lock_exclusive(&lock).unwrap();
    let contender = files::open_lock(&target).unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut updater = UpdateCheck::default();
    updater.window_open = true;
    updater.status = UpdateStatus::Available(Release {
        version: "v0.6.1".into(),
        notes: "Fixture".into(),
        asset: Ok(release::Asset {
            url: String::new(),
            size: 1,
            digest: String::new(),
            kind: release::ArchiveKind::Zip,
            member: "sundial".into(),
        }),
    });
    updater.install = InstallState::Downloading;
    updater.worker = Some(receiver);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut updater, &ctx, vec![]);
    let downloading = frame(&mut updater, &ctx, vec![]);
    frame(
        &mut updater,
        &ctx,
        vec![click_button(&downloading, "Cancel Download")],
    );
    let stopping = frame(&mut updater, &ctx, vec![]);
    assert!(
        updater
            .progress
            .cancel
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    assert!(FileExt::try_lock_exclusive(&contender).is_err());
    assert!(button_node(&stopping, "Download Update").is_none());
    assert!(button_node(&stopping, "Restart and Update").is_none());
    capture::write(&ctx, &stopping, "update-cancel-draining");

    // A worker can have completed just as cancellation was requested.
    let result = Ok(Work::Downloaded(Prepared {
        directory: workspace,
        lock,
        plan: files::Plan {
            target,
            version: "v0.6.1".into(),
            old_digest: String::new(),
            new_digest: String::new(),
            install: None,
        },
    }));
    assert!(
        sender.send(result).is_ok(),
        "the canceling updater still receives the worker result"
    );
    let canceled = frame(&mut updater, &ctx, vec![]);
    FileExt::try_lock_exclusive(&contender).unwrap();
    assert!(!workspace_path.exists());
    assert!(button_node(&canceled, "Download Update").is_some());
    assert!(button_node(&canceled, "Restart and Update").is_none());
    capture::write(&ctx, &canceled, "update-cancel-retry");
    artifact(
        "update-cancel-retry.json",
        &json!({
            "retry_available_while_worker_holds_lock": false,
            "late_staged_result_removed": true,
            "retry_available_after_lock_release": true,
        }),
    );
}
