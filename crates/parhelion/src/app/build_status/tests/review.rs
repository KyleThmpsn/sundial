//! Review progress reaches the rendered dialog and its copied activity, including failure.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::install::{InstallPhase, InstallProgress};
use crate::test_support::driver::{accessible, tap};

fn draw(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 1100.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.draw_build_status_window(ui),
    );
    capture::record(&output);
    output
}

fn settled(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    for _ in 0..3 {
        draw(ctx, app, Vec::new());
    }
    draw(ctx, app, Vec::new())
}

#[test]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn review_shows_current_work_copies_timing_and_clears_failed_work_on_retry() {
    let (result_sender, result_receiver) = mpsc::channel();
    let (progress_sender, progress_receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        build_dialog_step: BuildDialogStep::ReviewInstall,
        latest_build: Some(Ok(BuildReport {
            weapons: vec![],
            run_directory: "review-fixture".into(),
            manifest_path: "review-fixture/manifest.json".into(),
            artifacts: vec![],
            selection_fingerprint: "review-fixture".into(),
            staged_recipe_paths: vec![],
        })),
        replacement_receiver: Some(result_receiver),
        replacement_status: InstallStatus {
            receiver: Some(progress_receiver),
            ..Default::default()
        },
        ..Default::default()
    };
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    for (label, completed, seconds) in [
        ("Verifying Staged Packages and Recipes", 1, 0),
        ("Checking Installed Packages", 2, 75),
    ] {
        progress_sender
            .send((
                InstallProgress {
                    phase: InstallPhase::ReviewingAccount,
                    current_artifact: Some(label.into()),
                    completed,
                    total: 11,
                },
                Duration::from_secs(seconds),
            ))
            .unwrap();
        let output = settled(&ctx, &mut app);
        assert!(
            accessible(&output, label).is_some(),
            "the active review step is hidden"
        );
    }
    let output = settled(&ctx, &mut app);
    assert!(accessible(&output, "Elapsed 1m 15s").is_some());
    capture::write(&ctx, &output, "review-active");

    let header = accessible(&output, "Account Review Activity")
        .unwrap()
        .center();
    for events in tap(header) {
        draw(&ctx, &mut app, events);
    }
    let output = settled(&ctx, &mut app);
    let copy = accessible(&output, "Copy Progress").unwrap().center();
    let mut copied = None;
    for events in tap(copy) {
        let output = draw(&ctx, &mut app, events);
        for command in output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                copied = Some(text);
            }
        }
    }
    let copied = copied.expect("Copy Progress must produce clipboard text");
    assert!(copied.contains("[1m 15s]") && copied.contains("Verified Staged Packages and Recipes"));
    assert!(copied.contains("Checking Installed Packages"));

    result_sender
        .send(Err("Review fixture stopped before account changes".into()))
        .unwrap();
    let output = settled(&ctx, &mut app);
    capture::write(&ctx, &output, "review-failed");
    assert!(accessible(&output, "Review fixture stopped before account changes").is_some());
    assert!(app.replacement_receiver.is_none());
    assert!(app.replacement_status.started.is_none());
    assert!(app.replacement_status.receiver.is_none());
    assert!(app.install_receiver.is_none());
    assert!(!app.account_changed && !app.packages_changed);

    app.latest_build = None;
    app.start_replacement_review();
    assert!(app.replacement_status.progress.is_none());
    assert_eq!(app.replacement_status.elapsed, Duration::ZERO);
    if let Some(directory) = crate::test_support::artifacts("build-window") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("review-progress.txt"), copied).unwrap();
    }
}
