use super::*;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub(super) enum Sort {
    #[default]
    MostDownloaded,
    Name,
}

impl Sort {
    fn compare(self, left: &service::Entry, right: &service::Entry) -> std::cmp::Ordering {
        let popularity = if self == Self::MostDownloaded {
            right.downloads.cmp(&left.downloads)
        } else {
            std::cmp::Ordering::Equal
        };
        popularity
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.listing.id.cmp(&right.listing.id))
    }
}

impl Window {
    pub(super) fn browse(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, can_edit: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search Recipes or Creators")
                    .desired_width(260.0),
            );
            let tags: BTreeSet<&str> = self
                .catalog
                .as_ref()
                .into_iter()
                .flat_map(|catalog| &catalog.recipes)
                .flat_map(|entry| entry.listing.tags.iter().map(String::as_str))
                .collect();
            egui::ComboBox::from_id_salt("community-tag")
                .selected_text(if self.tag.is_empty() {
                    "All Tags"
                } else {
                    &self.tag
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.tag, String::new(), "All Tags");
                    for tag in tags {
                        ui.selectable_value(&mut self.tag, tag.to_owned(), tag);
                    }
                });
            egui::ComboBox::from_id_salt("community-sort")
                .selected_text(match self.sort {
                    Sort::MostDownloaded => "Most Downloaded",
                    Sort::Name => "Name",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort, Sort::MostDownloaded, "Most Downloaded");
                    ui.selectable_value(&mut self.sort, Sort::Name, "Name");
                });
            if ui
                .add_enabled(self.worker.is_none(), egui::Button::new("Refresh"))
                .clicked()
            {
                self.start(ctx, Job::Catalog, |client| {
                    client.catalog().map(Outcome::Catalog)
                });
            }
        });
        // Taken for this frame so the rows can borrow it while the window changes.
        let Some(catalog) = self.catalog.take() else {
            ui.add_space(20.0);
            ui.label("Refresh to load published community recipes.");
            return;
        };
        let query = self.search.trim().to_lowercase();
        let mut entries: Vec<&service::Entry> = catalog
            .recipes
            .iter()
            .filter(|entry| {
                (self.tab != Tab::Downloads || self.receipts.contains_key(&entry.listing.id))
                    && (self.tag.is_empty() || entry.listing.tags.contains(&self.tag))
                    && format!(
                        "{} {} {} {}",
                        entry.name,
                        entry.listing.author,
                        entry.listing.description,
                        entry.listing.tags.join(" ")
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        entries.sort_by(|left, right| self.sort.compare(left, right));
        let count = entries.len();
        ui.label(format!(
            "{count} recipe{}",
            if count == 1 { "" } else { "s" }
        ));
        ui.separator();
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical()
                .id_salt("community-list")
                .max_height(columns[0].available_height().max(200.0))
                .show(&mut columns[0], |ui| {
                    if entries.is_empty() {
                        ui.label("No recipes match this view.");
                    }
                    for entry in &entries {
                        ui.group(|ui| {
                            let selected = self.selected.as_ref() == Some(&entry.listing.id);
                            if ui
                                .add_enabled(
                                    self.worker.is_none(),
                                    egui::Button::new(&entry.name).selected(selected),
                                )
                                .clicked()
                            {
                                self.selected = Some(entry.listing.id.clone());
                                self.downloaded = None;
                                let entry = (*entry).clone();
                                self.start(ctx, Job::Download, move |client| {
                                    client
                                        .download(&entry)
                                        .map(|recipe| Outcome::Downloaded(Box::new(recipe)))
                                });
                            }
                            ui.label(format!(
                                "By {} · Revision {}",
                                entry.listing.author, entry.listing.version
                            ));
                            ui.small(entry.listing.tags.join(" · "));
                            if let Some(receipt) = self.receipts.get(&entry.listing.id) {
                                ui.label(if receipt.version < entry.listing.version {
                                    "Update Available"
                                } else {
                                    "In Your Library"
                                });
                            }
                        });
                        ui.add_space(5.0);
                    }
                });
            egui::ScrollArea::vertical()
                .id_salt("community-detail")
                .max_height(columns[1].available_height().max(200.0))
                .show(&mut columns[1], |ui| {
                    if let Some(downloaded) = self.downloaded.take() {
                        self.recipe_detail(ui, &downloaded, can_edit);
                        self.downloaded = Some(downloaded);
                    } else {
                        ui.label("No Recipe Selected");
                    }
                });
        });
        self.catalog = Some(catalog);
    }

    fn recipe_detail(&mut self, ui: &mut egui::Ui, downloaded: &Downloaded, can_edit: bool) {
        let listing = &downloaded.entry.listing;
        ui.heading(&downloaded.recipe.name);
        ui.label(format!("By {}", listing.author));
        ui.label(&listing.description);
        ui.add_space(8.0);
        ui.strong(if listing.gameplay_status == "author-tested" {
            "Author Tested"
        } else {
            "Gameplay Unverified"
        });
        ui.label(&listing.gameplay_notes);
        ui.label(format!(
            "Sundial: {}\nSunrise: {}",
            listing.tested_with.sundial, listing.tested_with.sunrise
        ));
        if let Some(original) = &listing.remix_of {
            ui.label(format!("Remix of {original}"));
        }
        ui.add_space(10.0);
        let receipt = self.receipts.get(&listing.id).cloned();
        let label = match &receipt {
            Some(receipt) if receipt.version < listing.version => "Update Recipe",
            Some(_) => "Check Local Copy",
            None => "Add to Library",
        };
        ui.add_enabled_ui(can_edit && self.worker.is_none(), |ui| {
            if ui.button(label).clicked() {
                self.action = Some(Action::Install(Box::new(downloaded.clone())));
            }
            if let Some(receipt) = receipt {
                // The parent resolves this direct filename against the configured library.
                if ui.button("Open in Workbench").clicked() {
                    self.action = Some(Action::Open(PathBuf::from(receipt.file_name)));
                }
            }
            ui.separator();
            ui.strong("Make a Remix");
            ui.add(egui::TextEdit::singleline(&mut self.remix_name).hint_text("Remix Name"));
            if ui
                .add_enabled(
                    !self.remix_name.trim().is_empty(),
                    egui::Button::new("Create Remix"),
                )
                .clicked()
            {
                self.action = Some(Action::Remix(
                    Box::new(downloaded.clone()),
                    self.remix_name.clone(),
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popularity_sorts_descending_with_stable_name_ties() {
        let mut first = super::super::tests::recipe().entry;
        first.name = "Alpha".into();
        let mut second = first.clone();
        second.name = "Beta".into();
        second.downloads = 10;
        assert!(Sort::MostDownloaded.compare(&second, &first).is_lt());
        assert!(Sort::Name.compare(&first, &second).is_lt());
        first.downloads = 10;
        assert!(Sort::MostDownloaded.compare(&first, &second).is_lt());
    }
}
