//! Recipe document lifecycle and explicit persistent-storage integration.
use super::*;

impl PackageAuthoringApp {
    /// Persistent storage belongs to the desktop open action, not UI construction.
    pub(super) fn initialize_storage(&mut self) {
        if self.recipe_library.is_some() {
            return;
        }
        let mut log = ActivityLog::new(LogEntry::info(format!(
            "Parhelion {} session started",
            env!("CARGO_PKG_VERSION")
        )));
        log.enable_file();
        log.push(LogEntry::info("Opening recipe library"));
        let mut recipe_entries = Vec::new();
        let mut enabled_recipe_paths = BTreeSet::new();
        let backup_preferences = match ParhelionPreferences::load_default() {
            Ok(preferences) => preferences,
            Err(error) => {
                log.push(LogEntry::error(format!(
                    "Could not load Parhelion backup preferences; using defaults: {error}"
                )));
                ParhelionPreferences::default()
            }
        };
        let recipe_library = match RecipeLibrary::open_default() {
            Ok(library) => {
                match library.scan() {
                    Ok(scan) => {
                        for error in scan.errors {
                            log.push(LogEntry::error(format!(
                                "Recipe library entry could not be loaded: {error}"
                            )));
                        }
                        recipe_entries = scan.entries;
                        match library.enabled_paths(&recipe_entries) {
                            Ok(enabled) => enabled_recipe_paths = enabled,
                            Err(error) => log.push(LogEntry::error(format!(
                                "Recipe selection state could not be loaded: {error}"
                            ))),
                        }
                    }
                    Err(error) => log.push(LogEntry::error(error)),
                }
                Some(library)
            }
            Err(error) => {
                log.push(LogEntry::error(format!(
                    "Recipe library is unavailable: {error}"
                )));
                None
            }
        };

        self.recipe_library = recipe_library;
        self.recipe_entries = recipe_entries;
        self.enabled_recipe_paths = enabled_recipe_paths;
        self.limit_package_backups = backup_preferences.limit_package_backups;
        self.package_backup_retention = backup_preferences.package_backup_retention;
        self.backup_recipe_snapshots = backup_preferences.backup_recipe_snapshots;
        self.log = log;
    }

    pub(super) fn batch_request(&self) -> Result<BatchBuildRequest, String> {
        if self.current_recipe_is_in_build()
            && let Some((_, error)) = &self.invalid_weapon_name
        {
            return Err(error.clone());
        }
        let mut recipes = Vec::with_capacity(self.enabled_recipe_paths.len());
        for path in &self.enabled_recipe_paths {
            let recipe = if self.recipe_path.as_ref() == Some(path) {
                let saved = WeaponRecipe::load_json(path).map_err(|error| {
                    format!(
                        "Could not check included recipe {}: {error}",
                        path.display()
                    )
                })?;
                if saved != self.recipe_baseline {
                    return Err(format!(
                        "{} changed on disk after you opened it. Reopen it before building; export any unsaved draft first.",
                        self.recipe.name
                    ));
                }
                self.recipe.clone()
            } else {
                WeaponRecipe::load_json(path).map_err(|error| {
                    format!("Could not load included recipe {}: {error}", path.display())
                })?
            };
            recipes.push(recipe);
        }
        Ok(BatchBuildRequest {
            package_directory: self.packages.clone(),
            staging_root: PathBuf::from(self.staging.trim()),
            ignore_installed_authored_overlays: self.ignore_installed,
            recipes,
        })
    }

    pub(super) fn snapshot(&self) -> Result<BatchBuildSnapshot, String> {
        if self.catalog.is_none()
            || self.catalog_receiver.is_some()
            || self.catalog_worker.is_some()
            || self.catalog_reload_pending
        {
            return Err(
                "The Sundial weapon catalog must finish loading before package authoring"
                    .to_owned(),
            );
        }
        BatchBuildSnapshot::new(self.batch_request()?)
    }

    pub(super) fn save_edits_for_build(&mut self) -> Result<(), String> {
        if let Some((_, error)) = &self.invalid_weapon_name {
            return Err(error.clone());
        }
        if !self.recipe_requires_initial_save && self.recipe == self.recipe_baseline {
            return Ok(());
        }
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("The recipe library is unavailable. Current edits could not be saved")?;
        let path = match self
            .recipe_path
            .as_ref()
            .filter(|path| path.starts_with(library.root()))
        {
            Some(path) => {
                library.save_existing_if_unchanged(path, &self.recipe_baseline, &self.recipe)?;
                path.clone()
            }
            None => library.save_new(&self.recipe)?,
        };
        self.recipe_path = Some(path.clone());
        self.recipe_requires_initial_save = false;
        self.recipe_dirty = false;
        self.recipe_baseline.clone_from(&self.recipe);
        self.observed_recipe.clone_from(&self.recipe);
        self.log.push(LogEntry::info(format!(
            "Saved current edits before building: {}",
            path.display()
        )));
        // Saving the open draft must not change which recipes the user selected for this build.
        let selected = self.enabled_recipe_paths.clone();
        self.refresh_recipe_library();
        self.enabled_recipe_paths = selected;
        Ok(())
    }

