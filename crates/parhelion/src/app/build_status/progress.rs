use super::*;
use crate::install::{InstallPhase, InstallProgress};
mod activity;
mod view;

pub(in crate::app) use activity::Activity;
pub(super) use view::draw_build;
use view::{ProgressView, draw_progress, install_detail};
/// Each build stage's share of step 1, in the build's order. Compiling items and building their
/// packages take most of a build, so they take most of the bar.
const BUILD_STAGES: [(BuildPhase, f32); 13] = [
    (BuildPhase::ReadingRecipes, 1.0),
    (BuildPhase::InspectingSource, 1.0),
    (BuildPhase::LoadingCatalog, 2.0),
    (BuildPhase::CheckingRecipes, 2.0),
    (BuildPhase::PreparingSource, 1.0),
    (BuildPhase::HashingSource, 2.0),
    (BuildPhase::CompilingProject, 60.0),
    (BuildPhase::BuildingPayloads, 20.0),
    (BuildPhase::RecheckingSource, 1.0),
    (BuildPhase::WritingPackages, 4.0),
    (BuildPhase::ValidatingPackages, 3.0),
    (BuildPhase::StagingRecipes, 1.0),
    (BuildPhase::WritingManifest, 1.0),
];

/// The share of what remains of the compile stages that a family pass leaves for the passes after
/// it. Whether another pass follows is known only once a pass ends.
const LATER_PASSES: f32 = 0.15;

/// How far step 1 is, which only moves forward. A family build compiles its mods in turn, each
/// with counters of its own, and can compile them again in a later pass. Each mod's compile fills
/// its share of its pass, by its recipes and maps, and a later pass fills part of what the pass
/// before left of the compile stages.
#[derive(Default)]
pub(in crate::app) struct BuildFraction {
    value: f32,
    /// The pass compiling, and how far through the compile stages the bar was when it started.
    pass: Option<(usize, f32)>,
}

impl BuildFraction {
    pub(in crate::app) fn value(&self) -> f32 {
        self.value
    }

    pub(in crate::app) fn advance(&mut self, progress: &TimedBuildProgress) {
        let estimate = if progress.phase == BuildPhase::Complete {
            1.0
        } else if let Some(member) = progress.member
            && let Some(within) =
                compile_fraction(progress.phase, progress.completed, progress.total)
        {
            let start = match self.pass.filter(|(pass, _)| *pass == member.pass) {
                Some((_, start)) => start,
                None => {
                    let start = self.compiled();
                    self.pass = Some((member.pass, start));
                    start
                }
            };
            let share = (1.0 - start) * (1.0 - LATER_PASSES);
            compile_overall(start + share * member.pass_fraction(within))
        } else {
            // Catalog loading has its own changing substeps, so its local counters cannot
            // estimate the fraction of the entire catalog load.
            let total = if progress.phase == BuildPhase::LoadingCatalog {
                0
            } else {
                progress.total
            };
            step_fraction(progress.phase, progress.completed, total)
        };
        self.value = self.value.max(estimate);
    }

    /// How far through the compile stages the bar is, from 0 to 1.
    fn compiled(&self) -> f32 {
        let (start, span) = compile_span();
        ((self.value - start) / span).clamp(0.0, 1.0)
    }
}

/// Where step 1 is when `completed` of `total` of `phase` is done.
fn step_fraction(phase: BuildPhase, completed: usize, total: usize) -> f32 {
    let all = BUILD_STAGES.iter().map(|(_, weight)| weight).sum::<f32>();
    let mut before = 0.0;
    for (stage, weight) in BUILD_STAGES {
        if stage == phase {
            let within = if total == 0 {
                0.0
            } else {
                completed.min(total) as f32 / total as f32
            };
            return (before + weight * within) / all;
        }
        before += weight;
    }
    0.0
}

/// Where the compile stages start in step 1, and their share of it.
fn compile_span() -> (f32, f32) {
    let start = step_fraction(BuildPhase::CompilingProject, 0, 0);
    (
        start,
        step_fraction(BuildPhase::RecheckingSource, 0, 0) - start,
    )
}

/// How far one mod's compile is through the compile stages, from 0 to 1, or `None` outside them.
fn compile_fraction(phase: BuildPhase, completed: usize, total: usize) -> Option<f32> {
    matches!(
        phase,
        BuildPhase::CompilingProject | BuildPhase::BuildingPayloads
    )
    .then(|| {
        let (start, span) = compile_span();
        (step_fraction(phase, completed, total) - start) / span
    })
}

/// Where step 1 is when the compile stages are `compiled` of the way through.
fn compile_overall(compiled: f32) -> f32 {
    let (start, span) = compile_span();
    start + span * compiled.clamp(0.0, 1.0)
}

#[derive(Default)]
pub(in crate::app) struct InstallStatus {
    pub progress: Option<InstallProgress>,
    pub receiver: Option<Receiver<(InstallProgress, Duration)>>,
    pub started: Option<Instant>,
    pub elapsed: Duration,
    pub activity: Activity,
    pub overall_fraction: f32,
}

