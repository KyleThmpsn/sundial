//! Preference controls and inventory-layout previews.
use crate::app::account_workspace as account;

use super::account_workspace::AccountSourceKind;
use super::preferences::{
    CharacterInventoryLayout, ColorTheme, ItemCardWidth, MAX_AUTOMATIC_BACKUP_LIMIT,
    MIN_AUTOMATIC_BACKUP_LIMIT, PlugSelectionMode, draw_plug_selection_warning,
};
use super::settings::{backups_path, preferences_path};
use super::{
    ARMOR_SLOTS, ConfirmationDialog, INVENTORY_LAYOUT_PREVIEW_HASH, InventoryLayoutPreviewItem,
    PreferencesTab, SundialApp, WEAPON_SLOTS, diagnostics, equipment, preferences,
};
use crate::account_contract::EQUIPMENT_SLOTS as SLOTS;
use crate::game_settings;
use crate::package_authoring::open_directory;
use eframe::egui;
use std::fs;

impl SundialApp {
    pub(super) fn save_preferences(&self) -> Result<(), String> {
        let path = preferences_path().ok_or("Could not locate Sundial's preferences folder")?;
        let mut preferences = self.preferences.clone();
        preferences.install = Some(self.install_path.clone());
        preferences.settings_layout = Some(self.settings_layout.preference_value().to_owned());
        preferences.normalize_for_runtime();
        preferences::store::save_preferences(&path, &preferences)
    }

