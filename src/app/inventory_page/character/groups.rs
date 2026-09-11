//! Bucket sections and virtualized character-inventory cards.

use crate::app::account_workspace as account;

use super::*;

impl SundialApp {
    pub(super) fn draw_character_inventory_groups(
        &mut self,
        ui: &mut egui::Ui,
        context: CharacterInventoryGroupsContext<'_>,
    ) -> Option<(
        InventoryItemSnapshot,
        InventoryItemUiId,
        CharacterInventoryItemRequest,
    )> {
        let CharacterInventoryGroupsContext {
            character_index,
            class_type,
            stored_count,
            editable,
            equipment_editable,
            allow_cross_class_subclasses,
            inventory_available,
            groups,
            bucket_usage,
            transfer_targets,
            occupied_equipment_slots,
        } = context;
        let mut pending = None;
        egui::ScrollArea::vertical()
            .id_salt(("character-inventory-buckets", character_index))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in groups {
                    let title =
                        bucket_header_label(&group, bucket_usage, InventoryScope::Character);
                    let picker_key = format!(
                        "character-inventory:{character_index}:add:{}:{}",
                        scope_id(group.key.scope),
                        group.key.native_id
                    );
                    let array_has_room =
                        inventory_available && stored_count < CHARACTER_INVENTORY_CAPACITY;
                    let bucket_has_room = group.addable
                        && bucket_key_has_room(group.key, group.capacity, bucket_usage);
                    let can_add = editable
                        && array_has_room
                        && bucket_usage.occupancy_complete
                        && bucket_has_room;
                    let repaint_context = ui.ctx().clone();
                    let mut toggle_header = false;
                    let mut open_picker = false;
                    let mut picker_anchor = None;
                    let mut header =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            ui.make_persistent_id((
                                "character-inventory-bucket",
                                character_index,
                                scope_id(group.key.scope),
                                group.key.native_id,
                            )),
                            true,
                        )
                        .show_header(ui, |ui| {
                            toggle_header = ui
                                .add(
                                    egui::Label::new(bucket_header_text(ui, &title))
                                        .sense(egui::Sense::click()),
                                )
                                .clicked();
                            if group.addable {
                                let response =
                                    ui.add_enabled(can_add, egui::Button::new("+").small());
                                let tooltip = bucket_add_tooltip(
                                    can_add,
                                    editable,
                                    true,
                                    array_has_room,
                                    bucket_usage.occupancy_complete,
                                    bucket_has_room,
                                    &group.label,
                                );
                                let response = if can_add {
                                    response.on_hover_text(tooltip)
                                } else {
                                    response.on_disabled_hover_text(tooltip)
                                };
                                open_picker = response.clicked();
                                picker_anchor = Some(response);
                            }
                        });
                    if toggle_header {
                        header.toggle();
                    }
                    if open_picker {
                        header.set_open(true);
                        self.open_bucket_picker(
                            &picker_key,
                            &format!("character-inventory:{character_index}:add:"),
                        );
                        repaint_context.request_repaint();
                    }
                    header.body(|ui| {
                        draw_bucket_details(ui, &group, bucket_usage, InventoryScope::Character);
                        if self.searches.contains_key(&picker_key) {
                            let action = ui
                                .add_enabled_ui(can_add, |ui| {
                                    let manifest = &self.manifest;
                                    let show_dummy_items = self.show_dummy_items;
                                    let request_open = take_bucket_picker_open_request(
                                        &mut self.searches,
                                        &picker_key,
                                        ui.input(|input| input.pointer.any_click()),
                                    );
                                    let query =
                                        self.searches.entry(picker_key.clone()).or_default();
                                    item_editor::draw_definition_picker_with_open_request_and_item_filter(
                                        ui,
                                        manifest,
                                        (
                                            "character-inventory-add",
                                            character_index,
                                            scope_id(group.key.scope),
                                            group.key.native_id,
                                        ),
                                        query,
                                        picker_height(),
                                        (picker_anchor.as_ref(), request_open),
                                        |ui, query, filter| {
                                            let in_bucket = |definition: &crate::catalog::InventoryDefinition<'_>| {
                                                definition.metadata.scope == group.key.scope
                                                    && definition.metadata.native_bucket_id
                                                        == group.key.native_id
                                            };
                                            let bucket_candidates = manifest
                                                .character_inventory_candidates(
                                                    "",
                                                    class_type,
                                                    show_dummy_items,
                                                    allow_cross_class_subclasses,
                                                )
                                                .filter(|definition| crate::account_contract::definition_available(definition.hash, self.document.supports_v13_account()))
                                                .filter(in_bucket)
                                                .collect::<Vec<_>>();
                                            let filter_candidates = bucket_candidates.iter()
                                                .filter_map(|definition| definition.item)
                                                .collect::<Vec<_>>();
                                            let filter_scope =
                                                item_editor::ItemFilterScope::from_candidates(
                                                    &filter_candidates,
                                                );
                                            let filter_option_clicked =
                                                item_editor::draw_item_filter_bar(
                                                ui,
                                                "bucket-item-filters",
                                                filter_scope,
                                                &filter_candidates,
                                                filter,
                                            );
                                            let search = crate::catalog::CatalogSearchQuery::new(query);
                                            let definitions = bucket_candidates.into_iter()
                                                .filter(|definition| search.matches(manifest, definition.hash, &[definition.name, definition.type_name]))
                                                .filter(|definition| {
                                                    definition.item.is_some_and(|item| {
                                                        filter.matches(manifest, item)
                                                    })
                                                });
                                            let choices = DefinitionPickerChoices {
                                                definitions: character_bucket_definition_choices(
                                                    definitions,
                                                ),
                                                existing_inventory: Vec::new(),
                                                clear: None,
                                                random_item_builder_hash: None,
                                                empty_message: if filter.is_active() {
                                                    "No items in this bucket match the current filters"
                                                        .to_owned()
                                                } else {
                                                    "No compatible items in this bucket".to_owned()
                                                },
                                            };
                                            (choices, filter_option_clicked)
                                        },
                                    )
                                })
                                .inner;
                            if let Some(ItemEditorAction::SetDefinition { hash }) = action {
                                let level = item_editor::new_inventory_item_level(
                                    group.key.native_id,
                                    self.manifest.item_power_cap(hash),
                                );
                                match u32::try_from(hash)
                                    .map_err(|_| {
                                        "The selected inventory hash does not fit in 32 bits"
                                            .to_owned()
                                    })
                                    .and_then(|hash| {
                                        i32::try_from(level)
                                            .map_err(|_| {
                                                "Could not infer a valid inventory item level"
                                                    .to_owned()
                                            })
                                            .and_then(|level| {
                                                account::add_inventory_item(
                                                        &mut self.document,
                                                        character_index,
                                                        NewInventoryItem::single(hash, level),
                                                    )
                                                    .map_err(|error| error.to_string())
                                            })
                                    }) {
                                    Ok(_) => {
                                        self.searches.remove(&picker_key);
                                        self.mark_inventory_changed(
                                            "Added an item to character inventory",
                                        );
                                    }
                                    Err(error) => self.set_status(error, true),
                                }
                            }
                        }
                        let (minimum_card_width, maximum_card_width) =
                            self.preferences.item_card_width.dimensions();
                        item_editor::draw_virtualized_responsive_item_cards(
                            ui,
                            (
                                "character-inventory-cards",
                                character_index,
                                scope_id(group.key.scope),
                                group.key.native_id,
                            ),
                            &group.items,
                            minimum_card_width,
                            maximum_card_width,
                            |entry| match entry {
                                CharacterInventoryEntry::Equipped(snapshot) => {
                                    egui::Id::new(("equipped", snapshot.slot))
                                }
                                CharacterInventoryEntry::Stored { ui_identity, .. } => {
                                    egui::Id::new(("stored", ui_identity))
                                }
                            },
                            |ui, entry| match entry {
                                CharacterInventoryEntry::Equipped(snapshot) => {
                                    self.draw_equipped_item_card(
                                        ui,
                                        character_index,
                                        snapshot,
                                        class_type,
                                        equipment_editable,
                                    );
                                }
                                CharacterInventoryEntry::Stored {
                                    snapshot,
                                    ui_identity,
                                } => {
                                    let request = self.draw_inventory_item_card(
                                        ui,
                                        snapshot,
                                        *ui_identity,
                                        editable,
                                        class_type,
                                        CharacterInventoryCardContext {
                                            bucket_usage,
                                            transfer_targets,
                                            occupied_equipment_slots,
                                        },
                                    );
                                    if pending.is_none()
                                        && let Some(request) = request
                                    {
                                        pending = Some((snapshot.clone(), *ui_identity, request));
                                    }
                                }
                            },
                        );
                    });
                }
            });
        pending
    }
}
