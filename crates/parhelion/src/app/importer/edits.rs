//! The edits a reimport finds in a saved recipe, and what the author chose to do with them.
//!
//! A saved recipe is compared with the recipe its earlier conversion prepared, which stays in
//! that conversion's folder. A recipe whose earlier conversion kept none has no edits a reimport
//! can tell from the converter's own changes, so it merges as it always has.
//!
//! An import never waits for the answer. With no saved preference it leaves each edited recipe
//! as it is and keeps its conversion, and the window asks once the import ends.
use super::*;
use crate::preferences::ReimportEdits;
use std::time::SystemTime;

/// What a reimport does with the edits a saved recipe carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Choice {
    /// Merges the conversion into the saved recipe and puts its perk edits back where they fit.
    Reapply,
    /// Replaces the saved recipe with the conversion's, after backing it up.
    Overwrite,
}

impl Choice {
    /// The saved preference's choice, or `None` when it asks.
    pub fn preferred(preference: ReimportEdits) -> Option<Self> {
        match preference {
            ReimportEdits::Ask => None,
            ReimportEdits::Reapply => Some(Self::Reapply),
            ReimportEdits::Overwrite => Some(Self::Overwrite),
        }
    }

    fn preference(self) -> ReimportEdits {
        match self {
            Self::Reapply => ReimportEdits::Reapply,
            Self::Overwrite => ReimportEdits::Overwrite,
        }
    }
}

/// An edited recipe an import converted and left unsaved, for the author's choice.
#[derive(Clone)]
pub(super) struct Pending {
    /// The saved recipe's name.
    pub name: String,
    /// The recipe the new conversion prepared.
    pub recipe: PathBuf,
    /// What the saved recipe changed from the recipe its own conversion prepared.
    pub changes: Vec<String>,
}

/// The edited recipes that wait for a choice, and the window's checkbox.
#[derive(Default)]
pub(super) struct Review {
    pub pending: Vec<Pending>,
    /// Whether the choice is saved as the preference.
    remember: bool,
}

