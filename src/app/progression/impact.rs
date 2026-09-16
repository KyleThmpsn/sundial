use super::*;

#[derive(Debug)]
pub(in crate::app) struct Change {
    pub name: String,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Default)]
pub(in crate::app) struct Review {
    pub related: Vec<Change>,
    pub fields: Vec<Change>,
}

impl Review {
    pub fn build(
        before: &CollectionStateSnapshot,
        after: &CollectionStateSnapshot,
        catalog: &Catalog,
        selected: &HashSet<u64>,
    ) -> Self {
        let mut review = Self::default();
        for value in [false, true] {
            let definitions = if value {
                catalog.unlock_value_definitions()
            } else {
                catalog.unlock_flag_definitions()
            };
            for (index, definition) in definitions.iter().enumerate() {
                let state = |snapshot: &CollectionStateSnapshot| {
                    if value {
                        snapshot.value(index, definition)
                    } else {
                        snapshot.flag_value(index, definition).map(i32::from)
                    }
                };
                let (old, new) = (state(before), state(after));
                if old != new {
                    let label = labels::unlock(catalog, index, value);
                    let text = |n: Option<i32>| {
                        n.map_or_else(
                            || "Not Saved".into(),
                            |n| {
                                if value {
                                    n.to_string()
                                } else if n == 0 {
                                    "Clear".into()
                                } else {
                                    "Set".into()
                                }
                            },
                        )
                    };
                    review.fields.push(Change {
                        name: format!("{} · {} · #{}", label.text, label.purpose, index),
                        before: text(old),
                        after: text(new),
                    });
                }
            }
        }
        for entry in catalog
            .collectibles()
            .iter()
            .filter(|entry| !selected.contains(&entry.hash))
        {
            let old =
                crate::app::collections_page::collectible_acquired_state(entry, before, catalog);
            let new =
                crate::app::collections_page::collectible_acquired_state(entry, after, catalog);
            if old != new {
                let text = |state| {
                    match state {
                        Some(true) => "Acquired",
                        Some(false) => "Not Acquired",
                        None => "Unresolved",
                    }
                    .into()
                };
                review.related.push(Change {
                    name: if entry.name.trim().is_empty() {
                        format!("Collection Item #{}", entry.index)
                    } else {
                        entry.name.clone()
                    },
                    before: text(old),
                    after: text(new),
                });
            }
        }
        review
            .related
            .extend(triumphs::related_changes(before, after, catalog, selected));
        review
    }
    pub fn draw(&self, ui: &mut egui::Ui) {
        if !self.related.is_empty() {
            ui.strong(format!("Also Changes {} Other Entries", self.related.len()));
            draw_changes(ui, "related_progression_changes", &self.related);
        }
        if !self.fields.is_empty() {
            egui::CollapsingHeader::new(format!("{} Saved Fields", self.fields.len()))
                .id_salt("review_saved_fields")
                .show(ui, |ui| {
                    draw_changes(ui, "progression_field_changes", &self.fields);
                });
        }
    }
}
fn draw_changes(ui: &mut egui::Ui, id: &str, rows: &[Change]) {
    let name_width = (ui.available_width() - 180.0).max(80.0);
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([12.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.weak("Field");
            ui.weak("Before");
            ui.weak("After");
            ui.end_row();
            for row in rows {
                ui.add_sized(
                    [name_width, 0.0],
                    egui::Label::new(destiny_text(ui, &row.name)).truncate(),
                )
                .on_hover_text(destiny_text(ui, &row.name));
                ui.label(&row.before);
                ui.label(&row.after);
                ui.end_row();
            }
        });
}
