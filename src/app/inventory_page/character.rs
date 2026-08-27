//! Character-scoped inventory, equipped-item, and transfer rendering.

use std::collections::HashMap;

use eframe::egui;

use crate::{
    catalog::{InventoryScope, ItemDef},
    hash::format_hash_hex,
};

use super::super::{
    ARMOR_SLOTS, CharacterInventoryLockFilter, CharacterInventorySort,
    CharacterInventorySourceFilter, PLUG_PICKER_MAX_HEIGHT, PLUG_PICKER_MIN_HEIGHT, SundialApp,
    WEAPON_SLOTS,
    equipment::{self, EquipmentSlotCard, EquippedItemSnapshot, class_name, native_plug_default},
    inspector::DefinitionInspectionContext,
    inventory::{
        self, CHARACTER_INVENTORY_CAPACITY, INVENTORY_FLAG_LOCKED, InventoryItemAction,
        InventoryItemSnapshot, ItemPlugs, NewInventoryItem, SchemaMode, set_inventory_locked_flag,
    },
    item_editor::{
        self, DefinitionPickerChoices, DefinitionSummary, ItemEditorAction, ItemHeader,
        NumericItemFields, PickerHeight,
    },
};
use super::{
    buckets::{
        add_candidate_buckets, bucket_add_tooltip, bucket_has_room, bucket_header_label,
        bucket_header_text, bucket_key_has_room, draw_bucket_details, scope_id,
    },
    definitions::{
        character_bucket_definition_choices, character_definition_choices,
        without_definition_groups,
    },
    interactions::{
        apply_inventory_actions_atomic, character_bucket_usage_detail, displayed_inventory_plugs,
        draw_character_transfer_destinations, inventory_item_state_key,
        inventory_item_ui_identities, take_bucket_picker_open_request,
    },
    model::{
        BucketUsage, CharacterInventoryCardContext, CharacterInventoryEditorContext,
        CharacterInventoryEntry, CharacterInventoryItemRequest, CharacterTransferDestination,
        CharacterTransferTarget, InventoryItemUiId, ResolvedDefinition,
    },
    presentation::{
        InventoryPageKind, draw_inventory_source_error, draw_schema_notice,
        draw_unresolved_bucket_warning, equipment_target_for_bucket, equipped_header_fill,
        picker_height, picker_height_with_transfer_destinations,
    },
};

struct CharacterInventorySources {
    class_type: u64,
    items: Vec<InventoryItemSnapshot>,
    inventory_error: Option<String>,
    equipped_items: Vec<EquippedItemSnapshot>,
    equipment_error: Option<String>,
}

