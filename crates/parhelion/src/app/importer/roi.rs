use super::*;
use parhelion_import::d1_roi;

pub(super) struct Inspected {
    pub report: d1_roi::Inspection,
    pub artifact: PathBuf,
    pub armor: Vec<d1_roi::Armor>,
    pub weapons: Vec<d1_roi::Weapon>,
    pub weapons_page: bool,
    pub weapon: Option<u32>,
    pub weapon_search: String,
    pub selected: BTreeSet<u32>,
    pub search: String,
    pub class: Option<u8>,
    pub exported: Option<PathBuf>,
}

impl PackageAuthoringApp {
    pub(super) fn scan_roi_source(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.read_requested = true;
        self.importer.catalog_target = self.packages.clone();
        self.importer.scan_progress = Some(ScanProgress::CheckingCache);
        self.importer.scan_started = Some(Instant::now());
        self.importer.scan_error = None;
        self.importer.roi = None;
        self.importer.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<Inspected> {
                let source = d1_roi::Source::open(&folder, |completed, total| {
                    let _ = sender.send(Event::ScanProgress(ScanProgress::ReadingItems {
                        completed,
                        total,
                        weapons: 0,
                    }));
                    ctx.request_repaint();
                })?;
                let armor = if source.report().missing_armor_tables.is_empty() {
                    source.armor()?
                } else {
                    Vec::new()
                };
                let weapons = if source.report().missing_armor_tables.is_empty() {
                    source.weapons()?
                } else {
                    Vec::new()
                };
                let root = root.join("importer/d1-roi");
                // Reserve a parent, then let the exporter create its fresh evidence directory.
                let parent = service::reserve_assets(&root, "inspection")?;
                let artifact = source.write_inspection(&parent.join("source"))?;
                Ok(Inspected {
                    report: source.report().clone(),
                    artifact,
                    armor,
                    weapons,
                    weapons_page: false,
                    weapon: None,
                    weapon_search: String::new(),
                    selected: BTreeSet::new(),
                    search: String::new(),
                    class: None,
                    exported: None,
                })
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::RoiInspected(Box::new(result)));
            ctx.request_repaint();
        });
    }

    pub(super) fn draw_roi_source(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Xbox Texture Library");
            ui.weak(
                self.importer
                    .settings
                    .d1_xg_library
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "xgpc.dll".to_owned()),
            );
            if ui.button("Choose Library").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Xbox Texture Library", &["dll"])
                    .pick_file()
            {
                self.importer.settings.d1_xg_library = Some(path);
                if let Err(error) = self.importer.save() {
                    self.importer.notice = error;
                }
            }
        });
        let Some(inspected) = &mut self.importer.roi else {
            return;
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut inspected.weapons_page, false, "Armor");
            ui.selectable_value(&mut inspected.weapons_page, true, "Weapons");
        });
        if inspected.weapons_page {
            self.draw_roi_weapons(ui);
            return;
        }
        ui.heading("Rise of Iron Armor");
        ui.label("Import armor and its linked ornaments as editable recipes. Both body variants and the source appearance are retained. Rendering and cloth motion still need verification in game.");
        ui.add_space(12.0);
        let report = &inspected.report;
        ui.label(format!(
            "{} packages · {} entries · {} models",
            report.package_count, report.entry_count, report.model_count
        ));
        if report.missing_armor_tables.is_empty() {
            ui.label("The source contains the item, text and art tables needed to locate armor.");
        } else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Missing armor data: {}.",
                    report.missing_armor_tables.join(", ")
                ),
            );
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open Inspection Folder").clicked()
                && let Some(parent) = inspected.artifact.parent()
                && let Err(error) = open_directory(parent)
            {
                self.importer.notice = error;
            }
            if let Some(path) = &inspected.exported {
                ui.label("Armor source export complete.");
                if ui.button("Open Export Folder").clicked()
                    && let Err(error) = open_directory(path)
                {
                    self.importer.notice = error;
                }
            }
        });
        if inspected.armor.is_empty() {
            return;
        }
        ui.separator();
        let mut export = false;
        let mut import = false;
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut inspected.search).hint_text("Search Armor"));
            egui::ComboBox::from_id_salt("roi-armor-class")
                .selected_text(class_name(inspected.class))
                .show_ui(ui, |ui| {
                    for class in [None, Some(0), Some(1), Some(2)] {
                        ui.selectable_value(&mut inspected.class, class, class_name(class));
                    }
                });
            export = ui
                .add_enabled(
                    !inspected.selected.is_empty(),
                    egui::Button::new(format!("Export Source ({})", inspected.selected.len())),
                )
                .clicked();
            import = ui
                .add_enabled(
                    !inspected.selected.is_empty(),
                    egui::Button::new(format!("Import Selected ({})", inspected.selected.len())),
                )
                .clicked();
        });
        let words: Vec<_> = inspected
            .search
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Select Visible").clicked() {
                inspected.selected.extend(
                    inspected
                        .armor
                        .iter()
                        .filter(|armor| {
                            inspected
                                .class
                                .is_none_or(|class| class == armor.class_type)
                                && words.iter().all(|word| {
                                    format!("{} {:08X}", armor.name, armor.hash)
                                        .to_lowercase()
                                        .contains(word)
                                })
                        })
                        .map(|armor| armor.hash),
                );
            }
            if ui.button("Clear Selection").clicked() {
                inspected.selected.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("roi-armor-list")
            .show(ui, |ui| {
                for armor in &inspected.armor {
                    if inspected
                        .class
                        .is_some_and(|class| class != armor.class_type)
                    {
                        continue;
                    }
                    let label = format!("{} {:08X}", armor.name, armor.hash).to_lowercase();
                    if !words.iter().all(|word| label.contains(word)) {
                        continue;
                    }
                    let mut selected = inspected.selected.contains(&armor.hash);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut selected, &armor.name).changed() {
                            if selected {
                                inspected.selected.insert(armor.hash);
                            } else {
                                inspected.selected.remove(&armor.hash);
                            }
                        }
                        ui.weak(format!(
                            "{} · {} · {:08X}",
                            class_name(Some(armor.class_type)),
                            armor.kind,
                            armor.hash
                        ));
                    });
                }
            });
        if export {
            self.export_roi_armor(ui.ctx());
        }
        if import {
            self.import_roi_armor(ui.ctx());
        }
    }

    fn draw_roi_weapons(&mut self, ui: &mut egui::Ui) {
        let Some(inspected) = &mut self.importer.roi else {
            return;
        };
        ui.heading("Rise of Iron Weapons");
        ui.label("Import source models, textures, icons and supported perks. Firing, reloads, audio and base stats come from a native weapon of the same type, chosen automatically. Unsupported talents are listed in the saved import report.");
        ui.label("Ordinary native weapons are tried before exotics, and the first with matching weapon and first-person rigs carries the import. Imported perks still need in-game verification.");
        ui.add(
            egui::TextEdit::singleline(&mut inspected.weapon_search).hint_text("Search Weapons"),
        );
        let words: Vec<_> = inspected
            .weapon_search
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("roi-weapons")
            .max_height(260.0)
            .show(ui, |ui| {
                for weapon in &inspected.weapons {
                    let text = format!("{} · {} · {:08X}", weapon.name, weapon.kind, weapon.hash);
                    if !words.iter().all(|word| text.to_lowercase().contains(word)) {
                        continue;
                    }
                    ui.selectable_value(&mut inspected.weapon, Some(weapon.hash), text);
                }
            });
        if ui
            .add_enabled(
                inspected.weapon.is_some(),
                egui::Button::new("Import Weapon"),
            )
            .clicked()
        {
            self.import_roi_weapon(ui.ctx());
        }
    }

    fn import_roi_weapon(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let Some(item) = inspected
            .weapons
            .iter()
            .find(|w| Some(w.hash) == inspected.weapon)
            .cloned()
        else {
            return;
        };
        let bases = self.roi_weapon_bases(&item);
        let Ok(root) = data_root() else { return };
        let native = PathBuf::from(&self.packages);
        let options = d1_roi::NativeOptions {
            xg_library: self.importer.settings.d1_xg_library.clone(),
        };
        self.run_import(ctx, vec![item.name.clone()], move |_, progress| {
            let model = service::model_directory(&root.join("models"), &item.name, item.hash)?;
            let output = service::reserve_assets(&model, "d1-roi")?.join("import");
            let source = d1_roi::Source::open(&folder, |_, _| {})?;
            source.prepare_weapon_on_base(item.hash, &native, &bases, &output, &options, progress)
        });
    }

    /// Stock native weapons of the source's type in the order the import tries them: a weapon of
    /// the same name, then ordinary weapons, then exotics, whose own exotic behavior would come
    /// with them.
    fn roi_weapon_bases(&self, weapon: &d1_roi::Weapon) -> Vec<u32> {
        let mut bases = self
            .donor_summaries
            .iter()
            .filter(|donor| {
                donor.type_name == weapon.kind
                    && self
                        .catalog
                        .as_ref()
                        .and_then(|c| c.item_definition_tag(donor.hash))
                        .is_some_and(crate::package_profile::is_stock_item_definition)
            })
            .collect::<Vec<_>>();
        bases.sort_by_key(|donor| {
            (
                !donor.name.eq_ignore_ascii_case(weapon.name.trim()),
                donor.rarity == sundial::investment::WeaponRarity::Exotic,
                !donor.collection_backed,
                donor.name.clone(),
                donor.hash,
            )
        });
        bases.into_iter().map(|donor| donor.hash).collect()
    }

    fn import_roi_armor(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let selected = inspected
            .armor
            .iter()
            .filter(|item| inspected.selected.contains(&item.hash))
            .cloned()
            .collect::<Vec<_>>();
        let Ok(root) = data_root() else {
            return;
        };
        let native = PathBuf::from(&self.packages);
        let options = d1_roi::NativeOptions {
            xg_library: self.importer.settings.d1_xg_library.clone(),
        };
        let gear = self.gear_donors_for(ItemKind::Armor).iter().filter(|donor| self.catalog.as_ref().and_then(|catalog| catalog.item_definition_tag(donor.hash)).is_none_or(crate::package_profile::is_stock_item_definition))
            .map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name,"bucket_hash":donor.bucket_hash,
                "class_type":self.catalog.as_ref().and_then(|catalog|catalog.item_class_type(donor.hash)),"collection_backed":donor.collection_backed})).collect::<Vec<_>>();
        let donors = serde_json::json!({"gear":gear});
        let names = selected.iter().map(|item| item.name.clone()).collect();
        self.run_import(ctx, names, move |index, progress| {
            let item = &selected[index];
            let model = service::model_directory(&root.join("models"), &item.name, item.hash)?;
            let output = service::reserve_assets(&model, "d1-roi")?.join("import");
            let source = d1_roi::Source::open(&folder, |_, _| {})?;
            source.prepare_armor(item.hash, &native, &donors, &output, &options, progress)
        });
    }

    fn export_roi_armor(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let selected: Vec<_> = inspected.selected.iter().copied().collect();
        let Ok(root) = data_root() else { return };
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.scan_started = Some(Instant::now());
        self.importer.scan_progress = Some(ScanProgress::CheckingCache);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<PathBuf> {
                let source = d1_roi::Source::open(&folder, |_, _| {})?;
                let output = service::reserve_assets(&root.join("importer/d1-roi"), "armor")?;
                source.write_inspection(&output.join("source"))?;
                for (index, item) in selected.iter().enumerate() {
                    let _ = sender.send(Event::ScanProgress(ScanProgress::ReadingItems {
                        completed: index,
                        total: selected.len(),
                        weapons: 0,
                    }));
                    ctx.request_repaint();
                    source.export_armor(*item, &output.join(format!("{item:08X}")))?;
                }
                Ok(output)
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::RoiExported(result));
            ctx.request_repaint();
        });
    }
}

fn class_name(class: Option<u8>) -> &'static str {
    match class {
        Some(0) => "Titan",
        Some(1) => "Hunter",
        Some(2) => "Warlock",
        _ => "All Classes",
    }
}