    pub(super) fn invalidate_results(&mut self) {
        self.build_progress = None;
        self.latest_build = None;
        self.latest_install = None;
        self.build_status_open = false;
        self.build_dialog_step = BuildDialogStep::Build;
    }

    pub(super) fn synchronize_recipe_dirty(&mut self) {
        self.recipe_dirty = self.recipe_requires_initial_save
            || self.recipe != self.recipe_baseline
            || self.invalid_weapon_name.is_some();
        if self.recipe != self.observed_recipe {
            self.observed_recipe.clone_from(&self.recipe);
            self.invalidate_results();
        }
    }

    pub(super) fn discard_recipe_changes(&mut self) {
        self.recipe = self.recipe_baseline.clone();
        self.recipe_dirty = self.recipe_requires_initial_save;
        self.clear_dependent_picker_queries();
        self.invalidate_results();
        self.log.push(LogEntry::info("Discarded recipe changes"));
    }

    pub(super) fn request_recipe_action(&mut self, action: PendingRecipeAction) -> bool {
        if self.recipe_dirty {
            self.pending_recipe_action = Some(action);
            false
        } else {
            self.execute_recipe_action(action)
        }
    }

    pub(super) fn execute_recipe_action(&mut self, action: PendingRecipeAction) -> bool {
        match action {
            PendingRecipeAction::Close => {
                self.close_approved = true;
                false
            }
            PendingRecipeAction::New => self.start_new_recipe(),
            PendingRecipeAction::Open(path) => self.open_recipe_path(&path),
            PendingRecipeAction::Import => self.import_recipe(),
        }
    }

    pub(super) fn take_close_approved(&mut self) -> bool {
        std::mem::take(&mut self.close_approved)
    }

    pub(super) fn open_recipe_path(&mut self, path: &Path) -> bool {
        match WeaponRecipe::load_json(path) {
            Ok(recipe) => {
                self.recipe = recipe;
                self.recipe_baseline = self.recipe.clone();
                self.recipe_path = Some(path.to_path_buf());
                self.recipe_requires_initial_save = false;
                self.recipe_dirty = false;
                self.clear_dependent_picker_queries();
                self.scroll_recipe_to_top = true;
                self.invalidate_results();
                self.log
                    .push(LogEntry::info(format!("Loaded recipe {}", path.display())));
                true
            }
            Err(error) => {
                self.log.push(LogEntry::error(format!(
                    "Could not load recipe {}: {error}",
                    path.display()
                )));
                false
            }
        }
    }

    pub(super) fn duplicate_recipe(&mut self) -> bool {
        if let Some((_, error)) = &self.invalid_weapon_name {
            self.log.push(LogEntry::error(error));
            return false;
        }
        let mut copy = self.recipe.clone();
        for suffix in 1..=10_000 {
            let name = if suffix == 1 {
                format!("{} Copy", self.recipe.name)
            } else {
                format!("{} Copy {suffix}", self.recipe.name)
            };
            if let Err(error) = copy.rename_authored_item(&name) {
                self.log.push(LogEntry::error(format!(
                    "Could not duplicate recipe: {error}"
                )));
                return false;
            }
            if self
                .recipe_entries
                .iter()
                .any(|entry| entry.namespace == copy.namespace)
            {
                continue;
            }
            self.recipe = copy;
            self.recipe_baseline = self.recipe.clone();
            self.recipe_path = None;
            self.recipe_requires_initial_save = true;
            self.recipe_dirty = true;
            self.clear_dependent_picker_queries();
            self.scroll_recipe_to_top = true;
            self.invalidate_results();
            return true;
        }
        self.log.push(LogEntry::error(
            "Could not allocate an unused recipe copy identity",
        ));
        false
    }

    pub(super) fn start_new_recipe(&mut self) -> bool {
        let Ok(recipe) = WeaponRecipe::new_unbound("New Recipe") else {
            self.log.push(LogEntry::error(
                "Could not allocate the built-in New Recipe identity",
            ));
            return false;
        };
        self.recipe = recipe;
        self.bind_default_donor();
        self.recipe_baseline = self.recipe.clone();
        self.recipe_path = None;
        self.recipe_requires_initial_save = false;
        self.recipe_dirty = false;
        self.advanced_gameplay_page = AdvancedGameplayPage::Runtime;
        self.clear_dependent_picker_queries();
        self.scroll_recipe_to_top = true;
        self.invalidate_results();
        true
    }

    pub(super) fn refresh_recipe_library(&mut self) {
        let Some(library) = self.recipe_library.as_ref() else {
            return;
        };
        match library.scan() {
            Ok(scan) => {
                self.recipe_entries = scan.entries;
                match library.enabled_paths(&self.recipe_entries) {
                    Ok(enabled) => self.enabled_recipe_paths = enabled,
                    Err(error) => self.log.push(LogEntry::error(format!(
                        "Recipe selection state could not be loaded: {error}"
                    ))),
                }
                for error in scan.errors {
                    self.log.push(LogEntry::error(format!(
                        "Recipe library entry could not be loaded: {error}"
                    )));
                }
                self.invalidate_results();
            }
            Err(error) => self.log.push(LogEntry::error(error)),
        }
    }