    pub(super) fn inventory_layout_preview_item(
        &self,
        ctx: &egui::Context,
    ) -> Option<InventoryLayoutPreviewItem> {
        let from_hash = |hash| self.inventory_layout_preview_item_for_hash(ctx, hash);
        from_hash(INVENTORY_LAYOUT_PREVIEW_HASH)
            .or_else(|| {
                account::equipped_item_snapshots(&self.document, self.selected_character)
                    .ok()?
                    .into_iter()
                    .filter_map(|snapshot| snapshot.definition_hash)
                    .find_map(from_hash)
            })
            .or_else(|| {
                SLOTS
                    .iter()
                    .filter(|(slot, _, _)| {
                        WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot)
                    })
                    .flat_map(|(_, _, bucket_hash)| {
                        self.manifest
                            .items_for_bucket(*bucket_hash)
                            .map(|item| item.hash)
                    })
                    .find_map(from_hash)
            })
    }

    pub(super) fn inventory_layout_preview_item_for_hash(
        &self,
        ctx: &egui::Context,
        hash: u64,
    ) -> Option<InventoryLayoutPreviewItem> {
        let item = self.manifest.item(hash)?;
        let &(slot, slot_label, bucket_hash) = SLOTS.iter().find(|(slot, _, bucket_hash)| {
            *bucket_hash == item.bucket_hash
                && (WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot))
        })?;
        (!item.name.trim().is_empty() && self.manifest.icon_texture(ctx, hash).is_some()).then(
            || InventoryLayoutPreviewItem {
                hash,
                slot,
                slot_label,
                bucket_hash,
                class_type: item.class_type,
                // Unlimited item caps are sentinels, not useful example Power.
                power: self.manifest.item_power_cap(hash).unwrap_or(136).min(136),
            },
        )
    }

    pub(super) fn draw_inventory_layout_choices(
        &mut self,
        ui: &mut egui::Ui,
        selected: &mut CharacterInventoryLayout,
    ) -> bool {
        let previous = *selected;
        ui.horizontal(|ui| {
            ui.selectable_value(
                selected,
                CharacterInventoryLayout::Cards,
                "Sundial Cards (Default)",
            );
            ui.selectable_value(
                selected,
                CharacterInventoryLayout::Panoptes,
                "Panoptes Grid",
            );
        });
        ui.label(
            egui::RichText::new(match selected {
                CharacterInventoryLayout::Cards => "Full item cards with inline editing controls",
                CharacterInventoryLayout::Panoptes => {
                    "Selected-item editor beside the equipped and inventory grid"
                }
            })
            .color(super::ui::secondary_text_color(ui)),
        );
        ui.add_space(6.0);
        let preview = self.inventory_layout_preview_item(ui.ctx());
        self.draw_inventory_layout_preview(ui, preview.as_ref(), *selected);
        previous != *selected
    }

    pub(super) fn draw_inventory_layout_preview(
        &mut self,
        ui: &mut egui::Ui,
        preview: Option<&InventoryLayoutPreviewItem>,
        layout: CharacterInventoryLayout,
    ) {
        let Some(preview) = preview else {
            ui.label(
                egui::RichText::new("Preview available after the catalog loads")
                    .color(super::ui::secondary_text_color(ui)),
            );
            return;
        };
        let snapshot = preview.snapshot();
        let preview_id = match layout {
            CharacterInventoryLayout::Cards => "cards",
            CharacterInventoryLayout::Panoptes => "panoptes",
        };
        ui.push_id(
            ("inventory-layout-preview", preview_id),
            |ui| match layout {
                CharacterInventoryLayout::Cards => {
                    let width = ItemCardWidth::Standard
                        .dimensions()
                        .1
                        .min(ui.available_width());
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                self.draw_equipment_slot_card(
                                    ui,
                                    0,
                                    equipment::EquipmentSlotCard {
                                        id_scope: "preferences-layout-preview",
                                        slot: preview.slot,
                                        label: preview.slot_label,
                                        bucket_hash: preview.bucket_hash,
                                        class_type: preview.class_type,
                                        editable: false,
                                        header_fill: None,
                                        snapshot: Some(&snapshot),
                                    },
                                );
                            },
                        );
                    });
                }
                CharacterInventoryLayout::Panoptes => {
                    let width = ui.available_width().min(880.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                self.draw_panoptes_layout_preview(
                                    ui,
                                    &snapshot,
                                    preview.class_type,
                                );
                            },
                        );
                    });
                }
            },
        );
    }

    pub(super) fn draw_preferences_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Preferences");
        ui.add_space(6.0);
        let mut reset_requested = false;
        ui.horizontal(|ui| {
            for tab in PreferencesTab::ALL {
                ui.selectable_value(&mut self.preferences_tab, tab, tab.label());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reset_requested = ui
                    .small_button("Reset Preferences…")
                    .on_hover_text(
                        "Reset interface, editing, saving, and experimental preferences. Paths, catalog data, and backups are not changed.",
                    )
                    .clicked();
            });
        });
        ui.separator();

        let mut preferences_changed = false;
        let mut preferences_reset = false;
        if reset_requested {
            self.reset_preferences_to_defaults(ctx);
            preferences_changed = true;
            preferences_reset = true;
        }

        let selected_tab = self.preferences_tab;
        preferences_changed |= egui::ScrollArea::vertical()
            .id_salt(("preferences", selected_tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                match selected_tab {
                    PreferencesTab::Interface => self.draw_interface_preferences(ui, ctx),
                    PreferencesTab::Editing => self.draw_editing_preferences(ui, ctx),
                    PreferencesTab::Installation => self.draw_installation_preferences(ui, ctx),
                    PreferencesTab::SavingRecovery => self.draw_saving_recovery_preferences(ui),
                }
            })
            .inner;

        if preferences_changed {
            match self.save_preferences() {
                Ok(()) => self.set_status(
                    if preferences_reset {
                        "Preferences reset to defaults"
                    } else {
                        "Preferences saved"
                    },
                    false,
                ),
                Err(error) => self.set_status(
                    format!("Preferences changed, but could not be saved: {error}"),
                    true,
                ),
            }
        }
    }

    pub(super) fn reset_preferences_to_defaults(&mut self, ctx: &egui::Context) {
        self.preferences.reset_editable_settings();
        ctx.set_theme(self.preferences.color_theme.egui_theme());
        self.plug_selection_mode = self.preferences.default_plug_selection_mode;
        self.troubleshooting_log_error = None;
        self.remember_plug_selection_mode_after_confirmation = false;
    }

    pub(super) fn draw_interface_preferences(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> bool {
        let mut preferences_changed = false;

        if ui
            .checkbox(
                &mut self.preferences.always_open_json_editor_in_second_window,
                "Open All Settings (JSON) in a Second Window",
            )
            .changed()
        {
            preferences_changed = true;
        }
        ui.add_space(12.0);

        super::ui::section_heading(ui, "Appearance");
        let mut requested_theme = self.preferences.color_theme;
        ui.horizontal(|ui| {
            ui.label("Color Theme:");
            ui.radio_value(&mut requested_theme, ColorTheme::Dark, "Dark (Recommended)");
            ui.radio_value(&mut requested_theme, ColorTheme::Light, "Light");
        });
        if requested_theme != self.preferences.color_theme {
            self.preferences.color_theme = requested_theme;
            ctx.set_theme(requested_theme.egui_theme());
            preferences_changed = true;
        }
        let mut requested_card_width = self.preferences.item_card_width;
        ui.horizontal_wrapped(|ui| {
            ui.label("Item Card Width:");
            ui.radio_value(&mut requested_card_width, ItemCardWidth::Compact, "Compact");
            ui.radio_value(
                &mut requested_card_width,
                ItemCardWidth::Standard,
                "Standard",
            );
            ui.radio_value(&mut requested_card_width, ItemCardWidth::Wide, "Wide");
        });
        if requested_card_width != self.preferences.item_card_width {
            self.preferences.item_card_width = requested_card_width;
            preferences_changed = true;
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            super::ui::section_heading(ui, "Loadout Layout");
            crate::ui_help::info(
                ui,
                "Used on Characters & loadouts. Character inventory keeps its item cards.",
            );
        });
        let mut requested_inventory_layout = self.preferences.character_inventory_layout;
        if self.draw_inventory_layout_choices(ui, &mut requested_inventory_layout) {
            self.preferences.character_inventory_layout = requested_inventory_layout;
            preferences_changed = true;
        }

        preferences_changed
    }

    pub(super) fn draw_editing_preferences(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> bool {
        let mut preferences_changed = false;

        ui.horizontal(|ui| {
            super::ui::section_heading(ui, "Item Editing");
            crate::ui_help::info(
                ui,
                "Choose the plug selection mode Sundial uses when it starts.",
            );
        });
        ui.add_space(4.0);

        let mut requested_mode = self.preferences.default_plug_selection_mode;
        ui.horizontal_wrapped(|ui| {
            ui.label("Default Plug Selection Mode:");
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::Supported,
                PlugSelectionMode::Supported.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::SocketAndGearType,
                PlugSelectionMode::SocketAndGearType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::MatchingSocketType,
                PlugSelectionMode::MatchingSocketType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::GearType,
                PlugSelectionMode::GearType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::AnyPlug,
                PlugSelectionMode::AnyPlug.label(),
            );
        });

        if requested_mode != self.preferences.default_plug_selection_mode {
            if requested_mode == PlugSelectionMode::AnyPlug
                && !self.preferences.really_unsafe_warning_acknowledged
            {
                self.remember_plug_selection_mode_after_confirmation = true;
                self.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
            } else {
                self.preferences.default_plug_selection_mode = requested_mode;
                self.plug_selection_mode = requested_mode;
                preferences_changed = true;
            }
        }

        if self.preferences.show_safety_warnings {
            draw_plug_selection_warning(ui, self.preferences.default_plug_selection_mode);
        }
        ui.add_space(6.0);

        let warning_response = ui.checkbox(
            &mut self.preferences.show_safety_warnings,
            "Show Plug-Selection Safety Warnings",
        );
        preferences_changed |= warning_response.changed();

        let hash_response = ui.checkbox(
            &mut self.preferences.show_plug_hashes,
            "Show Plug Hashes on Item Cards",
        );
        preferences_changed |= hash_response.changed();

        ui.add_space(12.0);
        super::ui::section_heading(ui, "Experimental");
        let mut enable_parhelion = self.preferences.experimental_package_authoring;
        let package_authoring_response = ui.horizontal(|ui| {
            let response = ui.checkbox(
                &mut enable_parhelion,
                "Enable Parhelion Weapon Workbench",
            );
            crate::ui_help::info(ui, "Build custom Destiny weapons by combining stats, plugs, private perks, runtime behavior, and appearance sources from the selected Shadowkeep installation.");
            response
        }).inner;
        if package_authoring_response.changed() {
            preferences_changed |= self.request_parhelion_enabled(enable_parhelion);
        }
        if self.preferences.experimental_package_authoring && ui.button("Open Parhelion").clicked()
        {
            self.open_package_authoring(ctx);
        }
        ui.add_space(6.0);
        if self.document.uses_json_account() && self.document.supports_v13_account() {
            preferences_changed |= ui.checkbox(
                &mut self.preferences.experimental_activity_state,
                "Show Activity State",
            ).on_hover_text("Shows the raw current activity index on v13+ accounts. Its effect in game is not verified. Saved values are preserved when hidden.").changed();
            ui.add_space(6.0);
        }
        preferences_changed |= ui
            .checkbox(
                &mut self.preferences.experimental_extended_fov,
                "Allow Field of View up to 155",
            )
            .changed();
        ui.label("Extends the Display slider on Sunrise schema 16 or newer. Existing saved values are preserved when disabled.");
        ui.add_space(6.0);
        let power_above_cap_response = ui.horizontal(|ui| {
            let response = ui.checkbox(
                &mut self.preferences.experimental_power_above_cap,
                "Allow Power Above Item Caps",
            );
            crate::ui_help::info(ui, "Allows manual Power values above an item's package-defined cap. Newly added items still start at their normal cap.");
            response
        }).inner;
        preferences_changed |= power_above_cap_response.changed();
        ui.label(
            "Destiny may display capped Power while the saved value still affects character Power.",
        );
        ui.add_space(6.0);
        let cross_class_subclasses_response = ui.horizontal(|ui| {
            let response = ui.checkbox(
                &mut self.preferences.experimental_cross_class_subclasses,
                "Allow Cross-Class Subclasses",
            );
            crate::ui_help::info(ui, "Shows every class's subclasses in the character editor and subclass inventory, and permits equipping them in Sundial.");
            response
        }).inner;
        preferences_changed |= cross_class_subclasses_response.changed();
        ui.label("Unsupported subclasses cannot be selected in game. Some combinations may behave incorrectly.");
        ui.add_space(6.0);
        let progression_response = ui
            .horizontal(|ui| {
                let response = ui.checkbox(
                    &mut self.preferences.experimental_progression,
                    "Enable Progression Editing",
                );
                crate::ui_help::info(
                    ui,
                    "Allows changes to Unlocks, Investment overrides, and Collections acquisition state. Browsing and inspection are always available.",
                );
                response
            })
            .inner;
        preferences_changed |= progression_response.changed();

        preferences_changed
    }

    pub(super) fn draw_installation_preferences(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> bool {
        let mut preferences_changed = false;
        ui.horizontal(|ui| {
            super::ui::section_heading(ui, "Installation and Compatibility");
            crate::ui_help::info(ui, "Select the Destiny 2 Shadowkeep installation. Sundial finds Project Sunrise's settings.json inside it automatically.");
        });
        ui.add_space(10.0);
        let account_source = self.document.source_info();
        ui.label("Sunrise Destiny 2 Installation");
        preference_path(ui, &self.install_path);
        if ui.button("Choose Installation…").clicked() {
            self.choose_install(ctx);
        }
        self.draw_runtime_preferences(ui);
        ui.add_space(8.0);
        egui::Grid::new("preferences_sunrise_grid")
            .num_columns(2)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Active Account Source");
                ui.colored_label(
                    match account_source.kind {
                        AccountSourceKind::Json => ui.visuals().text_color(),
                        AccountSourceKind::Sqlite => ui.visuals().hyperlink_color,
                        AccountSourceKind::Blocked => ui.visuals().error_fg_color,
                    },
                    account_source.label,
                );
                ui.end_row();
                ui.label("Settings Schema");
                ui.monospace(game_settings::schema_version(&self.document).map_or_else(
                    || "Missing or invalid".to_owned(),
                    |version| version.to_string(),
                ))
                .on_hover_text("Sundial uses this value to determine compatibility.");
                ui.end_row();
                ui.label("Detected Sunrise Version");
                ui.monospace(&self.sunrise_version)
                    .on_hover_text("Shown for reference. This does not control compatibility.");
                ui.end_row();
                ui.label("Account Format");
                ui.monospace(account_source.contract);
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.colored_label(
            if account_source.kind == AccountSourceKind::Blocked {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            },
            &account_source.detail,
        );
        if account_source.kind != AccountSourceKind::Json {
            ui.label("Account Database");
            preference_path(ui, &account_source.database_path);
        }
        ui.label(
            egui::RichText::new(
                "Sundial saves account edits only to the active source. It never mirrors account data between investment.sqlite3 and settings.json.",
            )
            .color(super::ui::secondary_text_color(ui)),
        );
        ui.add_space(12.0);
        self.draw_recovery_preferences(ui);
        ui.add_space(12.0);
        super::ui::section_heading(ui, "Catalog");
        ui.label(format!(
            "Local catalog cache: {}",
            self.manifest.cache_path.display()
        ));
        ui.label(if self.manifest.loaded_from_cache {
            "Loaded from local cache"
        } else {
            "Scanned from game packages"
        });
        let catalog_stats = self.manifest.stats();
        ui.label(format!(
            "{} items · {} plugs · {} icons · {} descriptions",
            catalog_stats.items,
            catalog_stats.plugs,
            catalog_stats.icons,
            catalog_stats.descriptions,
        ));
        if ui.button("Rebuild Catalog from Game Files").clicked() {
            self.rebuild_catalog(ctx);
        }
        ui.add_space(6.0);
        ui.label("The first scan reads the installed packages. Later starts use the local cache unless the package files change.");

        ui.add_space(12.0);
        super::ui::section_heading(ui, "Troubleshooting");
        if ui.button("Activity Log…").clicked() {
            self.activity_log_open = true;
        }
        let logging_response = ui.checkbox(
            &mut self.preferences.troubleshooting_logging,
            "Enable Troubleshooting Logging",
        );
        if logging_response.changed() {
            preferences_changed = true;
            if self.preferences.troubleshooting_logging {
                let _ = self.initialize_troubleshooting_log();
            } else {
                self.troubleshooting_log_error = None;
            }
        }
        ui.label(
            egui::RichText::new(
                "Saves startup details and Sundial activity. Copy Report includes current diagnostics and recent activity, including Parhelion when available.",
            )
            .color(super::ui::secondary_text_color(ui)),
        );
        if let Some(path) = diagnostics::log_path() {
            ui.monospace(path.display().to_string());
        } else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Sundial could not locate its local log folder.",
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Refresh Log").clicked() {
                let result = diagnostics::log_path().map_or_else(
                    || Err("Could not locate Sundial's local log folder".to_owned()),
                    |path| {
                        if path.is_file() {
                            self.append_troubleshooting_snapshot()
                        } else {
                            self.initialize_troubleshooting_log()
                        }
                    },
                );
                match result {
                    Ok(path) => self.set_status(
                        format!("Updated troubleshooting log at {}", path.display()),
                        false,
                    ),
                    Err(error) => self.set_status(error, true),
                }
            }
            if ui.button("Copy Report").clicked() {
                ui.ctx().copy_text(self.build_troubleshooting_report());
                self.set_status("Copied troubleshooting report", false);
            }
            if ui.button("Open Log Folder").clicked() {
                let result = diagnostics::log_path()
                    .ok_or("Could not locate Sundial's local log folder".to_owned())
                    .and_then(|path| {
                        let parent = path
                            .parent()
                            .ok_or("Sundial's troubleshooting log path has no parent folder")?;
                        fs::create_dir_all(parent).map_err(|error| {
                            format!("Could not create {}: {error}", parent.display())
                        })?;
                        open_directory(parent)
                    });
                match result {
                    Ok(()) => self.set_status("Opened the troubleshooting log folder", false),
                    Err(error) => self.set_status(error, true),
                }
            }
        });
        if let Some(error) = &self.troubleshooting_log_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        preferences_changed
    }

    pub(super) fn draw_saving_recovery_preferences(&mut self, ui: &mut egui::Ui) -> bool {
        let mut preferences_changed = false;

        super::ui::section_heading(ui, "Saving");
        let review_response = ui.horizontal(|ui| {
            let response = ui.checkbox(
                &mut self.preferences.review_changes_before_saving,
                "Review Changes Before Saving",
            );
            crate::ui_help::info(ui, "Adds a confirmation step listing changed fields. Validation, conflict checks, and backups always run.");
            response
        }).inner;
        preferences_changed |= review_response.changed();

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            super::ui::section_heading(ui, "Automatic Backups");
            crate::ui_help::info(ui, "Sundial creates a source-specific backup before every save. Each installation has its own backup history. Legacy unscoped backups, recovery snapshots, and manual settings.json.bak safety copies are never removed.");
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let retention_response =
                ui.checkbox(&mut self.preferences.limit_automatic_backups, "Keep Last");
            preferences_changed |= retention_response.changed();
            let limit_response = ui.add_enabled(
                self.preferences.limit_automatic_backups,
                egui::DragValue::new(&mut self.preferences.automatic_backup_limit)
                    .range(MIN_AUTOMATIC_BACKUP_LIMIT..=MAX_AUTOMATIC_BACKUP_LIMIT),
            );
            preferences_changed |= limit_response.changed();
            ui.label("automatic backups per source");
        });
        ui.label("When enabled, older automatic backups are removed after saving.");

        ui.add_space(12.0);
        self.draw_recovery_preferences(ui);
        preferences_changed
    }

    fn draw_recovery_preferences(&mut self, ui: &mut egui::Ui) {
        super::ui::section_heading(ui, "Recovery");
        let account_source = self.document.source_info();
        ui.label("Sunrise Settings");
        preference_path(ui, &self.settings_path);
        if ui
            .button("Reset to Sunrise Defaults…")
            .on_hover_text("Restore the settings bundled with this installed Sunrise version. The current settings.json is backed up first")
            .clicked()
        {
            self.confirmation = Some(ConfirmationDialog::ResetDefaults);
        }
        if account_source.kind != AccountSourceKind::Json {
            ui.add_space(8.0);
            ui.label("Sunrise Account Database");
            preference_path(ui, &account_source.database_path);
            if ui.button("Reset Account Database…")
                .on_hover_text("Reset characters, inventory, progression, and account preferences to the defaults bundled with this installed Sunrise version. A full recovery backup is created first")
                .clicked()
            {
                self.request_sqlite_defaults_reset();
            }
            if matches!(account_source.kind, AccountSourceKind::Sqlite | AccountSourceKind::Blocked)
                && ui.button("Restore Backup…")
                    .on_hover_text("Restore a verified Sundial account backup. The current database is preserved first")
                    .clicked()
            {
                self.request_sqlite_backup_restore();
            }
        }
        ui.add_space(8.0);
        if ui.button("Browse Backups…").clicked() {
            match backups_path()
                .ok_or("Could not locate Sundial's backups folder".to_owned())
                .and_then(|path| {
                    fs::create_dir_all(&path)
                        .map_err(|error| format!("Could not create {}: {error}", path.display()))
                        .map(|()| path)
                })
                .and_then(|path| open_directory(&path))
            {
                Ok(()) => self.set_status("Opened the backups folder", false),
                Err(error) => self.set_status(error, true),
            }
        }
    }
}

fn preference_path(ui: &mut egui::Ui, path: &std::path::Path) {
    ui.add(
        egui::Label::new(egui::RichText::new(path.display().to_string()).monospace())
            .wrap()
            .selectable(true),
    );
}
