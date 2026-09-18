//! Content, ranks, and saved-state inspection.
mod ranks;
use super::{state::*, table_ui::sortable_table_header, table_ui::*, *};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Entries,
    Ranks,
    Storage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Names {
    #[default]
    Named,
    Unnamed,
    All,
}

#[derive(Debug)]
struct Entry {
    index: usize,
    value: bool,
    name: String,
    purpose: String,
    sort_name: String,
    sort_purpose: String,
    category: &'static str,
    location: String,
    named: bool,
    reference: bool,
    scope: &'static str,
    search: String,
}

#[derive(Debug, Default)]
pub(super) struct Browser {
    pub tab: Tab,
    ranks: ranks::State,
    names: Names,
    entries: Option<Vec<Entry>>,
    snapshot: Option<CollectionStateSnapshot>,
    scope: Option<&'static str>,
    category: Option<&'static str>,
    value: Option<bool>,
    feedback: Option<String>,
    filtered: Option<(Filter, Vec<usize>)>,
    override_value: i32,
    preparing: Vec<Entry>,
    duplicates: HashMap<(String, String, &'static str), Option<usize>>,
}

impl Browser {
    pub fn invalidate(&mut self) {
        self.ranks.invalidate();
        self.snapshot = None;
        self.filtered = None;
    }

    pub fn reveal(&mut self, value: bool) {
        self.tab = Tab::Entries;
        self.value = Some(value);
        self.category = None;
        self.names = Names::All;
        self.scope = None;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.ranks.invalidate();
        self.entries = None;
        self.preparing.clear();
        self.duplicates.clear();
        self.snapshot = None;
        self.filtered = None;
        self.feedback = None;
    }
}

fn scope(definition: &UnlockDefinition, value: bool) -> &'static str {
    match (value, definition.bank()) {
        (_, 1) => "Account",
        (false, 2) => "Profile",
        (false, 3 | 6) | (true, 2) => "Character",
        _ => "Other",
    }
}

#[cfg(test)]
fn entries(catalog: &Catalog, value: bool) -> Vec<Entry> {
    let definitions = if value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| entry(catalog, index, value, definition))
        .collect()
}