    pub(super) fn save_library_recipe(&mut self) {
        if let Some((_, error)) = &self.invalid_weapon_name {
            self.log.push(LogEntry::error(error));
            return;
        }
        let Some(library) = self.recipe_library.as_ref() else {
            return;
        };
        let Some(path) = self.recipe_path.as_ref() else {
            return;
        };
        match library.save_existing_if_unchanged(path, &self.recipe_baseline, &self.recipe) {
            Ok(()) => {
                self.recipe_requires_initial_save = false;
                self.recipe_dirty = false;
                self.recipe_baseline = self.recipe.clone();
                self.log
                    .push(LogEntry::info(format!("Saved recipe {}", path.display())));
                self.refresh_recipe_library();
            }
            Err(error) => self.log.push(LogEntry::error(format!(
                "Could not save recipe {}: {error}",
                path.display()
            ))),
        }
    }

    pub(super) fn save_recipe_copy(&mut self) {
        if let Some((_, error)) = &self.invalid_weapon_name {
            self.log.push(LogEntry::error(error));
            return;
        }
        let Some(library) = self.recipe_library.as_ref() else {
            return;
        };
        match library.save_new(&self.recipe) {
            Ok(path) => {
                self.recipe_requires_initial_save = false;
                self.recipe_path = Some(path.clone());
                self.recipe_dirty = false;
                self.recipe_baseline = self.recipe.clone();
                self.log.push(LogEntry::info(format!(
                    "Saved recipe copy {}",
                    path.display()
                )));
                self.refresh_recipe_library();
            }
            Err(error) => self.log.push(LogEntry::error(error)),
        }
    }

    pub(super) fn import_recipe(&mut self) -> bool {
        let Some(library) = self.recipe_library.clone() else {
            return false;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Parhelion weapon recipe", &["json"])
            .set_directory(library.root())
            .pick_file()
        else {
            return false;
        };
        match library.import(&path) {
            Ok((destination, recipe)) => {
                self.recipe_requires_initial_save = false;
                self.recipe = recipe;
                self.recipe_baseline = self.recipe.clone();
                self.recipe_path = Some(destination.clone());
                self.recipe_dirty = false;
                self.clear_dependent_picker_queries();
                self.scroll_recipe_to_top = true;
                self.invalidate_results();
                self.log.push(LogEntry::info(format!(
                    "Imported {} to {}",
                    path.display(),
                    destination.display()
                )));
                self.refresh_recipe_library();
                true
            }
            Err(error) => {
                self.log.push(LogEntry::error(error));
                false
            }
        }
    }

    pub(super) fn export_recipe(&mut self) {
        if let Some((_, error)) = &self.invalid_weapon_name {
            self.log.push(LogEntry::error(error));
            return;
        }
        let Some(library) = self.recipe_library.as_ref() else {
            return;
        };
        let suggested = format!("{}.parhelion.json", self.recipe.slug());
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Parhelion weapon recipe", &["json"])
            .set_directory(library.root())
            .set_file_name(suggested)
            .save_file()
        else {
            return;
        };
        match self.export_recipe_to(&path) {
            Ok(()) => self.log.push(LogEntry::info(format!(
                "Exported recipe {}",
                path.display()
            ))),
            Err(error) => self.log.push(LogEntry::error(format!(
                "Could not export recipe {}: {error}",
                path.display()
            ))),
        }
    }

    pub(super) fn export_recipe_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        use sundial::package_authoring::{
            path_is_within, paths_equal, resolve_path_for_comparison,
        };

        let library = self
            .recipe_library
            .as_ref()
            .ok_or("Recipe library is unavailable")?;
        let target = resolve_path_for_comparison(path).map_err(|error| error.to_string())?;
        let current = self
            .recipe_path
            .as_deref()
            .map(resolve_path_for_comparison)
            .transpose()
            .map_err(|error| error.to_string())?;
        if current
            .as_ref()
            .is_some_and(|current| paths_equal(current, &target))
        {
            library.save_existing_if_unchanged(
                self.recipe_path
                    .as_ref()
                    .expect("current path was resolved"),
                &self.recipe_baseline,
                &self.recipe,
            )?;
            self.recipe_baseline = self.recipe.clone();
            self.recipe_dirty = false;
            self.recipe_requires_initial_save = false;
            self.refresh_recipe_library();
            return Ok(());
        }
        let root =
            resolve_path_for_comparison(library.root()).map_err(|error| error.to_string())?;
        if path_is_within(&target, &root) {
            return Err("Use Save Copy to create another library recipe, or export outside the recipe library.".into());
        }
        self.recipe
            .save_json(path)
            .map_err(|error| error.to_string())
    }
}
