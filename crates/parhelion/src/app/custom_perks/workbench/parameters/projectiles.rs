use super::*;

/// What the picker's cached rows were built from.
#[derive(Clone, PartialEq)]
struct ChoiceKey {
    catalog: usize,
    entries: usize,
    perk_names: usize,
    item_names: usize,
    filter: u8,
    unidentified: bool,
    query: String,
}

/// The picker's rows as catalog indexes and labels, in list order, with the families of variants
/// among them.
type Choices = Arc<(
    Vec<(usize, String)>,
    Vec<super::super::assets::variants::Group>,
)>;

impl PerkEditor {
    /// Display names for every catalog asset, numbered as variants where names repeat. The
    /// picker and the property headings share them, so both name an asset the same way.
    pub(super) fn projectile_labels_for(
        &self,
        ctx: &egui::Context,
        catalog: &Arc<entity::catalog::Catalog>,
    ) -> Arc<BTreeMap<u32, String>> {
        let id = egui::Id::new((
            "projectile-display-names",
            Arc::as_ptr(catalog) as usize,
            self.projectile_labels.len(),
            self.item_names.len(),
        ));
        ctx.data_mut(|data| {
            data.get_temp_mut_or_insert_with(id, || {
                Arc::new(catalog.discovery_labels_with(
                    |index| self.projectile_labels.get(&index).cloned(),
                    |item| self.item_names.get(&item).cloned(),
                ))
            })
            .clone()
        })
    }

    /// Carry mapped properties by meaning. Native field offsets are asset-specific.
    pub(super) fn select_projectile(
        &mut self,
        loaded: &PrivatePerkRuntimeGraph,
        source: u32,
        selected: Option<u32>,
    ) {
        if let Some((_, effective)) = loaded
            .projectile_slots
            .iter()
            .find(|(tag, _)| *tag == source)
        {
            let parameters = movement::mapped(loaded)
                .into_iter()
                .filter(|(tag, _)| tag == effective)
                .map(|(_, parameter)| parameter)
                .collect::<Vec<_>>();
            let carry = parameters
                .iter()
                .filter(|parameter| {
                    parameter.is_modified(&self.draft)
                        && parameters
                            .iter()
                            .filter(|other| other.kind == parameter.kind)
                            .count()
                            == 1
                })
                .filter_map(|parameter| {
                    parameter
                        .value(&self.draft)
                        .ok()
                        .map(|value| (parameter.kind, value.to_bits()))
                })
                .collect();
            self.pending_movement = Some((selected.unwrap_or(source), carry));
            let belongs = |locator: &WeaponRuntimeFieldLocator| {
                loaded
                    .graphs
                    .iter()
                    .filter(|(tag, _)| tag == effective)
                    .flat_map(|(_, graph)| graph.fields())
                    .any(|field| guided::equivalent(loaded, &field.locator, locator))
            };
            self.draft.retain(|value| !belongs(&value.locator));
            self.value_text.retain(|(locator, _), _| !belongs(locator));
        }
        self.projectile_draft
            .retain(|selection| selection.source_graph != source);
        if let Some(selected) = selected {
            self.projectile_draft.push(ProjectileSelection {
                source_graph: source,
                donor_graph: selected,
            });
        }
        self.projectile_draft
            .sort_by_key(|selection| selection.source_graph);
        self.parameter_error = None;
    }

    /// Draws every projectile slot in one list. Returns whether a selection changed.
    pub(super) fn draw_projectiles(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) -> bool {
        if loaded.projectile_slots.is_empty() {
            return false;
        }
        ui.strong("Projectiles and Emitters");
        self.draw_projectile_notes(ui, loaded);
        let mut change = None;
        for (ordinal, &(source, _)) in loaded.projectile_slots.iter().enumerate() {
            if loaded.projectile_slots.len() > 1 {
                ui.label(format!("Asset {}", ordinal + 1));
            }
            if let Some(selected) = self.draw_projectile_slot(ui, loaded, source) {
                change = Some((source, selected));
            }
        }
        if let Some((source, selected)) = change {
            self.select_projectile(loaded, source, selected);
            return true;
        }
        ui.add_space(8.0);
        false
    }

