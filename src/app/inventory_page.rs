//! Guided editing for account profile items and per-character inventory.
//!
//! The page intentionally renders snapshots and applies typed actions from `inventory`; it never
//! reaches into the document with ad-hoc JSON pointers. Shared item controls live in
//! `item_editor`, while this module owns inventory-specific policy such as scope and
//! bucket-capacity checks.

use std::{cmp::Reverse, collections::HashMap, sync::Arc};

use eframe::egui;

use crate::{
    catalog::{InventoryDefinition, InventoryMetadata, InventoryScope, ItemDef},
    hash::{format_hash, parse_hash, parse_unsigned_value},
};

use super::{
    ITEM_PICKER_MAX_HEIGHT, ITEM_PICKER_MIN_HEIGHT, PLUG_PICKER_MAX_HEIGHT, PLUG_PICKER_MIN_HEIGHT,
    SLOTS, SundialApp,
    equipment::{
        EquipmentSlotCard, EquippedItemSnapshot, class_name, equipped_item_snapshots,
        inferred_item_level, native_plug_default,
    },
    inventory::{
        self, CHARACTER_INVENTORY_CAPACITY, DismantleGearClass, DismantleRarity,
        DismantleRewardAction, DismantleRewardSnapshot, INVENTORY_FLAG_LOCKED,
        INVENTORY_FLAG_MASTERWORK, InventoryItemAction, InventoryItemLocation,
        InventoryItemSnapshot, ItemPlugs, NewInventoryItem, ProfileItemAction, ProfileItemSnapshot,
        SchemaMode, inventory_masterwork_feature_present, set_inventory_locked_flag,
        set_inventory_masterwork_flag,
    },
    item_editor::{
        self, DefinitionChoice, DefinitionPickerChoices, DefinitionSummary, ItemEditorAction,
        ItemHeader, NumericItemFields, PickerHeight,
    },
    ui::single_line_galley as transfer_menu_galley,
};

const BUCKET_HEADER_SIZE_DELTA: f32 = 1.0;
const TRANSFER_DESTINATION_ROW_HEIGHT: f32 = 28.0;
const TRANSFER_DESTINATION_ROW_SPACING: f32 = 2.0;
const TRANSFER_FOOTER_CHROME_HEIGHT: f32 = 28.0;
const TRANSFER_PICKER_MIN_LIST_HEIGHT: f32 = 176.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum ProfileInventorySection {
    #[default]
    SharedItems,
    DismantleRewards,
}

#[derive(Clone)]
struct ResolvedDefinition {
    name: String,
    type_name: String,
    metadata: InventoryMetadata,
    item: Option<Arc<ItemDef>>,
}

struct BucketUsage {
    counts: HashMap<u8, usize>,
    unresolved_count: usize,
    occupancy_complete: bool,
}

struct CharacterTransferTarget {
    character_index: usize,
    label: String,
    class_type: u64,
    stored_count: Option<usize>,
    usage: Option<BucketUsage>,
    unavailable_reason: Option<String>,
}

struct CharacterInventoryCardContext<'a> {
    bucket_usage: &'a BucketUsage,
    transfer_targets: &'a [CharacterTransferTarget],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BucketKey {
    scope: InventoryScope,
    native_id: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct InventoryItemUiId {
    character_index: usize,
    instance_soid: u64,
    duplicate_ordinal: Option<usize>,
}

struct ItemBucket<T> {
    key: BucketKey,
    label: String,
    capacity: Option<u16>,
    addable: bool,
    items: Vec<T>,
}

#[derive(Clone)]
enum CharacterInventoryEntry {
    Equipped(EquippedItemSnapshot),
    Stored {
        snapshot: InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
    },
}

