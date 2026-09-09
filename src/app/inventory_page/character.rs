//! Character-scoped inventory, equipped-item, and transfer rendering.

use crate::app::account_workspace as account;

mod groups;
mod item_card;

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
        CharacterTransferTarget, InventoryItemUiId, ItemBucket, ResolvedDefinition,
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

struct CharacterInventoryGroupsContext<'a> {
    character_index: usize,
    class_type: u64,
    stored_count: usize,
    editable: bool,
    equipment_editable: bool,
    allow_cross_class_subclasses: bool,
    inventory_available: bool,
    groups: Vec<ItemBucket<CharacterInventoryEntry>>,
    bucket_usage: &'a BucketUsage,
    transfer_targets: &'a [CharacterTransferTarget],
    occupied_equipment_slots: &'a [&'static str],
}

impl SundialApp {
    pub(in crate::app) fn draw_character_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        let editable = account::can_mutate_character_inventory(&self.document);
        let equipment_editable = account::can_mutate_equipment(&self.document);
        ui.horizontal(|ui| {
            ui.heading("Character Inventory");
            crate::ui_help::info(ui, "Items stored separately for each character, with equipped items shown in their native buckets.");
        });
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
        let class_type = account::character_metadata(&self.document, character_index)
            .ok()
            .map(|metadata| u64::from(metadata.class_type))
            .unwrap_or(99);
        let (items, inventory_error) =
            match account::character_inventory(&self.document, character_index) {
                Ok(items) => (items.unwrap_or_default(), None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
        let (equipped_items, equipment_error) =
            match account::equipped_item_snapshots(&self.document, character_index) {
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
        let inventory_capacity = account::character_inventory_capacity(&self.document);
        let randomize_request = ui
            .horizontal_wrapped(|ui| {
                ui.add_enabled_ui(equipment_editable, |ui| {
                    self.draw_plug_safety_choice(ui, true);
                });
                ui.separator();
                let request = equipment::draw_randomize_menu(ui, equipment_editable, editable);
                if equipment::draw_armor_stats_button(ui, equipment_editable).clicked() {
                    self.armor_stats_adjuster.open(character_index);
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{stored_count} / {inventory_capacity} stored · {equipped_count} equipped"
                    ))
                    .weak(),
                );
                request
            })
            .inner;
        equipment::draw_randomize_dialogs(self, ui.ctx(), character_index, randomize_request);
        equipment::draw_armor_stats_window(self, ui.ctx(), character_index);
        self.draw_equipped_armor_stat_row(ui, character_index);
        if self.preferences.show_safety_warnings {
            super::super::draw_plug_selection_warning(ui, self.plug_selection_mode);
        }
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
        let allow_cross_class_subclasses = self.preferences.experimental_cross_class_subclasses;
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
            .copied()
            .filter(|metadata| {
                crate::account_contract::inventory_bucket_available(
                    metadata.native_bucket_id,
                    self.document.supports_v13_account(),
                )
            });
        let mut groups = self.group_items_by_bucket(
            entries,
            CharacterInventoryEntry::definition_hash,
            InventoryScope::Character,
        );
        if !filters_active {
            add_candidate_buckets(&mut groups, candidate_buckets, InventoryScope::Character);
        }
        super::buckets::prepare_character_buckets(
            &mut groups,
            self.document.supports_v13_account(),
        );
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

        let pending = self.draw_character_inventory_groups(
            ui,
            CharacterInventoryGroupsContext {
                character_index,
                class_type,
                stored_count,
                editable,
                equipment_editable,
                allow_cross_class_subclasses,
                inventory_available: inventory_error.is_none(),
                groups,
                bucket_usage: &bucket_usage,
                transfer_targets: &transfer_targets,
                occupied_equipment_slots: &occupied_equipment_slots,
            },
        );
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
                match crate::app::account_validation::apply_with_bucket_limits(
                    &mut self.document,
                    &self.manifest,
                    |candidate| {
                        apply_inventory_actions_atomic(candidate, snapshot.location, actions)
                    },
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
                match crate::app::account_validation::apply_with_bucket_limits(
                    &mut self.document,
                    &self.manifest,
                    |candidate| {
                        account::move_inventory_item_to_character(
                            candidate,
                            snapshot.location,
                            destination_character_index,
                        )
                        .map_err(|error| error.to_string())
                    },
                ) {
                    Ok(_) => {
                        let class_type = account::character_metadata(
                            &self.document,
                            destination_character_index,
                        )
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

    fn character_transfer_targets(
        &self,
        source_character_index: usize,
    ) -> Vec<CharacterTransferTarget> {
        (0..self.character_count())
            .filter(|character_index| *character_index != source_character_index)
            .map(|character_index| {
                let class_type = account::character_metadata(&self.document, character_index)
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
                match account::character_inventory(&self.document, character_index) {
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
                        let equipment =
                            account::equipped_item_snapshots(&self.document, character_index);
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
                        if !equipment::item_class_is_compatible(
                            item,
                            class_type,
                            self.preferences.experimental_cross_class_subclasses,
                        ) {
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
                                count >= account::character_inventory_capacity(&self.document)
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
                let detail = unavailable_reason
                    .clone()
                    .or(bucket_detail)
                    .unwrap_or_else(|| "Bucket usage unavailable".to_owned());
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
