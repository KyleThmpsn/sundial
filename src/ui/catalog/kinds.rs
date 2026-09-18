//! Read-only engine kinds and their decoded stock uses. Editing actions belong to the host.
use crate::{
    investment::{
        PerkSources,
        native_content::kinds::{Family, users},
    },
    sandbox_perk::{
        dependencies,
        nodes::{CONDITIONS, EFFECTS, NodeKind, Support},
    },
};
use eframe::egui;

#[derive(Clone, Copy)]
pub enum Source<'a> {
    Loading,
    Unavailable,
    Ready(&'a dependencies::Index),
}
impl<'a> Source<'a> {
    fn index(self) -> Option<&'a dependencies::Index> {
        match self {
            Self::Ready(index) => Some(index),
            _ => None,
        }
    }
}
#[derive(Clone, Copy)]
pub enum UseLocation {
    Menu,
    Footer,
}
/// Optional host actions. A read-only catalog supplies None.
pub type UseActions<'a> = dyn FnMut(&mut egui::Ui, Option<u16>, UseLocation) + 'a;

#[derive(Default)]
pub struct Kinds {
    query: String,
    pub selected: Option<(Family, u8)>,
    authorable_only: bool,
    selected_use: Option<u16>,
    sort: (UseColumn, bool),
}
fn hint(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text).small().weak()).wrap())
}
/// A column of the Stock Uses table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UseColumn {
    #[default]
    Effect,
    Name,
    Description,
}

impl UseColumn {
    const ALL: [Self; 3] = [Self::Effect, Self::Name, Self::Description];

    fn label(self) -> &'static str {
        match self {
            Self::Effect => "Effect",
            Self::Name => "Referenced By",
            Self::Description => "Decoded Behavior",
        }
    }
}

/// One row of the Stock Uses table.
struct StockUse {
    index: u16,
    name: String,
    description: String,
}

fn usage_rows(perks: &[&dependencies::Perk], sources: &PerkSources) -> Vec<StockUse> {
    perks
        .iter()
        .filter_map(|perk| {
            let index = u16::try_from(perk.index).ok()?;
            let behavior = perk.behavior.as_ref()?;
            Some(StockUse {
                index,
                name: sources.summary(perk.index),
                description: behavior.headline.clone(),
            })
        })
        .collect()
}

/// The rows for one kind, sorted by the chosen column. The effect column sorts by index,
/// the others by their text, and the index breaks every tie so the order is stable.
pub(crate) fn sorted_uses(
    mut rows: Vec<(u16, String, String)>,
    column: UseColumn,
    descending: bool,
) -> Vec<(u16, String, String)> {
    rows.sort_by(|a, b| {
        let order = match column {
            UseColumn::Effect => a.0.cmp(&b.0),
            UseColumn::Name => a.1.to_lowercase().cmp(&b.1.to_lowercase()),
            UseColumn::Description => a.2.to_lowercase().cmp(&b.2.to_lowercase()),
        }
        .then(a.0.cmp(&b.0));
        if descending { order.reverse() } else { order }
    });
    rows
}