enum CharacterInventoryItemRequest {
    Apply(Vec<InventoryItemAction>),
    Equip(&'static str),
    MoveTo(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CharacterTransferDestination {
    character_index: usize,
    label: String,
    detail: String,
    enabled: bool,
    tooltip: String,
}

impl CharacterInventoryEntry {
    fn definition_hash(&self) -> Option<u64> {
        match self {
            Self::Equipped(snapshot) => snapshot.definition_hash,
            Self::Stored { snapshot, .. } => Some(u64::from(snapshot.definition_hash)),
        }
    }
}

impl SundialApp {
    pub(super) fn draw_profile_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        let section_id = ui.make_persistent_id("profile-inventory-section");
        let mut section = ui.data_mut(|data| {
            data.get_temp::<ProfileInventorySection>(section_id)
                .unwrap_or_default()
        });
        let dismantle_rewards_available = mode.supports_dismantle_rewards() && !mode.is_future();
        if !dismantle_rewards_available {
            section = ProfileInventorySection::SharedItems;
        }

        ui.heading("Profile inventory");
        ui.label(
            "Items shared by the account and available to every character. Profile inventory is supported by every Sunrise settings schema Sundial can edit.",
        );
        draw_schema_notice(ui, mode, InventoryPageKind::Profile);
        ui.add_space(4.0);

        if dismantle_rewards_available {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::SharedItems,
                    "Shared items",
                );
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::DismantleRewards,
                    "Dismantle rewards",
                );
            });
            ui.add_space(4.0);
        }
        ui.data_mut(|data| data.insert_temp(section_id, section));

        egui::ScrollArea::vertical()
            .id_salt(("profile-inventory-page", section))
            .show(ui, |ui| match section {
                ProfileInventorySection::SharedItems => {
                    self.draw_profile_items_section(ui, mode);
                }
                ProfileInventorySection::DismantleRewards => {
                    self.draw_dismantle_reward_section(ui, mode);
                }
            });
    }

    pub(super) fn draw_character_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        ui.heading("Character inventory");
        ui.label(
            "Items stored separately for each character, with equipped items shown in their native buckets.",
        );
        draw_schema_notice(ui, mode, InventoryPageKind::Character);
        ui.add_space(4.0);
        self.draw_character_inventory_section(ui, mode);
    }

    fn draw_dismantle_reward_section(&mut self, ui: &mut egui::Ui, mode: SchemaMode) {
        if !mode.supports_dismantle_rewards() || mode.is_future() {
            return;
        }
        let rewards = match inventory::dismantle_rewards(&self.document) {
            Ok(rewards) => rewards.unwrap_or_default(),
            Err(error) => {
                ui.strong("Dismantle rewards");
                draw_section_error(ui, &error.to_string());
                return;
            }
        };
        let editable = mode.can_mutate_dismantle_rewards();
        let capacity = mode.dismantle_reward_capacity();
        let account_ready = inventory::profile_item_target_exists(&self.document).unwrap_or(false);
        let has_room = capacity.is_some_and(|capacity| rewards.len() < capacity);
        let picker_key = "dismantle-rewards:add".to_owned();
        let mut picker_anchor = None;
        let mut open_picker = false;

        ui.horizontal_wrapped(|ui| {
            ui.strong("Dismantle rewards");
            let count = capacity.map_or_else(
                || format!("{} policies", rewards.len()),
                |capacity| format!("{} / {capacity}", rewards.len()),
            );
            ui.label(egui::RichText::new(count).weak());
            let can_add = editable && account_ready && has_room;
            let response = ui.add_enabled(can_add, egui::Button::new("+").small());
            let response = if can_add {
                response.on_hover_text("Add a dismantle payout policy")
            } else {
                response.on_disabled_hover_text(if !editable {
                    "Dismantle-policy editing is disabled for this schema"
                } else if !account_ready {
                    "An existing state.account object is required"
                } else {
                    "The dismantle-policy array is full"
                })
            };
            if response.clicked() {
                self.searches.entry(picker_key.clone()).or_default();
                open_picker = true;
            }
            picker_anchor = Some(response);
        });
        ui.label(
            "Materials credited when Sunrise dismantles weapons or armor. Matching policies are added together.",
        );
        if mode.supports_filtered_dismantle_rewards() {
            ui.label(
                egui::RichText::new(
                    "Leave a filter on Any to match every rarity, gear class, or masterwork state.",
                )
                .weak(),
            );
        }
        ui.add_space(4.0);

        if self.searches.contains_key(&picker_key) {
            let action = ui
                .add_enabled_ui(editable && account_ready && has_room, |ui| {
                    let manifest = &self.manifest;
                    let query = self.searches.entry(picker_key.clone()).or_default();
                    item_editor::draw_definition_picker_with_open_request(
                        ui,
                        manifest,
                        "dismantle-reward-add-definition",
                        query,
                        picker_height(),
                        (picker_anchor.as_ref(), open_picker),
                        |query| DefinitionPickerChoices {
                            definitions: without_definition_groups(profile_definition_choices(
                                manifest
                                    .profile_item_candidates(query)
                                    .filter(|definition| u32::try_from(definition.hash).is_ok()),
                            )),
                            existing_inventory: Vec::new(),
                            clear: None,
                            empty_message: "No profile material definitions match".to_owned(),
                        },
                    )
                })
                .inner;
            if let Some(ItemEditorAction::SetDefinition { hash }) = action
                && let Ok(hash) = u32::try_from(hash)
            {
                match inventory::add_dismantle_reward(&mut self.document, hash) {
                    Ok(_) => {
                        self.searches.remove(&picker_key);
                        self.mark_inventory_changed("Added a dismantle reward policy");
                    }
                    Err(error) => self.set_status(error.to_string(), true),
                }
            }
        }

        if rewards.is_empty() {
            ui.label(egui::RichText::new("No dismantle payout policies.").weak());
            return;
        }

        let mut pending = None;
        let (minimum_card_width, maximum_card_width) = self.item_card_width.dimensions();
        item_editor::draw_responsive_item_cards(
            ui,
            &rewards,
            minimum_card_width,
            maximum_card_width,
            |ui, reward| {
                if pending.is_none()
                    && let Some(action) =
                        self.draw_dismantle_reward_card(ui, reward, editable, mode)
                {
                    pending = Some((reward.location, action));
                }
            },
        );
        if let Some((location, action)) = pending {
            let structural = matches!(action, DismantleRewardAction::Remove);
            match inventory::apply_dismantle_reward_action(&mut self.document, location, action) {
                Ok(()) => {
                    self.mark_inventory_changed(if structural {
                        "Removed a dismantle reward policy"
                    } else {
                        "Updated a dismantle reward policy"
                    });
                    if structural {
                        self.searches
                            .retain(|key, _| !key.starts_with("dismantle-rewards:edit:"));
                    }
                }
                Err(error) => self.set_status(error.to_string(), true),
            }
        }
    }

    fn draw_dismantle_reward_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DismantleRewardSnapshot,
        editable: bool,
        mode: SchemaMode,
    ) -> Option<DismantleRewardAction> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let valid = resolved
            .as_ref()
            .is_some_and(|definition| definition.metadata.is_profile_items_candidate());
        let hash_text = format_hash(u64::from(snapshot.definition_hash));
        let key = format!("dismantle-rewards:edit:{}", snapshot.location.index);
        let mut definition_hash = snapshot.definition_hash;
        let mut quantity = snapshot.quantity;
        let mut rarities = snapshot.rarities.clone();
        let mut gear_class = snapshot.gear_class;
        let mut masterworked = snapshot.masterworked;
        let mut changed = false;
        let mut remove_requested = false;
        let mut swap_requested = false;
        let mut swap_response = None;

        ui.push_id(("dismantle-reward", snapshot.location.index), |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = resolved.as_ref().map_or(
                        DefinitionSummary::Unknown { hash: &hash_text },
                        |definition| DefinitionSummary::Known {
                            name: &definition.name,
                            hash: &hash_text,
                            type_name: &definition.type_name,
                        },
                    );
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        Some(u64::from(snapshot.definition_hash)),
                        ItemHeader {
                            label: None,
                            soid: None,
                            definition,
                            icon: None,
                            fill: item_editor::muted_item_header_fill(ui),
                            valid,
                            invalid_message: "not a profile-scoped material definition",
                        },
                        |ui| {
                            ui.add_enabled_ui(editable, |ui| {
                                if item_editor::draw_trash_button(
                                    ui,
                                    true,
                                    "Delete dismantle policy",
                                )
                                .on_hover_text("Delete this payout policy")
                                .clicked()
                                {
                                    remove_requested = true;
                                }
                                let response = ui
                                    .add(egui::Button::new("Swap").small())
                                    .on_hover_text("Choose a different payout material");
                                swap_requested = response.clicked();
                                swap_response = Some(response);
                            });
                        },
                    );

                    ui.add_enabled_ui(editable, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Quantity");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut quantity)
                                        .range(1..=i32::MAX)
                                        .speed(1),
                                )
                                .changed();

                            if mode.supports_filtered_dismantle_rewards() {
                                ui.label("Rarity");
                                egui::ComboBox::from_id_salt("rarity")
                                    .selected_text(dismantle_rarity_summary(&rarities))
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(rarities.is_empty(), "Any").clicked()
                                            && !rarities.is_empty()
                                        {
                                            rarities.clear();
                                            changed = true;
                                        }
                                        ui.separator();
                                        for rarity in DismantleRarity::ALL {
                                            let selected = rarities.contains(&rarity);
                                            if ui
                                                .selectable_label(
                                                    selected,
                                                    dismantle_rarity_label(rarity),
                                                )
                                                .clicked()
                                            {
                                                if selected {
                                                    rarities.retain(|value| *value != rarity);
                                                } else {
                                                    rarities.push(rarity);
                                                    rarities.sort_unstable();
                                                }
                                                changed = true;
                                            }
                                        }
                                    });
                            }
                        });

                        if mode.supports_filtered_dismantle_rewards() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Class");
                                egui::ComboBox::from_id_salt("class")
                                    .selected_text(dismantle_class_label(gear_class))
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(&mut gear_class, None, "Any gear")
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut gear_class,
                                                Some(DismantleGearClass::Weapon),
                                                "Weapon",
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut gear_class,
                                                Some(DismantleGearClass::Armor),
                                                "Armor",
                                            )
                                            .changed();
                                    });

                                ui.label("Masterwork");
                                egui::ComboBox::from_id_salt("masterwork")
                                    .selected_text(dismantle_masterwork_label(masterworked))
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(&mut masterworked, None, "Any state")
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut masterworked,
                                                Some(true),
                                                "Masterworked",
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut masterworked,
                                                Some(false),
                                                "Not masterworked",
                                            )
                                            .changed();
                                    });
                            });
                        }

                        let picker_anchor = header_response.clone()
                            | swap_response
                                .clone()
                                .unwrap_or_else(|| header_response.clone());
                        let manifest = &self.manifest;
                        let query = self.searches.entry(key.clone()).or_default();
                        if let Some(ItemEditorAction::SetDefinition { hash }) =
                            item_editor::draw_definition_picker_with_open_request(
                                ui,
                                manifest,
                                ("dismantle-reward-definition", snapshot.location.index),
                                query,
                                picker_height(),
                                (Some(&picker_anchor), swap_requested),
                                |query| DefinitionPickerChoices {
                                    definitions: without_definition_groups(
                                        profile_definition_choices(
                                            manifest.profile_item_candidates(query).filter(
                                                |definition| u32::try_from(definition.hash).is_ok(),
                                            ),
                                        ),
                                    ),
                                    existing_inventory: Vec::new(),
                                    clear: None,
                                    empty_message: "No profile material definitions match"
                                        .to_owned(),
                                },
                            )
                            && let Ok(hash) = u32::try_from(hash)
                        {
                            definition_hash = hash;
                            changed = true;
                        }
                    });
                });
        });

        if remove_requested {
            Some(DismantleRewardAction::Remove)
        } else if changed {
            Some(DismantleRewardAction::SetPolicy {
                definition_hash,
                quantity,
                rarities,
                gear_class,
                masterworked,
            })
        } else {
            None
        }
    }

    fn draw_profile_items_section(&mut self, ui: &mut egui::Ui, mode: SchemaMode) {
        let snapshots = match inventory::profile_items(&self.document) {
            Ok(items) => items,
            Err(error) => {
                draw_section_error(ui, &error.to_string());
                return;
            }
        };
        let items = snapshots.unwrap_or_default();
        let profile_item_count = items.len();
        let editable = mode.can_mutate_profile_items();
        let capacity = mode.profile_item_capacity();

        ui.horizontal_wrapped(|ui| {
            ui.strong("Shared items");
            let count = capacity.map_or_else(
                || format!("{} items", items.len()),
                |capacity| format!("{} / {capacity}", items.len()),
            );
            ui.label(egui::RichText::new(count).weak());
        });
        ui.label("Stackable profile-scoped definitions only.");

        let bucket_usage = self.profile_bucket_usage(&items);
        let account_ready = inventory::profile_item_target_exists(&self.document).unwrap_or(false);
        if !editable {
            ui.label(
                egui::RichText::new("Profile-item editing is disabled for this schema.").weak(),
            );
        } else if capacity.is_some_and(|capacity| items.len() >= capacity) {
            ui.label(egui::RichText::new("The profile-item array is full for this schema.").weak());
        } else if !account_ready {
            ui.label(
                egui::RichText::new(
                    "Add controls require an existing state.account object; existing rows remain visible.",
                )
                .weak(),
            );
        } else if bucket_usage.unresolved_count > 0 {
            draw_unresolved_bucket_warning(ui);
        }
        ui.add_space(4.0);

        let candidate_buckets = distinct_candidate_buckets(
            self.manifest
                .profile_item_candidates("")
                .map(|definition| *definition.metadata),
        );
        let mut groups = self.group_items_by_bucket(
            items,
            |item| Some(u64::from(item.definition_hash)),
            InventoryScope::Profile,
        );
        add_candidate_buckets(&mut groups, candidate_buckets, InventoryScope::Profile);
        if groups.is_empty() {
            ui.label(egui::RichText::new("No profile inventory buckets are available.").weak());
            return;
        }

        let mut pending = None;
        for group in groups {
            let title = bucket_header_label(&group, &bucket_usage, InventoryScope::Profile);
            let picker_key = format!(
                "profile-items:add:{}:{}",
                scope_id(group.key.scope),
                group.key.native_id
            );
            let array_has_room = capacity.is_some_and(|capacity| profile_item_count < capacity);
            let bucket_has_room =
                group.addable && bucket_key_has_room(group.key, group.capacity, &bucket_usage);
            let can_add = editable
                && account_ready
                && array_has_room
                && bucket_usage.occupancy_complete
                && bucket_has_room;
            let repaint_context = ui.ctx().clone();
            let mut toggle_header = false;
            let mut open_picker = false;
            let mut picker_anchor = None;
            let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id((
                    "profile-items-bucket",
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
                    let response = ui.add_enabled(can_add, egui::Button::new("+").small());
                    let tooltip = bucket_add_tooltip(
                        can_add,
                        editable,
                        account_ready,
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
                self.open_bucket_picker(&picker_key, "profile-items:add:");
                repaint_context.request_repaint();
            }
            header.body(|ui| {
                draw_bucket_details(ui, &group, &bucket_usage, InventoryScope::Profile);
                if self.searches.contains_key(&picker_key) {
                    let action = ui
                        .add_enabled_ui(can_add, |ui| {
                            let manifest = &self.manifest;
                            let request_open = take_bucket_picker_open_request(
                                &mut self.searches,
                                &picker_key,
                                ui.input(|input| input.pointer.any_click()),
                            );
                            let query = self.searches.entry(picker_key.clone()).or_default();
                            item_editor::draw_definition_picker_with_open_request(
                                ui,
                                manifest,
                                (
                                    "profile-items-add-definition",
                                    scope_id(group.key.scope),
                                    group.key.native_id,
                                ),
                                query,
                                picker_height(),
                                (picker_anchor.as_ref(), request_open),
                                |query| DefinitionPickerChoices {
                                    definitions: profile_bucket_definition_choices(
                                        manifest.profile_item_candidates(query).filter(
                                            |definition| {
                                                definition.metadata.scope == group.key.scope
                                                    && definition.metadata.native_bucket_id
                                                        == group.key.native_id
                                                    && u32::try_from(definition.hash).is_ok()
                                            },
                                        ),
                                    ),
                                    existing_inventory: Vec::new(),
                                    clear: None,
                                    empty_message: "No safe definitions in this bucket match"
                                        .to_owned(),
                                },
                            )
                        })
                        .inner;
                    if let Some(ItemEditorAction::SetDefinition { hash }) = action {
                        match u32::try_from(hash)
                            .map_err(|_| {
                                "The selected profile-item hash does not fit in 32 bits".to_owned()
                            })
                            .and_then(|hash| {
                                inventory::add_profile_item(&mut self.document, hash, 1)
                                    .map_err(|error| error.to_string())
                            }) {
                            Ok(_) => {
                                self.searches.remove(&picker_key);
                                self.mark_inventory_changed("Added a shared profile item");
                            }
                            Err(error) => self.set_status(error, true),
                        }
                    }
                }
                let (minimum_card_width, maximum_card_width) = self.item_card_width.dimensions();
                item_editor::draw_responsive_item_cards(
                    ui,
                    &group.items,
                    minimum_card_width,
                    maximum_card_width,
                    |ui, snapshot| {
                        let action =
                            self.draw_profile_item_card(ui, snapshot, editable, &bucket_usage);
                        if pending.is_none()
                            && let Some(action) = action
                        {
                            pending = Some((snapshot.location, action));
                        }
                    },
                );
            });
        }
        if let Some((location, action)) = pending {
            let structural = matches!(action, ProfileItemAction::Remove);
            match inventory::apply_profile_item_action(&mut self.document, location, action) {
                Ok(()) => {
                    self.mark_inventory_changed(if structural {
                        "Removed a shared profile item"
                    } else {
                        "Updated a shared profile item"
                    });
                    if structural {
                        self.searches.retain(|key, _| {
                            key.starts_with("profile-items:add:")
                                || !key.starts_with("profile-items:")
                        });
                    }
                }
                Err(error) => self.set_status(error.to_string(), true),
            }
        }
    }

    fn draw_profile_item_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &ProfileItemSnapshot,
        editable: bool,
        bucket_usage: &BucketUsage,
    ) -> Option<ProfileItemAction> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let metadata = self
            .manifest
            .inventory_metadata(u64::from(snapshot.definition_hash))
            .copied();
        let hash_text = format_hash(u64::from(snapshot.definition_hash));
        let valid = resolved
            .as_ref()
            .is_some_and(|definition| definition.metadata.is_profile_items_candidate());
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
        let mut remove_requested = false;
        let mut swap_requested = false;
        let mut swap_response = None;

        ui.push_id(("profile-item", snapshot.location.index), |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = resolved.as_ref().map_or(
                        DefinitionSummary::Unknown { hash: &hash_text },
                        |definition| DefinitionSummary::Known {
                            name: &definition.name,
                            hash: &hash_text,
                            type_name: &definition.type_name,
                        },
                    );
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        Some(u64::from(snapshot.definition_hash)),
                        ItemHeader {
                            label: None,
                            soid: None,
                            definition,
                            icon: None,
                            fill: item_editor::muted_item_header_fill(ui),
                            valid,
                            invalid_message: "not a profile-scoped stackable definition",
                        },
                        |_| {},
                    );
                    ui.add_enabled_ui(editable, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            for action in item_editor::draw_level_and_quantity(
                                ui,
                                ("profile-item-numeric", snapshot.location.index),
                                NumericItemFields {
                                    level: None,
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

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(4.0);
                                    if item_editor::draw_trash_button(
                                        ui,
                                        true,
                                        "Delete shared item",
                                    )
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
                                },
                            );
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
                                    definitions: without_definition_groups(
                                        profile_definition_choices(
                                            manifest.profile_item_candidates(query).filter(
                                                |definition| {
                                                    u32::try_from(definition.hash).is_ok()
                                                        && profile_swap_candidate(
                                                            definition.metadata,
                                                            current_bucket,
                                                            snapshot.quantity,
                                                            bucket_usage,
                                                            replacing_unresolved,
                                                        )
                                                },
                                            ),
                                        ),
                                    ),
                                    existing_inventory: Vec::new(),
                                    clear: None,
                                    empty_message: "No safe profile-item definitions match"
                                        .to_owned(),
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
    }

    fn draw_character_inventory_section(&mut self, ui: &mut egui::Ui, mode: SchemaMode) {
        self.draw_character_tabs(ui);
        ui.separator();

        let character_index = self.selected_character;
        let class_type = self
            .characters()
            .and_then(|characters| characters.get(character_index))
            .and_then(|character| character.get("class"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(99);
        let (items, inventory_error) =
            match inventory::character_inventory(&self.document, character_index) {
                Ok(items) => (items.unwrap_or_default(), None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
        let (equipped_items, equipment_error) =
            match equipped_item_snapshots(&self.document, character_index) {
                Ok(items) => (items, None),
                Err(error) => (Vec::new(), Some(error)),
            };
        let stored_count = items.len();
        let equipped_count = equipped_items.len();
        let editable = mode.can_mutate_character_inventory();
        let equipment_editable = mode.can_mutate_equipment();

        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("Character {}", character_index + 1));
            ui.label(egui::RichText::new(format!(
                "{stored_count} / {CHARACTER_INVENTORY_CAPACITY} stored · {equipped_count} equipped"
            )).weak());
        });
        ui.add_enabled_ui(equipment_editable, |ui| self.draw_item_safety_controls(ui));
        ui.separator();

        if !editable {
            let message = if equipment_editable {
                "Stored character-inventory editing requires Sunrise settings schema 6; equipped loadout items remain editable."
            } else {
                "Stored character-inventory editing requires Sunrise settings schema 6; equipped loadout editing is also disabled for this schema."
            };
            ui.label(egui::RichText::new(message).weak());
        } else if stored_count >= CHARACTER_INVENTORY_CAPACITY {
            ui.label(egui::RichText::new("This character inventory is full.").weak());
        }
        if let Some(error) = &inventory_error {
            draw_inventory_source_error(ui, "Stored inventory", error);
        }
        if let Some(error) = &equipment_error {
            draw_inventory_source_error(ui, "Equipped items", error);
        }

        let mut bucket_usage = self.inventory_bucket_usage(&items, character_index);
        if inventory_error.is_some() || equipment_error.is_some() {
            bucket_usage.occupancy_complete = false;
        }
        if bucket_usage.unresolved_count > 0 {
            draw_unresolved_bucket_warning(ui);
        }
        ui.add_space(4.0);

        let ui_identities = inventory_item_ui_identities(&items);
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
        let candidate_buckets = distinct_candidate_buckets(
            self.manifest
                .character_inventory_candidates("", class_type, self.show_dummy_items)
                .map(|definition| *definition.metadata),
        );
        let mut groups = self.group_items_by_bucket(
            entries,
            CharacterInventoryEntry::definition_hash,
            InventoryScope::Character,
        );
        add_candidate_buckets(&mut groups, candidate_buckets, InventoryScope::Character);
        if groups.is_empty() {
            ui.label(egui::RichText::new("No character inventory buckets are available.").weak());
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
                                            empty_message: "No compatible items in this bucket"
                                                .to_owned(),
                                        },
                                    )
                                })
                                .inner;
                            if let Some(ItemEditorAction::SetDefinition { hash }) = action {
                                let level = default_inventory_item_level(
                                    &self.document,
                                    character_index,
                                    group.key.native_id,
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
                                                inventory::add_inventory_item(
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
                        item_editor::draw_responsive_item_cards(
                            ui,
                            &group.items,
                            minimum_card_width,
                            maximum_card_width,
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
            match request {
                CharacterInventoryItemRequest::Apply(actions) => {
                    let structural = actions
                        .iter()
                        .any(|action| matches!(action, InventoryItemAction::Remove));
                    let definition_changed = actions
                        .iter()
                        .any(|action| matches!(action, InventoryItemAction::SetDefinitionHash(_)));
                    match apply_inventory_actions_atomic(
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
                        Err(error) => self.set_status(error.to_string(), true),
                    }
                }
                CharacterInventoryItemRequest::Equip(slot) => {
                    if self.equip_stored_item(snapshot.location, slot) {
                        self.clear_inventory_item_picker_state(ui_identity, true);
                    }
                }
                CharacterInventoryItemRequest::MoveTo(destination_character_index) => {
                    match inventory::move_inventory_item_to_character(
                        &mut self.document,
                        snapshot.location,
                        destination_character_index,
                    ) {
                        Ok(_) => {
                            let class_type = self
                                .characters()
                                .and_then(|characters| characters.get(destination_character_index))
                                .and_then(|character| character.get("class"))
                                .and_then(serde_json::Value::as_u64)
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
        let hash_text = format_hash(u64::from(snapshot.definition_hash));
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
        let masterwork_feature_present = inventory_masterwork_feature_present(snapshot.flags);
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
        let target_occupied = equipment_target.is_some_and(|(slot, _)| {
            self.characters()
                .and_then(|characters| characters.get(snapshot.location.character_index))
                .and_then(|character| character.get("equipment"))
                .and_then(serde_json::Value::as_object)
                .and_then(|equipment| equipment.get(slot))
                .is_some_and(|item| !item.is_null())
        });

        ui.push_id(
            ("character-inventory-item", ui_identity),
            |ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = resolved.as_ref().map_or(
                        DefinitionSummary::Unknown { hash: &hash_text },
                        |definition| DefinitionSummary::Known {
                            name: &definition.name,
                            hash: &hash_text,
                            type_name: &definition.type_name,
                        },
                    );
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        Some(u64::from(snapshot.definition_hash)),
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
                            if masterwork_feature_present {
                                ui.add_space(8.0);
                                let mut masterworked = flags & INVENTORY_FLAG_MASTERWORK != 0;
                                if ui.checkbox(&mut masterworked, "Masterwork").changed() {
                                    requested.push(InventoryItemAction::SetFlags(
                                        set_inventory_masterwork_flag(
                                            snapshot.flags,
                                            masterworked,
                                        ),
                                    ));
                                }
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

                        let picker_anchor = header_response.clone()
                            | swap_response
                                .expect("a character inventory item card always draws Swap");
                        let picker_action = {
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
                        };
                        move_requested = picker_action.1;
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
                    });
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
                        socket_index,
                        item.sockets.get(socket_index),
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
                        if inventory.flags.is_some()
                            && let Some(catalyst) = self
                                .manifest
                                .catalyst_socket(item)
                                .filter(|catalyst| catalyst.socket_index == socket_index)
                            && let Some(state) = catalyst.state_for_selected_plug(hash)
                        {
                            let (_, masterworked) = catalyst.authored_state(state);
                            requested.push(InventoryItemAction::SetFlags(
                                set_inventory_masterwork_flag(inventory.flags, masterworked),
                            ));
                        }
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
        let Some(characters) = self.characters() else {
            return Vec::new();
        };

        characters
            .iter()
            .enumerate()
            .filter(|(character_index, _)| *character_index != source_character_index)
            .map(|(character_index, character)| {
                let class_type = character
                    .get("class")
                    .and_then(serde_json::Value::as_u64)
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
                match inventory::character_inventory(&self.document, character_index) {
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
                        CharacterTransferTarget {
                            character_index,
                            label,
                            class_type,
                            stored_count: Some(items.len()),
                            usage: Some(self.inventory_bucket_usage(&items, character_index)),
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
                            if target
                                .stored_count
                                .is_some_and(|count| count >= CHARACTER_INVENTORY_CAPACITY)
                            {
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
        character_index: usize,
    ) -> BucketUsage {
        let mut counts = HashMap::new();
        let mut unresolved_count = 0;
        let character = self
            .characters()
            .and_then(|characters| characters.get(character_index));
        let mut occupancy_complete = character.is_some();
        let equipment_value = character.and_then(|character| character.get("equipment"));
        if let Some(equipment) = equipment_value.and_then(serde_json::Value::as_object) {
            if equipment.keys().any(|slot| {
                !super::SLOTS
                    .iter()
                    .any(|(known_slot, _, _)| *known_slot == slot)
            }) {
                occupancy_complete = false;
            }
            for equipped in equipment.values().filter(|value| !value.is_null()) {
                let metadata = equipped
                    .get("definition_hash")
                    .and_then(parse_unsigned_value)
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
        } else if equipment_value.is_some() {
            occupancy_complete = false;
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

    fn profile_bucket_usage(&self, items: &[ProfileItemSnapshot]) -> BucketUsage {
        let mut counts = HashMap::new();
        let mut unresolved_count = 0;
        let mut occupancy_complete = true;
        for item in items {
            match self
                .manifest
                .inventory_metadata(u64::from(item.definition_hash))
            {
                Some(metadata) if metadata.scope == InventoryScope::Profile => {
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

    fn resolve_inventory_definition(&self, hash: u32) -> Option<ResolvedDefinition> {
        self.manifest
            .inventory_definition(u64::from(hash))
            .map(|definition| ResolvedDefinition {
                name: definition.name.to_owned(),
                type_name: definition.type_name.to_owned(),
                metadata: *definition.metadata,
                item: self.manifest.item_handle(u64::from(hash)),
            })
    }

    fn group_items_by_bucket<T>(
        &self,
        items: Vec<T>,
        definition_hash: impl Fn(&T) -> Option<u64>,
        expected_scope: InventoryScope,
    ) -> Vec<ItemBucket<T>> {
        let mut groups = Vec::<ItemBucket<T>>::new();
        for item in items {
            let metadata = self
                .manifest
                .inventory_metadata(definition_hash(&item).unwrap_or_default())
                .copied()
                .unwrap_or_default();
            let key = BucketKey {
                scope: metadata.scope,
                native_id: metadata.native_bucket_id,
            };
            if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
                group.items.push(item);
            } else {
                groups.push(ItemBucket {
                    key,
                    label: metadata.bucket_label(),
                    capacity: metadata.authored_row_capacity(),
                    addable: false,
                    items: vec![item],
                });
            }
        }
        groups.sort_by_cached_key(|group| {
            (
                page_bucket_display_rank(expected_scope, group.key),
                group.label.to_lowercase(),
                group.key.native_id,
            )
        });
        groups
    }

    fn open_bucket_picker(&mut self, key: &str, prefix: &str) {
        self.searches
            .retain(|stored, _| !stored.starts_with(prefix));
        self.searches.insert(key.to_owned(), String::new());
        self.searches
            .insert(bucket_picker_open_request_key(key), String::new());
    }

    fn clear_inventory_item_picker_state(&mut self, ui_identity: InventoryItemUiId, removed: bool) {
        let key = inventory_item_state_key(ui_identity);
        if removed {
            self.searches.remove(&key);
        }
        let plug_prefix = format!("{key}:plug:");
        self.plug_searches
            .retain(|stored, _| !stored.starts_with(&plug_prefix));
    }

    fn mark_inventory_changed(&mut self, status: &str) {
        self.dirty = true;
        self.set_status(format!("{status}; click Save to write it"), false);
    }
}

fn distinct_candidate_buckets(
    metadata: impl IntoIterator<Item = InventoryMetadata>,
) -> Vec<InventoryMetadata> {
    let mut buckets = Vec::new();
    for metadata in metadata {
        if !buckets.iter().any(|stored: &InventoryMetadata| {
            stored.scope == metadata.scope && stored.native_bucket_id == metadata.native_bucket_id
        }) {
            buckets.push(metadata);
        }
    }
    buckets
}

fn bucket_picker_open_request_key(picker_key: &str) -> String {
    format!("{picker_key}:request-open")
}

fn take_bucket_picker_open_request(
    searches: &mut HashMap<String, String>,
    picker_key: &str,
    pointer_clicked: bool,
) -> bool {
    if pointer_clicked {
        return false;
    }
    searches
        .remove(&bucket_picker_open_request_key(picker_key))
        .is_some()
}

fn add_candidate_buckets<T>(
    groups: &mut Vec<ItemBucket<T>>,
    candidates: impl IntoIterator<Item = InventoryMetadata>,
    expected_scope: InventoryScope,
) {
    for metadata in candidates {
        let key = BucketKey {
            scope: metadata.scope,
            native_id: metadata.native_bucket_id,
        };
        if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
            group.addable = true;
            group.capacity = metadata.authored_row_capacity();
            group.label = metadata.bucket_label();
        } else {
            groups.push(ItemBucket {
                key,
                label: metadata.bucket_label(),
                capacity: metadata.authored_row_capacity(),
                addable: true,
                items: Vec::new(),
            });
        }
    }
    groups.sort_by_cached_key(|group| {
        (
            page_bucket_display_rank(expected_scope, group.key),
            group.label.to_lowercase(),
            group.key.native_id,
        )
    });
}

fn bucket_header_label<T>(
    group: &ItemBucket<T>,
    usage: &BucketUsage,
    expected_scope: InventoryScope,
) -> String {
    if group.key.scope == expected_scope
        && let Some(capacity) = group.capacity
    {
        let occupied = usage
            .counts
            .get(&group.key.native_id)
            .copied()
            .unwrap_or_default();
        return format!("{} · {occupied} / {capacity}", group.label);
    }
    format!("{} · {}", group.label, item_count_label(group.items.len()))
}

fn bucket_header_text(ui: &egui::Ui, text: &str) -> egui::RichText {
    let size = egui::TextStyle::Body.resolve(ui.style()).size + BUCKET_HEADER_SIZE_DELTA;
    egui::RichText::new(text).strong().size(size)
}

fn bucket_key_has_room(key: BucketKey, capacity: Option<u16>, usage: &BucketUsage) -> bool {
    capacity.is_some_and(|capacity| {
        let occupied = usage
            .counts
            .get(&key.native_id)
            .copied()
            .unwrap_or_default();
        occupied.saturating_add(usage.unresolved_count) < usize::from(capacity)
    })
}

fn bucket_add_tooltip(
    can_add: bool,
    editable: bool,
    target_ready: bool,
    array_has_room: bool,
    occupancy_complete: bool,
    bucket_has_room: bool,
    bucket_label: &str,
) -> String {
    if can_add {
        return format!("Add an item to {bucket_label}");
    }
    if !editable {
        "Adding items is disabled for this schema".to_owned()
    } else if !target_ready {
        "The inventory target is missing or invalid".to_owned()
    } else if !array_has_room {
        "The inventory array is full".to_owned()
    } else if !occupancy_complete {
        "Bucket occupancy cannot be established until malformed or unsupported rows are repaired"
            .to_owned()
    } else if !bucket_has_room {
        format!("{bucket_label} is at capacity")
    } else {
        "This bucket cannot accept another item".to_owned()
    }
}

fn default_inventory_item_level(
    document: &serde_json::Value,
    character_index: usize,
    native_bucket_id: u8,
) -> i64 {
    if native_bucket_id <= 7 {
        inferred_item_level(document, character_index)
    } else {
        0
    }
}

fn equipment_target_for_bucket(bucket_hash: u64) -> Option<(&'static str, &'static str)> {
    SLOTS
        .iter()
        .find_map(|(slot, label, bucket)| (*bucket == bucket_hash).then_some((*slot, *label)))
}

fn equipped_header_fill(ui: &egui::Ui) -> egui::Color32 {
    let base = ui.visuals().panel_fill;
    let accent = egui::Color32::from_rgb(255, 210, 72);
    blend_color(base, accent, 0.16)
}

fn blend_color(base: egui::Color32, accent: egui::Color32, amount: f32) -> egui::Color32 {
    let mix = |base: u8, accent: u8| {
        (f32::from(base) + (f32::from(accent) - f32::from(base)) * amount).round() as u8
    };
    egui::Color32::from_rgba_premultiplied(
        mix(base.r(), accent.r()),
        mix(base.g(), accent.g()),
        mix(base.b(), accent.b()),
        base.a(),
    )
}

fn picker_height() -> PickerHeight {
    PickerHeight {
        min: ITEM_PICKER_MIN_HEIGHT,
        max: ITEM_PICKER_MAX_HEIGHT,
    }
}

fn picker_height_with_transfer_destinations(destination_count: usize) -> PickerHeight {
    if destination_count == 0 {
        return picker_height();
    }
    let destination_count = u16::try_from(destination_count).unwrap_or(u16::MAX);
    let footer_height = TRANSFER_FOOTER_CHROME_HEIGHT
        + TRANSFER_DESTINATION_ROW_HEIGHT * f32::from(destination_count)
        + TRANSFER_DESTINATION_ROW_SPACING * f32::from(destination_count.saturating_sub(1));
    let min = (ITEM_PICKER_MIN_HEIGHT - footer_height).max(TRANSFER_PICKER_MIN_LIST_HEIGHT);
    PickerHeight {
        min,
        max: (ITEM_PICKER_MAX_HEIGHT - footer_height).max(min),
    }
}

fn dismantle_rarity_label(rarity: DismantleRarity) -> &'static str {
    match rarity {
        DismantleRarity::Common => "Common",
        DismantleRarity::Uncommon => "Uncommon",
        DismantleRarity::Rare => "Rare",
        DismantleRarity::Legendary => "Legendary",
        DismantleRarity::Exotic => "Exotic",
    }
}

fn dismantle_rarity_summary(rarities: &[DismantleRarity]) -> String {
    match rarities {
        [] => "Any rarity".to_owned(),
        [rarity] => dismantle_rarity_label(*rarity).to_owned(),
        rarities => format!("{} rarities", rarities.len()),
    }
}

fn dismantle_class_label(gear_class: Option<DismantleGearClass>) -> &'static str {
    match gear_class {
        None => "Any gear",
        Some(DismantleGearClass::Weapon) => "Weapon",
        Some(DismantleGearClass::Armor) => "Armor",
    }
}

fn dismantle_masterwork_label(masterworked: Option<bool>) -> &'static str {
    match masterworked {
        None => "Any state",
        Some(true) => "Masterworked",
        Some(false) => "Not masterworked",
    }
}

#[derive(Clone, Copy)]
enum InventoryPageKind {
    Profile,
    Character,
}

fn draw_schema_notice(ui: &mut egui::Ui, mode: SchemaMode, page: InventoryPageKind) {
    match mode {
        SchemaMode::MissingOrInvalid => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The settings schema is missing or invalid. Existing items are shown read-only.",
            );
        }
        SchemaMode::Unsupported(version) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Schema {version} predates the schemas supported by this Sundial release. Existing items are shown read-only."
                ),
            );
        }
        SchemaMode::PreInventory(version) if matches!(page, InventoryPageKind::Character) => {
            ui.label(
                egui::RichText::new(format!(
                    "Schema {version} supports profile inventory and equipped loadouts, but stored character inventory requires schema 6. Stored rows are never created or rewritten here."
                ))
                .weak(),
            );
        }
        SchemaMode::PreInventory(_) | SchemaMode::Inventory(_) => {}
        SchemaMode::Future(version) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Schema {version} is newer than this Sundial release. Known item fields remain editable; unrecognized fields are preserved."
                ),
            );
        }
    }

    let editable = !mode.is_read_only()
        && match page {
            InventoryPageKind::Profile => mode.can_mutate_profile_items(),
            InventoryPageKind::Character => {
                mode.can_mutate_character_inventory() || mode.can_mutate_equipment()
            }
        };
    if !editable {
        ui.label(
            egui::RichText::new(
                "Guided controls are disabled; All settings (JSON) remains available for inspection.",
            )
            .weak(),
        );
    }
}

fn profile_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    grouped_definition_choices(
        definitions,
        |key| profile_bucket_rank(key.native_id),
        |definitions| {
            definitions.sort_by_cached_key(|definition| {
                (
                    profile_name_priority(*definition),
                    Reverse(definition.metadata.max_stack_size),
                    definition.name.to_lowercase(),
                    definition.hash,
                )
            });
        },
    )
}

fn profile_bucket_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    let mut definitions = definitions.into_iter().collect::<Vec<_>>();
    definitions.sort_by_cached_key(|definition| {
        (
            profile_name_priority(*definition),
            Reverse(definition.metadata.max_stack_size),
            definition.name.to_lowercase(),
            definition.hash,
        )
    });
    definitions
        .into_iter()
        .map(definition_choice_without_group)
        .collect()
}

fn character_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    grouped_definition_choices(
        definitions,
        |key| character_bucket_rank(key.native_id),
        |definitions| {
            definitions
                .sort_by_cached_key(|definition| (definition.name.to_lowercase(), definition.hash));
        },
    )
}

fn without_definition_groups(mut choices: Vec<DefinitionChoice>) -> Vec<DefinitionChoice> {
    for choice in &mut choices {
        choice.group = None;
    }
    choices
}

fn character_bucket_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    let mut definitions = definitions.into_iter().collect::<Vec<_>>();
    definitions.sort_by_cached_key(|definition| (definition.name.to_lowercase(), definition.hash));
    definitions
        .into_iter()
        .map(definition_choice_without_group)
        .collect()
}

fn grouped_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
    bucket_rank: impl Fn(BucketKey) -> u16,
    mut sort_definitions: impl FnMut(&mut Vec<InventoryDefinition<'a>>),
) -> Vec<DefinitionChoice> {
    let mut buckets = Vec::<(BucketKey, String, Vec<InventoryDefinition<'a>>)>::new();
    for definition in definitions {
        let key = BucketKey {
            scope: definition.metadata.scope,
            native_id: definition.metadata.native_bucket_id,
        };
        if let Some((_, _, items)) = buckets.iter_mut().find(|(stored, _, _)| *stored == key) {
            items.push(definition);
        } else {
            buckets.push((key, definition.metadata.bucket_label(), vec![definition]));
        }
    }

    for (_, _, definitions) in &mut buckets {
        sort_definitions(definitions);
    }
    buckets.sort_by_cached_key(|(key, label, _)| {
        (bucket_rank(*key), label.to_lowercase(), key.native_id)
    });

    buckets
        .into_iter()
        .flat_map(|(_, group, definitions)| {
            definitions
                .into_iter()
                .map(move |definition| definition_choice(definition, group.clone()))
        })
        .collect()
}

fn definition_choice_without_group(definition: InventoryDefinition<'_>) -> DefinitionChoice {
    DefinitionChoice {
        hash: definition.hash,
        name: definition.name.to_owned(),
        type_name: definition.type_name.to_owned(),
        group: None,
    }
}

fn definition_choice(definition: InventoryDefinition<'_>, group: String) -> DefinitionChoice {
    DefinitionChoice {
        hash: definition.hash,
        name: definition.name.to_owned(),
        type_name: definition.type_name.to_owned(),
        group: Some(group),
    }
}

const fn profile_bucket_rank(bucket: u8) -> u16 {
    match bucket {
        21 => 0, // Glimmer
        22 => 1, // Legendary Shards
        24 => 2, // Bright Dust
        15 => 3, // Consumables and materials
        23 => 4, // Silver
        14 => 5, // Shaders
        13 => 6, // Modifications
        42 => 7, // General profile items
        _ => 8,
    }
}

const fn character_bucket_rank(bucket: u8) -> u16 {
    match bucket {
        0 => 0,   // Kinetic weapons
        1 => 1,   // Energy weapons
        2 => 2,   // Power weapons
        3 => 3,   // Helmets
        4 => 4,   // Gauntlets
        5 => 5,   // Chest armor
        6 => 6,   // Leg armor
        7 => 7,   // Class items
        8 => 8,   // Ghost shells
        9 => 9,   // Vehicles
        10 => 10, // Ships
        16 => 11, // Subclasses
        17 => 12, // Clan banners
        27 => 13, // Emblems
        41 => 14, // Emotes
        47 => 15, // Finishers
        49 => 16, // Seasonal artifacts
        _ => 100 + bucket as u16,
    }
}

fn page_bucket_display_rank(expected_scope: InventoryScope, key: BucketKey) -> u16 {
    if key.scope == expected_scope {
        return match expected_scope {
            InventoryScope::Character => character_bucket_rank(key.native_id),
            InventoryScope::Profile => profile_bucket_rank(key.native_id),
            InventoryScope::SmallProfile => key.native_id as u16,
            InventoryScope::Unknown => u16::MAX,
        };
    }
    match key.scope {
        InventoryScope::Unknown => u16::MAX,
        _ => 10_000 + u16::from(scope_id(key.scope)) * 256 + key.native_id as u16,
    }
}

fn profile_name_priority(definition: InventoryDefinition<'_>) -> u8 {
    let label = format!("{} {}", definition.name, definition.type_name).to_lowercase();
    if [
        "currency",
        "material",
        "consumable",
        "token",
        "shader",
        "ornament",
        "mod",
    ]
    .iter()
    .any(|term| label.contains(term))
    {
        0
    } else {
        1
    }
}

fn item_count_label(count: usize) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

const fn scope_id(scope: InventoryScope) -> u8 {
    match scope {
        InventoryScope::Unknown => 0,
        InventoryScope::Character => 1,
        InventoryScope::Profile => 2,
        InventoryScope::SmallProfile => 3,
    }
}

fn draw_bucket_details<T>(
    ui: &mut egui::Ui,
    group: &ItemBucket<T>,
    _usage: &BucketUsage,
    expected_scope: InventoryScope,
) {
    if group.key.scope == InventoryScope::Unknown {
        ui.label(
            egui::RichText::new(
                "No installed bucket metadata is available; these rows remain in their original order.",
            )
            .weak(),
        );
        return;
    }
    if group.key.scope != expected_scope {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{} scope · native bucket {} · wrong scope for this page, so this row is not counted toward valid bucket occupancy",
                group.key.scope.label(),
                group.key.native_id
            ),
        );
    }
}

