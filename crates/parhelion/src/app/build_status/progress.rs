use super::*;
use crate::install::{InstallPhase, InstallProgress};
use std::collections::VecDeque;

const MAX_EVENTS: usize = 2_000;
const BUILD_STAGES: [BuildPhase; 12] = [
    BuildPhase::InspectingSource,
    BuildPhase::LoadingCatalog,
    BuildPhase::CheckingRecipes,
    BuildPhase::PreparingSource,
    BuildPhase::HashingSource,
    BuildPhase::CompilingProject,
    BuildPhase::BuildingPayloads,
    BuildPhase::RecheckingSource,
    BuildPhase::WritingPackages,
    BuildPhase::ValidatingPackages,
    BuildPhase::StagingRecipes,
    BuildPhase::WritingManifest,
];

#[derive(Default)]
pub(in crate::app) struct Activity {
    lines: VecDeque<String>,
}

impl Activity {
    pub(in crate::app) fn update_last(&mut self, elapsed: Duration, message: String) {
        if let Some(line) = self.lines.back_mut() {
            *line = format!("[{}] {message}", format_elapsed(elapsed));
        } else {
            self.push(elapsed, message);
        }
    }

    pub(in crate::app) fn push(&mut self, elapsed: Duration, message: String) {
        if self.lines.len() == MAX_EVENTS {
            self.lines.pop_front();
        }
        self.lines
            .push_back(format!("[{}] {message}", format_elapsed(elapsed)));
    }

    pub(super) fn draw(&self, ui: &mut egui::Ui, id: &'static str) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Activity");
            ui.weak(format!("{} recent events", self.lines.len()));
            if ui
                .add_enabled(!self.lines.is_empty(), egui::Button::new("Copy Progress"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(self.lines.iter().cloned().collect::<Vec<_>>().join("\n"));
            }
        });
        egui::Frame::group(ui.style())
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .max_height(180.0)
                    .auto_shrink([false, true])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.lines.is_empty() {
                            ui.weak("Waiting for the worker to report its first operation.");
                        }
                        for line in &self.lines {
                            ui.add(egui::Label::new(line).wrap());
                        }
                    });
            });
    }
}

pub(in crate::app) fn message(
    label: &str,
    item: Option<&str>,
    completed: usize,
    total: usize,
) -> String {
    let count = if total > 0 {
        format!(" ({}/{total})", completed.min(total))
    } else {
        String::new()
    };
    format!(
        "{label}{count}{}",
        item.map_or_else(String::new, |item| format!(": {item}"))
    )
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
            if self.progress.as_ref() != Some(&progress) {
                let message = message(
                    progress.phase.label(),
                    progress.current_artifact.as_deref(),
                    progress.completed,
                    progress.total,
                );
                let same_operation = self.progress.as_ref().is_some_and(|previous| {
                    previous.phase == progress.phase
                        && previous.current_artifact == progress.current_artifact
                        && previous.total == progress.total
                        && previous.completed <= progress.completed
                });
                if same_operation {
                    self.activity.update_last(elapsed, message);
                } else {
                    self.activity.push(elapsed, message.clone());
                    log.push(LogEntry::info(message));
                }
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
        self.activity.draw(ui, "install-progress-activity");
    }
}

pub(super) fn draw_build(
    ui: &mut egui::Ui,
    progress: &TimedBuildProgress,
    elapsed: Duration,
    running: bool,
) {
    let complete = progress.phase == BuildPhase::Complete;
    draw_progress(
        ui,
        &ProgressView {
            title: build_title(progress.phase),
            detail: build_detail(progress.phase),
            complete,
            completed: progress.completed,
            total: progress.total,
            fraction: progress.fraction(),
            item: progress.current_artifact.as_deref(),
            elapsed,
        },
        running,
    );
}

struct ProgressView<'a> {
    title: &'a str,
    detail: &'a str,
    complete: bool,
    completed: usize,
    total: usize,
    fraction: f32,
    item: Option<&'a str>,
    elapsed: Duration,
}

