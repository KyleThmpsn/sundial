use super::*;

impl SundialApp {
    pub(super) fn draw_postmaster(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.document.dawn_account() else {
            return;
        };
        let Some(character) = doc.characters().characters().get(self.selected_character) else {
            return;
        };
        let items = character
            .inventory
            .iter()
            .filter(|i| doc.is_postmaster(i.instance_soid.get()))
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.weak("No items at the Postmaster.");
            return;
        }
        let mut action = None;
        ui.set_max_width(760.0);
        let name_width = (ui.available_width() - 285.0).max(140.0);
        egui::Grid::new("postmaster-items")
            .striped(true)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for title in ["Item", "Quantity", "", ""] {
                    ui.strong(title);
                }
                ui.end_row();
                for item in items {
                    let soid = item.instance_soid.get();
                    ui.allocate_ui_with_layout(
                        egui::vec2(name_width, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(name_width);
                            item_label(ui, &self.manifest, item.definition_hash.get());
                        },
                    );
                    let id = ui.make_persistent_id((soid, "quantity"));
                    let mut quantity = ui
                        .data(|d| d.get_temp::<i32>(id))
                        .unwrap_or(item.quantity)
                        .clamp(1, item.quantity);
                    let partial = self
                        .manifest
                        .inventory_definition(u64::from(item.definition_hash.get()))
                        .is_some_and(|d| {
                            d.metadata.scope == crate::catalog::InventoryScope::Profile
                        });
                    ui.push_id((soid, "quantity"), |ui| {
                        ui.add_enabled(
                            partial,
                            egui::DragValue::new(&mut quantity).range(1..=item.quantity),
                        )
                    });
                    if !partial {
                        quantity = item.quantity;
                    }
                    ui.data_mut(|d| d.insert_temp(id, quantity));
                    let recovery = doc
                        .preview_postmaster_recovery(
                            self.selected_character,
                            soid,
                            quantity,
                            |hash| {
                                self.manifest
                                    .inventory_definition(u64::from(hash))
                                    .map(|d| *d.metadata)
                            },
                        )
                        .and_then(|recovered| {
                            let mut preview = self.document.clone();
                            *preview
                                .dawn_account_mut()
                                .ok_or("Dawn account is unavailable")? = recovered;
                            crate::app::account_validation::validate_new_bucket_overflows(
                                &preview,
                                &self.document,
                                &self.manifest,
                            )
                        });
                    if ui
                        .add_enabled(recovery.is_ok(), egui::Button::new("Recover"))
                        .on_disabled_hover_text(recovery.err().unwrap_or_default())
                        .clicked()
                    {
                        action = Some((soid, Some(quantity)));
                    }
                    let confirm = ui.make_persistent_id((
                        &self.document.source_info().database_path,
                        character.soid,
                        soid,
                        "confirm-discard",
                    ));
                    if discard_button(ui, confirm, item.flags.unwrap_or_default() & 1 == 0) {
                        action = Some((soid, None));
                    }
                    ui.end_row();
                }
            });
        if ui.is_enabled()
            && let Some((soid, quantity)) = action
        {
            let result = crate::app::account_validation::apply_with_bucket_limits(
                &mut self.document,
                &self.manifest,
                |document| {
                    let doc = document
                        .dawn_account_mut()
                        .ok_or("Dawn account is unavailable")?;
                    if let Some(quantity) = quantity {
                        doc.recover_postmaster(self.selected_character, soid, quantity, |hash| {
                            self.manifest
                                .inventory_definition(u64::from(hash))
                                .map(|d| *d.metadata)
                        })
                    } else {
                        doc.discard_postmaster(self.selected_character, soid)
                    }
                },
            );
            self.finish_dawn_edit(result, "Postmaster Updated");
        }
    }
}

fn discard_button(ui: &mut egui::Ui, id: egui::Id, unlocked: bool) -> bool {
    let pass = ui.ctx().cumulative_pass_nr();
    let mut armed = ui
        .data(|d| d.get_temp::<(u64, bool)>(id))
        .is_some_and(|(previous, armed)| armed && previous.saturating_add(1) >= pass);
    armed &= unlocked;
    let response = ui.add_enabled(unlocked, egui::Button::new(if armed { "Confirm Discard" } else { "Discard" }))
        .on_hover_text("Removes the entire stack without granting dismantle rewards. Undo is available before and after saving.");
    let discard = response.clicked() && armed;
    if response.clicked() {
        armed = !armed;
    } else if response.clicked_elsewhere() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        armed = false;
    }
    ui.data_mut(|d| d.insert_temp(id, (pass, armed)));
    discard
}