/// The recipe the conversion `saved` names prepared, before any edit. `None` when that
/// conversion's folder holds no recipe of the item.
pub(super) fn pristine(saved: &WeaponRecipe) -> Option<WeaponRecipe> {
    let graph = saved.overrides.imported_graph.as_ref()?;
    let conversion = parhelion_import::conversion_folder(&graph.directory)?;
    // Each importer keeps its prepared recipe in its own working folder, a few levels down.
    const DEPTH: usize = 3;
    const LARGEST: u64 = 64 << 20;
    let mut found: Option<(SystemTime, WeaponRecipe)> = None;
    let mut folders = vec![(conversion, 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                let graphs = depth == 0
                    && [parhelion_import::GRAPH, parhelion_import::ORNAMENTS]
                        .iter()
                        .any(|name| entry.file_name() == *name);
                if !graphs && depth < DEPTH {
                    folders.push((path, depth + 1));
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "json")
                || metadata.len() > LARGEST
            {
                continue;
            }
            let Ok(recipe) = WeaponRecipe::load_json(&path) else {
                continue;
            };
            if recipe.identity.item_hash != saved.identity.item_hash || recipe.kind != saved.kind {
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if found.as_ref().is_none_or(|(newest, _)| modified > *newest) {
                found = Some((modified, recipe));
            }
        }
    }
    found.map(|(_, recipe)| recipe)
}

/// Whether `saved` differs from `pristine`, the recipe its conversion prepared, in more than a
/// reimport's own merge writes.
pub(super) fn edited(saved: &WeaponRecipe, pristine: &WeaponRecipe) -> bool {
    !saved.same_saved_content(&comparable(saved, pristine))
}

/// `pristine` with the fields a reimport's own merge writes into `saved`. The saved recipe names
/// the graph where the library keeps it, and a merged weapon names its conversion's carrier as
/// its appearance.
fn comparable(saved: &WeaponRecipe, pristine: &WeaponRecipe) -> WeaponRecipe {
    let mut pristine = pristine.clone();
    pristine.overrides.imported_graph = saved.overrides.imported_graph.clone();
    if pristine.presentation_donor.is_none()
        && saved.presentation_donor.as_ref() == Some(&pristine.donor)
    {
        pristine.presentation_donor = saved.presentation_donor.clone();
    }
    pristine
}

/// What `saved` changed from `pristine`, the recipe its conversion prepared, one line for each
/// recipe field that differs, with both values where they are short enough to show.
pub(super) fn changes(saved: &WeaponRecipe, pristine: &WeaponRecipe) -> Vec<String> {
    use serde_json::Value;
    let fields = |recipe: &WeaponRecipe| {
        let mut fields = match serde_json::to_value(recipe) {
            Ok(Value::Object(fields)) => fields,
            _ => return Vec::new(),
        };
        // The overrides are listed beside the recipe's own fields.
        let overrides = match fields.remove("overrides") {
            Some(Value::Object(overrides)) => overrides,
            _ => serde_json::Map::new(),
        };
        fields.into_iter().chain(overrides).collect::<Vec<_>>()
    };
    let before: BTreeMap<_, _> = fields(&comparable(saved, pristine)).into_iter().collect();
    let after: BTreeMap<_, _> = fields(saved).into_iter().collect();
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut lines = Vec::new();
    for key in keys {
        let (was, now) = (before.get(key), after.get(key));
        if was == now {
            continue;
        }
        let label = label(key);
        let empty = |value: Option<&Value>| value.is_none_or(Value::is_null);
        lines.push(match (now.and_then(brief), was.and_then(brief)) {
            _ if empty(now) => format!("{label} removed"),
            (Some(now), _) if empty(was) => format!("{label} added: {now}"),
            _ if empty(was) => format!("{label} added"),
            (Some(now), Some(was)) => format!("{label}: {now}. Imported as {was}"),
            _ => match (now, was) {
                (Some(Value::Array(now)), Some(Value::Array(was))) => {
                    let rows: Vec<_> = (0..now.len().max(was.len()))
                        .filter(|&row| now.get(row) != was.get(row))
                        .map(|row| (row + 1).to_string())
                        .collect();
                    format!("{label} changed in {}", rows.join(", "))
                }
                _ => format!("{label} changed"),
            },
        });
    }
    if lines.is_empty() {
        lines.push("Saved values changed".to_owned());
    }
    lines
}

/// A recipe field's name as the workbench shows it, or its own name in title case.
fn label(key: &str) -> String {
    let known = match key {
        "flavor" => "Description",
        "donor" => "Base Weapon",
        "presentation_donor" => "Appearance",
        "icon_donor" => "Icon",
        "socket_columns" => "Perk Sockets",
        "socket_plug_variants" => "Custom Perks",
        "investment_stats" => "Stats",
        "removed_investment_stats" => "Removed Stats",
        "base_sandbox_perks" => "Base Perks",
        "modern_damage_type" => "Damage Type",
        "inventory_slot" => "Slot",
        "ammo_type" => "Ammo",
        "icon_edit" => "Icon Artwork",
        "hud_icon" => "HUD Icon",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    key.split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A value short enough to show: text, a number, a switch, or a named item.
fn brief(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(on) => if *on { "On" } else { "Off" }.to_owned(),
        Value::Object(fields) => fields.get("name")?.as_str()?.to_owned(),
        _ => return None,
    };
    (!text.is_empty() && text.chars().count() <= 80).then_some(text)
}

fn variants(
    recipe: &WeaponRecipe,
    socket: usize,
) -> Vec<&crate::recipe::WeaponSocketPlugVariantRecipe> {
    recipe
        .overrides
        .socket_plug_variants
        .iter()
        .filter(|variant| usize::from(variant.socket_index) == socket)
        .collect()
}

/// Puts the perk edits of `saved` back into `updated`, which holds the new conversion's sockets.
/// A socket whose column or private plugs `saved` changed from `pristine` keeps the saved ones,
/// and every other socket follows the conversion. Returns whether the edits were placed. They
/// are not when the conversion is built on another base or lays its sockets out differently,
/// since a saved socket then names plugs of a base the recipe no longer has.
pub(super) fn reapply_sockets(
    pristine: &WeaponRecipe,
    saved: &WeaponRecipe,
    updated: &mut WeaponRecipe,
) -> bool {
    let (before, edits) = (&pristine.overrides, &saved.overrides);
    let base_perks = edits.base_sandbox_perks != before.base_sandbox_perks;
    let count = edits.socket_columns.len().max(before.socket_columns.len());
    let edited: Vec<usize> = (0..count)
        .filter(|&socket| {
            edits.socket_columns.get(socket) != before.socket_columns.get(socket)
                || variants(saved, socket) != variants(pristine, socket)
        })
        .collect();
    if edited.is_empty() && !base_perks {
        return true;
    }
    let columns = &mut updated.overrides.socket_columns;
    if updated.donor.item_hash != saved.donor.item_hash
        || pristine.donor.item_hash != saved.donor.item_hash
        || columns.len() != before.socket_columns.len()
        || edits.socket_columns.len() < columns.len()
    {
        return false;
    }
    columns.extend_from_slice(&edits.socket_columns[columns.len()..]);
    for &socket in &edited {
        columns[socket] = edits.socket_columns[socket].clone();
    }
    let plugs = &mut updated.overrides.socket_plug_variants;
    plugs.retain(|variant| !edited.contains(&usize::from(variant.socket_index)));
    plugs.extend(
        edits
            .socket_plug_variants
            .iter()
            .filter(|variant| edited.contains(&usize::from(variant.socket_index)))
            .cloned(),
    );
    plugs.sort_by_key(|variant| (variant.socket_index, variant.choice_index));
    if base_perks {
        updated.overrides.base_sandbox_perks = edits.base_sandbox_perks.clone();
    }
    true
}

impl PackageAuthoringApp {
    /// The preference for edited recipes, beside the importer's own.
    pub(super) fn draw_reimport_edits_preference(&mut self, ui: &mut egui::Ui) {
        let label = |preference| match preference {
            ReimportEdits::Ask => "Ask Each Time",
            ReimportEdits::Reapply => "Reapply Edits",
            ReimportEdits::Overwrite => "Overwrite Edits",
        };
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Edited Recipes on Reimport");
            egui::ComboBox::from_id_salt("parhelion-reimport-edits")
                .selected_text(label(self.reimport_edits))
                .show_ui(ui, |ui| {
                    for preference in [
                        ReimportEdits::Ask,
                        ReimportEdits::Reapply,
                        ReimportEdits::Overwrite,
                    ] {
                        changed |= ui
                            .selectable_value(
                                &mut self.reimport_edits,
                                preference,
                                label(preference),
                            )
                            .changed();
                    }
                });
        });
        if changed {
            self.save_workbench_preferences();
        }
    }

    /// Asks what to do with the edited recipes the finished import left unsaved.
    pub(super) fn draw_import_edits(&mut self, ctx: &egui::Context) {
        if self.importer.edits.pending.is_empty()
            || self.importer.refresh.running()
            || !self.importer_idle()
        {
            return;
        }
        let pending = &self.importer.edits.pending;
        let summary = match pending.len() {
            1 => "This recipe was changed after it was imported. Its new conversion is ready and has not been applied yet.".to_owned(),
            count => format!(
                "These {count} recipes were changed after they were imported. Their new conversions are ready and have not been applied yet."
            ),
        };
        let mut remember = self.importer.edits.remember;
        let mut choice = None;
        let available = ctx.content_rect().size() - egui::vec2(48.0, 48.0);
        let width = 440.0_f32.min(available.x - 40.0).max(240.0);
        egui::Window::new("Edited Recipes")
            .id(egui::Id::new("parhelion-import-edits"))
            .collapsible(false)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(width)
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(20))
            .show(ctx, |ui| {
                style::workbench_style(ui);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 8.0;
                let muted = style::secondary(ui.visuals());
                ui.label(egui::RichText::new("Importer · Library").color(muted));
                ui.label(egui::RichText::new("Edited Recipes").size(24.0).strong());
                ui.label(summary);
                // Each recipe opens to what it changed from the recipe its conversion prepared.
                egui::ScrollArea::vertical()
                    .id_salt("import-edits-recipes")
                    .max_height((available.y - 320.0).max(80.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (index, item) in pending.iter().enumerate() {
                            egui::CollapsingHeader::new(&item.name)
                                .id_salt(("import-edits-recipe", index))
                                .show(ui, |ui| {
                                    for change in &item.changes {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(change).color(muted),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                        }
                    });
                ui.label("Reapply Edits keeps your changes on the new conversion wherever they still fit. Overwrite Edits discards them and uses the new conversion as imported.");
                ui.label(
                    egui::RichText::new(
                        "Either way, a recipe that loses changes is backed up first.",
                    )
                    .color(muted),
                );
                ui.checkbox(&mut remember, "Save This Preference and Don't Ask Again")
                    .on_hover_text("Change it later in Parhelion Preferences.");
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
                    if ui.add(style::primary(ui, "Reapply Edits")).clicked() {
                        choice = Some(Choice::Reapply);
                    }
                    if ui.button("Overwrite Edits").clicked() {
                        choice = Some(Choice::Overwrite);
                    }
                });
            });
        self.importer.edits.remember = remember;
        let Some(choice) = choice else {
            return;
        };
        if std::mem::take(&mut self.importer.edits.remember) {
            self.reimport_edits = choice.preference();
            self.save_workbench_preferences();
        }
        self.apply_import_edits(ctx, choice);
    }

    /// Saves the edited recipes that waited, as `choice` says, on the importer's worker.
    fn apply_import_edits(&mut self, ctx: &egui::Context, choice: Choice) {
        let Some(library) = self.sweeping_library() else {
            return;
        };
        let pending = std::mem::take(&mut self.importer.edits.pending);
        let existing: Vec<_> = self
            .recipe_entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.identity_hash))
            .collect();
        let imports = data_root().ok().map(|root| root.join(service::IMPORTS));
        // An open recipe keeps the conversion it names until it is saved or closed.
        let open: Vec<_> = serde_json::to_value(&self.recipe).into_iter().collect();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.importer.task = Some(Task::new(
            receiver,
            Work::Import {
                progress: Progress {
                    total: pending.len(),
                    done: 0,
                    slots: vec![None],
                    finishing: false,
                },
                cancel: cancel.clone(),
            },
        ));
        self.importer.outcome = None;
        self.importer.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut paths = Vec::new();
            let mut errors = Vec::new();
            let mut items = BTreeSet::new();
            let mut done = 0;
            for item in &pending {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let _ = sender.send(Event::Slot(0, Some(item.name.clone())));
                ctx.request_repaint();
                match save_import(&library, &existing, &item.recipe, &mut |_, _| Some(choice)) {
                    Ok(Some((path, note))) => {
                        paths.push(path);
                        errors.extend(note.map(|note| format!("{}: {note}", item.name)));
                        items.extend(
                            imports
                                .as_deref()
                                .and_then(|imports| item_folder(imports, &item.recipe)),
                        );
                    }
                    Ok(None) => {}
                    Err(error) => errors.push(format!("{}: {error}", item.name)),
                }
                done += 1;
                let _ = sender.send(Event::Slot(0, None));
                let _ = sender.send(Event::Done(done));
                ctx.request_repaint();
            }
            let _ = sender.send(Event::Finishing);
            ctx.request_repaint();
            let removed = remove_earlier_conversions(
                &library,
                &items,
                &open,
                parhelion_import::store::Shared::default(),
            );
            let _ = sender.send(Event::Imported {
                paths,
                errors,
                cancelled: done < pending.len(),
                removed,
                // A cancelled recipe waits for the next answer.
                edited: pending[done..].to_vec(),
            });
            ctx.request_repaint();
        });
    }
}
