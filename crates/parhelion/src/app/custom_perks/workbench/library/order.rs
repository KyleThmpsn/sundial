//! One stable order for open drafts and saved library entries.
use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::app::custom_perks::workbench) enum Order {
    #[default]
    Recent,
    NameAscending,
    NameDescending,
}

impl Order {
    fn label(self) -> &'static str {
        match self {
            Self::Recent => "Most Recent",
            Self::NameAscending => "Name A to Z",
            Self::NameDescending => "Name Z to A",
        }
    }
}

/// Width of the sort combo beside the library search.
const ORDER_WIDTH: f32 = 104.0;
/// The narrowest the library search box may get before the sort moves to its own line.
const SEARCH_WIDTH: f32 = 160.0;

impl Workbench {
    pub(super) fn draw_library_search(&mut self, ui: &mut egui::Ui) {
        let before = self.library_order;
        let row_height = ui.spacing().interact_size.y;
        let shared =
            ui.available_width() >= ORDER_WIDTH + SEARCH_WIDTH + ui.spacing().item_spacing.x;
        let mut changed = false;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                crate::app::style::compact_controls(ui);
                if shared {
                    self.draw_library_order(ui);
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search Perks")
                        .desired_width(ui.available_width()),
                );
                changed = response.changed();
                crate::app::style::named_control(response, "Search Custom Perks");
            },
        );
        if !shared {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    crate::app::style::compact_controls(ui);
                    self.draw_library_order(ui);
                },
            );
        }
        if changed || before != self.library_order {
            self.reveal_document = true;
        }
    }

    fn draw_library_order(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("perk-library-order")
            .width(ORDER_WIDTH)
            .selected_text(self.library_order.label())
            .show_ui(ui, |ui| {
                for order in [Order::Recent, Order::NameAscending, Order::NameDescending] {
                    ui.selectable_value(&mut self.library_order, order, order.label());
                }
            });
        pickers::name_combo(ui, "perk-library-order", "Sort Custom Perks");
    }

    pub(super) fn library_rows(&self) -> Vec<PerkSource> {
        let saved = self
            .entries
            .iter()
            .map(|entry| (entry.recipe.id.as_str(), entry.modified))
            .collect::<BTreeMap<_, _>>();
        let open = self
            .documents
            .iter()
            .map(|document| document.recipe.id.as_str())
            .collect::<BTreeSet<_>>();
        let documents = self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, document)| !document.untouched_copy())
            .map(|(index, document)| {
                let modified = document
                    .modified
                    .or_else(|| saved.get(document.recipe.id.as_str()).copied().flatten());
                (PerkSource::Document(index), &document.recipe, modified)
            });
        let entries = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !open.contains(entry.recipe.id.as_str()))
            .map(|(index, entry)| (PerkSource::Entry(index), &entry.recipe, entry.modified));
        let mut rows = documents
            .chain(entries)
            .map(|(source, recipe, modified)| {
                (
                    source,
                    display_name(&recipe.name).to_lowercase(),
                    &recipe.id,
                    modified,
                )
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            let order = match self.library_order {
                Order::Recent => b.3.cmp(&a.3).then_with(|| a.1.cmp(&b.1)),
                Order::NameAscending => a.1.cmp(&b.1),
                Order::NameDescending => b.1.cmp(&a.1),
            };
            order.then_with(|| a.2.cmp(b.2))
        });
        rows.into_iter().map(|row| row.0).collect()
    }
}