fn draw_character_transfer_destinations(
    ui: &mut egui::Ui,
    destinations: &[CharacterTransferDestination],
) -> Option<usize> {
    if destinations.is_empty() {
        return None;
    }

    let mut selected = None;
    ui.separator();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TRANSFER_DESTINATION_ROW_SPACING;
        ui.label(egui::RichText::new("Move to another character").strong());
        for destination in destinations {
            let response = ui
                .add_enabled_ui(destination.enabled, |ui| {
                    draw_character_transfer_destination(ui, destination)
                })
                .inner;
            let response = if destination.enabled {
                response.on_hover_text(&destination.tooltip)
            } else {
                response.on_disabled_hover_text(&destination.tooltip)
            };
            if response.clicked() {
                selected = Some(destination.character_index);
            }
        }
    });
    selected
}

fn draw_character_transfer_destination(
    ui: &mut egui::Ui,
    destination: &CharacterTransferDestination,
) -> egui::Response {
    draw_inventory_item_menu_text(ui, &destination.label, &destination.detail)
}

fn character_bucket_usage_detail(
    metadata: InventoryMetadata,
    usage: &BucketUsage,
) -> Option<String> {
    let capacity = metadata.authored_row_capacity()?;
    let occupied = usage
        .counts
        .get(&metadata.native_bucket_id)
        .copied()
        .unwrap_or_default();
    Some(format!(
        "{} · {occupied} / {capacity} slots used",
        metadata.bucket_label()
    ))
}