fn entry(catalog: &Catalog, index: usize, value: bool, definition: &UnlockDefinition) -> Entry {
    let label = labels::unlock(catalog, index, value);
    let named = label.named;
    let scope = scope(definition, value);
    let mut search = format!(
        "{} {scope} {index} {} {:08x} {} {} {} {}",
        label.text,
        definition.hash,
        definition.hash,
        format_hash_hex(definition.hash),
        label.purpose,
        label.category,
        label.location
    );
    if let Some(slot) = definition.compact_slot {
        search.push_str(&format!(" {slot}"));
    }
    if let Some(description) = &definition.description {
        search.push(' ');
        search.push_str(description);
    }
    for context in &definition.tested_by {
        search.push(' ');
        search.push_str(&context.name);
        for part in context.paths.iter().flatten() {
            search.push(' ');
            search.push_str(part);
        }
    }
    Entry {
        index,
        value,
        sort_name: label.text.to_lowercase(),
        sort_purpose: label.purpose.to_lowercase(),
        name: label.text,
        purpose: label.purpose,
        category: label.category,
        location: label.location,
        named,
        reference: label.reference,
        scope,
        search: search.to_lowercase(),
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    policy: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut browser = std::mem::take(&mut state.unlock_browser);
    let previous = browser.tab;
    progression_toolbar(ui, |ui| {
        for (tab, label) in [
            (Tab::Entries, "Content"),
            (Tab::Ranks, "Ranks"),
            (Tab::Storage, "Saved Values"),
        ] {
            ui.selectable_value(&mut browser.tab, tab, label);
        }
    });
    if browser.tab != previous {
        browser.reset();
        state.query.clear();
        state.add_open = false;
    }
    ui.add_space(6.0);
    let changed = match browser.tab {
        Tab::Storage => super::page::draw_storage(ui, document, policy, catalog, state),
        Tab::Ranks => ranks::draw(ui, document, policy, catalog, state, &mut browser.ranks),
        Tab::Entries => draw_entries(ui, document, catalog, state, &mut browser),
    };
    if changed {
        browser.invalidate();
    }
    state.unlock_browser = browser;
    changed
}

fn draw_entries(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
    browser: &mut Browser,
) -> bool {
    if browser.entries.is_none() {
        let flags = catalog.unlock_flag_definitions();
        let values = catalog.unlock_value_definitions();
        let total = flags.len() + values.len();
        let start = std::time::Instant::now();
        while browser.preparing.len() < total {
            let cursor = browser.preparing.len();
            let mut row = if cursor < flags.len() {
                entry(catalog, cursor, false, &flags[cursor])
            } else {
                let index = cursor - flags.len();
                entry(catalog, index, true, &values[index])
            };
            match browser
                .duplicates
                .entry((row.name.clone(), row.purpose.clone(), row.scope))
            {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(cursor));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if let Some(first) = entry.get_mut().take() {
                        let original = &mut browser.preparing[first];
                        original.name.push_str(&format!(" #{}", original.index));
                        original
                            .sort_name
                            .push_str(&format!(" #{}", original.index));
                    }
                    row.name.push_str(&format!(" #{}", row.index));
                    row.sort_name.push_str(&format!(" #{}", row.index));
                }
            }
            browser.preparing.push(row);
            if start.elapsed() >= std::time::Duration::from_millis(6) {
                break;
            }
        }
        if browser.preparing.len() < total {
            ui.add(
                egui::ProgressBar::new(browser.preparing.len() as f32 / total as f32).text(
                    format!("Preparing Content {} / {total}", browser.preparing.len()),
                ),
            );
            ui.ctx().request_repaint();
            return false;
        }
        browser.entries = Some(std::mem::take(&mut browser.preparing));
        browser.duplicates.clear();
    }
    let entries = browser.entries.as_ref().expect("prepared content");
    let sort = state
        .table_sorts
        .get("content_unlocks")
        .copied()
        .unwrap_or(TableSort::ascending(0));
    progression_toolbar(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Search content or ID…")
                .desired_width(200.0),
        );
        egui::ComboBox::from_id_salt("unlock_names")
            .selected_text(match browser.names {
                Names::Named => "Identified Content",
                Names::Unnamed => "Unidentified",
                Names::All => "All Definitions",
            })
            .show_ui(ui, |ui| {
                for (filter, label) in [
                    (Names::Named, "Identified Content"),
                    (Names::Unnamed, "Unidentified"),
                    (Names::All, "All Definitions"),
                ] {
                    ui.selectable_value(&mut browser.names, filter, label);
                }
            });
        egui::ComboBox::from_id_salt("unlock_scope")
            .selected_text(browser.scope.unwrap_or("All Scopes"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut browser.scope, None, "All Scopes");
                for scope in ["Account", "Profile", "Character", "Other"] {
                    ui.selectable_value(&mut browser.scope, Some(scope), scope);
                }
            });
        egui::ComboBox::from_id_salt("unlock_category")
            .selected_text(browser.category.unwrap_or("All Content"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut browser.category, None, "All Content");
                for category in [
                    "Triumphs",
                    "Collections",
                    "Items",
                    "Activities",
                    "Ranks",
                    "Objectives",
                    "Other",
                ] {
                    ui.selectable_value(&mut browser.category, Some(category), category);
                }
            });
        egui::ComboBox::from_id_salt("unlock_kind")
            .selected_text(browser.value.map_or("All Types", |value| {
                if value { "Counters" } else { "Unlocks" }
            }))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut browser.value, None, "All Types");
                ui.selectable_value(&mut browser.value, Some(false), "Unlocks");
                ui.selectable_value(&mut browser.value, Some(true), "Counters");
            });
        let query = state.query.trim().to_lowercase();
        if browser.snapshot.is_none() {
            browser.snapshot = collection_state_snapshot(document);
        }
        let Some(snapshot) = browser.snapshot.as_ref() else {
            ui.label("Progression state unavailable");
            return;
        };
        let filter = Filter {
            query,
            names: browser.names,
            scope: browser.scope,
            sort,
            value: browser.value,
            category: browser.category,
        };
        if browser
            .filtered
            .as_ref()
            .is_none_or(|(previous, _)| previous != &filter)
        {
            let indices = filtered_rows(entries, &filter, snapshot, catalog);
            browser.filtered = Some((filter, indices));
        }
        let filtered = &browser.filtered.as_ref().expect("filtered rows").1;
        ui.weak(format!("{} entries", filtered.len()));
    });
    let Some(snapshot) = browser.snapshot.as_ref() else {
        return false;
    };
    let filtered = &browser.filtered.as_ref().expect("filtered rows").1;
    let purpose_width = (ui.available_width() * 0.23).clamp(100.0, 210.0);
    let name_width =
        (ui.available_width() - purpose_width - 238.0 - TABLE_COLUMN_GAP * 4.0).max(150.0);
    sortable_table_header(
        ui,
        "content_unlocks",
        &[
            (name_width, "Content"),
            (purpose_width, "Purpose"),
            (88.0, "Scope"),
            (90.0, "Saved Value"),
            (60.0, "Details"),
        ],
        TableSort::ascending(0),
        state,
    );
    ui.separator();
    if let Some(feedback) = &browser.feedback {
        ui.colored_label(ui.visuals().error_fg_color, feedback);
    }
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt("content_unlocks")
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("content_unlock_rows")
                    .striped(true)
                    .num_columns(5)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for offset in range {
                            changed |= draw_entry(
                                ui,
                                &entries[filtered[offset]],
                                name_width,
                                purpose_width,
                                &mut EntryContext {
                                    document,
                                    catalog,
                                    snapshot,
                                    read_only: state.read_only,
                                    inspector: &mut state.metadata_inspector,
                                    feedback: &mut browser.feedback,
                                    override_value: &mut browser.override_value,
                                },
                            );
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

#[derive(Debug, PartialEq, Eq)]
struct Filter {
    query: String,
    names: Names,
    scope: Option<&'static str>,
    sort: TableSort,
    value: Option<bool>,
    category: Option<&'static str>,
}

fn filtered_rows(
    entries: &[Entry],
    filter: &Filter,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<usize> {
    let mut filtered = (0..entries.len())
        .filter(|index| {
            let entry = &entries[*index];
            (match filter.names {
                Names::Named => entry.named,
                Names::Unnamed => !entry.named,
                Names::All => true,
            }) && filter.scope.is_none_or(|scope| entry.scope == scope)
                && filter
                    .category
                    .is_none_or(|category| entry.category == category)
                && filter.value.is_none_or(|value| entry.value == value)
                && filter
                    .query
                    .split_whitespace()
                    .all(|term| entry.search.contains(term))
        })
        .collect::<Vec<_>>();
    match filter.sort.column {
        0 => filtered.sort_unstable_by(|a, b| {
            entries[*a]
                .sort_name
                .cmp(&entries[*b].sort_name)
                .then(a.cmp(b))
        }),
        1 => filtered.sort_unstable_by(|a, b| {
            entries[*a]
                .sort_purpose
                .cmp(&entries[*b].sort_purpose)
                .then(a.cmp(b))
        }),
        2 => filtered.sort_by_key(|index| (entries[*index].scope, entries[*index].index)),
        3 => filtered.sort_by_key(|index| {
            let entry = &entries[*index];
            if entry.value {
                catalog
                    .unlock_value_definition(entry.index)
                    .and_then(|definition| snapshot.value(entry.index, definition))
            } else {
                catalog
                    .unlock_flag_definition(entry.index)
                    .and_then(|definition| snapshot.flag_value(entry.index, definition))
                    .map(i32::from)
            }
        }),
        _ => filtered.sort_by_key(|index| entries[*index].index),
    }
    if filter.sort.descending {
        filtered.reverse();
    }
    filtered
}

struct EntryContext<'a> {
    document: &'a mut Value,
    catalog: &'a Catalog,
    snapshot: &'a CollectionStateSnapshot,
    read_only: bool,
    inspector: &'a mut ProgressionInspectorState,
    feedback: &'a mut Option<String>,
    override_value: &'a mut i32,
}

enum Edit {
    Saved(i32),
    Override(i32),
}

fn edit_blocked(
    value: bool,
    index: usize,
    definition: &UnlockDefinition,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<(&'static str, &'static str)> {
    if snapshot.is_native()
        && ((value && seasonal::is_derived_value(index))
            || (!value
                && catalog
                    .seasonal()
                    .is_some_and(|season| season.mod_for_flag(index).is_some())))
    {
        return Some((
            "Seasonal",
            "Edit this in Seasonal. Its saved fields must be updated together.",
        ));
    }
    if (value && definition.bank() == 4) || (!value && definition.bank() == 5) {
        return Some((
            "Item Context",
            "This value is read from an item at runtime. An account override cannot change it.",
        ));
    }
    if definition.bank() > 6 {
        return Some((
            "Unknown Bank",
            "This storage bank is not decoded. Open Details to inspect its references.",
        ));
    }
    None
}

fn override_cell(
    ui: &mut egui::Ui,
    value: bool,
    row: &Entry,
    context: &mut EntryContext<'_>,
) -> Option<Edit> {
    let mut requested = None;
    ui.allocate_ui_with_layout(
        egui::vec2(90.0, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_enabled_ui(!context.read_only, |ui| {
                ui.push_id(("add_content_override", value, row.index), |ui| {
                    ui.menu_button("Add Override", |ui| {
                        ui.label("No saved value. Choose an account override.");
                        if value {
                            ui.add(egui::DragValue::new(context.override_value));
                            if ui.button("Save Override").clicked() {
                                requested = Some(Edit::Override(*context.override_value));
                                ui.close_menu();
                            }
                        } else {
                            for (number, label) in [(2, "Set"), (1, "Force Clear")] {
                                if ui.button(label).clicked() {
                                    requested = Some(Edit::Override(number));
                                    ui.close_menu();
                                }
                            }
                        }
                    })
                    .response
                    .on_hover_text("No saved account value exists. Add an override to author one.");
                });
            });
        },
    );
    requested
}

fn saved_cell(
    ui: &mut egui::Ui,
    value: bool,
    row: &Entry,
    definition: &UnlockDefinition,
    context: &mut EntryContext<'_>,
) -> Option<Edit> {
    if let Some((label, reason)) = edit_blocked(
        value,
        row.index,
        definition,
        context.snapshot,
        context.catalog,
    ) {
        table_cell(ui, 90.0, label).on_hover_text(reason);
        return None;
    }
    if value {
        let Some(mut saved) = context.snapshot.value(row.index, definition) else {
            return override_cell(ui, value, row, context);
        };
        return ui
            .push_id(("counter_value", row.index), |ui| {
                table_drag_value(ui, 90.0, &mut saved, !context.read_only)
            })
            .inner
            .on_hover_text(
                context
                    .snapshot
                    .evaluated_value(row.index, context.catalog)
                    .map_or_else(
                        || "The runtime value depends on unavailable context".into(),
                        |value| format!("Evaluated Value: {value}"),
                    ),
            )
            .changed()
            .then_some(Edit::Saved(saved));
    }
    let Some(mut saved) = context.snapshot.flag_value(row.index, definition) else {
        return override_cell(ui, value, row, context);
    };
    let label = if saved { "Set" } else { "Clear" };
    ui.allocate_ui_with_layout(
        egui::vec2(90.0, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(90.0, TABLE_CELL_HEIGHT));
            ui.push_id(("flag_value", row.index), |ui| {
                ui.add_enabled(!context.read_only, egui::Checkbox::new(&mut saved, label))
                    .on_hover_text(
                        context
                            .snapshot
                            .evaluated_flag(row.index, context.catalog)
                            .map_or("The runtime state depends on unavailable context", |set| {
                                if set {
                                    "Evaluated State: Set"
                                } else {
                                    "Evaluated State: Clear"
                                }
                            }),
                    )
            })
            .inner
            .changed()
        },
    )
    .inner
    .then_some(Edit::Saved(i32::from(saved)))
}

fn draw_entry(
    ui: &mut egui::Ui,
    row: &Entry,
    name_width: f32,
    purpose_width: f32,
    context: &mut EntryContext<'_>,
) -> bool {
    let value = row.value;
    let definition = if value {
        context.catalog.unlock_value_definition(row.index)
    } else {
        context.catalog.unlock_flag_definition(row.index)
    }
    .expect("catalog entry");
    let selection = if value {
        MetadataSelection::ValueDefinition(row.index)
    } else {
        MetadataSelection::FlagDefinition(row.index)
    };
    let response = table_link(ui, name_width, destiny_text(ui, &row.name));
    let response = if row.reference {
        response.on_hover_text("Linked content")
    } else {
        response
    };
    if response.clicked() {
        context.inspector.open(selection);
    }
    table_cell(ui, purpose_width, destiny_text(ui, &row.purpose))
        .on_hover_text(format!("{}\n{}", row.category, row.location));
    table_cell(ui, 88.0, row.scope);
    let mut changed = false;
    if let Some(requested) = saved_cell(ui, value, row, definition, context) {
        changed = match requested {
            Edit::Override(number) => super::mutations::set_investment_override(
                context.document,
                if value {
                    InvestmentTable::ValueOverrides
                } else {
                    InvestmentTable::FlagOverrides
                },
                row.index,
                number,
            )
            .changed(),
            Edit::Saved(number) if value => {
                set_collection_value(context.document, row.index, definition, number).changed()
            }
            Edit::Saved(number) => {
                set_collection_flag(context.document, row.index, definition, number != 0).changed()
            }
        };
        *context.feedback = (!changed).then(|| "This entry could not be changed. Check the saved value limits and the 100-entry override capacity in Details.".into());
    }
    if table_link(ui, 60.0, "Details")
        .on_hover_text(format!(
            "Definition #{} · {}",
            row.index,
            format_hash_hex(definition.hash)
        ))
        .clicked()
    {
        context.inspector.open(selection);
    }
    changed
}

#[cfg(test)]
mod tests;
