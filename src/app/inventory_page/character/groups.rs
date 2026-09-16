//! Bucket sections and virtualized character-inventory cards.

use crate::app::account_workspace as account;

use super::*;

/// Catalog inputs a bucket picker needs that do not depend on the bucket itself.
struct BucketPickerContext<'a> {
    catalog: &'a crate::catalog::Catalog,
    class_type: u64,
    show_dummy_items: bool,
    allow_cross_class_subclasses: bool,
    supports_emote_collection: bool,
}

/// Builds the picker choices for one inventory bucket, and reports a filter chip click.
fn bucket_picker_choices(
    ui: &mut egui::Ui,
    query: &str,
    filter: &mut item_editor::ItemFilter,
    bucket: &ItemBucket<CharacterInventoryEntry>,
    context: &BucketPickerContext<'_>,
) -> (DefinitionPickerChoices, bool) {
    let catalog = context.catalog;
    let in_bucket = |definition: &crate::catalog::InventoryDefinition<'_>| {
        definition.metadata.scope == bucket.key.scope
            && definition.metadata.native_bucket_id == bucket.key.native_id
    };
    let bucket_candidates = catalog
        .character_inventory_candidates(
            "",
            context.class_type,
            context.show_dummy_items,
            context.allow_cross_class_subclasses,
        )
        .filter(|definition| {
            crate::account_contract::definition_available(
                definition.hash,
                context.supports_emote_collection,
            )
        })
        .filter(in_bucket)
        .collect::<Vec<_>>();
    let filter_candidates = bucket_candidates
        .iter()
        .filter_map(|definition| definition.item)
        .collect::<Vec<_>>();
    let filter_scope = item_editor::ItemFilterScope::from_candidates(&filter_candidates);
    let filter_option_clicked = item_editor::draw_item_filter_bar(
        ui,
        "bucket-item-filters",
        filter_scope,
        &filter_candidates,
        filter,
    );
    let search = crate::catalog::CatalogSearchQuery::new(query);
    let definitions = bucket_candidates
        .into_iter()
        .filter(|definition| {
            search.matches(
                catalog,
                definition.hash,
                &[definition.name, definition.type_name],
            )
        })
        .filter(|definition| {
            definition
                .item
                .is_some_and(|item| filter.matches(catalog, item))
        });
    let choices = DefinitionPickerChoices {
        definitions: character_bucket_definition_choices(definitions),
        existing_inventory: Vec::new(),
        clear: None,
        random_item_builder_hash: None,
        empty_message: if filter.is_active() {
            "No items in this bucket match the current filters".to_owned()
        } else {
            "No compatible items in this bucket".to_owned()
        },
    };
    (choices, filter_option_clicked)
}

impl SundialApp {
    /// Adds a picked definition to the character inventory, reporting failures as status.
    fn add_picked_inventory_item(
        &mut self,
        hash: u64,
        bucket: &ItemBucket<CharacterInventoryEntry>,
        character_index: usize,
        picker_key: &str,
    ) {
        let level = item_editor::new_inventory_item_level(
            bucket.key.native_id,
            self.manifest.item_power_cap(hash),
        );
        let outcome = u32::try_from(hash)
            .map_err(|_| "The selected inventory hash does not fit in 32 bits".to_owned())
            .and_then(|hash| {
                i32::try_from(level)
                    .map_err(|_| "Could not infer a valid inventory item level".to_owned())
                    .and_then(|level| {
                        account::add_inventory_item(
                            &mut self.document,
                            character_index,
                            NewInventoryItem::single(hash, level),
                        )
                        .map_err(|error| error.to_string())
                    })
            });
        match outcome {
            Ok(_) => {
                self.searches.remove(picker_key);
                self.mark_inventory_changed("Added an item to character inventory");
            }
            Err(error) => self.set_status(error, true),
        }
    }

    /// Draws the add-item picker for one bucket and applies whatever the user selects.
    fn draw_bucket_add_picker(
        &mut self,
        ui: &mut egui::Ui,
        bucket: &ItemBucket<CharacterInventoryEntry>,
        groups: &CharacterInventoryGroupsContext<'_>,
        picker_key: &str,
        picker_anchor: Option<&egui::Response>,
        can_add: bool,
    ) {
        let character_index = groups.character_index;
        let action = ui
            .add_enabled_ui(can_add, |ui| {
                let context = BucketPickerContext {
                    catalog: &self.manifest,
                    class_type: groups.class_type,
                    show_dummy_items: self.show_dummy_items,
                    allow_cross_class_subclasses: groups.allow_cross_class_subclasses,
                    supports_emote_collection: self.document.supports_emote_collection(),
                };
                let request_open = take_bucket_picker_open_request(
                    &mut self.searches,
                    picker_key,
                    ui.input(|input| input.pointer.any_click()),
                );
                let query = self.searches.entry(picker_key.to_owned()).or_default();
                item_editor::draw_definition_picker_with_open_request_and_item_filter(
                    ui,
                    context.catalog,
                    (
                        "character-inventory-add",
                        character_index,
                        scope_id(bucket.key.scope),
                        bucket.key.native_id,
                    ),
                    query,
                    picker_height(),
                    (picker_anchor, request_open),
                    |ui, query, filter| bucket_picker_choices(ui, query, filter, bucket, &context),
                )
            })
            .inner;
        if let Some(ItemEditorAction::SetDefinition { hash }) = action {
            self.add_picked_inventory_item(hash, bucket, character_index, picker_key);
        }
    }