fn draw_inventory_item_menu_text(
    ui: &mut egui::Ui,
    primary_text: &str,
    secondary_text: &str,
) -> egui::Response {
    const HORIZONTAL_PADDING: f32 = 4.0;
    const TEXT_GAP: f32 = 8.0;
    const PRIMARY_WIDTH_SHARE: f32 = 0.45;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), TRANSFER_DESTINATION_ROW_HEIGHT),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }

    let text_width = (rect.width() - HORIZONTAL_PADDING * 2.0).max(0.0);
    let available_text_width = (text_width - TEXT_GAP).max(0.0);
    let primary_font = egui::TextStyle::Button.resolve(ui.style());
    let secondary_font = egui::TextStyle::Body.resolve(ui.style());
    let primary_color = visuals.text_color();
    let secondary_color = ui.visuals().weak_text_color();
    let natural_primary = transfer_menu_galley(
        ui,
        primary_text,
        primary_font.clone(),
        primary_color,
        f32::INFINITY,
    );
    let natural_secondary = transfer_menu_galley(
        ui,
        secondary_text,
        secondary_font.clone(),
        secondary_color,
        f32::INFINITY,
    );
    let reserved_primary_width = natural_primary
        .size()
        .x
        .min(available_text_width * PRIMARY_WIDTH_SHARE);
    let secondary_width = natural_secondary
        .size()
        .x
        .min((available_text_width - reserved_primary_width).max(0.0));
    let primary_width = natural_primary
        .size()
        .x
        .min((available_text_width - secondary_width).max(0.0));
    let primary =
        transfer_menu_galley(ui, primary_text, primary_font, primary_color, primary_width);
    let secondary = transfer_menu_galley(
        ui,
        secondary_text,
        secondary_font,
        secondary_color,
        secondary_width,
    );
    let primary_position = egui::pos2(
        rect.left() + HORIZONTAL_PADDING,
        rect.center().y - primary.size().y / 2.0,
    );
    let secondary_position = egui::pos2(
        rect.right() - HORIZONTAL_PADDING - secondary.size().x,
        rect.center().y - secondary.size().y / 2.0,
    );
    ui.painter()
        .galley(primary_position, primary, primary_color);
    ui.painter()
        .galley(secondary_position, secondary, secondary_color);
    response
}

