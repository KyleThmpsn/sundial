//! TFT paths and resolved perk references, using the shared catalog inspector.
use super::{BrowserList, assets};
use crate::{
    investment::{WeaponSandboxPerkChoice, native_content::Catalog},
    package_runtime::tft,
    sandbox_perk::dependencies,
};
use eframe::egui;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Perks,
    Paths,
    References,
}

use std::hash::{Hash, Hasher};

struct Row {
    index: usize,
    label: String,
    detail: String,
    search: String,
}

#[derive(Default)]
pub struct Browser {
    query: String,
    view: View,
    source: Option<(Arc<tft::Index>, Arc<dependencies::Index>, u64)>,
    rows: Vec<Row>,
    filtered: Vec<usize>,
    filter_query: Option<String>,
}

impl Browser {
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        view: View,
        choices: &[WeaponSandboxPerkChoice],
        data: &Catalog,
    ) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if view == View::Perks {
            for choice in choices {
                (choice.perk_index, &choice.representative_name).hash(&mut hasher);
            }
        }
        let labels = hasher.finish();
        let same_source = self
            .source
            .as_ref()
            .is_some_and(|(names, perks, previous)| {
                Arc::ptr_eq(names, &data.names)
                    && Arc::ptr_eq(perks, &data.perks)
                    && *previous == labels
            });
        if self.view != view || !same_source {
            self.rows = rows(data, choices, view);
            self.view = view;
            self.source = Some((data.names.clone(), data.perks.clone(), labels));
            self.filter_query = None;
        }
        let mut reset = false;
        let count_rect = ui
            .horizontal(|ui| {
                reset |= super::search(
                    ui,
                    &mut self.query,
                    false,
                    ui.available_width() - 150.0,
                    "Search Native Content",
                );
                ui.allocate_exact_size(
                    egui::vec2(86.0, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                )
                .0
            })
            .inner;
        let query = self.query.trim().to_ascii_lowercase();
        reset |= self.filter_query.as_ref() != Some(&query);
        if reset {
            self.filtered = self
                .rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    query
                        .split_whitespace()
                        .all(|word| row.search.contains(word.strip_prefix("0x").unwrap_or(word)))
                        .then_some(index)
                })
                .collect();
            self.filter_query = Some(query);
        }
        ui.put(
            count_rect,
            egui::Label::new(format!("{} Results", self.filtered.len())),
        );
        ui.separator();
        let keys = self
            .filtered
            .iter()
            .map(|&row| self.rows[row].index as u64)
            .collect::<Vec<_>>();
        BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(110.0),
            reset,
            row_height: crate::investment::authoring_choice_row_height(ui),
        }
        .draw_body(
            ui,
            |ui, index, selected| {
                let row = &self.rows[self.filtered[index]];
                crate::investment::draw_asset_choice_row(ui, &row.label, &row.detail, selected)
            },
            |ui, index| {
                let row = &self.rows[self.filtered[index]];
                ui.heading(&row.label);
                details(ui, data, view, row.index);
                None::<()>
            },
        );
    }
}

fn rows(data: &Catalog, choices: &[WeaponSandboxPerkChoice], view: View) -> Vec<Row> {
    let labels = choices
        .iter()
        .map(|choice| {
            (
                usize::from(choice.perk_index),
                choice.representative_name.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut rows: Vec<Row> = match view {
        View::Perks => data
            .perks
            .perks
            .iter()
            .enumerate()
            .map(|(index, perk)| {
                let name = labels.get(&perk.index).map_or_else(
                    || format!("Effect {}", perk.index),
                    |name| (*name).to_owned(),
                );
                let paths = u16::try_from(perk.index)
                    .ok()
                    .and_then(|index| data.perk_search.get(&index))
                    .map_or("", String::as_str);
                Row {
                    index,
                    label: name.clone(),
                    detail: format!("Effect {} · 0x{:08X}", perk.index, perk.hash),
                    search: format!("{name} {} {:08X} {paths}", perk.index, perk.hash),
                }
            })
            .collect(),
        View::References => data
            .names
            .references
            .iter()
            .enumerate()
            .map(|(index, reference)| Row {
                index,
                label: tft::asset_label(&reference.path),
                detail: reference.path.clone(),
                search: format!(
                    "{} {:08X} {:08X}",
                    reference.path, reference.source, reference.target
                ),
            })
            .collect(),
        View::Paths => data
            .names
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| Row {
                index,
                label: tft::asset_label(&path.path),
                detail: path.path.clone(),
                search: format!("{} {:08X}", path.path, path.source),
            })
            .collect(),
    };
    rows.sort_by_cached_key(|row| (row.label.to_ascii_lowercase(), row.index));
    for row in &mut rows {
        row.search.make_ascii_lowercase();
    }
    rows
}

fn details(ui: &mut egui::Ui, data: &Catalog, view: View, index: usize) {
    match view {
        View::Perks => {
            let Some(perk) = data.perks.perks.get(index) else {
                return;
            };
            ui.strong(perk.status());
            if let Some(action) = perk.action {
                ui.label(format!("Action 0x{action:08X}"));
            }
            if let Some(error) = &perk.error {
                ui.colored_label(ui.visuals().warn_fg_color, error);
            }
            if let Some(assets) = data.perk_assets.get(index) {
                for (label, references) in [
                    ("Action References", &assets.action),
                    ("Graph References", &assets.graphs),
                    ("Component References", &assets.components),
                ] {
                    if references.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{label} ({})", references.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for &index in references {
                                draw_reference(ui, &data.names.references[index]);
                            }
                        });
                }
            }
            for graph in &perk.graphs {
                ui.small(format!(
                    "Graph 0x{:08X} · {} Components",
                    graph.tag,
                    graph.components.len()
                ));
            }
        }
        View::References => {
            if let Some(reference) = data.names.references.get(index) {
                draw_reference(ui, reference);
            }
        }
        View::Paths => {
            if let Some(path) = data.names.paths.get(index) {
                assets::draw_path(ui, &path.path);
                ui.small(format!(
                    "Stored in 0x{:08X} + 0x{:X}",
                    path.source, path.offset
                ));
                crate::ui_help::info(
                    ui,
                    "A path stored here may refer to another resource. TFT References shows the resolved links.",
                );
            }
        }
    }
}

fn draw_reference(ui: &mut egui::Ui, reference: &tft::Reference) {
    assets::draw_path(ui, &reference.path);
    ui.small(format!(
        "0x{:08X} + 0x{:X} → 0x{:08X} · Class 0x{:08X}",
        reference.source, reference.offset, reference.target, reference.target_class
    ));
}
