//! Item cards and definition pickers.
use super::*;

impl SundialApp {
    pub(super) fn draw_profile_item_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &ProfileItemSnapshot,
        editable: bool,
        bucket_usage: &BucketUsage,
    ) -> Option<Edit> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let metadata = self
            .manifest
            .inventory_metadata(u64::from(snapshot.definition_hash))
            .copied();
        let current_bucket = metadata
            .filter(|metadata| metadata.scope == InventoryScope::Profile)
            .map(|metadata| metadata.native_bucket_id);
        let replacing_unresolved =
            metadata.is_none_or(|metadata| metadata.scope == InventoryScope::Unknown);
        let quantity_max = metadata
            .and_then(|metadata| metadata.max_stack_size)
            .map_or(i64::from(i32::MAX), |maximum| {
                i64::from(maximum.min(i32::MAX as u32))
            })
            .max(i64::from(snapshot.quantity));
        let key = format!("profile-items:{}", snapshot.location.index);
        let mut requested = None;
        let mut seen_request = None;
        let mut remove_requested = false;
        let mut swap_requested = false;
        let mut swap_response = None;

        ui.push_id(("profile-item", snapshot.location.index), |ui| {
            item_editor::draw_item_card(ui, |ui| {
                let (header_response, inspection_context) = draw_profile_material_header(
                    ui,
                    &self.manifest,
                    resolved.as_ref(),
                    snapshot.definition_hash,
                    snapshot.quantity,
                    format!("Profile Inventory · Item {}", snapshot.location.index + 1),
                    "not a profile-scoped stackable definition",
                );
                item_editor::draw_context_menu(
                    ui,
                    &header_response,
                    Some((u64::from(snapshot.definition_hash), inspection_context)),
                    |ui| {
                        ui.add_enabled_ui(editable, |ui| {
                            let seen = self.document.native_account().and_then(|document| {
                                document.profile_item_seen(snapshot.location.index)
                            });
                            seen_request = item_editor::draw_seen_flag(ui, seen);
                        });
                    },
                );
                ui.add_enabled_ui(editable, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        for action in item_editor::draw_level_and_quantity(
                            ui,
                            ("profile-item-numeric", snapshot.location.index),
                            NumericItemFields {
                                level: None,
                                power_max: None,
                                allow_power_above_cap: false,
                                quantity: Some(i64::from(snapshot.quantity)),
                                quantity_max: Some(quantity_max),
                            },
                        ) {
                            if let ItemEditorAction::SetQuantity { quantity } = action
                                && let Ok(quantity) = i32::try_from(quantity)
                            {
                                requested = Some(ProfileItemAction::SetQuantity(quantity));
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            if item_editor::draw_trash_button(ui, true, "Delete shared item")
                                .on_hover_text("Delete this shared item")
                                .clicked()
                            {
                                remove_requested = true;
                            }
                            let response = ui
                                .add(egui::Button::new("Swap").small())
                                .on_hover_text("Open the item picker");
                            if response.clicked() {
                                swap_requested = true;
                            }
                            swap_response = Some(response);
                        });
                    });

                    let picker_anchor = header_response.clone()
                        | swap_response.expect("a profile item card always draws Swap");
                    let picker_action = {
                        let manifest = &self.manifest;
                        let query = self.searches.entry(key.clone()).or_default();
                        item_editor::draw_definition_picker_with_open_request(
                            ui,
                            manifest,
                            ("profile-item-definition", snapshot.location.index),
                            query,
                            picker_height(),
                            (Some(&picker_anchor), swap_requested),
                            |query| DefinitionPickerChoices {
                                definitions: without_definition_groups(profile_definition_choices(
                                    manifest
                                        .profile_item_candidates(query)
                                        .filter(|definition| {
                                            u32::try_from(definition.hash).is_ok()
                                                && profile_swap_candidate(
                                                    definition.metadata,
                                                    current_bucket,
                                                    snapshot.quantity,
                                                    bucket_usage,
                                                    replacing_unresolved,
                                                )
                                        }),
                                )),
                                existing_inventory: Vec::new(),
                                clear: None,
                                random_item_builder_hash: None,
                                empty_message: "No safe profile-item definitions match".to_owned(),
                            },
                        )
                    };
                    if let Some(ItemEditorAction::SetDefinition { hash }) = picker_action
                        && let Ok(hash) = u32::try_from(hash)
                    {
                        requested = Some(ProfileItemAction::SetDefinitionHash(hash));
                        self.searches.insert(key.clone(), String::new());
                    }
                });
            });
        });
        if remove_requested {
            requested = Some(ProfileItemAction::Remove);
        }
        requested
            .map(|action| Edit::Item {
                location: snapshot.location,
                action,
            })
            .or_else(|| {
                seen_request.map(|seen| Edit::Seen {
                    position: snapshot.location.index,
                    seen,
                })
            })
    }
}
