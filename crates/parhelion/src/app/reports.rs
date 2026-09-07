use super::*;

pub(super) fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let tenths = elapsed.subsec_millis() / 100;
    if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}.{tenths}s")
    }
}

pub(super) fn draw_build_report(ui: &mut egui::Ui, build: &BuildReport) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.heading(
            egui::RichText::new("Build Validated").color(style::success_color(ui.visuals())),
        );
        ui.strong(format!(
            "{} packages staged for installation",
            build.artifacts.len()
        ));
        for weapon in &build.weapons {
            ui.label(format!(
                "{} · namespace {} · item 0x{:08X}",
                weapon.name, weapon.namespace, weapon.item_hash
            ));
        }
        ui.label(build.run_directory.display().to_string());
        draw_artifact_grid(
            ui,
            "build_artifacts",
            build.artifacts.iter().map(|artifact| {
                (
                    artifact.file_name.as_str(),
                    artifact.byte_length.to_string(),
                    artifact.sha256.as_str(),
                )
            }),
        );
        ui.label(format!("Manifest: {}", build.manifest_path.display()));
        ui.monospace(format!(
            "Selection SHA-256: {}",
            build.selection_fingerprint
        ));
        if !build.staged_recipe_paths.is_empty() {
            ui.label(format!(
                "Recipe snapshot: {}",
                build.run_directory.join("recipes").display()
            ));
        }
    });
}

pub(super) fn draw_install_report(ui: &mut egui::Ui, report: &InstallReport) {
    if let Some(path) = &report.cleaned_account {
        ui.label(format!("Removed the confirmed obsolete items and references from {}. Its original account backup is stored with the package backup and excluded from automatic pruning.", path.display()));
    }
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.heading(
            egui::RichText::new("Packages Installed").color(style::success_color(ui.visuals())),
        );
        ui.label(format!(
            "Installed {} verified files to {}",
            report.artifacts.len(),
            report.target_packages_directory.display()
        ));
        ui.label(format!("Backup: {}", report.backup_directory.display()));
        if let Some(cache) = &report.invalidated_sunrise_cache {
            if let Some(quarantine) = &cache.retained_quarantine_path {
                ui.label(format!(
                    "Sunrise cache invalidated; backup: {}; retained quarantine: {}",
                    cache.backup_path.display(),
                    quarantine.display()
                ));
            } else {
                ui.label(format!(
                    "Sunrise cache invalidated; backup: {}",
                    cache.backup_path.display()
                ));
            }
        } else {
            ui.label("No Sunrise build-data cache needed invalidation");
        }
        if report.invalidated_package_header_caches.is_empty() {
            ui.label("No client package-header cache needed invalidation");
        } else {
            ui.label(format!(
                "Client package-header caches invalidated: {}",
                report.invalidated_package_header_caches.len()
            ));
            for cache in &report.invalidated_package_header_caches {
                let mut message = format!("Backup: {}", cache.backup_path.display());
                if let Some(quarantine) = &cache.retained_quarantine_path {
                    message.push_str(&format!(
                        "; retained quarantine: {}",
                        quarantine.display()
                    ));
                }
                ui.label(message);
            }
        }
        match &report.profile_sync {
            Some(Ok(sync)) => {
                ui.label(format!(
                    "Authored collection unlocks synchronized: {}/{} newly acquired in {}",
                    sync.newly_set_unlocks,
                    sync.total_unlocks,
                    sync.settings_path.display()
                ));
                if let Some(backup) = &sync.backup_path {
                    ui.label(format!("Account backup: {}", backup.display()));
                }
            }
            Some(Err(error)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Packages installed, but authored collection unlock synchronization failed: {error}"
                    ),
                );
            }
            None => {}
        }
        draw_artifact_grid(
            ui,
            "installed_artifacts",
            report.artifacts.iter().map(|artifact| {
                (
                    artifact.file_name.as_str(),
                    artifact.byte_length.to_string(),
                    artifact.sha256.as_str(),
                )
            }),
        );
    });
}

pub(super) fn draw_artifact_grid<'a>(
    ui: &mut egui::Ui,
    id: &str,
    artifacts: impl IntoIterator<Item = (&'a str, String, &'a str)>,
) {
    egui::Grid::new(id)
        .striped(true)
        .num_columns(3)
        .show(ui, |ui| {
            ui.strong("Package");
            ui.strong("Bytes");
            ui.strong("SHA-256");
            ui.end_row();
            for (file_name, byte_length, sha256) in artifacts {
                ui.monospace(file_name);
                ui.label(byte_length);
                ui.add(egui::Label::new(egui::RichText::new(sha256).monospace()).truncate());
                ui.end_row();
            }
        });
}