    /// The catalog notices shown once above the projectile pickers.
    fn draw_projectile_notes(&self, ui: &mut egui::Ui, loaded: &PrivatePerkRuntimeGraph) {
        if !loaded.projectile_catalog.errors.is_empty() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "{} resources could not be read.",
                    loaded.projectile_catalog.errors.len()
                ),
            );
            egui::CollapsingHeader::new("Catalog Read Errors").show(ui, |ui| {
                for error in &loaded.projectile_catalog.errors {
                    ui.label(error);
                }
            });
        }
    }

    /// One projectile or emitter picker. Returns a new selection when the user picked one,
    /// with `None` inside meaning the original asset.
    fn draw_projectile_slot(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        source: u32,
    ) -> Option<Option<u32>> {
        {
            let labels = self.projectile_labels_for(ui.ctx(), &loaded.projectile_catalog);
            let label_for = |entry: &entity::catalog::Entry| {
                labels
                    .get(&entry.graph)
                    .cloned()
                    .unwrap_or_else(|| entry.discovery_label_with(|_| None, |_| None))
            };
            let original = self
                .projectile_draft
                .iter()
                .find(|selection| selection.source_graph == source)
                .map(|selection| selection.donor_graph);
            let mut selected = original;
            let tag = original.unwrap_or(source);
            let current = loaded
                .projectile_catalog
                .entries
                .iter()
                .find(|choice| choice.graph == tag);
            let label = current.map_or_else(|| format!("Missing Asset · 0x{tag:08X}"), label_for);
            use super::super::{assets, assets::variants, pickers};
            let picked = pickers::browser(
                ui,
                ("perk-projectile", source),
                &label,
                "Choose a Projectile or Emitter",
                &mut String::new(),
                |ui, query, reset, height| {
                    let filter_id = ui.make_persistent_id("projectile-kind");
                    let mut filter = ui.data(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
                    let before = filter;
                    let mut visibility = (false, false);
                    let mut use_original = false;
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut filter, 0, "All Types");
                        ui.selectable_value(&mut filter, 1, "Projectiles");
                        ui.selectable_value(&mut filter, 2, "Emitters");
                        ui.separator();
                        visibility = pickers::show_all(ui, "projectile-swap");
                        use_original = ui
                            .add_enabled(original.is_some(), egui::Button::new("Use Original"))
                            .clicked();
                    });
                    if use_original {
                        return Some(None);
                    }
                    ui.data_mut(|state| state.insert_temp(filter_id, filter));
                    let key = ChoiceKey {
                        catalog: Arc::as_ptr(&loaded.projectile_catalog) as usize,
                        entries: loaded.projectile_catalog.entries.len(),
                        perk_names: self.projectile_labels.len(),
                        item_names: self.item_names.len(),
                        filter,
                        unidentified: visibility.0,
                        query: query.replace("0x", ""),
                    };
                    let cache_id = egui::Id::new(("projectile-choices", source));
                    let cached = ui
                        .data(|state| state.get_temp::<(ChoiceKey, Choices)>(cache_id))
                        .filter(|(built, _)| *built == key)
                        .map(|(_, choices)| choices);
                    let choices = cached.unwrap_or_else(|| {
                        let rows =
                            self.projectile_choices(&loaded.projectile_catalog, &key, label_for);
                        let families =
                            variants::group(rows.iter().map(|(_, label)| label.as_str()));
                        let choices = Arc::new((rows, families));
                        ui.data_mut(|state| {
                            state.insert_temp(cache_id, (key, Arc::clone(&choices)))
                        });
                        choices
                    });
                    let (rows, families) = &*choices;
                    // Assets that differ only by their variant number share one row, which
                    // opens to them, as in the asset picker.
                    let open_id = egui::Id::new(("projectile-families-open", source));
                    let entry = |row: usize| &loaded.projectile_catalog.entries[rows[row].0];
                    // Opening the picker shows the projectile in use, with its family open to
                    // it. A search moves on from it.
                    let reveal = (reset && query.is_empty())
                        .then(|| (0..rows.len()).find(|&row| entry(row).graph == tag))
                        .flatten();
                    if let Some(group) =
                        reveal.and_then(|position| variants::family_of(families, position))
                    {
                        variants::open(ui.ctx(), open_id, group.key);
                    }
                    let open = variants::opened(ui.ctx(), open_id);
                    let shown = variants::shown(families, &open);
                    let keys = shown
                        .iter()
                        .map(|row| row.key(|position| entry(position).graph))
                        .collect::<Vec<_>>();
                    let mut toggled = None;
                    let picked = pickers::BrowserList {
                        keys: &keys,
                        height,
                        reset: reset || visibility.1 || filter != before,
                        row_height: sundial::investment::authoring_choice_row_height(ui),
                        select: reveal.map(|position| u64::from(entry(position).graph)),
                    }
                    .draw_activating(
                        ui,
                        |ui, index, selected| {
                            let row = shown[index];
                            let position = row.position();
                            let (choice, label) = (entry(position), &rows[position].1);
                            let technical = assets::technical_name(choice);
                            let (title, detail) = match row {
                                variants::Shown::Family(group, _) => {
                                    variants::family_text(group, &technical)
                                }
                                variants::Shown::Variant(_) => {
                                    (variants::variant_title(label), technical)
                                }
                                variants::Shown::Asset(_) => (label.clone(), technical),
                            };
                            let draw = |ui: &mut egui::Ui| {
                                sundial::investment::draw_asset_choice_row(
                                    ui, &title, &detail, selected,
                                )
                            };
                            let response = match row {
                                variants::Shown::Variant(_) => variants::indented(ui, draw),
                                variants::Shown::Family(..) => variants::with_caret_room(ui, draw),
                                variants::Shown::Asset(_) => draw(ui),
                            };
                            if let variants::Shown::Family(group, opened) = row {
                                variants::paint_caret(ui, &response, opened, selected);
                                // A double-click opens a family, so its second click, which
                                // lands on the family the first click selected, leaves it open.
                                if (response.clicked() && !(response.double_clicked() && selected))
                                    || variants::keyboard_toggle(ui, selected, opened)
                                {
                                    toggled = Some(group.key);
                                }
                            }
                            response
                        },
                        |ui, index, activated| {
                            let row = shown[index];
                            // A double-click uses an asset, as Use Asset does. On a family it
                            // only opens the family.
                            let activated =
                                activated && !matches!(row, variants::Shown::Family(..));
                            let position = row.position();
                            let (choice, label) = (entry(position), &rows[position].1);
                            ui.heading(label);
                            // A family shows its first variant, which its Use button takes.
                            if let variants::Shown::Family(group, _) = row {
                                ui.weak(format!("1 of {} Variants", group.members.len()));
                            }
                            if ui
                                .add(crate::app::style::primary(ui, "Use Asset"))
                                .clicked()
                                || activated
                            {
                                return Some((choice.graph != source).then_some(choice.graph));
                            }
                            assets::asset_details(ui, choice, &loaded.projectile_catalog, "");
                            None
                        },
                    );
                    if let Some(key) = toggled {
                        variants::toggle(ui.ctx(), open_id, key);
                    }
                    picked
                },
            );
            if let Some(picked) = picked {
                selected = picked;
            }
            (selected != original).then_some(selected)
        }
    }

    /// The picker's rows for one filter and query, sorted the way the list shows them.
    fn projectile_choices(
        &self,
        catalog: &entity::catalog::Catalog,
        key: &ChoiceKey,
        label_for: impl Fn(&entity::catalog::Entry) -> String,
    ) -> Vec<(usize, String)> {
        let mut choices = catalog
            .entries
            .iter()
            .enumerate()
            .filter(|(_, choice)| {
                matches!(
                    choice.kind,
                    entity::Kind::Projectile | entity::Kind::Emitter
                )
            })
            .filter(|(_, choice)| {
                key.unidentified
                    || choice.has_discovery_identity_with(
                        |index| self.projectile_labels.get(&index).cloned(),
                        |item| self.item_names.get(&item).cloned(),
                    )
            })
            .filter(|(_, choice)| {
                key.filter == 0
                    || (key.filter == 1 && choice.kind == entity::Kind::Projectile)
                    || (key.filter == 2 && choice.kind == entity::Kind::Emitter)
            })
            .filter_map(|(index, choice)| {
                let label = label_for(choice);
                let roles = choice.source_hint.as_deref().unwrap_or_default();
                let contexts = choice
                    .contexts
                    .iter()
                    .map(|context| context.path.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let search = format!(
                    "{label} {:08X} {} {roles} {contexts}",
                    choice.graph,
                    choice
                        .native_paths
                        .iter()
                        .chain(choice.native_name.iter())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                super::super::pickers::matches(&key.query, &search).then_some((index, label))
            })
            .collect::<Vec<_>>();
        // Projectiles a player knows by name lead, as they do in the asset picker.
        choices.sort_by_cached_key(|(index, label)| {
            let choice = &catalog.entries[*index];
            let label = label.to_lowercase();
            (
                super::super::assets::suggested::rank(&label),
                choice.label_rank(),
                label,
                choice.graph,
            )
        });
        choices
    }
}