fn inventory_item_ui_identities(items: &[InventoryItemSnapshot]) -> Vec<InventoryItemUiId> {
    let mut totals = HashMap::<u64, usize>::new();
    for item in items {
        *totals.entry(item.instance_soid).or_default() += 1;
    }
    let mut seen = HashMap::<u64, usize>::new();
    items
        .iter()
        .map(|item| {
            let duplicate_ordinal = (totals[&item.instance_soid] > 1).then(|| {
                let ordinal = seen.entry(item.instance_soid).or_default();
                let current = *ordinal;
                *ordinal += 1;
                current
            });
            InventoryItemUiId {
                character_index: item.location.character_index,
                instance_soid: item.instance_soid,
                duplicate_ordinal,
            }
        })
        .collect()
}

fn inventory_item_state_key(identity: InventoryItemUiId) -> String {
    let duplicate = identity
        .duplicate_ordinal
        .map_or_else(String::new, |ordinal| format!(":duplicate-{ordinal}"));
    format!(
        "character-inventory:{}:{}{}",
        identity.character_index, identity.instance_soid, duplicate
    )
}

fn draw_section_error(ui: &mut egui::Ui, error: &str) {
    ui.colored_label(ui.visuals().error_fg_color, error);
    ui.label(
        egui::RichText::new(
            "This section was left untouched. Repair it in All settings (JSON) before using guided controls.",
        )
        .weak(),
    );
}