impl Kinds {
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        sources: &PerkSources,
        source: Source<'_>,
        actions: &mut Option<&mut UseActions<'_>>,
    ) {
        hint(
            ui,
            &format!(
                "{} effect kinds and {} condition kinds registered by the supported client. Pick one to see installed effect entries whose decoded actions contain it.",
                EFFECTS.len(),
                CONDITIONS.len()
            ),
        );
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search Kinds")
                    .desired_width(220.0),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search Kinds")
            });
            ui.checkbox(&mut self.authorable_only, "Authorable Only");
        });
        let query = self.query.trim().to_lowercase();
        ui.add_space(4.0);
        let height = (ui.available_height() - 8.0).max(200.0);
        let list_width = (ui.available_width() * 0.5).clamp(300.0, 520.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(list_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(list_width);
                        egui::ScrollArea::vertical()
                            .id_salt("engine-catalog-kinds")
                            .max_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.draw_kinds(ui, &query, source.index()));
                    },
                );
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width((ui.available_width() - 8.0).max(240.0));
                    egui::ScrollArea::vertical()
                        .id_salt("engine-catalog-details")
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.draw_details(ui, sources, source, actions);
                        });
                });
            },
        );
    }

    /// One table per family. The name and the Stock Uses count both select the kind.
    fn draw_kinds(&mut self, ui: &mut egui::Ui, query: &str, index: Option<&dependencies::Index>) {
        for family in Family::ALL {
            let nodes = family
                .nodes()
                .iter()
                .filter(|node| {
                    matches_query(node, query)
                        && (!self.authorable_only || node.support == Support::Authorable)
                })
                .collect::<Vec<_>>();
            ui.strong(format!("{} · {}", family.label(), nodes.len()));
            if nodes.is_empty() {
                ui.weak("No matching kinds.");
                ui.add_space(6.0);
                continue;
            }
            egui::Grid::new(("engine-catalog", family.label()))
                .num_columns(4)
                .striped(true)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for label in ["Kind", "Name", "Effect Entries", "Support"] {
                        ui.small(label);
                    }
                    ui.end_row();
                    for node in nodes {
                        let selected = self.selected == Some((family, node.kind));
                        ui.small(node.kind.to_string());
                        if ui
                            .selectable_label(selected, node.name)
                            .on_hover_text(node.summary)
                            .clicked()
                        {
                            self.select(family, node.kind);
                        }
                        let count = index.map(|index| users(&index.perks, family, node.kind).len());
                        if ui
                            .add_enabled(count.is_some(), egui::Button::new(count.map_or_else(|| "…".into(), |count| count.to_string())).small())
                            .on_hover_text("Decoded installed effect entries containing this kind. Entries with unreadable actions are excluded.")
                            .clicked()
                        {
                            self.select(family, node.kind);
                        }
                        super::support_badge(ui, node.support);
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
        }
    }

    fn select(&mut self, family: Family, kind: u8) {
        if self.selected != Some((family, kind)) {
            self.selected_use = None;
        }
        self.selected = Some((family, kind));
    }

    fn draw_details(
        &mut self,
        ui: &mut egui::Ui,
        sources: &PerkSources,
        source: Source<'_>,
        actions: &mut Option<&mut UseActions<'_>>,
    ) {
        let Some((family, kind)) = self.selected else {
            ui.weak("Pick a kind on the left, or click its Effect Entries count.");
            return;
        };
        let Some(node) = family.nodes().iter().find(|node| node.kind == kind) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(node.name);
            super::support_badge(ui, node.support);
        });
        ui.small(format!(
            "{} kind {} · {}",
            family.label().trim_end_matches('s'),
            node.kind,
            if node.class == 0 {
                "not observed in the bundled reference survey".to_owned()
            } else {
                format!("class 0x{:08X} · {} bytes", node.class, node.struct_size)
            }
        ));
        ui.add_space(4.0);
        ui.label(node.summary);
        ui.add_space(4.0);
        ui.small(node.evidence);
        ui.add_space(8.0);
        ui.strong("Installed Effect Entries");
        let Some(index) = source.index() else {
            if matches!(source, Source::Loading) {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading Native Content…");
                });
            } else {
                ui.weak("The effect list is unavailable until native content has been read.");
            }
            return;
        };
        let perks = users(&index.perks, family, kind);
        let unreadable = index
            .perks
            .iter()
            .filter(|perk| {
                perk.error.is_some() || (perk.action.is_some() && perk.behavior.is_none())
            })
            .count();
        ui.small(format!("{} decoded effect entries contain this kind. {} entries could not be inspected completely.", perks.len(), unreadable));
        if perks.is_empty() {
            ui.weak("No decoded installed action contains this kind. Unreadable actions may still contain it.");
            return;
        }
        let rows = usage_rows(&perks, sources);
        self.draw_uses(ui, rows, sources, actions);
        if let Some(selected) = self.selected_use {
            super::draw_sources(ui, sources, usize::from(selected));
        }
    }

    /// The Stock Uses table. Clicking a header sorts by that column and again reverses it.
    fn draw_uses(
        &mut self,
        ui: &mut egui::Ui,
        rows: Vec<StockUse>,
        sources: &PerkSources,
        actions: &mut Option<&mut UseActions<'_>>,
    ) {
        self.selected_use = self
            .selected_use
            .filter(|selected| rows.iter().any(|row| row.index == *selected));
        // Fixed name and effect columns, so the description takes what they leave on one
        // line and the table never widens the window.
        let name_width = 200.0;
        let description_width = (ui.available_width() - name_width - 120.0).max(120.0);
        egui::Grid::new("engine-catalog-users")
            .num_columns(3)
            .striped(true)
            .spacing([12.0, 2.0])
            .min_col_width(40.0)
            .show(ui, |ui| {
                for column in UseColumn::ALL {
                    let (current, descending) = self.sort;
                    let title = if current == column {
                        format!("{} {}", column.label(), if descending { "⬇" } else { "⬆" })
                    } else {
                        column.label().to_owned()
                    };
                    if ui
                        .selectable_label(current == column, egui::RichText::new(title).small())
                        .on_hover_text("Sort by this column. Click again to reverse.")
                        .clicked()
                    {
                        self.sort = (column, current == column && !descending);
                    }
                }
                ui.end_row();
                let sorted = sorted_uses(
                    rows.into_iter()
                        .map(|row| (row.index, row.name, row.description))
                        .collect(),
                    self.sort.0,
                    self.sort.1,
                );
                for (index, name, description) in sorted {
                    let selected = self.selected_use == Some(index);
                    let mut response = ui.selectable_label(selected, index.to_string());
                    response |= ui
                        .scope(|ui| {
                            ui.set_max_width(name_width);
                            ui.add(
                                egui::Label::new(&name)
                                    .truncate()
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                            )
                        })
                        .inner
                        .on_hover_text(sources.details(usize::from(index)));
                    response |= ui
                        .scope(|ui| {
                            ui.set_max_width(description_width);
                            ui.add(
                                egui::Label::new(egui::RichText::new(&description).small())
                                    .truncate()
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                            )
                        })
                        .inner
                        .on_hover_text(&description);
                    if response.clicked() {
                        self.selected_use = Some(index);
                    }
                    if let Some(actions) = actions.as_deref_mut() {
                        response.context_menu(|ui| actions(ui, Some(index), UseLocation::Menu));
                    }
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        if let Some(actions) = actions.as_deref_mut() {
            ui.horizontal(|ui| actions(ui, self.selected_use, UseLocation::Footer));
        }
    }
}
/// Whether a kind matches every word of a search: its number, name or summary.
fn matches_query(node: &NodeKind, query: &str) -> bool {
    let text = format!("{} {} {}", node.kind, node.name, node.summary).to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perk(index: usize, effects: &[u8], conditions: &[u8]) -> dependencies::Perk {
        dependencies::Perk {
            index,
            hash: 0,
            runtime_key: 0,
            action: Some(0x8080_0000),
            graphs: Vec::new(),
            error: None,
            behavior: Some(dependencies::Behavior {
                headline: String::new(),
                support: Support::Readable,
                editable: true,
                program: None,
                condition_kinds: conditions.to_vec(),
                effect_kinds: effects.to_vec(),
                details: Vec::new(),
                notes: Vec::new(),
            }),
        }
    }

    #[test]
    fn users_come_from_the_decoded_behaviors_of_each_family() {
        let mut undecoded = perk(2, &[1], &[]);
        undecoded.behavior = None;
        let perks = vec![perk(0, &[1, 3], &[0]), perk(1, &[3], &[1]), undecoded];
        let indices = |family, kind| {
            users(&perks, family, kind)
                .into_iter()
                .map(|perk| perk.index)
                .collect::<Vec<_>>()
        };
        assert_eq!(indices(Family::Effects, 1), vec![0]);
        assert_eq!(indices(Family::Effects, 3), vec![0, 1]);
        assert_eq!(indices(Family::Conditions, 1), vec![1]);
        assert!(indices(Family::Effects, 9).is_empty());
    }

    #[test]
    fn installed_users_exclude_failed_and_unassigned_entries_even_with_stale_digests() {
        let mut failed = perk(1, &[1], &[]);
        failed.error = Some("Incomplete action".into());
        let mut unassigned = perk(2, &[1], &[]);
        unassigned.action = None;
        let perks = [perk(0, &[1], &[]), failed, unassigned];
        let found = users(&perks, Family::Effects, 1);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].index, 0);
    }

    #[test]
    fn effect_rows_describe_decoded_actions_and_display_all_source_names() {
        let mut effect = perk(453, &[43], &[26]);
        effect.behavior.as_mut().unwrap().headline = "Decoded action summary.".into();
        let sources = [(1, "Thorn Catalyst"), (2, "Masterwork Weapon")]
            .into_iter()
            .map(|(hash, name)| {
                (
                    453,
                    crate::investment::PerkSource {
                        hash,
                        name: name.into(),
                        type_name: String::new(),
                    },
                )
            })
            .collect();
        let rows = usage_rows(&[&effect], &sources);
        assert_eq!(rows[0].description, "Decoded action summary.");
        assert!(rows[0].name.contains("Masterwork Weapon"));
        assert!(rows[0].name.contains("Thorn Catalyst"));
        let mut engine = Kinds::default();
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                engine.draw_uses(ui, usage_rows(&[&effect], &sources), &sources, &mut None)
            });
        });
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Referenced By"));
        assert!(labels.contains(&"Decoded Behavior"));
    }

    #[test]
    fn searches_match_numbers_names_and_summaries() {
        let create = EFFECTS
            .iter()
            .find(|node| node.name == "Create Entity")
            .expect("the create entity kind");
        assert!(matches_query(create, "create"));
        assert!(matches_query(create, &create.kind.to_string()));
        assert!(matches_query(create, "entity create"));
        assert!(!matches_query(create, "ammunition"));
    }

    #[test]
    fn stock_uses_sort_by_each_column_in_both_directions() {
        let rows = || {
            vec![
                (
                    421,
                    "Outlaw".to_owned(),
                    "Precision kills reload.".to_owned(),
                ),
                (
                    338,
                    "Rampage".to_owned(),
                    "Kills increase damage.".to_owned(),
                ),
                (
                    405,
                    "dragonfly".to_owned(),
                    "Precision kills explode.".to_owned(),
                ),
            ]
        };
        let indices = |sorted: Vec<(u16, String, String)>| {
            sorted.into_iter().map(|row| row.0).collect::<Vec<_>>()
        };
        assert_eq!(
            indices(sorted_uses(rows(), UseColumn::Effect, false)),
            [338, 405, 421]
        );
        assert_eq!(
            indices(sorted_uses(rows(), UseColumn::Effect, true)),
            [421, 405, 338]
        );
        // Names sort without regard to case.
        assert_eq!(
            indices(sorted_uses(rows(), UseColumn::Name, false)),
            [405, 421, 338]
        );
        assert_eq!(
            indices(sorted_uses(rows(), UseColumn::Description, true)),
            [421, 405, 338]
        );
    }
}
