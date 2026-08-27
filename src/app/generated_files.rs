use std::{fs, path::PathBuf};

use eframe::egui;

use super::{SundialApp, settings};
use crate::orbit_map;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileSaveAction {
    Save,
    SaveAndExit,
    ResetDefaults,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileDecision {
    Ask,
    Replace,
    KeepExisting,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedFileKind {
    OrbitMap,
}

impl GeneratedFileKind {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::OrbitMap => "Orbit map",
        }
    }

    pub(super) const fn file_name(self) -> &'static str {
        match self {
            Self::OrbitMap => "orbit_map.txt",
        }
    }
}

pub(super) struct PendingGeneratedFile {
    pub(super) kind: GeneratedFileKind,
    pub(super) path: PathBuf,
    pub(super) existing: String,
    pub(super) generated: String,
    pub(super) diff: String,
    pub(super) action: GeneratedFileSaveAction,
}

pub(super) enum GeneratedFilePlan {
    Current(GeneratedFileKind, PathBuf),
    Write(GeneratedFileKind, String),
    KeepExisting(GeneratedFileKind, PathBuf),
}

impl SundialApp {
    pub(super) fn prepare_generated_files(
        &mut self,
        orbit_supported: bool,
        action: GeneratedFileSaveAction,
    ) -> Result<Option<Vec<GeneratedFilePlan>>, String> {
        let mut files = Vec::new();
        if orbit_supported {
            files.push((
                GeneratedFileKind::OrbitMap,
                orbit_map::path(&self.settings_path)?,
                orbit_map::document(self.manifest.orbit_map_entries()),
            ));
        }
        let mut plans = Vec::with_capacity(files.len());
        for (kind, path, generated) in files {
            let decision = self
                .generated_file_decisions
                .iter()
                .rev()
                .find_map(|(saved_kind, decision)| (*saved_kind == kind).then_some(*decision))
                .unwrap_or(GeneratedFileDecision::Ask);
            match decision {
                GeneratedFileDecision::Replace => {
                    plans.push(GeneratedFilePlan::Write(kind, generated));
                }
                GeneratedFileDecision::KeepExisting => {
                    plans.push(GeneratedFilePlan::KeepExisting(kind, path));
                }
                GeneratedFileDecision::Ask => match fs::read(&path) {
                    Ok(raw) => {
                        let existing = String::from_utf8_lossy(&raw).into_owned();
                        if normalized_generated_document(&existing)
                            == normalized_generated_document(&generated)
                        {
                            plans.push(GeneratedFilePlan::Current(kind, path));
                        } else {
                            let diff = generated_file_diff(kind.file_name(), &existing, &generated);
                            self.pending_generated_file = Some(PendingGeneratedFile {
                                kind,
                                path: path.clone(),
                                existing,
                                generated,
                                diff,
                                action,
                            });
                            self.set_status(
                                format!(
                                    "Save paused: {} differs from Sundial's package-generated {}",
                                    path.display(),
                                    kind.label()
                                ),
                                false,
                            );
                            return Ok(None);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        plans.push(GeneratedFilePlan::Write(kind, generated));
                    }
                    Err(error) => {
                        return Err(format!("Could not read {}: {error}", path.display()));
                    }
                },
            }
        }
        Ok(Some(plans))
    }

    pub(super) fn complete_generated_file_plans(
        &self,
        plans: Vec<GeneratedFilePlan>,
    ) -> Result<String, String> {
        let mut note = String::new();
        for plan in plans {
            match plan {
                GeneratedFilePlan::Current(kind, path) => {
                    note.push_str(&format!(" {} unchanged: {}.", kind.label(), path.display()))
                }
                GeneratedFilePlan::KeepExisting(kind, path) => note.push_str(&format!(
                    " Existing {} kept: {}.",
                    kind.label(),
                    path.display()
                )),
                GeneratedFilePlan::Write(kind, document) => {
                    let path = match kind {
                        GeneratedFileKind::OrbitMap => {
                            orbit_map::save(&self.settings_path, &document)?
                        }
                    };
                    note.push_str(&format!(" {}: {}.", kind.label(), path.display()));
                }
            }
        }
        Ok(note)
    }

    pub(super) fn resume_generated_file_action(
        &mut self,
        ctx: &egui::Context,
        action: GeneratedFileSaveAction,
        kind: GeneratedFileKind,
        decision: GeneratedFileDecision,
    ) {
        self.generated_file_decisions
            .retain(|(saved_kind, _)| *saved_kind != kind);
        if decision != GeneratedFileDecision::Ask {
            self.generated_file_decisions.push((kind, decision));
        }
        match action {
            GeneratedFileSaveAction::Save => {
                let _ = self.save_with_generated_files(action);
            }
            GeneratedFileSaveAction::SaveAndExit => {
                let safe_to_close = self.save_with_generated_files(action);
                if !self.has_unsaved_changes() && safe_to_close {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            GeneratedFileSaveAction::ResetDefaults => {
                self.reset_to_sunrise_defaults_with_generated_files();
            }
        }
    }
}

pub(super) fn settings_size_note(result: &settings::SaveJsonResult) -> String {
    let limit = settings_size_label(result.size_limit_bytes);
    if result.exceeds_size_limit {
        format!(
            " Warning: the compacted file is {} bytes, above this Sunrise schema's {limit} settings limit, and may not load.",
            result.encoded_bytes,
        )
    } else if result.compacted {
        format!(
            " Sunrise-style formatting exceeded this schema's {limit} limit, so Sundial compacted the file to {} bytes.",
            result.encoded_bytes,
        )
    } else {
        String::new()
    }
}

pub(super) fn normalized_generated_document(document: &str) -> String {
    document
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end_matches('\n')
        .to_owned()
}

pub(super) fn generated_file_diff(file_name: &str, existing: &str, generated: &str) -> String {
    let existing = normalized_generated_document(existing);
    let generated = normalized_generated_document(generated);
    let before = existing.lines().collect::<Vec<_>>();
    let after = generated.lines().collect::<Vec<_>>();
    let mut common = vec![vec![0_usize; after.len() + 1]; before.len() + 1];
    for before_index in (0..before.len()).rev() {
        for after_index in (0..after.len()).rev() {
            common[before_index][after_index] = if before[before_index] == after[after_index] {
                common[before_index + 1][after_index + 1] + 1
            } else {
                common[before_index + 1][after_index].max(common[before_index][after_index + 1])
            };
        }
    }

    let mut diff = format!("--- Existing {file_name}\n+++ Package-generated {file_name}\n");
    let (mut before_index, mut after_index) = (0, 0);
    while before_index < before.len() || after_index < after.len() {
        if before_index < before.len()
            && after_index < after.len()
            && before[before_index] == after[after_index]
        {
            diff.push_str("  ");
            diff.push_str(before[before_index]);
            before_index += 1;
            after_index += 1;
        } else if after_index == after.len()
            || (before_index < before.len()
                && common[before_index + 1][after_index] >= common[before_index][after_index + 1])
        {
            diff.push_str("- ");
            diff.push_str(before[before_index]);
            before_index += 1;
        } else {
            diff.push_str("+ ");
            diff.push_str(after[after_index]);
            after_index += 1;
        }
        diff.push('\n');
    }
    diff
}

pub(super) fn settings_size_label(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    if bytes % MIB == 0 {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{} KiB", bytes / KIB)
    }
}
