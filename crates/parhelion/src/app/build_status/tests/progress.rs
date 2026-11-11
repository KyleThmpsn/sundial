//! Build activity must retain hidden diagnostics, preserve names and progress units, copy the
//! selected view and keep the newest events when a large history reaches its limit.
use super::*;
use crate::workflow::BuildActivity;

fn event(
    phase: BuildPhase,
    item: Option<&str>,
    counts: (usize, usize),
    activity: Option<BuildActivity>,
) -> BuildWorkerEvent {
    BuildWorkerEvent::Progress(TimedBuildProgress {
        phase,
        current_artifact: item.map(str::to_owned),
        completed: counts.0,
        total: counts.1,
        elapsed: Duration::from_secs(3),
        activity,
        member: None,
    })
}

fn draw(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(ctx, app, Vec::new());
        sundial::test_support::capture::record(&output);
    }
    output
}

fn press(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    label: &str,
) -> Vec<egui::OutputCommand> {
    let output = draw(ctx, app);
    let at = crate::test_support::driver::accessible(&output, label)
        .unwrap_or_else(|| panic!("No {label} control"))
        .center();
    let mut commands = Vec::new();
    for events in crate::test_support::driver::tap(at) {
        let output = frame(ctx, app, events);
        sundial::test_support::capture::record(&output);
        commands.extend(output.platform_output.commands);
    }
    commands
}

fn copy(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> String {
    press(ctx, app, "Copy Progress")
        .into_iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        })
        .expect("Copy Progress writes to the clipboard")
}

fn artifacts() -> PathBuf {
    let path = crate::test_support::artifacts("build-progress")
        .unwrap_or_else(|| tempfile::tempdir().unwrap().keep());
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn check_summary(text: &str, name: &str, warning: &str) {
    assert!(text.contains("2/550 recipes complete"));
    assert!(text.contains("1/2 input checks complete"));
    assert!(text.contains(warning));
    assert!(text.contains(name));
    assert!(!text.contains("Runtime timings:"));
    assert!(!text.contains("Runtime entries:"));
}

#[test]
fn worker_progress_keeps_diagnostics_available_behind_the_dialog_toggle() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let (sender, receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        build_receiver: Some(receiver),
        ..Default::default()
    };
    let name = "Rufus's Fury (Adept)";
    let timing = "Runtime timings: compilation 6.066s, cache save 0.246s";
    let entries = "Runtime entries: 180 in 81D4328B-81D43324, 821FF202-821FF21B";
    let warning = "Could not save an optional cache file. The build is continuing.";
    for message in [
        event(BuildPhase::ReadingRecipes, Some(name), (2, 550), None),
        event(
            BuildPhase::InspectingSource,
            Some("Checking Recipe Inputs"),
            (1, 2),
            None,
        ),
        event(BuildPhase::CompilingProject, Some(name), (1, 1669), None),
        event(
            BuildPhase::CompilingProject,
            None,
            (1, 1669),
            Some(BuildActivity::RuntimeDetail(timing.into())),
        ),
        event(
            BuildPhase::CompilingProject,
            None,
            (1, 1669),
            Some(BuildActivity::RuntimeDetail(entries.into())),
        ),
        event(
            BuildPhase::CompilingProject,
            None,
            (1, 1669),
            Some(BuildActivity::Diagnostic(warning.into())),
        ),
    ] {
        sender.send(message).unwrap();
    }
    app.poll_build();
    let output = draw(&ctx, &mut app);
    let painted = crate::test_support::driver::painted_text(&output);
    assert!(painted.contains("1 of 1669 build steps complete"));
    assert!(painted.contains(name));
    assert!(!painted.contains(timing));
    sundial::test_support::capture::write(&ctx, &output, "build-progress-summary");
    let summary = copy(&ctx, &mut app);
    check_summary(&summary, name, warning);

    press(&ctx, &mut app, "Show Detailed Runtime Output");
    let details = copy(&ctx, &mut app);
    assert!(details.contains(timing));
    assert!(details.contains(entries));
    assert!(details.contains(warning));
    assert!(details.find(timing) < details.find(entries));
    let output = draw(&ctx, &mut app);
    sundial::test_support::capture::write(&ctx, &output, "build-progress-details");
    press(&ctx, &mut app, "Show Detailed Runtime Output");
    assert_eq!(copy(&ctx, &mut app), summary);
    press(&ctx, &mut app, "Show Detailed Runtime Output");
    app.build_status_open = false;
    frame(&ctx, &mut app, Vec::new());
    app.build_status_open = true;
    assert_eq!(copy(&ctx, &mut app), details);

    let out = artifacts();
    std::fs::write(out.join("summary.txt"), summary).unwrap();
    std::fs::write(out.join("runtime-details.txt"), details).unwrap();
    std::fs::write(
        out.join("filtering.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "recipe": name, "warnings_visible": true, "details_default": false,
            "toggle_round_trip": true, "dialog_reopen": true,
            "capture_summary": "build-progress-summary", "capture_details": "build-progress-details"
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!("Build progress artifacts: {}", out.display());
}

#[test]
fn a_large_worker_history_retains_and_copies_the_newest_events() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let (sender, receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        build_receiver: Some(receiver),
        ..Default::default()
    };
    sender
        .send(event(
            BuildPhase::CompilingProject,
            Some("Current Operation"),
            (0, 2),
            None,
        ))
        .unwrap();
    for index in 0..100_010 {
        sender
            .send(event(
                BuildPhase::CompilingProject,
                None,
                (0, 0),
                Some(BuildActivity::Diagnostic(format!(
                    "Retained Build Event {index}"
                ))),
            ))
            .unwrap();
    }
    app.poll_build();
    sender
        .send(event(
            BuildPhase::CompilingProject,
            Some("Current Operation"),
            (1, 2),
            None,
        ))
        .unwrap();
    app.poll_build();
    draw(&ctx, &mut app);
    let text = copy(&ctx, &mut app);
    assert_eq!(text.lines().count(), 100_000);
    assert!(
        text.lines()
            .next()
            .unwrap()
            .ends_with("Retained Build Event 11")
    );
    assert!(text.lines().last().unwrap().ends_with("Current Operation"));
    assert!(text.contains("Retained Build Event 50000"));
    let out = artifacts();
    std::fs::write(out.join("large-history.txt"), &text).unwrap();
    std::fs::write(
        out.join("retention.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "received_events": 100012, "copied_events": text.lines().count(),
            "first": text.lines().next(), "last": text.lines().last()
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!("Large history artifacts: {}", out.display());
}
