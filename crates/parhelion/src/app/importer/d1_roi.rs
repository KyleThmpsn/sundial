use super::*;
use parhelion_import::d1_roi;

pub(super) struct Inspected {
    pub report: d1_roi::Inspection,
    pub artifact: PathBuf,
    pub armor: Vec<d1_roi::Armor>,
    pub weapons: Vec<d1_roi::Weapon>,
    pub artifacts: Vec<d1_roi::Artifact>,
    /// The source subclasses a whole grid import covers, each by its self-revive choice.
    pub subclasses: Vec<d1_roi::AbilityChoice>,
    /// Each listed subclass's strings tag, which the list draws its source icon from.
    pub subclass_strings: BTreeMap<u32, u32>,
}

impl PackageAuthoringApp {
    pub(super) fn scan_roi_source(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let (sender, receiver) = mpsc::channel();
        self.importer.task = Some(Task::new(receiver, Work::Scan(ScanProgress::CheckingCache)));
        self.importer.read_requested = true;
        self.importer.catalog_target = self.packages.clone();
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
                let artifacts = if source.report().missing_armor_tables.is_empty() {
                    source.artifacts()?
                } else {
                    Vec::new()
                };
                // A package set without the subclass tables lists none. Only a subclass whose
                // grid holds a self-revive choice has a whole grid import so far.
                let mut subclasses = source.revival_choices().unwrap_or_default();
                subclasses.sort_by_key(|choice| choice.subclass);
                subclasses.dedup_by_key(|choice| choice.subclass);
                let subclass_strings = subclasses
                    .iter()
                    .filter_map(|choice| {
                        Some((
                            choice.subclass,
                            source.subclass_strings(choice.subclass).ok()?,
                        ))
                    })
                    .collect();
                let artifact = write_inspection(&root.join("importer"), source.report())?;
                Ok(Inspected {
                    report: source.report().clone(),
                    artifact,
                    armor,
                    weapons,
                    artifacts,
                    subclasses,
                    subclass_strings,
                })
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::RoiInspected(Box::new(result)));
            ctx.request_repaint();
        });
    }

    pub(super) fn draw_roi_details(&mut self, ui: &mut egui::Ui) {
        let Some(inspected) = &self.importer.roi else {
            return;
        };
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
        });
    }

    /// `reimport` names the saved namespace of each source item a library reimport updates.
    pub(super) fn import_roi_selected(
        &mut self,
        ctx: &egui::Context,
        reimport: &BTreeMap<u32, Saved>,
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
                items.push((item.hash, item.name.clone(), ItemKind::Armor, None));
            }
        }
        for item in &inspected.artifacts {
            if selected.contains(&item.hash) {
                items.push((item.hash, item.name.clone(), ItemKind::Artifact, None));
            }
        }
        for item in &inspected.weapons {
            if selected.contains(&item.hash) {
                items.push((
                    item.hash,
                    item.name.clone(),
                    ItemKind::Weapon,
                    Some(self.roi_weapon_bases(item)),
                ));
            }
        }
        let subclasses: BTreeMap<u32, d1_roi::AbilityChoice> = inspected
            .subclasses
            .iter()
            .filter(|choice| selected.contains(&choice.subclass))
            .map(|choice| (choice.subclass, choice.clone()))
            .collect();
        for choice in subclasses.values() {
            items.push((
                choice.subclass,
                choice.subclass_name.trim().to_owned(),
                ItemKind::Subclass,
                None,
            ));
        }
        let subclass_base = self.roi_subclass_base();
        if items.is_empty() {
            return;
        }
        let Ok(root) = data_root() else { return };
        let xg_library = self.importer.settings.d1_xg_library.clone();
        let ornaments = self.importer.settings.d1_roi_ornaments;
        let reimport = reimport.clone();
        let namespaces: BTreeMap<u32, String> = items
            .iter()
            .filter_map(|(hash, ..)| {
                Some((*hash, self.saved_namespace(Source::D1RiseOfIron, *hash)?))
            })
            .collect();
        let gear = self.gear_donors_for(ItemKind::Armor).iter().filter(|donor| self.catalog.as_ref().and_then(|catalog|catalog.item_definition_tag(donor.hash)).is_none_or(crate::package::profile::is_stock_item_definition))
            .map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name,"bucket_hash":donor.bucket_hash,
                "class_type":self.catalog.as_ref().and_then(|catalog|catalog.item_class_type(donor.hash)),"collection_backed":donor.collection_backed})).collect::<Vec<_>>();
        let donors = serde_json::json!({"gear":gear});
        let names = items.iter().map(|(_, name, ..)| name.clone()).collect();
        self.run_import(ctx, names, move |index, native, progress| {
            let (hash, _, item_kind, bases) = &items[index];
            let item = service::item_directory(
                &root.join(service::IMPORTS),
                Source::D1RiseOfIron.folder(),
                *hash,
            )?;
            let kind = match item_kind {
                ItemKind::Weapon => "weapon",
                ItemKind::Artifact => "artifact",
                ItemKind::Subclass => "subclass",
                _ => "armor",
            };
            let output = service::reserve_assets(&item, kind)?;
            if *item_kind == ItemKind::Subclass {
                let choice = subclasses
                    .get(hash)
                    .ok_or_else(|| anyhow::anyhow!("The source subclass is no longer listed"))?;
                let (base, base_name) = subclass_base.clone().ok_or_else(|| {
                    anyhow::anyhow!("No installed stock solar Warlock subclass to build on")
                })?;
                let namespace = reimport
                    .get(hash)
                    .map(|saved| saved.namespace.clone())
                    .or_else(|| namespaces.get(hash).cloned())
                    .unwrap_or_else(|| d1_roi::namespace(&choice.subclass_name, *hash));
                let draft = crate::subclass::import::draft(
                    &namespace,
                    base,
                    &base_name,
                    choice.subclass_name.trim(),
                )
                .map_err(anyhow::Error::msg)?;
                // The grid preparation loads the native catalog, which accepts a game folder only. A
                // stock view holds the packages and the decoder but no executable, so it gets an
                // empty one. The game's own folder already has its executable and is left alone.
                if let Some(root) = native.parent() {
                    let executable = root.join("destiny2.exe");
                    let view = root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(".parhelion-package-view-"));
                    if view && !executable.exists() {
                        std::fs::write(&executable, [])?;
                    }
                }
                progress("Translating the subclass grid…".into());
                // The self-revive choice takes a passive node of the middle tree, whose Super
                // node holds the source Super.
                let prepared =
                    crate::subclass::import::prepare_grid(&crate::subclass::import::Request {
                        source_packages: &folder,
                        native_packages: native,
                        output: &output.join("prepared"),
                        recipe: &draft,
                        choice,
                        entry: crate::subclass::AttunementPath::Middle.entries()[3],
                    })
                    .map_err(anyhow::Error::msg)?;
                let recipe = output.join("recipe.parhelion.json");
                prepared.recipe().save_json(&recipe)?;
                return Ok(recipe);
            }
            let source = d1_roi::Source::open(&folder, |_, _| {})?;
            // A reimported item brings ornaments as its recipe did, and any other as the
            // importer's Ornaments choice says.
            let saved = reimport.get(hash);
            let options = d1_roi::NativeOptions {
                xg_library: xg_library.clone(),
                namespace: saved
                    .map(|saved| saved.namespace.clone())
                    .or_else(|| namespaces.get(hash).cloned()),
                ornaments: saved.map_or(ornaments, |saved| saved.ornaments),
            };
            match bases {
                Some(bases) => {
                    source.prepare_weapon_on_base(*hash, native, bases, &output, &options, progress)
                }
                None if *item_kind == ItemKind::Artifact => {
                    source.prepare_artifact(*hash, native, &output, &options, progress)
                }
                None => source.prepare_armor(*hash, native, &donors, &output, &options, progress),
            }
        });
    }

    /// The one stock solar Warlock subclass, which a source subclass grid is laid out on.
    pub(super) fn roi_subclass_base(&self) -> Option<(u32, String)> {
        let subclasses = self
            .catalog
            .as_ref()?
            .subclasses(crate::package::profile::is_stock_item_definition);
        let mut bases = subclasses.into_iter().filter(|candidate| {
            candidate.class_type == 2
                && candidate.damage_type == Some(sundial::investment::WeaponDamageType::Solar)
        });
        let base = bases.next()?;
        bases.next().is_none().then_some((base.hash, base.name))
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
                        .is_some_and(crate::package::profile::is_stock_item_definition)
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
}

