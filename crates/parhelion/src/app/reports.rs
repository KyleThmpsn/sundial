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
        ui.add_space(6.0);
        egui::Grid::new("build_weapon_names")
            .num_columns(2)
            .spacing([24.0, 5.0])
            .show(ui, |ui| {
                for (index, weapon) in build.weapons.iter().enumerate() {
                    ui.label(&weapon.name);
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
                if build.weapons.len() % 2 == 1 {
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        ui.label("Review the installation to see package and account changes before installing.");
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Package Details")
            .id_salt("build_package_details")
            .show(ui, |ui| draw_build_details(ui, build));
    });
}

pub(super) fn draw_build_details(ui: &mut egui::Ui, build: &BuildReport) {
    path_row(ui, "Staging Folder", &build.run_directory);
    draw_artifact_grid(
        ui,
        "build_artifacts",
        build.artifacts.iter().map(|artifact| {
            (
                artifact.file_name.as_str(),
                format_file_size(artifact.byte_length),
                artifact.sha256.as_str(),
            )
        }),
    );
    path_row(ui, "Manifest", &build.manifest_path);
    ui.label("Selection Fingerprint").on_hover_ui(|ui| {
        ui.monospace(&build.selection_fingerprint);
        if ui.button("Copy Fingerprint").clicked() {
            ui.ctx().copy_text(build.selection_fingerprint.clone());
        }
    });
    if !build.staged_recipe_paths.is_empty() {
        path_row(ui, "Recipe Snapshot", &build.run_directory.join("recipes"));
    }
}

fn format_file_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (KIB * KIB))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn draw_install_report(ui: &mut egui::Ui, report: &InstallReport) -> bool {
    ui.heading(egui::RichText::new("Packages Installed").color(style::success_color(ui.visuals())));
    ui.strong(format!(
        "{} verified packages installed",
        report.artifacts.len()
    ));
    if report.cleaned_account.is_some() {
        ui.label(
            "Applied the reviewed item, socket selection, and reference changes to this account.",
        );
    }
    match &report.profile_sync {
        Some(Ok(sync)) => {
            ui.label(format!(
                "Collections updated: {} unlocks available, {} newly acquired.",
                sync.total_unlocks, sync.newly_set_unlocks
            ));
        }
        Some(Err(error)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Packages installed, but Collections could not be updated: {error}"),
            );
        }
        None => {}
    }
    if let Some(warning) = &report.backup_prune_warning {
        ui.colored_label(ui.visuals().warn_fg_color, warning);
    }
    ui.add_space(6.0);
    ui.label(if report.cleaned_account.is_some() {
        "The account and package backup is protected from automatic cleanup."
    } else {
        "A backup was saved before installation."
    });
    let open_backup = ui.button("Open Backup Folder").clicked();
    ui.add_space(6.0);
    egui::CollapsingHeader::new("Installation Details").show(ui, |ui| {
        path_row(ui, "Installed To", &report.target_packages_directory);
        path_row(ui, "Backup Folder", &report.backup_directory);
        if let Some(path) = &report.cleaned_account {
            path_row(ui, "Account", path);
        }
        if let Some(Ok(sync)) = &report.profile_sync {
            if report.cleaned_account.as_ref() != Some(&sync.settings_path) {
                path_row(ui, "Account", &sync.settings_path);
            }
        }
        if let Some(cache) = &report.invalidated_sunrise_cache {
            ui.label("Sunrise build-data cache refreshed.");
            if let Some(path) = &cache.retained_quarantine_path {
                path_row(ui, "Retained Cache", path);
            }
        }
        if !report.invalidated_package_header_caches.is_empty() {
            ui.label(format!(
                "{} client package-header caches refreshed.",
                report.invalidated_package_header_caches.len()
            ));
            for cache in &report.invalidated_package_header_caches {
                if let Some(path) = &cache.retained_quarantine_path {
                    path_row(ui, "Retained Cache", path);
                }
            }
        }
        draw_artifact_grid(
            ui,
            "installed_artifacts",
            report.artifacts.iter().map(|artifact| {
                (
                    artifact.file_name.as_str(),
                    format_file_size(artifact.byte_length),
                    artifact.sha256.as_str(),
                )
            }),
        );
    });
    open_backup
}

/// Display normalization only. Keep canonical paths for file operations.
pub(super) fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(tail) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{tail}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}

pub(super) fn path_row(ui: &mut egui::Ui, label: &str, path: &Path) {
    let display = display_path(path);
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.add(egui::Label::new(&display).truncate())
            .on_hover_ui(|ui| {
                ui.label(&display);
                if ui.button("Copy Path").clicked() {
                    ui.ctx().copy_text(display.clone());
                }
            });
    });
}

pub(super) fn draw_artifact_grid<'a>(
    ui: &mut egui::Ui,
    id: &str,
    artifacts: impl IntoIterator<Item = (&'a str, String, &'a str)>,
) {
    egui::Grid::new(id)
        .striped(true)
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            ui.strong("Package");
            ui.strong("Size");
            ui.end_row();
            for (file_name, byte_length, sha256) in artifacts {
                ui.monospace(file_name).on_hover_ui(|ui| {
                    ui.strong("SHA-256");
                    ui.monospace(sha256);
                    if ui.button("Copy SHA-256").clicked() {
                        ui.ctx().copy_text(sha256.to_owned());
                    }
                });
                ui.label(byte_length);
                ui.end_row();
            }
        });
}

#[test]
fn display_paths_remove_windows_prefix_without_changing_network_roots() {
    for (input, expected) in [
        (
            r"\\?\C:\Games\Destiny2\settings.json",
            r"C:\Games\Destiny2\settings.json",
        ),
        (
            r"\\?\UNC\server\share\settings.json",
            r"\\server\share\settings.json",
        ),
        (
            r"C:\Games\Destiny2\settings.json",
            r"C:\Games\Destiny2\settings.json",
        ),
        ("/home/player/settings.json", "/home/player/settings.json"),
    ] {
        assert_eq!(display_path(Path::new(input)), expected);
    }
}
