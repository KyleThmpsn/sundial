use super::*;

#[cfg(test)]
mod tests;

pub(super) enum LibraryAction {
    Import,
    ExportOpen,
    ChooseExport,
    ExportAll,
    ExportSelected,
    Entry(PathBuf, EntryAction),
}

#[derive(Clone, Copy)]
pub(super) enum EntryAction {
    Open,
    Duplicate,
    Export,
    OpenFolder,
    CopyPath,
    Restore,
}

pub(super) fn entry_menu(
    ui: &mut egui::Ui,
    entry: &RecipeLibraryEntry,
    can_restore: bool,
) -> Option<EntryAction> {
    let mut action = None;
    ui.weak(&entry.name);
    ui.separator();
    for (label, choice) in [
        ("Open", EntryAction::Open),
        ("Duplicate", EntryAction::Duplicate),
        ("Export…", EntryAction::Export),
    ] {
        if ui.button(label).clicked() {
            action = Some(choice);
            ui.close_menu();
        }
    }
    ui.separator();
    for (label, choice) in [
        ("Open Recipe Folder", EntryAction::OpenFolder),
        ("Copy File Path", EntryAction::CopyPath),
    ] {
        if ui.button(label).clicked() {
            action = Some(choice);
            ui.close_menu();
        }
    }
    if entry.bundled {
        ui.separator();
        if ui
            .add_enabled(can_restore, egui::Button::new("Restore This Recipe…"))
            .on_hover_text(
                "Restore this bundled recipe with a backup. Save or discard its open edits first.",
            )
            .clicked()
        {
            action = Some(EntryAction::Restore);
            ui.close_menu();
        }
    }
    action
}

impl PackageAuthoringApp {
    pub(super) fn apply_library_action(&mut self, ctx: &egui::Context, action: LibraryAction) {
        match action {
            LibraryAction::Import => {
                self.import_library_files(ctx);
            }
            LibraryAction::ExportOpen => self.export_recipe(),
            LibraryAction::ChooseExport => {
                self.library_state.export_selection = Some(BTreeSet::new());
            }
            LibraryAction::ExportAll => {
                self.export_library_bundle(
                    ctx,
                    self.recipe_entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect(),
                );
            }
            LibraryAction::ExportSelected => {
                if let Some(selected) = &self.library_state.export_selection {
                    self.export_library_bundle(ctx, selected.clone());
                }
            }
            LibraryAction::Entry(path, EntryAction::Open) => {
                self.library_open = false;
                self.library_state.highlighted.remove(&path);
                self.request_recipe_action(PendingRecipeAction::Open(path));
            }
            LibraryAction::Entry(path, EntryAction::CopyPath) => {
                ctx.copy_text(path.display().to_string());
                self.log.push(LogEntry::info("Copied recipe file path"));
            }
            LibraryAction::Entry(path, EntryAction::OpenFolder) => {
                if let Some(parent) = path.parent()
                    && let Err(error) = open_directory(parent)
                {
                    self.log.push(LogEntry::error(error));
                }
            }
            LibraryAction::Entry(path, EntryAction::Restore) => {
                let result = self
                    .recipe_library
                    .as_ref()
                    .ok_or("Recipe library is unavailable".to_owned())
                    .and_then(|library| library.prepare_restore_recipe(&path));
                match result {
                    Ok(preview) => self.library_state.restore = Some(preview),
                    Err(error) => self.report_library_error(error),
                }
            }
            LibraryAction::Entry(path, action) => {
                if let Err(error) = self.transfer_library_entry(&path, action) {
                    self.report_library_error(error);
                }
            }
        }
    }

    fn transfer_library_entry(&mut self, source: &Path, action: EntryAction) -> Result<(), String> {
        if matches!(action, EntryAction::Export) && self.recipe_path.as_deref() == Some(source) {
            self.export_recipe();
            return Ok(());
        }
        let recipe = if self.recipe_path.as_deref() == Some(source) {
            if let Some((_, error)) = &self.invalid_weapon_name {
                return Err(error.clone());
            }
            self.recipe.clone()
        } else {
            WeaponRecipe::load_json(source).map_err(|error| error.to_string())?
        };
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("Recipe library is unavailable")?;
        match action {
            EntryAction::Duplicate => {
                let copy = library.duplicate(&recipe)?;
                self.refresh_recipe_library();
                self.reveal_library_entries(vec![copy.clone()]);
                self.library_state.errors.clear();
                self.library_state.notice = Some(format!("Created a copy of {}.", recipe.name));
                self.log.push(LogEntry::info(format!(
                    "Created recipe copy {}",
                    copy.display()
                )));
            }
            EntryAction::Export => {
                let Some(destination) = rfd::FileDialog::new()
                    .set_title(format!("Export {}", recipe.name))
                    .add_filter("Parhelion Weapon Recipe", &["json"])
                    .set_file_name(format!("{}.parhelion.json", recipe.slug()))
                    .save_file()
                else {
                    return Ok(());
                };
                library.export(&recipe, &destination)?;
                self.library_state.notice = Some(format!("Exported {}.", recipe.name));
                self.library_state.errors.clear();
                self.log.push(LogEntry::info(format!(
                    "Exported recipe {}",
                    destination.display()
                )));
            }
            _ => unreachable!("non-transfer actions are handled before loading a recipe"),
        }
        Ok(())
    }
}
