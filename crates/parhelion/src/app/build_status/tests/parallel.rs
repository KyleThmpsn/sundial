//! Interleaved worker events must stay truthful in the real dialog and copied activity.
//! Failure cases are recorded in docs/build-performance-2026-10-08/follow-ups/failure-model.md.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::test_support::driver::{accessible, tap};
use crate::workflow::{BuildActivity, OperationStatus};

fn draw(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = frame(ctx, app, events);
    capture::record(&output);
    output
}

fn copy(ctx: &egui::Context, app: &mut PackageAuthoringApp, name: &str) -> String {
    for _ in 0..3 {
        draw(ctx, app, Vec::new());
    }
    let output = draw(ctx, app, Vec::new());
    capture::write(ctx, &output, name);
    let at = accessible(&output, "Copy Progress").unwrap().center();
    let mut copied = None;
    for events in tap(at) {
        for command in draw(ctx, app, events).platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                copied = Some(text);
            }
        }
    }
    copied.expect("Copy Progress must publish the rendered activity")
}

#[test]
fn parallel_workers_keep_their_own_completion_and_worker_timestamps() {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        ..Default::default()
    };
    let recipe = app.recipe.clone();
    let origin = Instant::now() - Duration::from_secs(120);
    let (sender, receiver) = mpsc::channel();
    app.build_started = Some(origin);
    app.build_receiver = Some(receiver);
    let event = |seconds, label: &str, activity, completed| {
        sender
            .send(BuildWorkerEvent::Progress(
                TimedBuildProgress::from_progress(
                    BuildProgress {
                        phase: BuildPhase::BuildingPayloads,
                        current_artifact: Some(label.to_owned()),
                        completed,
                        total: 2,
                        timestamp: origin + Duration::from_secs(seconds),
                        activity: Some(activity),
                    },
                    origin,
                ),
            ))
            .unwrap();
    };
    let package = |id, status, seconds| BuildActivity::Package {
        id,
        step: 0,
        status,
        duration: Duration::from_secs(seconds),
    };
    event(
        10,
        "Preparing Entries: w64_active_058c_1.pkg",
        package(0x058C, OperationStatus::Started, 0),
        0,
    );
    event(
        11,
        "Reading Source Package: w64_done_058d_1.pkg",
        package(0x058D, OperationStatus::Started, 0),
        0,
    );
    event(
        12,
        "",
        BuildActivity::Diagnostic("Runtime cache: no saved fragment".into()),
        0,
    );
    event(
        13,
        "Reading Source Package: w64_done_058d_1.pkg",
        package(0x058D, OperationStatus::Finished, 2),
        1,
    );
    app.poll_build();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let active = copy(&ctx, &mut app, "parallel-build-active");
    assert!(active.contains("[10.0s]") && active.contains("Preparing Entries: w64_active"));
    assert!(!active.contains("Prepared Entries: w64_active"));
    assert!(active.contains("[13.0s]") && active.contains("Read Source Package: w64_done"));
    assert!(
        active.contains("2.0s"),
        "The worker's duration must survive delayed delivery"
    );
    assert!(active.contains("Runtime cache: no saved fragment"));
    assert!(
        !active.contains("2m 0s"),
        "UI polling time is not the event timestamp"
    );

    event(
        14,
        "Preparing Entries: w64_active_058c_1.pkg",
        package(0x058C, OperationStatus::Failed, 4),
        1,
    );
    sender
        .send(BuildWorkerEvent::Finished {
            result: Err("Package preparation failed before staging"
                .to_owned()
                .into()),
            elapsed: Duration::from_secs(14),
        })
        .unwrap();
    app.poll_build();
    let failed = copy(&ctx, &mut app, "parallel-build-failed");
    assert!(failed.contains("[14.0s]") && failed.contains("Failed"));
    assert!(failed.contains("4.0s"));
    assert!(!failed.contains("Prepared Entries: w64_active"));
    assert!(failed.contains("Read Source Package: w64_done"));
    assert_eq!(app.recipe, recipe);
    assert!(!app.account_changed && !app.packages_changed);
    crate::test_support::artifact(
        "parallel-build-progress.json",
        &serde_json::json!({
            "active":active,"failed":failed,
            "producer_seconds":[10,11,12,13,14],"delivery_after_seconds":120,
            "recipe_unchanged":true,"package_writes":false,
            "repeat_filter":"app::build_status::tests::parallel::parallel_workers_keep_their_own_completion_and_worker_timestamps",
            "limits":"Rendered egui dialog and clipboard output with controlled worker events."
        }),
    );
}
