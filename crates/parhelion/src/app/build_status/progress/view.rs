//! Progress headings, descriptions and counters for the build and installation views.
use super::*;

pub(in crate::app::build_status) fn draw_build(
    ui: &mut egui::Ui,
    progress: &TimedBuildProgress,
    elapsed: Duration,
    running: bool,
) {
    let complete = progress.phase == BuildPhase::Complete;
    let title = progress.title();
    draw_progress(
        ui,
        &ProgressView {
            title: &title,
            detail: build_detail(progress.phase),
            complete,
            completed: progress.completed,
            total: progress.total,
            unit: progress.phase.unit(progress.total),
            fraction: progress.fraction(),
            item: progress.current_artifact.as_deref(),
            elapsed,
        },
        running,
    );
}

pub(super) struct ProgressView<'a> {
    pub(super) title: &'a str,
    pub(super) detail: &'a str,
    pub(super) complete: bool,
    pub(super) completed: usize,
    pub(super) total: usize,
    pub(super) unit: &'static str,
    pub(super) fraction: f32,
    pub(super) item: Option<&'a str>,
    pub(super) elapsed: Duration,
}

pub(super) fn draw_progress(ui: &mut egui::Ui, info: &ProgressView<'_>, running: bool) {
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
                "{} of {} {} complete",
                info.completed, info.total, info.unit
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

fn build_detail(phase: BuildPhase) -> &'static str {
    match phase {
        BuildPhase::ReadingRecipes => "Loading the selected recipes and their saved inputs.",
        BuildPhase::InspectingSource => "Checking the source packages and the selected recipes.",
        BuildPhase::LoadingCatalog => "Loading the installed item and perk definitions.",
        BuildPhase::CheckingRecipes => {
            "Checking recipes that need donor compatibility checks. Recipes without catalog overrides skip this phase."
        }
        BuildPhase::PreparingSource => "Preparing the stock package set for compilation.",
        BuildPhase::HashingSource => "Checking the game files used by this build.",
        BuildPhase::RecheckingSource => {
            "Confirming the source packages did not change during compilation."
        }
        BuildPhase::CompilingProject => {
            "Compiling item definitions, perks, artwork, Collections entries, and text."
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

pub(super) fn install_detail(phase: InstallPhase) -> &'static str {
    match phase {
        InstallPhase::Checking => {
            "Checking Destiny 2, recovery state, runtime support, and staged package integrity."
        }
        InstallPhase::ReviewingAccount => {
            "Comparing installed and incoming items, sockets, equipment slots, and account references."
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
            "Synchronizing authored item unlocks with the selected account."
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
