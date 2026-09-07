use super::*;

impl PerkEditor {
    pub(super) fn draw_runtime_fields(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        experimental: bool,
    ) {
        ui.strong("Package Parameters");
        ui.label("Named package fields can be edited below. Their names come from the data; cross-weapon behavior and gameplay-safe ranges are not guaranteed.");
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
        let mut remove = None;
        for (index, value) in self.draft.iter().enumerate() {
            if validation::fields_for(loaded, &value.locator).len() != 1 {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Saved Edit {} Cannot Be Resolved Uniquely", index + 1),
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
        let query = self.query.trim().to_lowercase();
        let mut occurrences = BTreeMap::new();
        for field in loaded.graphs.iter().flat_map(|(_, graph)| graph.fields()) {
            *occurrences.entry(field.locator.clone()).or_insert(0usize) += 1;
        }
        let mut visible = 0;
        for (_, graph) in &loaded.graphs {
            for field in graph.fields() {
                if occurrences[&field.locator] != 1 {
                    continue;
                }
                let saved = self
                    .draft
                    .iter()
                    .find(|value| guided::equivalent(loaded, &field.locator, &value.locator));
                let unknown = field.source == WeaponRuntimeFieldSource::OpaqueNativeType;
                if unknown && !(experimental && self.show_all_native_values) {
                    continue;
                }
                let mut field = field.clone();
                if let Some(value) = saved {
                    field.locator = value.locator.clone();
                }
                if !private_perk_runtime_field_is_visible(
                    &field,
                    &query,
                    &self.draft,
                    experimental && self.show_all_native_values,
                    1,
                ) {
                    continue;
                }
                visible += 1;
                ui.push_id(&field.locator, |ui| {
                    if unknown {
                        ui.label("Experimental — Unknown Byte Range");
                    }
                    draw_runtime_value_override_field(
                        ui,
                        &field,
                        &mut self.draft,
                        &mut self.value_text,
                    );
                    if !unknown {
                        ui.label(format!("Original: {}", original_value(&field.value)));
                    }
                });
            }
        }
        if visible == 0 {
            ui.label("No supported package fields match this view.");
        }
        if !experimental
            && self.draft.iter().any(|value| {
                validation::fields_for(loaded, &value.locator)
                    .first()
                    .is_some_and(|field| field.source == WeaponRuntimeFieldSource::OpaqueNativeType)
            })
        {
            ui.label("Saved low-level edits are preserved. Verified controls above remain editable; other byte edits require experimental controls in Preferences.");
        }
    }
}

fn original_value(value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => value.to_string(),
        WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => format!("{:?}", bits.map(f32::from_bits)),
        WeaponRuntimeValue::Bytes(bytes) => {
            format!("{} bytes (Reset restores the package value)", bytes.len())
        }
    }
}