impl SundialApp {
    pub(in crate::app) fn draw_character_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        let editable = self
            .account_workspace
            .can_mutate_character_inventory(&self.document);
        let equipment_editable = self.account_workspace.can_mutate_equipment(&self.document);
        ui.heading("Character inventory");
        ui.label(
            "Items stored separately for each character, with equipped items shown in their native buckets.",
        );
        if self.document.uses_json_account() {
            draw_schema_notice(ui, mode, InventoryPageKind::Character);
        }
        ui.add_space(4.0);
        self.draw_character_inventory_section(ui, mode, editable, equipment_editable);
    }

    fn draw_character_inventory_view_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.character_inventory_query)
                    .hint_text("Search inventory by name, type, hash, or instance…")
                    .desired_width(360.0),
            );
            egui::ComboBox::from_id_salt("character-inventory-source-filter")
                .selected_text(self.character_inventory_source_filter.label())
                .show_ui(ui, |ui| {
                    for filter in [
                        CharacterInventorySourceFilter::All,
                        CharacterInventorySourceFilter::Stored,
                        CharacterInventorySourceFilter::Equipped,
                    ] {
                        ui.selectable_value(
                            &mut self.character_inventory_source_filter,
                            filter,
                            filter.label(),
                        );
                    }
                });
            egui::ComboBox::from_id_salt("character-inventory-sort")
                .selected_text(self.character_inventory_sort.label())
                .show_ui(ui, |ui| {
                    for sort in [
                        CharacterInventorySort::InventoryOrder,
                        CharacterInventorySort::Name,
                        CharacterInventorySort::PowerDescending,
                    ] {
                        ui.selectable_value(&mut self.character_inventory_sort, sort, sort.label());
                    }
                });
            egui::ComboBox::from_id_salt("character-inventory-lock-filter")
                .selected_text(self.character_inventory_lock_filter.label())
                .show_ui(ui, |ui| {
                    for filter in [
                        CharacterInventoryLockFilter::All,
                        CharacterInventoryLockFilter::Locked,
                        CharacterInventoryLockFilter::Unlocked,
                    ] {
                        ui.selectable_value(
                            &mut self.character_inventory_lock_filter,
                            filter,
                            filter.label(),
                        );
                    }
                });
            if (!self.character_inventory_query.is_empty()
                || self.character_inventory_source_filter != CharacterInventorySourceFilter::All
                || self.character_inventory_lock_filter != CharacterInventoryLockFilter::All
                || self.character_inventory_sort != CharacterInventorySort::InventoryOrder)
                && ui.button("Reset view").clicked()
            {
                self.character_inventory_query.clear();
                self.character_inventory_source_filter = CharacterInventorySourceFilter::All;
                self.character_inventory_lock_filter = CharacterInventoryLockFilter::All;
                self.character_inventory_sort = CharacterInventorySort::InventoryOrder;
            }
        });
    }

    fn character_inventory_entry_matches(
        &self,
        entry: &CharacterInventoryEntry,
        query: &str,
    ) -> bool {
        let source_matches = match self.character_inventory_source_filter {
            CharacterInventorySourceFilter::All => true,
            CharacterInventorySourceFilter::Stored => entry.is_stored(),
            CharacterInventorySourceFilter::Equipped => !entry.is_stored(),
        };
        let lock_matches = match self.character_inventory_lock_filter {
            CharacterInventoryLockFilter::All => true,
            CharacterInventoryLockFilter::Locked => entry.locked(),
            CharacterInventoryLockFilter::Unlocked => !entry.locked(),
        };
        if !source_matches || !lock_matches || query.is_empty() {
            return source_matches && lock_matches;
        }
        let hash = entry.definition_hash();
        let definition_matches =
            hash.and_then(|hash| self.manifest.item(hash))
                .is_some_and(|item| {
                    item.name.to_ascii_lowercase().contains(query)
                        || item.type_name.to_ascii_lowercase().contains(query)
                });
        let hash_matches = hash.is_some_and(|hash| {
            hash.to_string().contains(query)
                || format_hash_hex(hash).to_ascii_lowercase().contains(query)
        });
        let instance_matches = match entry {
            CharacterInventoryEntry::Equipped(snapshot) => snapshot
                .instance_soid
                .is_some_and(|soid| format_hash_hex(soid).to_ascii_lowercase().contains(query)),
            CharacterInventoryEntry::Stored { snapshot, .. } => {
                format_hash_hex(snapshot.instance_soid)
                    .to_ascii_lowercase()
                    .contains(query)
            }
        };
        definition_matches || hash_matches || instance_matches
    }

    fn character_inventory_sources(&self, character_index: usize) -> CharacterInventorySources {
        let class_type = self
            .account_workspace
            .character_metadata(&self.document, character_index)
            .ok()
            .map(|metadata| u64::from(metadata.class_type))
            .unwrap_or(99);
        let (items, inventory_error) = match self
            .account_workspace
            .character_inventory(&self.document, character_index)
        {
            Ok(items) => (items.unwrap_or_default(), None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let (equipped_items, equipment_error) = match self
            .account_workspace
            .equipped_item_snapshots(&self.document, character_index)
        {
            Ok(items) => (items, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        CharacterInventorySources {
            class_type,
            items,
            inventory_error,
            equipped_items,
            equipment_error,
        }
    }

    fn draw_character_inventory_header(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        sources: &CharacterInventorySources,
        editable: bool,
        equipment_editable: bool,
    ) {
        let stored_count = sources.items.len();
        let equipped_count = sources.equipped_items.len();
        let inventory_capacity = self
            .account_workspace
            .character_inventory_capacity(&self.document);
        let randomize_request = ui
            .horizontal_wrapped(|ui| {
                ui.strong(format!("Character {}", character_index + 1));
                ui.label(
                    egui::RichText::new(format!(
                        "{stored_count} / {inventory_capacity} stored · {equipped_count} equipped"
                    ))
                    .weak(),
                );
                let request = equipment::draw_randomize_menu(ui, equipment_editable, editable);
                if equipment::draw_armor_stats_button(ui, equipment_editable).clicked() {
                    self.armor_stats_adjuster.open(character_index);
                }
                request
            })
            .inner;
        equipment::draw_randomize_dialogs(self, ui.ctx(), character_index, randomize_request);
        equipment::draw_armor_stats_window(self, ui.ctx(), character_index);
        self.draw_equipped_armor_stat_row(ui, character_index);
        ui.add_enabled_ui(equipment_editable, |ui| self.draw_item_safety_controls(ui));
        ui.separator();
    }

    fn draw_character_inventory_notices(
        &self,
        ui: &mut egui::Ui,
        sources: &CharacterInventorySources,
        editable: bool,
        equipment_editable: bool,
    ) {
        if !editable {
            let message = if equipment_editable {
                "Stored character-inventory editing requires Sunrise settings schema 6; equipped loadout items remain editable."
            } else {
                "Stored character-inventory editing requires Sunrise settings schema 6; equipped loadout editing is also disabled for this schema."
            };
            ui.label(egui::RichText::new(message).weak());
        } else if sources.items.len() >= CHARACTER_INVENTORY_CAPACITY {
            ui.label(egui::RichText::new("This character inventory is full.").weak());
        }
        if let Some(error) = &sources.inventory_error {
            draw_inventory_source_error(ui, "Stored inventory", error);
        }
        if let Some(error) = &sources.equipment_error {
            draw_inventory_source_error(ui, "Equipped items", error);
        }
    }

    fn draw_character_inventory_section(
        &mut self,
        ui: &mut egui::Ui,
        _mode: SchemaMode,
        editable: bool,
        equipment_editable: bool,
    ) {
        self.draw_character_tabs(ui);
        ui.separator();

        let character_index = self.selected_character;
        let sources = self.character_inventory_sources(character_index);
        self.draw_character_inventory_header(
            ui,
            character_index,
            &sources,
            editable,
            equipment_editable,
        );
        self.draw_character_inventory_notices(ui, &sources, editable, equipment_editable);
        let CharacterInventorySources {
            class_type,
            items,
            inventory_error,
            equipped_items,
            equipment_error,
        } = sources;
        let stored_count = items.len();
        self.draw_character_inventory_view_controls(ui);

        let mut bucket_usage = self.inventory_bucket_usage(
            &items,
            equipment_error
                .is_none()
                .then_some(equipped_items.as_slice()),
        );
        if inventory_error.is_some() || equipment_error.is_some() {
            bucket_usage.occupancy_complete = false;
        }
        if bucket_usage.unresolved_count > 0 {
            draw_unresolved_bucket_warning(ui);
        }
        ui.add_space(4.0);

        let ui_identities = inventory_item_ui_identities(&items);
        let occupied_equipment_slots = equipped_items
            .iter()
            .map(|item| item.slot)
            .collect::<Vec<_>>();
        let transfer_targets = self.character_transfer_targets(character_index);
        let mut entries = equipped_items
            .into_iter()
            .map(CharacterInventoryEntry::Equipped)
            .collect::<Vec<_>>();
        entries.extend(
            items
                .into_iter()
                .zip(ui_identities)
                .map(|(snapshot, ui_identity)| CharacterInventoryEntry::Stored {
                    snapshot,
                    ui_identity,
                }),
        );
        let filters_active = !self.character_inventory_query.trim().is_empty()
            || self.character_inventory_source_filter != CharacterInventorySourceFilter::All
            || self.character_inventory_lock_filter != CharacterInventoryLockFilter::All;
        let query = self.character_inventory_query.trim().to_ascii_lowercase();
        entries.retain(|entry| self.character_inventory_entry_matches(entry, &query));
        if filters_active {
            ui.label(
                egui::RichText::new(format!("Showing {} matching items", entries.len())).weak(),
            );
        }
        let candidate_buckets = self
            .manifest
            .character_inventory_candidate_buckets(class_type, self.show_dummy_items)
            .iter()
            .copied();
        let mut groups = self.group_items_by_bucket(
            entries,
            CharacterInventoryEntry::definition_hash,
            InventoryScope::Character,
        );
        if !filters_active {
            add_candidate_buckets(&mut groups, candidate_buckets, InventoryScope::Character);
        }
        if self.character_inventory_sort != CharacterInventorySort::InventoryOrder {
            for group in &mut groups {
                match self.character_inventory_sort {
                    CharacterInventorySort::InventoryOrder => {}
                    CharacterInventorySort::Name => group.items.sort_by_cached_key(|entry| {
                        entry
                            .definition_hash()
                            .and_then(|hash| self.manifest.item(hash))
                            .map_or_else(String::new, |item| item.name.to_ascii_lowercase())
                    }),
                    CharacterInventorySort::PowerDescending => {
                        group
                            .items
                            .sort_by_key(|entry| std::cmp::Reverse(entry.level()));
                    }
                }
            }
        }
        if groups.is_empty() {
            ui.label(
                egui::RichText::new(if filters_active {
                    "No inventory items match the current view."
                } else {
                    "No character inventory buckets are available."
                })
                .weak(),
            );
            return;
        }

        let mut pending = None;
        egui::ScrollArea::vertical()
            .id_salt(("character-inventory-buckets", character_index))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in groups {
                    let title =
                        bucket_header_label(&group, &bucket_usage, InventoryScope::Character);
                    let picker_key = format!(
                        "character-inventory:{character_index}:add:{}:{}",
                        scope_id(group.key.scope),
                        group.key.native_id
                    );
                    let array_has_room =
                        inventory_error.is_none() && stored_count < CHARACTER_INVENTORY_CAPACITY;
                    let bucket_has_room = group.addable
                        && bucket_key_has_room(group.key, group.capacity, &bucket_usage);
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
                        draw_bucket_details(ui, &group, &bucket_usage, InventoryScope::Character);
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
                                    item_editor::draw_definition_picker_with_open_request(
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
                                        |query| DefinitionPickerChoices {
                                            definitions: character_bucket_definition_choices(
                                                manifest
                                                    .character_inventory_candidates(
                                                        query,
                                                        class_type,
                                                        show_dummy_items,
                                                    )
                                                    .filter(|definition| {
                                                        definition.metadata.scope == group.key.scope
                                                            && definition.metadata.native_bucket_id
                                                                == group.key.native_id
                                                    }),
                                            ),
                                            existing_inventory: Vec::new(),
                                            clear: None,
                                            random_item_builder_hash: None,
                                            empty_message: "No compatible items in this bucket"
                                                .to_owned(),
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
                                                self.account_workspace
                                                    .add_inventory_item(
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
                            self.item_card_width.dimensions();
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
                                            bucket_usage: &bucket_usage,
                                            transfer_targets: &transfer_targets,
                                            occupied_equipment_slots: &occupied_equipment_slots,
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
        if let Some((snapshot, ui_identity, request)) = pending {
            self.apply_character_inventory_item_request(&snapshot, ui_identity, request);
        }
    }

    pub(in crate::app) fn character_inventory_editor_context(
        &self,
        editable: bool,
        class_type: u64,
    ) -> CharacterInventoryEditorContext {
        CharacterInventoryEditorContext {
            editable,
            class_type,
        }
    }

    pub(in crate::app) fn apply_character_inventory_item_actions(
        &mut self,
        snapshot: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        actions: Vec<InventoryItemAction>,
    ) {
        self.apply_character_inventory_item_request(
            snapshot,
            ui_identity,
            CharacterInventoryItemRequest::Apply(actions),
        );
    }

    fn apply_character_inventory_item_request(
        &mut self,
        snapshot: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        request: CharacterInventoryItemRequest,
    ) {
        match request {
            CharacterInventoryItemRequest::Apply(actions) => {
                let structural = actions
                    .iter()
                    .any(|action| matches!(action, InventoryItemAction::Remove));
                let definition_changed = actions
                    .iter()
                    .any(|action| matches!(action, InventoryItemAction::SetDefinitionHash(_)));
                match apply_inventory_actions_atomic(
                    self.account_workspace,
                    &mut self.document,
                    snapshot.location,
                    actions,
                ) {
                    Ok(()) => {
                        self.mark_inventory_changed(if structural {
                            "Removed an item from character inventory"
                        } else {
                            "Updated a character inventory item"
                        });
                        if structural || definition_changed {
                            self.clear_inventory_item_picker_state(ui_identity, structural);
                        }
                    }
                    Err(error) => self.set_status(error, true),
                }
            }
            CharacterInventoryItemRequest::Equip(slot) => {
                if self.equip_stored_item(snapshot.location, slot) {
                    self.clear_inventory_item_picker_state(ui_identity, true);
                }
            }
            CharacterInventoryItemRequest::MoveTo(destination_character_index) => {
                match self.account_workspace.move_inventory_item_to_character(
                    &mut self.document,
                    snapshot.location,
                    destination_character_index,
                ) {
                    Ok(_) => {
                        let class_type = self
                            .account_workspace
                            .character_metadata(&self.document, destination_character_index)
                            .ok()
                            .map(|metadata| u64::from(metadata.class_type))
                            .unwrap_or(99);
                        self.mark_inventory_changed(&format!(
                            "Moved an item to Character {} · {}",
                            destination_character_index + 1,
                            class_name(class_type)
                        ));
                        self.clear_inventory_item_picker_state(ui_identity, true);
                    }
                    Err(error) => self.set_status(error.to_string(), true),
                }
            }
        }
    }

    fn draw_equipped_item_card(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        snapshot: &EquippedItemSnapshot,
        class_type: u64,
        editable: bool,
    ) {
        self.draw_equipment_slot_card(
            ui,
            character_index,
            EquipmentSlotCard {
                id_scope: "character-inventory-equipped",
                slot: snapshot.slot,
                label: snapshot.slot_label,
                bucket_hash: snapshot.bucket_hash,
                class_type,
                editable,
                header_fill: Some(equipped_header_fill(ui)),
                snapshot: Some(snapshot),
            },
        );
    }

    fn draw_inventory_item_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        editable: bool,
        class_type: u64,
        context: CharacterInventoryCardContext<'_>,
    ) -> Option<CharacterInventoryItemRequest> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let metadata = self
            .manifest
            .inventory_metadata(u64::from(snapshot.definition_hash))
            .copied();
        let hash_hex_text = format_hash_hex(u64::from(snapshot.definition_hash));
        let soid_text = format!("0x{:016X}", snapshot.instance_soid);
        let current_bucket = metadata
            .filter(|metadata| metadata.scope == InventoryScope::Character)
            .map(|metadata| metadata.native_bucket_id);
        let replacing_unresolved =
            metadata.is_none_or(|metadata| metadata.scope == InventoryScope::Unknown);
        let valid = resolved.as_ref().is_some_and(|definition| {
            definition.metadata.is_character_inventory_candidate()
                && definition
                    .item
                    .as_ref()
                    .is_some_and(|item| item.class_type == 3 || item.class_type == class_type)
        });
        let key = inventory_item_state_key(ui_identity);
        let mut requested = Vec::new();
        let mut remove_requested = false;
        let mut equip_requested = None;
        let mut move_requested = None;
        let mut swap_requested = false;
        let mut swap_response = None;
        let transfer_destinations =
            self.character_transfer_destinations(context.transfer_targets, resolved.as_ref());
        let equipment_target = resolved
            .as_ref()
            .and_then(|definition| definition.item.as_ref())
            .and_then(|item| equipment_target_for_bucket(item.bucket_hash));
        let target_occupied = equipment_target
            .is_some_and(|(slot, _)| context.occupied_equipment_slots.contains(&slot));

        ui.push_id(
            ("character-inventory-item", ui_identity),
            |ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = resolved.as_ref().map_or(
        DefinitionSummary::Unknown {
            hash_display_text: &hash_hex_text,
        },
                        |definition| DefinitionSummary::Known {
                            name: &definition.name,
            hash_display_text: &hash_hex_text,
                            type_name: &definition.type_name,
                        },
                    );
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        Some(u64::from(snapshot.definition_hash)),
                        Some(DefinitionInspectionContext {
                            source: format!(
                                "Character {} inventory · item {}",
                                snapshot.location.character_index + 1,
                                snapshot.location.item_index + 1
                            ),
                            instance_id: Some(soid_text.clone()),
                            authored_level: Some(i64::from(snapshot.level)),
                            flags: snapshot.flags,
                            plug_count: Some(match &snapshot.plugs {
                                ItemPlugs::NativeDefaults => resolved
                                    .as_ref()
                                    .and_then(|definition| definition.item.as_ref())
                                    .map_or(0, |item| item.default_plugs.len()),
                                ItemPlugs::Authored(plugs) => plugs.len(),
                            }),
                        }),
                        ItemHeader {
                            label: None,
                            soid: Some(&soid_text),
                            definition,
                            icon: None,
                            fill: item_editor::muted_item_header_fill(ui),
                            valid,
                            invalid_message: "not valid for this character inventory",
                        },
                        |_| {},
                    );
                    ui.add_enabled_ui(editable, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            for action in item_editor::draw_level_and_quantity(
                                ui,
                                ("character-inventory-numeric", ui_identity),
                                NumericItemFields {
                                    level: Some(i64::from(snapshot.level)),
                                    power_max: self
                                        .manifest
                                        .item_power_cap(u64::from(snapshot.definition_hash)),
                                    allow_power_above_cap: self.experimental_power_above_cap,
                                    quantity: None,
                                    quantity_max: None,
                                },
                            ) {
                                if let ItemEditorAction::SetLevel { level } = action
                                    && let Ok(level) = i32::try_from(level)
                                {
                                    requested.push(InventoryItemAction::SetLevel(level));
                                }
                            }
                            ui.add_space(8.0);
                            let flags = snapshot.flags.unwrap_or_default();
                            let locked = flags & INVENTORY_FLAG_LOCKED != 0;
                            let lock_response = if locked {
                                item_editor::draw_lock_button(ui, true, "Unlock stored item")
                                    .on_hover_text("Unlock this item")
                            } else {
                                item_editor::draw_unlock_button(ui, true, "Lock stored item")
                                    .on_hover_text("Lock this item")
                            };
                            if lock_response.clicked() {
                                requested.push(InventoryItemAction::SetFlags(
                                    set_inventory_locked_flag(snapshot.flags, !locked),
                                ));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(4.0);
                                    if item_editor::draw_trash_button(
                                        ui,
                                        true,
                                        "Delete stored item",
                                    )
                                        .on_hover_text("Delete this stored item")
                                        .clicked()
                                    {
                                        remove_requested = true;
                                    }
                                    let response = ui
                                        .add(egui::Button::new("Swap").small())
                                        .on_hover_text("Open item picker");
                                    swap_requested = response.clicked();
                                    swap_response = Some(response);
                                    if let Some((slot, slot_label)) = equipment_target {
                                        let can_equip = editable && valid && snapshot.quantity == 1;
                                        let tooltip = if snapshot.quantity != 1 {
                                            "Only a single inventory item can be equipped at a time"
                                                .to_owned()
                                        } else if !valid {
                                            format!(
                                                "This item is not valid for the {slot_label} slot"
                                            )
                                        } else if target_occupied {
                                            format!(
                                                "Equip in the {slot_label} slot and move its current item here"
                                            )
                                        } else {
                                            format!("Equip in the empty {slot_label} slot")
                                        };
                                        let response = ui.add_enabled(
                                            can_equip,
                                            egui::Button::new("Equip").small(),
                                        );
                                        let response = if can_equip {
                                            response.on_hover_text(tooltip)
                                        } else {
                                            response.on_disabled_hover_text(tooltip)
                                        };
                                        if response.clicked() {
                                            equip_requested = Some(slot);
                                        }
                                    }
                                },
                            );
                        });
                    });

                        let picker_anchor = swap_response.map_or_else(
                            || header_response.clone(),
                            |swap_response| header_response.clone() | swap_response,
                        );
                        let picker_action = ui.add_enabled_ui(editable, |ui| {
                            let manifest = &self.manifest;
                            let show_dummy_items = self.show_dummy_items;
                            let query = self.searches.entry(key.clone()).or_default();
                            item_editor::draw_definition_picker_with_open_request_and_footer(
                                ui,
                                manifest,
                                ("character-inventory-definition", ui_identity),
                                query,
                                picker_height_with_transfer_destinations(
                                    transfer_destinations.len(),
                                ),
                                (Some(&picker_anchor), swap_requested),
                                (
                                    |query| DefinitionPickerChoices {
                                        definitions: without_definition_groups(
                                            character_definition_choices(
                                                manifest
                                                    .character_inventory_candidates(
                                                        query,
                                                        class_type,
                                                        show_dummy_items,
                                                    )
                                                    .filter(|definition| {
                                                        bucket_has_room(
                                                            definition.metadata,
                                                            context.bucket_usage,
                                                            current_bucket,
                                                            replacing_unresolved,
                                                        )
                                                    }),
                                            ),
                                        ),
                                        existing_inventory: Vec::new(),
                                        clear: None,
                                        random_item_builder_hash: equipment_target
                                            .filter(|(slot, _)| {
                                                WEAPON_SLOTS.contains(slot)
                                                    || ARMOR_SLOTS.contains(slot)
                                            })
                                            .map(|_| u64::from(snapshot.definition_hash)),
                                        empty_message:
                                            "No compatible items with space in this bucket".to_owned(),
                                    },
                                    |ui| {
                                        draw_character_transfer_destinations(
                                            ui,
                                            &transfer_destinations,
                                        )
                                    },
                                ),
                            )
                        }).inner;
                        move_requested = picker_action.1;
                        if let Some(ItemEditorAction::OpenInRandomItemBuilder { hash }) =
                            picker_action.0
                        {
                            equipment::request_inventory_item_builder(
                                ui.ctx(),
                                snapshot.location.character_index,
                                hash,
                                &snapshot.plugs,
                            );
                        }
                        if let Some(ItemEditorAction::SetDefinition { hash }) = picker_action.0
                            && let Ok(hash) = u32::try_from(hash)
                        {
                            requested.push(InventoryItemAction::SetDefinitionHash(hash));
                            requested.push(InventoryItemAction::SetPlugs(
                                ItemPlugs::NativeDefaults,
                            ));
                            if let Some(maximum) = self
                                .manifest
                                .inventory_metadata(u64::from(hash))
                                .and_then(|metadata| metadata.max_stack_size)
                                .map(|maximum| maximum.min(i32::MAX as u32) as i32)
                                && snapshot.quantity > maximum
                            {
                                requested.push(InventoryItemAction::SetQuantity(maximum.max(1)));
                            }
                            self.searches.insert(key.clone(), String::new());
                        }
                    if !requested
                        .iter()
                        .any(|action| matches!(action, InventoryItemAction::Remove))
                    {
                        if let Some(item) = resolved
                            .as_ref()
                            .and_then(|definition| definition.item.as_ref())
                        {
                            self.draw_inventory_plugs(
                                ui,
                                snapshot,
                                ui_identity,
                                item,
                                editable,
                                &mut requested,
                            );
                        } else if matches!(snapshot.plugs, ItemPlugs::Authored(ref plugs) if !plugs.is_empty())
                        {
                            ui.label(
                                egui::RichText::new(
                                    "Plugs are preserved but cannot be guided without an installed item definition.",
                                )
                                .weak(),
                            );
                        }
                    }
                    });
            },
        );
        if remove_requested {
            return Some(CharacterInventoryItemRequest::Apply(vec![
                InventoryItemAction::Remove,
            ]));
        }
        if let Some(destination_character_index) = move_requested {
            return Some(CharacterInventoryItemRequest::MoveTo(
                destination_character_index,
            ));
        }
        if let Some(slot) = equip_requested {
            return Some(CharacterInventoryItemRequest::Equip(slot));
        }
        (!requested.is_empty()).then_some(CharacterInventoryItemRequest::Apply(requested))
    }

    fn draw_inventory_plugs(
        &mut self,
        ui: &mut egui::Ui,
        inventory: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        item: &ItemDef,
        editable: bool,
        requested: &mut Vec<InventoryItemAction>,
    ) {
        let (current_plugs, native_defaults) = displayed_inventory_plugs(inventory, item);
        if item.sockets.is_empty() && current_plugs.is_empty() {
            return;
        }
        let socket_count = item
            .sockets
            .len()
            .max(current_plugs.len())
            .min(inventory::MAX_ITEM_PLUGS);
        let title = if native_defaults {
            format!("Plugs ({socket_count}, native defaults)")
        } else {
            format!("Plugs ({socket_count})")
        };
        egui::CollapsingHeader::new(title)
            .id_salt(("character-inventory-plugs", ui_identity))
            .show(ui, |ui| {
                for socket_index in 0..socket_count {
                    let current_hash = current_plugs
                        .get(socket_index)
                        .copied()
                        .flatten()
                        .map(u64::from);
                    let native_default = native_plug_default(&item.default_plugs, socket_index);
                    let query_key = format!(
                        "{}:plug:{socket_index}",
                        inventory_item_state_key(ui_identity)
                    );
                    let mut query = self
                        .plug_searches
                        .get(&query_key)
                        .cloned()
                        .unwrap_or_default();
                    let picker_snapshot = item_editor::plug_picker_snapshot(
                        &self.manifest,
                        item,
                        socket_index,
                        current_hash,
                        current_hash.map_or_else(
                            || "None".to_owned(),
                            |hash| self.manifest.plug_label(hash, self.show_plug_hashes),
                        ),
                        native_default,
                        self.plug_selection_mode,
                    );
                    let searchable = picker_snapshot.choices.len() > 12;
                    let action = ui
                        .add_enabled_ui(editable, |ui| {
                            item_editor::draw_plug_picker(
                                ui,
                                &self.manifest,
                                ("character-inventory-plug", ui_identity, socket_index),
                                &mut query,
                                &picker_snapshot,
                                PickerHeight {
                                    min: PLUG_PICKER_MIN_HEIGHT,
                                    max: PLUG_PICKER_MAX_HEIGHT,
                                },
                            )
                        })
                        .inner;
                    if let Some(ItemEditorAction::SetPlug { socket_index, hash }) = action {
                        let mut plugs = current_plugs.clone();
                        while plugs.len() <= socket_index {
                            plugs.push(None);
                        }
                        plugs[socket_index] = hash.and_then(|hash| u32::try_from(hash).ok());
                        requested.push(InventoryItemAction::SetPlugs(ItemPlugs::Authored(plugs)));
                    }
                    if searchable {
                        self.plug_searches.insert(query_key, query);
                    } else {
                        self.plug_searches.remove(&query_key);
                    }
                }
            });
    }

    fn character_transfer_targets(
        &self,
        source_character_index: usize,
    ) -> Vec<CharacterTransferTarget> {
        (0..self.character_count())
            .filter(|character_index| *character_index != source_character_index)
            .map(|character_index| {
                let class_type = self
                    .account_workspace
                    .character_metadata(&self.document, character_index)
                    .ok()
                    .map(|metadata| u64::from(metadata.class_type))
                    .unwrap_or(99);
                let label = format!(
                    "Character {} · {}",
                    character_index + 1,
                    class_name(class_type)
                );
                if !matches!(class_type, 0..=2) {
                    return CharacterTransferTarget {
                        character_index,
                        label,
                        class_type,
                        stored_count: None,
                        usage: None,
                        unavailable_reason: Some("Invalid character class".to_owned()),
                    };
                }
                match self
                    .account_workspace
                    .character_inventory(&self.document, character_index)
                {
                    Err(_) => CharacterTransferTarget {
                        character_index,
                        label,
                        class_type,
                        stored_count: None,
                        usage: None,
                        unavailable_reason: Some("Inventory could not be read".to_owned()),
                    },
                    Ok(items) => {
                        let items = items.unwrap_or_default();
                        let equipment = self
                            .account_workspace
                            .equipped_item_snapshots(&self.document, character_index);
                        let usage = self.inventory_bucket_usage(&items, equipment.as_deref().ok());
                        CharacterTransferTarget {
                            character_index,
                            label,
                            class_type,
                            stored_count: Some(items.len()),
                            usage: Some(usage),
                            unavailable_reason: None,
                        }
                    }
                }
            })
            .collect()
    }

    fn character_transfer_destinations(
        &self,
        targets: &[CharacterTransferTarget],
        definition: Option<&ResolvedDefinition>,
    ) -> Vec<CharacterTransferDestination> {
        targets
            .iter()
            .map(|target| {
                let character_index = target.character_index;
                let class_type = target.class_type;
                let label = &target.label;
                let mut bucket_detail = None;
                let unavailable_reason = if let Some(reason) = &target.unavailable_reason {
                    Some(reason.clone())
                } else if let Some(definition) = definition {
                    if definition.metadata.scope != InventoryScope::Character {
                        Some("Item bucket could not be verified".to_owned())
                    } else if let Some(item) = &definition.item {
                        if item.class_type != 3 && item.class_type != class_type {
                            Some("Not compatible with this character".to_owned())
                        } else {
                            let Some(usage) = &target.usage else {
                                return CharacterTransferDestination {
                                    character_index,
                                    label: label.clone(),
                                    detail: "Bucket usage unavailable".to_owned(),
                                    enabled: false,
                                    tooltip: "Inventory could not be read".to_owned(),
                                };
                            };
                            bucket_detail =
                                character_bucket_usage_detail(definition.metadata, usage);
                            if target.stored_count.is_some_and(|count| {
                                count
                                    >= self
                                        .account_workspace
                                        .character_inventory_capacity(&self.document)
                            }) {
                                Some("Inventory is full".to_owned())
                            } else if definition.metadata.authored_row_capacity().is_none() {
                                Some("Bucket capacity could not be verified".to_owned())
                            } else if !usage.occupancy_complete {
                                Some("Bucket occupancy could not be verified".to_owned())
                            } else if !bucket_has_room(&definition.metadata, usage, None, false) {
                                Some(format!("{} is full", definition.metadata.bucket_label()))
                            } else {
                                None
                            }
                        }
                    } else {
                        Some("Item definition is incomplete".to_owned())
                    }
                } else {
                    Some("Item definition is unavailable".to_owned())
                };
                let enabled = unavailable_reason.is_none();
                let detail = bucket_detail.unwrap_or_else(|| {
                    unavailable_reason
                        .clone()
                        .unwrap_or_else(|| "Bucket usage unavailable".to_owned())
                });
                CharacterTransferDestination {
                    character_index,
                    label: label.clone(),
                    detail,
                    enabled,
                    tooltip: unavailable_reason
                        .unwrap_or_else(|| format!("Move this item to {label}")),
                }
            })
            .collect()
    }

    fn inventory_bucket_usage(
        &self,
        items: &[InventoryItemSnapshot],
        equipment: Option<&[EquippedItemSnapshot]>,
    ) -> BucketUsage {
        let mut counts = HashMap::new();
        let mut unresolved_count = 0;
        let mut occupancy_complete = equipment.is_some();
        if let Some(equipment) = equipment {
            for equipped in equipment {
                let metadata = equipped
                    .definition_hash
                    .and_then(|hash| self.manifest.inventory_metadata(hash));
                match metadata {
                    Some(metadata) if metadata.scope == InventoryScope::Character => {
                        *counts.entry(metadata.native_bucket_id).or_default() += 1;
                    }
                    Some(metadata) if metadata.scope != InventoryScope::Unknown => {
                        unresolved_count += 1;
                        occupancy_complete = false;
                    }
                    Some(_) | None => unresolved_count += 1,
                }
            }
        }

        for item in items {
            match self
                .manifest
                .inventory_metadata(u64::from(item.definition_hash))
            {
                Some(metadata) if metadata.scope == InventoryScope::Character => {
                    *counts.entry(metadata.native_bucket_id).or_default() += 1;
                }
                Some(metadata) if metadata.scope != InventoryScope::Unknown => {
                    unresolved_count += 1;
                    occupancy_complete = false;
                }
                Some(_) | None => unresolved_count += 1,
            }
        }
        BucketUsage {
            counts,
            unresolved_count,
            occupancy_complete,
        }
    }
}