fn draw_inventory_source_error(ui: &mut egui::Ui, source: &str, error: &str) {
    ui.colored_label(
        ui.visuals().error_fg_color,
        format!("{source} could not be read: {error}"),
    );
    ui.label(
        egui::RichText::new(
            "The other inventory source remains visible, but additions are disabled until this is repaired in All settings (JSON).",
        )
        .weak(),
    );
}

fn draw_unresolved_bucket_warning(ui: &mut egui::Ui) {
    ui.colored_label(
        ui.visuals().warn_fg_color,
        "At least one existing definition has no known bucket. Unknown rows are conservatively counted against each candidate bucket, so some additions or cross-bucket replacements may be hidden.",
    );
}

fn bucket_has_room(
    metadata: &InventoryMetadata,
    usage: &BucketUsage,
    current_bucket: Option<u8>,
    replacing_unresolved: bool,
) -> bool {
    if current_bucket == Some(metadata.native_bucket_id) {
        return true;
    }
    metadata.authored_row_capacity().is_some_and(|capacity| {
        let known = usage
            .counts
            .get(&metadata.native_bucket_id)
            .copied()
            .unwrap_or_default();
        let unresolved = usage
            .unresolved_count
            .saturating_sub(usize::from(replacing_unresolved));
        known.saturating_add(unresolved) < usize::from(capacity)
    })
}