impl InstallStatus {
    pub(in crate::app) fn poll(&mut self, log: &mut ActivityLog) {
        while let Some(Ok((progress, elapsed))) = self.receiver.as_ref().map(Receiver::try_recv) {
            if self.progress.as_ref() != Some(&progress)
                && let Some(message) = self.activity.progress(
                    elapsed,
                    progress.phase.label(),
                    progress.current_artifact.as_deref(),
                    (progress.completed, progress.total, "operations"),
                    matches!(
                        progress.phase,
                        InstallPhase::Checking
                            | InstallPhase::ReviewingAccount
                            | InstallPhase::Rechecking
                    ),
                    progress.phase != InstallPhase::RollingBack,
                )
            {
                log.push(LogEntry::info(message));
            }
            self.elapsed = elapsed;
            if progress.phase == InstallPhase::Complete {
                self.overall_fraction = 1.0;
            } else if progress.phase != InstallPhase::RollingBack {
                self.overall_fraction = self.overall_fraction.max(stage_fraction(
                    progress.phase,
                    &InstallPhase::STAGES,
                    progress.completed,
                    progress.total,
                ));
            }
            self.progress = Some(progress);
        }
    }

    pub(in crate::app) fn finish(&mut self) {
        if let Some(started) = self.started.take() {
            self.elapsed = started.elapsed();
        }
        self.receiver = None;
    }

    pub(super) fn draw(&self, ui: &mut egui::Ui, running: bool) {
        let elapsed = self
            .started
            .map_or(self.elapsed, |started| started.elapsed());
        if let Some(progress) = &self.progress {
            let complete = progress.phase == InstallPhase::Complete;
            let info = ProgressView {
                title: progress.phase.label(),
                detail: install_detail(progress.phase),
                complete,
                completed: progress.completed,
                total: progress.total,
                unit: "operations",
                fraction: if progress.total == 0 {
                    0.0
                } else {
                    (progress.completed as f32 / progress.total as f32).clamp(0.0, 1.0)
                },
                item: progress.current_artifact.as_deref(),
                elapsed,
            };
            draw_progress(ui, &info, running);
        } else {
            ui.heading("Starting Installation");
            ui.add(
                sundial::investment::progress_bar(0.0)
                    .animate(running)
                    .text("Waiting for installation checks"),
            );
        }
        ui.add_space(12.0);
        self.activity
            .draw(ui, "install-progress-activity", Some(0.0));
    }
}

fn stage_fraction<P: PartialEq>(phase: P, stages: &[P], completed: usize, total: usize) -> f32 {
    let Some(stage) = stages.iter().position(|candidate| *candidate == phase) else {
        return 0.0;
    };
    let within_stage = if total == 0 {
        0.0
    } else {
        completed.min(total) as f32 / total as f32
    };
    (stage as f32 + within_stage) / stages.len() as f32
}

pub(super) fn draw_steps(ui: &mut egui::Ui, app: &PackageAuthoringApp) {
    let built = matches!(app.latest_build, Some(Ok(_)));
    let installed = matches!(app.latest_install, Some(Ok(_)));
    let active = match app.build_dialog_step {
        BuildDialogStep::Build => 0,
        BuildDialogStep::ReviewInstall => 1,
        BuildDialogStep::Install => 2,
    };
    let build_fraction = app.build_progress.as_ref().map_or(0.0, |progress| {
        if built || progress.phase == BuildPhase::Complete {
            1.0
        } else {
            app.build_fraction.value()
        }
    });
    let install_fraction = app
        .install_status
        .progress
        .as_ref()
        .map_or(0.0, |progress| {
            if installed || progress.phase == InstallPhase::Complete {
                1.0
            } else {
                stage_fraction(
                    progress.phase,
                    &InstallPhase::STAGES,
                    progress.completed,
                    progress.total,
                )
            }
        })
        .max(app.install_status.overall_fraction);
    let fractions = [
        build_fraction,
        if active == 2 { 1.0 } else { 0.0 },
        install_fraction,
    ];
    let running = [
        app.build_receiver.is_some(),
        app.replacement_receiver.is_some(),
        app.install_receiver.is_some(),
    ];
    ui.columns(3, |columns| {
        for (index, (column, label)) in columns
            .iter_mut()
            .zip(["Build", "Review", "Install"])
            .enumerate()
        {
            let done =
                (index == 0 && built) || (index == 1 && active == 2) || (index == 2 && installed);
            let fraction = if done { 1.0 } else { fractions[index] };
            let failed = match index {
                0 => matches!(app.latest_build, Some(Err(_))),
                1 => matches!(app.replacement_review, Some(Err(_))),
                _ => matches!(app.latest_install, Some(Err(_))),
            };
            let restoring = index == 2
                && app
                    .install_status
                    .progress
                    .as_ref()
                    .is_some_and(|progress| progress.phase == InstallPhase::RollingBack);
            let color = if failed {
                column.visuals().error_fg_color
            } else if done {
                style::success_color(column.visuals())
            } else {
                column.visuals().text_color()
            };
            egui::Frame::group(column.style())
                .fill(if index == active {
                    column.visuals().faint_bg_color
                } else {
                    egui::Color32::TRANSPARENT
                })
                .inner_margin(10)
                .show(column, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(format!("{}. {label}", index + 1))
                            .strong()
                            .color(color),
                    );
                    let status = if failed {
                        "Needs Attention".to_owned()
                    } else if restoring {
                        "Restoring…".to_owned()
                    } else if done {
                        "Complete".to_owned()
                    } else if index == 1 && active == 1 {
                        if running[index] {
                            "Checking…"
                        } else {
                            "Review Required"
                        }
                        .to_owned()
                    } else if index == active {
                        if running[index] {
                            format!("{:.0}%", (fraction * 100.0).floor().min(99.0))
                        } else {
                            "Stopped".to_owned()
                        }
                    } else {
                        "Up Next".to_owned()
                    };
                    ui.label(status);
                    ui.add(
                        sundial::investment::progress_bar(fraction)
                            .desired_width(ui.available_width())
                            .desired_height(6.0)
                            .animate(running[index]),
                    );
                });
        }
    });
    ui.add_space(14.0);
}
