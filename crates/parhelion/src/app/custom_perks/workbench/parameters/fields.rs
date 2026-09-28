use super::*;

impl PerkEditor {
    /// Saved edits that match no single field, each with a way to remove it.
    pub(super) fn draw_unresolved_edits(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) {
        let mut remove = None;
        for (index, value) in self.draft.iter().enumerate() {
            if validation::fields_for(loaded, &value.locator).len() != 1 {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Saved field {} cannot be resolved", index + 1),
                    );
                    if ui.button("Remove Saved Edit").clicked() {
                        remove = Some(index);
                    }
                });
            }
        }
        if let Some(index) = remove {
            let removed = self.draft.remove(index);
            self.value_text
                .retain(|(locator, _), _| locator != &removed.locator);
        }
    }

    pub(super) fn draw_runtime_fields(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        experimental: bool,
    ) {
        ui.add_space(8.0);
        ui.strong("Package Parameters");
        ui.horizontal_wrapped(|ui| {
            ui.label("Filter");
            named_control(
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .desired_width(220.0)
                        .hint_text("Parameter name or type"),
                ),
                "Filter Custom Perk Parameters",
            );
            if experimental {
                ui.checkbox(&mut self.show_all_native_values, "Show Unknown Bytes");
            }
        });
        let query = self.query.trim().to_lowercase();
        let mut visible = 0;
        // Only opaque byte ranges are drawn here, and only while they are shown.
        if experimental && self.show_all_native_values {
            let mut occurrences = BTreeMap::new();
            for field in loaded.graphs.iter().flat_map(|(_, graph)| graph.fields()) {
                *occurrences.entry(&field.locator).or_insert(0usize) += 1;
            }
            for (_, graph) in &loaded.graphs {
                for field in graph.fields() {
                    if field.source != WeaponRuntimeFieldSource::OpaqueNativeType
                        || occurrences[&field.locator] != 1
                    {
                        continue;
                    }
                    let saved = self
                        .draft
                        .iter()
                        .find(|value| guided::equivalent(loaded, &field.locator, &value.locator));
                    let mut field = field.clone();
                    if let Some(value) = saved {
                        field.locator = value.locator.clone();
                    }
                    if !private_perk_runtime_field_is_visible(&field, &query, &self.draft, true, 1)
                    {
                        continue;
                    }
                    visible += 1;
                    ui.push_id(&field.locator, |ui| {
                        ui.label("Unknown Byte Range");
                        draw_runtime_value_override_field(
                            ui,
                            &field,
                            &mut self.draft,
                            &mut self.value_text,
                        );
                    });
                }
            }
        }
        visible += self.draw_native_fields(ui, loaded, &query);
        if visible == 0 {
            ui.label("No supported package fields match this view.");
        }
        structure::draw(ui, loaded, &query);
    }
}