fn profile_swap_candidate(
    metadata: &InventoryMetadata,
    current_bucket: Option<u8>,
    quantity: i32,
    usage: &BucketUsage,
    replacing_unresolved: bool,
) -> bool {
    current_bucket.is_none_or(|bucket| {
        metadata.scope == InventoryScope::Profile && metadata.native_bucket_id == bucket
    }) && metadata
        .max_stack_size
        .is_some_and(|maximum| maximum >= quantity as u32)
        && bucket_has_room(metadata, usage, current_bucket, replacing_unresolved)
}

fn displayed_inventory_plugs(
    inventory: &InventoryItemSnapshot,
    item: &ItemDef,
) -> (Vec<Option<u32>>, bool) {
    match &inventory.plugs {
        ItemPlugs::NativeDefaults => (
            item.default_plugs
                .iter()
                .map(|hash| {
                    hash.as_deref()
                        .and_then(parse_hash)
                        .and_then(|hash| u32::try_from(hash).ok())
                })
                .collect(),
            true,
        ),
        ItemPlugs::Authored(plugs) => (plugs.clone(), false),
    }
}

fn apply_inventory_actions_atomic(
    document: &mut serde_json::Value,
    location: InventoryItemLocation,
    actions: Vec<InventoryItemAction>,
) -> Result<(), String> {
    let mut candidate = document.clone();
    for action in actions {
        inventory::apply_inventory_item_action(&mut candidate, location, action)
            .map_err(|error| error.to_string())?;
    }
    *document = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ItemStackability;
    use serde_json::json;

    #[test]
    fn profile_browse_keeps_all_results_across_named_buckets() {
        let modifications = InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id: 13,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(1),
            bucket_capacity: Some(200),
        };
        let glimmer = InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id: 21,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(999_999),
            bucket_capacity: Some(1),
        };
        let many_alphabetical_rows = (0_u64..120).map(|hash| InventoryDefinition {
            hash,
            name: "Alphabetical modification",
            type_name: "Modification",
            metadata: &modifications,
            item: None,
        });
        let currency = std::iter::once(InventoryDefinition {
            hash: 1_000,
            name: "Glimmer",
            type_name: "Currency",
            metadata: &glimmer,
            item: None,
        });

        let choices = profile_definition_choices(many_alphabetical_rows.chain(currency));
        assert_eq!(choices.len(), 121);
        assert!(
            choices
                .iter()
                .any(|choice| choice.group.as_deref() == Some("Glimmer"))
        );
    }

    #[test]
    fn shared_item_swap_candidates_stay_in_the_current_bucket() {
        let metadata = |native_bucket_id| InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(999_999),
            bucket_capacity: Some(10),
        };
        let current = metadata(15);
        let other = metadata(21);
        let usage = BucketUsage {
            counts: HashMap::from([(15, 1), (21, 1)]),
            unresolved_count: 0,
            occupancy_complete: true,
        };

        assert!(profile_swap_candidate(
            &current,
            Some(15),
            100,
            &usage,
            false
        ));
        assert!(!profile_swap_candidate(
            &other,
            Some(15),
            100,
            &usage,
            false
        ));
        assert!(profile_swap_candidate(&other, None, 100, &usage, true));
    }

    #[test]
    fn character_browse_keeps_all_results_and_orders_native_buckets() {
        let metadata = |native_bucket_id| InventoryMetadata {
            scope: InventoryScope::Character,
            native_bucket_id,
            stackability: ItemStackability::Instanced,
            max_stack_size: Some(1),
            bucket_capacity: Some(200),
        };
        let kinetic = metadata(0);
        let chest = metadata(5);
        let artifact = metadata(49);
        let many_chest_rows = (0_u64..120).map(|hash| InventoryDefinition {
            hash,
            name: "Chest item",
            type_name: "Chest armor",
            metadata: &chest,
            item: None,
        });
        let edge_buckets = [
            InventoryDefinition {
                hash: 1_000,
                name: "Kinetic item",
                type_name: "Kinetic weapon",
                metadata: &kinetic,
                item: None,
            },
            InventoryDefinition {
                hash: 1_001,
                name: "Artifact item",
                type_name: "Seasonal artifact",
                metadata: &artifact,
                item: None,
            },
        ];

        let choices = character_definition_choices(many_chest_rows.chain(edge_buckets));
        assert_eq!(choices.len(), 122);
        let group_position = |group| {
            choices
                .iter()
                .position(|choice| choice.group.as_deref() == Some(group))
                .unwrap()
        };
        assert!(group_position("Kinetic weapons") < group_position("Chest armor"));
        assert!(group_position("Chest armor") < group_position("Seasonal artifacts"));
    }

    #[test]
    fn character_item_ui_ids_survive_index_shifts_and_disambiguate_bad_soids() {
        let snapshot = |item_index, instance_soid| InventoryItemSnapshot {
            location: InventoryItemLocation {
                character_index: 0,
                item_index,
            },
            instance_soid,
            definition_hash: 1,
            level: 1,
            quantity: 1,
            plugs: ItemPlugs::NativeDefaults,
            flags: None,
        };
        let before = inventory_item_ui_identities(&[snapshot(0, 10), snapshot(1, 20)]);
        let after = inventory_item_ui_identities(&[snapshot(0, 20)]);
        assert_eq!(before[1], after[0]);

        let duplicates = inventory_item_ui_identities(&[snapshot(0, 20), snapshot(1, 20)]);
        assert_ne!(duplicates[0], duplicates[1]);
        assert_eq!(duplicates[0].duplicate_ordinal, Some(0));
        assert_eq!(duplicates[1].duplicate_ordinal, Some(1));
    }

    #[test]
    fn bucket_picker_open_request_waits_for_the_originating_click_frame() {
        let picker_key = "character-inventory:0:add:1:4";
        let request_key = bucket_picker_open_request_key(picker_key);
        let mut searches = HashMap::from([(request_key.clone(), String::new())]);

        assert!(!take_bucket_picker_open_request(
            &mut searches,
            picker_key,
            true
        ));
        assert!(searches.contains_key(&request_key));
        assert!(take_bucket_picker_open_request(
            &mut searches,
            picker_key,
            false
        ));
        assert!(!searches.contains_key(&request_key));
        assert!(!take_bucket_picker_open_request(
            &mut searches,
            picker_key,
            false
        ));
    }

    #[test]
    fn bucket_capacity_counts_only_present_rows_and_allows_same_bucket_replacement() {
        let metadata = InventoryMetadata {
            scope: InventoryScope::Character,
            native_bucket_id: 4,
            bucket_capacity: Some(3),
            ..InventoryMetadata::default()
        };
        let one_row_free = BucketUsage {
            // Present equipment and inventory rows are both included in this count.
            counts: HashMap::from([(4, 2)]),
            unresolved_count: 0,
            occupancy_complete: true,
        };
        assert!(bucket_has_room(&metadata, &one_row_free, None, false));

        let full = BucketUsage {
            counts: HashMap::from([(4, 3)]),
            unresolved_count: 0,
            occupancy_complete: true,
        };
        assert!(bucket_has_room(&metadata, &full, Some(4), false));
        assert!(!bucket_has_room(&metadata, &full, None, false));

        let unresolved = BucketUsage {
            counts: HashMap::new(),
            unresolved_count: 3,
            occupancy_complete: true,
        };
        assert!(bucket_has_room(&metadata, &unresolved, Some(4), false));
        assert!(!bucket_has_room(&metadata, &unresolved, None, false));
        assert!(bucket_has_room(&metadata, &unresolved, None, true));
    }

    #[test]
    fn multi_field_inventory_edits_are_atomic() {
        let mut document = json!({
            "version": 6,
            "state": {
                "account": {},
                "characters": [{
                    "soid": 1,
                    "equipment": {},
                    "inventory": [{
                        "instance_soid": "0x4000000000000001",
                        "definition_hash": "0x0000002A",
                        "level": 106,
                        "quantity": 1,
                        "plugs": null
                    }]
                }]
            }
        });
        let before = document.clone();
        let error = apply_inventory_actions_atomic(
            &mut document,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            vec![
                InventoryItemAction::SetQuantity(2),
                InventoryItemAction::SetFlags(Some(8)),
            ],
        )
        .unwrap_err();

        assert!(error.contains("flags"));
        assert_eq!(document, before);
    }
}
