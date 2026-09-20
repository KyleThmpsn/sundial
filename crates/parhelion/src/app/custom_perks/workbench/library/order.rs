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

impl Workbench {
    pub(super) fn draw_library_search(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                crate::app::style::compact_controls(ui);
                let before = self.library_order;
                egui::ComboBox::from_id_salt("perk-library-order")
                    .width(104.0)
                    .selected_text(self.library_order.label())
                    .show_ui(ui, |ui| {
                        for order in [Order::Recent, Order::NameAscending, Order::NameDescending] {
                            ui.selectable_value(&mut self.library_order, order, order.label());
                        }
                    });
                pickers::name_combo(ui, "perk-library-order", "Sort Custom Perks");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search Perks")
                        .desired_width(ui.available_width()),
                );
                let changed = response.changed() || before != self.library_order;
                crate::app::style::named_control(response, "Search Custom Perks");
                if changed {
                    self.reveal_document = true;
                }
            },
        );
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
        let documents = self.documents.iter().enumerate().map(|(index, document)| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_and_name_orders_merge_saved_perks_and_drafts_without_duplicates() {
        let temporary = tempfile::tempdir().unwrap();
        let library = Library::open(temporary.path().to_owned()).unwrap();
        let at = |seconds| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
        let mut recipe = PerkRecipe::new();
        recipe.name = "Alpha".into();
        let mut alpha = library.save(&recipe, None).unwrap();
        alpha.modified = at(10);
        let mut draft = Document::new(PerkRecipe::new(), None);
        draft.recipe.name = "Bravo".into();
        draft.modified = at(20);
        recipe = PerkRecipe::new();
        recipe.name = "Charlie".into();
        let mut charlie = library.save(&recipe, None).unwrap();
        charlie.modified = at(30);
        let opened = Document::new(alpha.recipe.clone(), Some(alpha.baseline.clone()));
        let mut workbench = Workbench {
            entries: vec![alpha, charlie],
            documents: vec![opened, draft],
            ..Default::default()
        };
        assert_eq!(
            workbench.library_rows(),
            [
                PerkSource::Entry(1),
                PerkSource::Document(1),
                PerkSource::Document(0)
            ]
        );
        workbench.library_order = Order::NameAscending;
        assert_eq!(
            workbench.library_rows(),
            [
                PerkSource::Document(0),
                PerkSource::Document(1),
                PerkSource::Entry(1)
            ]
        );
        workbench.library_order = Order::NameDescending;
        assert_eq!(
            workbench.library_rows(),
            [
                PerkSource::Entry(1),
                PerkSource::Document(1),
                PerkSource::Document(0)
            ]
        );
        workbench.documents[0].modified = at(40);
        workbench.library_order = Order::Recent;
        assert_eq!(workbench.library_rows()[0], PerkSource::Document(0));
        let restored: Vec<Document> =
            serde_json::from_slice(&serde_json::to_vec(&workbench.documents).unwrap()).unwrap();
        assert_eq!(restored[0].modified, at(40));
    }
}
