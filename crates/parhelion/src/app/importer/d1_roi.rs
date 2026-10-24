use super::*;
use parhelion_import::d1_roi;

pub(super) struct Inspected {
    pub report: d1_roi::Inspection,
    pub artifact: PathBuf,
    pub armor: Vec<d1_roi::Armor>,
    pub weapons: Vec<d1_roi::Weapon>,
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
        self.importer.weapons.clear();
        self.importer.selected.clear();
        self.importer.icons = icons::Icons::default();
        self.importer.browser.dirty = true;
        self.importer.browser.reset_scroll = true;
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
                    exported: None,
                })
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::RoiInspected(Box::new(result)));
            ctx.request_repaint();
        });
    }

    pub(super) fn draw_roi_details(&mut self, ui: &mut egui::Ui) {
        let ready = self.importer.tools_ready();
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let mut export = false;
        let selected = inspected
            .armor
            .iter()
            .filter(|item| self.importer.selected.contains(&item.hash))
            .count();
        ui.menu_button("Source Details", |ui| {
            ui.weak(format!(
                "{} packages · {} entries · {} models",
                inspected.report.package_count,
                inspected.report.entry_count,
                inspected.report.model_count
            ));
            if !inspected.report.missing_armor_tables.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "Missing data: {}",
                        inspected.report.missing_armor_tables.join(", ")
                    ),
                );
            }
            if ui.button("Open Inspection Folder").clicked()
                && let Some(parent) = inspected.artifact.parent()
                && let Err(error) = open_directory(parent)
            {
                self.importer.notice = error;
            }
            export = ui
                .add_enabled(
                    ready && selected > 0,
                    egui::Button::new(format!("Export Armor Source ({selected})")),
                )
                .clicked();
            if let Some(path) = &inspected.exported
                && ui.button("Open Export Folder").clicked()
                && let Err(error) = open_directory(path)
            {
                self.importer.notice = error;
            }
        });
        if export {
            self.export_roi_armor(ui.ctx());
        }
    }

    /// `reimport` names the saved namespace of each source item a library reimport updates.
    pub(super) fn import_roi_selected(
        &mut self,
        ctx: &egui::Context,
        reimport: &BTreeMap<u32, String>,
    ) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let selected: BTreeSet<_> = self
            .importer
            .selected_weapons()
            .map(|item| item.hash)
            .collect();
        let mut items = Vec::new();
        for item in &inspected.armor {
            if selected.contains(&item.hash) {
                items.push((item.hash, item.name.clone(), None));
            }
        }
        for item in &inspected.weapons {
            if selected.contains(&item.hash) {
                items.push((
                    item.hash,
                    item.name.clone(),
                    Some(self.roi_weapon_bases(item)),
                ));
            }
        }
        if items.is_empty() {
            return;
        }
        let Ok(root) = data_root() else { return };
        let xg_library = self.importer.settings.d1_xg_library.clone();
        let reimport = reimport.clone();
        let gear = self.gear_donors_for(ItemKind::Armor).iter().filter(|donor| self.catalog.as_ref().and_then(|catalog|catalog.item_definition_tag(donor.hash)).is_none_or(crate::package_profile::is_stock_item_definition))
            .map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name,"bucket_hash":donor.bucket_hash,
                "class_type":self.catalog.as_ref().and_then(|catalog|catalog.item_class_type(donor.hash)),"collection_backed":donor.collection_backed})).collect::<Vec<_>>();
        let donors = serde_json::json!({"gear":gear});
        let names = items.iter().map(|(_, name, _)| name.clone()).collect();
        self.run_import(ctx, names, move |index, native, progress| {
            let (hash, name, bases) = &items[index];
            let model = service::model_directory(&root.join("models"), name, *hash)?;
            let output = service::reserve_assets(&model, "d1-roi")?.join("import");
            let source = d1_roi::Source::open(&folder, |_, _| {})?;
            let options = d1_roi::NativeOptions {
                xg_library: xg_library.clone(),
                namespace: reimport.get(hash).cloned(),
            };
            match bases {
                Some(bases) => {
                    source.prepare_weapon_on_base(*hash, native, bases, &output, &options, progress)
                }
                None => source.prepare_armor(*hash, native, &donors, &output, &options, progress),
            }
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

    fn export_roi_armor(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Some(inspected) = &self.importer.roi else {
            return;
        };
        let selected: Vec<_> = inspected
            .armor
            .iter()
            .filter(|item| self.importer.selected.contains(&item.hash))
            .map(|item| item.hash)
            .collect();
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

impl Inspected {
    pub(super) fn rows(&self) -> Vec<Weapon> {
        let mut rows: Vec<_> = self
            .armor
            .iter()
            .map(|item| Weapon {
                hash: item.hash,
                name: item.name.clone(),
                weapon_type: item.kind.clone(),
                bucket_hash: [3448274439, 3551918588, 14239492, 20886954, 1585787867]
                    .get(usize::from(item.slot).saturating_sub(1))
                    .copied(),
                class_type: Some(item.class_type),
                rarity: Some(item.rarity),
                icon_index: Some(item.strings as usize),
                ..Weapon::default()
            })
            .collect();
        rows.extend(self.weapons.iter().map(|item| Weapon {
            hash: item.hash,
            name: item.name.clone(),
            weapon_type: item.kind.clone(),
            rarity: Some(item.rarity),
            ammo: Some(u16::from(item.slot - 5)),
            icon_index: Some(item.strings as usize),
            ..Weapon::default()
        }));
        rows.sort_by(|a, b| (&a.name, a.hash).cmp(&(&b.name, b.hash)));
        rows
    }
}