    /// Draws the virtualized cards for one bucket, returning any item request the user made.
    fn draw_bucket_item_cards(
        &mut self,
        ui: &mut egui::Ui,
        bucket: &ItemBucket<CharacterInventoryEntry>,
        context: &CharacterInventoryGroupsContext<'_>,
    ) -> Option<(
        InventoryItemSnapshot,
        InventoryItemUiId,
        CharacterInventoryItemRequest,
    )> {
        let character_index = context.character_index;
        let class_type = context.class_type;
        let editable = context.editable;
        let equipment_editable = context.equipment_editable;
        let (minimum_card_width, maximum_card_width) =
            self.preferences.item_card_width.dimensions();
        let mut pending = None;
        item_editor::draw_virtualized_responsive_item_cards(
            ui,
            (
                "character-inventory-cards",
                character_index,
                scope_id(bucket.key.scope),
                bucket.key.native_id,
            ),
            &bucket.items,
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
                            bucket_usage: context.bucket_usage,
                            transfer_targets: context.transfer_targets,
                            occupied_equipment_slots: context.occupied_equipment_slots,
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
        pending
    }

    /// Draws one collapsible bucket section, returning any item request the user made.
    fn draw_character_inventory_bucket(
        &mut self,
        ui: &mut egui::Ui,
        bucket: &ItemBucket<CharacterInventoryEntry>,
        context: &CharacterInventoryGroupsContext<'_>,
    ) -> Option<(
        InventoryItemSnapshot,
        InventoryItemUiId,
        CharacterInventoryItemRequest,
    )> {
        let character_index = context.character_index;
        let bucket_usage = context.bucket_usage;
        let title = bucket_header_label(bucket, bucket_usage, InventoryScope::Character);
        let picker_key = format!(
            "character-inventory:{character_index}:add:{}:{}",
            scope_id(bucket.key.scope),
            bucket.key.native_id
        );
        let array_has_room = context.inventory_available
            && context.stored_count < account::character_inventory_capacity(&self.document);
        let bucket_blocker =
            bucket_add_blocker(bucket.key, bucket.capacity, bucket_usage, &bucket.label);
        let can_add = context.editable
            && bucket.addable
            && array_has_room
            && bucket_usage.occupancy_complete
            && bucket_blocker.is_none();
        let repaint_context = ui.ctx().clone();
        let mut toggle_header = false;
        let mut open_picker = false;
        let mut picker_anchor = None;
        let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id((
                "character-inventory-bucket",
                character_index,
                scope_id(bucket.key.scope),
                bucket.key.native_id,
            )),
            true,
        )
        .show_header(ui, |ui| {
            toggle_header = ui
                .add(egui::Label::new(bucket_header_text(ui, &title)).sense(egui::Sense::click()))
                .clicked();
            if bucket.addable {
                let response = ui.add_enabled(can_add, egui::Button::new("+").small());
                let tooltip = bucket_add_tooltip(
                    can_add,
                    context.editable,
                    true,
                    array_has_room,
                    bucket_usage.occupancy_complete,
                    bucket_blocker.as_deref(),
                    &bucket.label,
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
        let mut pending = None;
        header.body(|ui| {
            draw_bucket_details(ui, bucket, bucket_usage, InventoryScope::Character);
            if self.searches.contains_key(&picker_key) {
                self.draw_bucket_add_picker(
                    ui,
                    bucket,
                    context,
                    &picker_key,
                    picker_anchor.as_ref(),
                    can_add,
                );
            }
            pending = self.draw_bucket_item_cards(ui, bucket, context);
        });
        pending
    }

    pub(super) fn draw_character_inventory_groups(
        &mut self,
        ui: &mut egui::Ui,
        context: CharacterInventoryGroupsContext<'_>,
    ) -> Option<(
        InventoryItemSnapshot,
        InventoryItemUiId,
        CharacterInventoryItemRequest,
    )> {
        let character_index = context.character_index;
        let mut pending = None;
        egui::ScrollArea::vertical()
            .id_salt(("character-inventory-buckets", character_index))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for bucket in &context.groups {
                    let request = self.draw_character_inventory_bucket(ui, bucket, &context);
                    if pending.is_none() {
                        pending = request;
                    }
                }
            });
        pending
    }
}