/// Writes a source read's report to the importer's `catalog`, named for its importer as the other
/// sources' files there are, replacing the previous read's whole. It removes the
/// `d1-roi/inspection-<n>` folders that earlier reads kept one report each in.
fn write_inspection(importer: &Path, report: &d1_roi::Inspection) -> anyhow::Result<PathBuf> {
    let catalog = importer.join("catalog");
    std::fs::create_dir_all(&catalog)?;
    let path = catalog.join("d1_roi.json");
    let mut file = tempfile::NamedTempFile::new_in(&catalog)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.persist(&path)?;
    let former = importer.join("d1-roi");
    if let Ok(entries) = std::fs::read_dir(&former) {
        for entry in entries.flatten() {
            let inspection = entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("inspection-"))
                .is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|c| c.is_ascii_digit())
                });
            if inspection && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        // Armor source exports made there earlier keep the folder.
        let _ = std::fs::remove_dir(&former);
    }
    Ok(path)
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
        rows.extend(self.artifacts.iter().map(|item| Weapon {
            hash: item.hash,
            name: item.name.clone(),
            weapon_type: item.kind.clone(),
            bucket_hash: Some(1_506_418_338),
            class_type: Some(item.class_type),
            rarity: Some(item.rarity),
            icon_index: Some(item.strings as usize),
            ..Weapon::default()
        }));
        rows.extend(self.subclasses.iter().map(|choice| {
            Weapon {
                hash: choice.subclass,
                name: choice.subclass_name.trim().to_owned(),
                weapon_type: "Subclass".to_owned(),
                bucket_hash: Some(3_284_755_031),
                class_type: Some(2),
                icon_index: self
                    .subclass_strings
                    .get(&choice.subclass)
                    .map(|strings| *strings as usize),
                ..Weapon::default()
            }
        }));
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