fn draw_progress(ui: &mut egui::Ui, info: &ProgressView<'_>, running: bool) {
    ui.heading(info.title);
    ui.label(info.detail);
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("Current Operation");
        ui.weak(format!("Elapsed {}", format_elapsed(info.elapsed)));
    });
    if !info.complete {
        if let Some(item) = info.item {
            ui.add(egui::Label::new(item).wrap());
        }
        let text = if info.total <= 1 {
            if running { "Working…" } else { "Stopped" }.to_owned()
        } else {
            format!(
                "{} of {} operations complete",
                info.completed.min(info.total),
                info.total
            )
        };
        ui.add(
            sundial::investment::progress_bar(info.fraction)
                .animate(running)
                .text(text),
        );
    }
    if running {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

fn build_title(phase: BuildPhase) -> &'static str {
    match phase {
        BuildPhase::InspectingSource => "Inspecting Source Packages",
        BuildPhase::LoadingCatalog => "Loading Donor Catalog",
        BuildPhase::CheckingRecipes => "Checking Recipe Compatibility",
        BuildPhase::PreparingSource => "Preparing Source Packages",
        BuildPhase::HashingSource => "Recording Source Checksums",
        BuildPhase::CompilingProject => "Compiling Weapons",
        BuildPhase::BuildingPayloads => "Building Package Payloads",
        BuildPhase::RecheckingSource => "Rechecking Source Packages",
        BuildPhase::WritingPackages => "Writing Packages",
        BuildPhase::ValidatingPackages => "Validating Packages",
        BuildPhase::StagingRecipes => "Saving Recipe Snapshots",
        BuildPhase::WritingManifest => "Writing Build Manifest",
        BuildPhase::Complete => "Build Validated",
    }
}

fn build_detail(phase: BuildPhase) -> &'static str {
    match phase {
        BuildPhase::InspectingSource => "Checking the source packages and the selected recipes.",
        BuildPhase::LoadingCatalog => "Loading the installed weapon and perk definitions.",
        BuildPhase::CheckingRecipes => "Checking each recipe against its selected donors.",
        BuildPhase::PreparingSource => "Preparing the stock package set for compilation.",
        BuildPhase::HashingSource => "Recording the original package checksums.",
        BuildPhase::RecheckingSource => {
            "Confirming the source packages did not change during compilation."
        }
        BuildPhase::CompilingProject => {
            "Compiling weapon definitions, perks, artwork, Collections entries, and text."
        }
        BuildPhase::BuildingPayloads => "Preparing, encoding, and validating each package payload.",
        BuildPhase::WritingPackages => {
            "Writing the compiled package set to a separate staging folder."
        }
        BuildPhase::ValidatingPackages => {
            "Checking each package and recording its verified size and checksum."
        }
        BuildPhase::StagingRecipes => "Saving the exact recipes used for this build.",
        BuildPhase::WritingManifest => {
            "Recording the package set and build selection for installation review."
        }
        BuildPhase::Complete => "Packages are staged and ready for installation review.",
    }
}

fn install_detail(phase: InstallPhase) -> &'static str {
    match phase {
        InstallPhase::Checking => {
            "Checking Destiny 2, recovery state, runtime support, and staged package integrity."
        }
        InstallPhase::ReviewingAccount => {
            "Comparing installed and incoming weapons, sockets, equipment slots, and account references."
        }
        InstallPhase::BackingUp => {
            "Saving existing packages, game caches, and recipe snapshots before replacement."
        }
        InstallPhase::Preparing => {
            "Copying and verifying temporary files beside the installation target."
        }
        InstallPhase::Rechecking => {
            "Checking for concurrent changes and saving the recovery record before installation."
        }
        InstallPhase::UpdatingAccount => {
            "Applying only the account changes shown in your installation review."
        }
        InstallPhase::Installing => {
            "Replacing the reviewed package set and removing obsolete authored packages."
        }
        InstallPhase::Verifying => {
            "Comparing installed file sizes and checksums with the verified build."
        }
        InstallPhase::RefreshingCaches => {
            "Refreshing game caches and verifying the account transaction."
        }
        InstallPhase::Finalizing => {
            "Saving the committed transaction so recovery can recognize the installed set."
        }
        InstallPhase::SyncingCollections => {
            "Synchronizing authored weapon unlocks with the selected account."
        }
        InstallPhase::CleaningUp => {
            "Finalizing the recovery backup and applying automatic backup retention."
        }
        InstallPhase::Complete => {
            "The package transaction finished. See the result for any account or cleanup warnings."
        }
        InstallPhase::RollingBack => {
            "An operation failed. Restoring the previous files and account from the recovery backup."
        }
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
            // Catalog loading has its own changing substeps, so its local counters cannot
            // estimate the fraction of the entire catalog load.
            let total = if progress.phase == BuildPhase::LoadingCatalog {
                0
            } else {
                progress.total
            };
            stage_fraction(progress.phase, &BUILD_STAGES, progress.completed, total)
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
