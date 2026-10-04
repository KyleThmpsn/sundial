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
                    "Could not load Parhelion backup preferences. Using defaults: {error}"
                )));
                self.preferences_error = Some(error);
                ParhelionPreferences::default()
            }
        };
        let recipe_library = match RecipeLibrary::open_default() {
            Ok(library) => {
                if let Some(refresh) = library.defaults_refresh() {
                    log.push(LogEntry::info(refresh.summary("recipes")));
                }
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
        self.library_state.refresh_metadata(&self.recipe_entries);
        self.enabled_recipe_paths = enabled_recipe_paths;
        self.limit_package_backups = backup_preferences.limit_package_backups;
        self.show_technical_build = backup_preferences.show_technical_build;
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
            recipes.push(self.recipe_for_build(path)?);
        }
        if let Some(library) = &self.recipe_library {
            let included = recipes
                .iter()
                .map(|recipe| {
                    recipe
                        .identity
                        .item_hash
                        .parse_u32()
                        .map_err(|error| error.to_string())
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            let mut referenced = recipes
                .iter()
                .flat_map(|recipe| &recipe.overrides.socket_columns)
                .flatten()
                .flat_map(|column| &column.choices)
                .map(|hash| hash.parse_u32().map_err(|error| error.to_string()))
                .collect::<Result<BTreeSet<_>, _>>()?;
            referenced.retain(|hash| {
                !included.contains(hash)
                    && !self
                        .catalog
                        .as_ref()
                        .and_then(|catalog| catalog.item_definition_tag(*hash))
                        .is_some_and(crate::package_profile::is_stock_item_definition)
            });
            // Shader recipes cannot contain sockets, so these dependencies have no further
            // socket dependencies. Keep one copy even when several items select the shader.
            for entry in library.shader_entries(&referenced)? {
                let recipe = self.recipe_for_build(&entry.path)?;
                if recipe.kind != ItemKind::Shader
                    || recipe.identity.item_hash.parse_u32().ok() != Some(entry.identity_hash)
                {
                    return Err(format!(
                        "Shader recipe {} changed identity while preparing the build. Select the shader again.",
                        entry.path.display()
                    ));
                }
                recipes.push(recipe);
            }
        }
        Ok(BatchBuildRequest {
            package_directory: self.packages.clone(),
            staging_root: PathBuf::from(self.staging.trim()),
            ignore_installed_authored_overlays: self.ignore_installed,
            recipes,
        })
    }

    /// Explicit items and required shaders use the same draft and concurrent-change checks.
    fn recipe_for_build(&self, path: &Path) -> Result<WeaponRecipe, String> {
        let mut recipe = if self.recipe_path.as_deref() == Some(path) {
            if let Some((_, error)) = &self.invalid_weapon_name {
                return Err(error.clone());
            }
            let saved = WeaponRecipe::load_json(path).map_err(|error| {
                format!(
                    "Could not check included recipe {}: {error}",
                    path.display()
                )
            })?;
            if !saved.same_saved_content(&self.recipe_baseline) {
                return Err(format!(
                    "{} changed on disk after you opened it. Reopen it before building. Export any unsaved draft first.",
                    self.recipe.name
                ));
            }
            self.recipe.clone()
        } else {
            WeaponRecipe::load_json(path).map_err(|error| {
                format!("Could not load included recipe {}: {error}", path.display())
            })?
        };
        if let Some(catalog) = self.catalog.as_ref() {
            custom_perks::repair_socket_picks(self.recipe_library.as_ref(), catalog, &mut recipe)?;
        }
        self.rebase_library_donor(recipe)
    }

    /// Rebuilds a recipe whose donor is a weapon from the library on that weapon's own recipe.
    ///
    /// The build reads the game's own tables, where a weapon Parhelion built does not exist,
    /// so it cannot serve as a donor as it is. Its recipe can: the result keeps this recipe's
    /// identity and settings on top of the base recipe's stock donor and changes. A base that
    /// itself builds on a library weapon is followed the same way.
    pub(super) fn rebase_library_donor(
        &self,
        recipe: WeaponRecipe,
    ) -> Result<WeaponRecipe, String> {
        let mut current = recipe;
        let mut seen = Vec::new();
        loop {
            let Ok(donor) = current.donor.item_hash.parse_u32() else {
                return Ok(current);
            };
            // An installed weapon is always itself. Authored identities are derived hashes and
            // nothing stops one from matching a real item, so the game's own tables win rather
            // than a recipe quietly building on something the reader never chose.
            if self.donor_summaries.iter().any(|stock| stock.hash == donor) {
                return Ok(current);
            }
            let Some(entry) = self
                .recipe_entries
                .iter()
                .find(|entry| entry.identity_hash == donor && entry.identity_hash != 0)
            else {
                return Ok(current);
            };
            if seen.contains(&donor)
                || entry.identity_hash == current.identity.item_hash.parse_u32().unwrap_or_default()
            {
                return Err(format!(
                    "{} builds on itself through {}. Choose a stock weapon as its donor.",
                    current.name, entry.name
                ));
            }
            seen.push(donor);
            let base = WeaponRecipe::load_json(&entry.path).map_err(|error| {
                format!(
                    "{} builds on {}, whose recipe could not be loaded: {error}",
                    current.name, entry.name
                )
            })?;
            current = current.rebased_onto(&base).map_err(|error| {
                format!(
                    "{} could not be built on {}: {error}",
                    current.name, entry.name
                )
            })?;
        }
    }

    pub(super) fn snapshot(&self) -> Result<BatchBuildSnapshot, String> {
        if self.catalog.is_none()
            || self.catalog_receiver.is_some()
            || self.catalog_worker.is_some()
            || self.catalog_reload_pending
        {
            return Err("Wait for the weapon catalog to finish loading".to_owned());
        }
        BatchBuildSnapshot::new(self.batch_request()?)
    }

    pub(super) fn save_edits_for_build(&mut self) -> Result<(), String> {
        if let Some((_, error)) = &self.invalid_weapon_name {
            return Err(error.clone());
        }
        if let Some(catalog) = self.catalog.as_ref() {
            let repaired = custom_perks::repair_socket_picks(
                self.recipe_library.as_ref(),
                catalog,
                &mut self.recipe,
            )?;
            if repaired != 0 {
                self.log.push(LogEntry::info(format!(
                    "Recovered custom perk data for {repaired} socket choices"
                )));
            }
        }
        if !self.recipe_requires_initial_save && self.recipe == self.recipe_baseline {
            return Ok(());
        }
        let existing = self
            .recipe_path
            .as_deref()
            .filter(|_| self.recipe_saves_in_place());
        let path = self.write_recipe(existing)?;
        let message = format!("Saved current edits before building: {}", path.display());
        self.accept_saved_recipe(path, message);
        Ok(())
    }

    pub(super) fn invalidate_results(&mut self) {
        // A worker owns an earlier snapshot and can still finish after an edit.
        self.build_invalidated |= self.build_receiver.is_some();
        self.build_progress = None;
        self.build_activity = build_status::Activity::default();
        if self.install_receiver.is_none() {
            self.install_status = build_status::InstallStatus::default();
        }
        self.latest_build = None;
        self.build_blocker = None;
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
            self.advance_recipe_revision();
            self.invalidate_results();
        }
    }

    /// Marks every view cached from the recipe as out of date.
    pub(super) fn advance_recipe_revision(&mut self) {
        self.recipe_revision = self.recipe_revision.wrapping_add(1);
    }

    pub(super) fn discard_recipe_changes(&mut self) {
        self.recipe = self.recipe_baseline.clone();
        self.advance_recipe_revision();
        self.behavior_pins.clear();
        self.recipe_dirty = self.recipe_requires_initial_save;
        self.clear_dependent_picker_queries();
        self.invalidate_results();
        self.log.push(LogEntry::info("Discarded recipe changes"));
    }

    pub(super) fn request_recipe_action(&mut self, action: PendingRecipeAction) -> bool {
        self.pending_recipe_error = None;
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
                // The hosted app stays alive while its window is closed. An
                // approved discard must also clear the draft before reopening.
                if self.recipe_dirty {
                    if self.recipe_path.is_some() {
                        self.discard_recipe_changes();
                    } else {
                        self.start_new_recipe(self.recipe.kind);
                    }
                }
                self.close_approved = true;
                false
            }
            PendingRecipeAction::New(kind) => self.start_new_recipe(kind),
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
                self.behavior_pins.clear();
                self.recipe_baseline = self.recipe.clone();
                self.advance_recipe_revision();
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
        let copy = match self.recipe.unused_copy(
            self.recipe_entries
                .iter()
                .map(|entry| entry.namespace.as_str()),
        ) {
            Ok(copy) => copy,
            Err(error) => {
                self.log.push(LogEntry::error(error));
                return false;
            }
        };
        self.recipe = copy;
        self.behavior_pins.clear();
        self.recipe_baseline = self.recipe.clone();
        self.advance_recipe_revision();
        self.recipe_path = None;
        self.recipe_requires_initial_save = true;
        self.recipe_dirty = true;
        self.clear_dependent_picker_queries();
        self.scroll_recipe_to_top = true;
        self.invalidate_results();
        true
    }

    pub(super) fn start_new_recipe(&mut self, kind: ItemKind) -> bool {
        let Ok(recipe) = WeaponRecipe::new_unbound_kind(kind) else {
            self.log.push(LogEntry::error(format!(
                "Could not allocate the built-in New {} identity",
                kind.label()
            )));
            return false;
        };
        self.recipe = recipe;
        self.behavior_pins.clear();
        self.bind_default_donor();
        self.recipe_baseline = self.recipe.clone();
        self.advance_recipe_revision();
        self.recipe_path = None;
        self.recipe_requires_initial_save = false;
        self.recipe_dirty = false;
        self.clear_dependent_picker_queries();
        self.scroll_recipe_to_top = true;
        self.invalidate_results();
        true
    }

    pub(super) fn refresh_recipe_library(&mut self) {
        self.installed.request();
        self.perk_workbench.saved_weapons_changed();
        let Some(library) = self.recipe_library.as_ref() else {
            return;
        };
        match library.scan() {
            Ok(scan) => {
                self.recipe_entries = scan.entries;
                self.library_state.refresh_metadata(&self.recipe_entries);
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

    /// Whether the open recipe already lives in the recipe library, so saving updates its file.
    pub(super) fn recipe_saves_in_place(&self) -> bool {
        self.recipe_library
            .as_ref()
            .zip(self.recipe_path.as_ref())
            .is_some_and(|(library, path)| path.starts_with(library.root()))
    }

    /// Saves the open recipe the way the toolbar's save button does.
    pub(super) fn save_open_recipe(&mut self) {
        if let Err(error) = self.try_save_open_recipe() {
            self.log.push(LogEntry::error(error));
        }
    }

    pub(super) fn save_library_recipe(&mut self) {
        let result = self
            .recipe_path
            .clone()
            .ok_or_else(|| "Open a saved recipe before saving changes".to_owned())
            .and_then(|path| self.save_recipe_at(Some(&path)));
        if let Err(error) = result {
            self.log.push(LogEntry::error(error));
        }
    }

    pub(super) fn save_recipe_copy(&mut self) {
        if let Err(error) = self.save_recipe_at(None) {
            self.log.push(LogEntry::error(error));
        }
    }

    pub(super) fn try_save_open_recipe(&mut self) -> Result<(), String> {
        let existing = self
            .recipe_path
            .clone()
            .filter(|_| self.recipe_saves_in_place());
        self.save_recipe_at(existing.as_deref())
    }

    fn save_recipe_at(&mut self, existing: Option<&Path>) -> Result<(), String> {
        let path = self.write_recipe(existing)?;
        let message = format!("Saved recipe {}", path.display());
        self.accept_saved_recipe(path, message);
        Ok(())
    }

    /// Validate and persist before accepting a new baseline or continuing navigation.
    fn write_recipe(&self, existing: Option<&Path>) -> Result<PathBuf, String> {
        if let Some((_, error)) = &self.invalid_weapon_name {
            return Err(error.clone());
        }
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("The recipe library is unavailable. Current edits could not be saved")?;
        match existing {
            Some(path) => {
                library
                    .save_existing_if_unchanged(path, &self.recipe_baseline, &self.recipe)
                    .map_err(|error| {
                        format!("Could not save recipe {}: {error}", path.display())
                    })?;
                Ok(path.to_owned())
            }
            None => library.save_new(&self.recipe),
        }
    }

    fn accept_saved_recipe(&mut self, path: PathBuf, message: String) {
        self.recipe_path = Some(path);
        self.recipe_requires_initial_save = false;
        self.recipe_dirty = false;
        self.recipe_baseline.clone_from(&self.recipe);
        self.observed_recipe.clone_from(&self.recipe);
        self.advance_recipe_revision();
        self.log.push(LogEntry::info(message));
        // Saving must not change the recipes selected for the next build.
        let selected = std::mem::take(&mut self.enabled_recipe_paths);
        self.refresh_recipe_library();
        self.enabled_recipe_paths = selected;
    }

    pub(super) fn import_recipe(&mut self) -> bool {
        let Some(library) = self.recipe_library.clone() else {
            return false;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Parhelion Weapon Recipe", &["json"])
            .set_directory(library.root())
            .pick_file()
        else {
            return false;
        };
        match library.import(&path) {
            Ok((destination, recipe)) => {
                self.recipe_requires_initial_save = false;
                self.recipe = recipe;
                self.behavior_pins.clear();
                self.recipe_baseline = self.recipe.clone();
                self.advance_recipe_revision();
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
                self.reveal_library_entries(vec![destination]);
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
        if self.recipe_library.is_none() {
            return;
        }
        let suggested = format!("{}.parhelion.json", self.recipe.slug());
        let Some(path) = rfd::FileDialog::new()
            .set_title(format!("Export {}", self.recipe.name))
            .add_filter("Parhelion Weapon Recipe", &["json"])
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
        use sundial::package_authoring::{paths_equal, resolve_path_for_comparison};

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
            self.advance_recipe_revision();
            self.recipe_dirty = false;
            self.recipe_requires_initial_save = false;
            self.refresh_recipe_library();
            return Ok(());
        }
        library.export(&self.recipe, path)
    }
}
