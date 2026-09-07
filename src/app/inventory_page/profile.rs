//! Profile-scoped inventory and dismantle-reward rendering.

use crate::app::account_workspace as account;

use eframe::egui;

use crate::{catalog::InventoryScope, hash::format_hash_hex};

use super::super::{
    SundialApp,
    inspector::DefinitionInspectionContext,
    inventory::{
        self, DismantleGearClass, DismantleRarity, DismantleRewardAction, DismantleRewardSnapshot,
        ProfileItemAction, ProfileItemSnapshot, SchemaMode,
    },
    item_editor::{
        self, DefinitionPickerChoices, DefinitionSummary, ItemEditorAction, ItemHeader,
        NumericItemFields,
    },
};
use super::{
    buckets::{
        add_candidate_buckets, bucket_add_tooltip, bucket_header_label, bucket_header_text,
        bucket_key_has_room, distinct_candidate_buckets, draw_bucket_details,
        profile_swap_candidate, scope_id,
    },
    definitions::{
        profile_bucket_definition_choices, profile_definition_choices, without_definition_groups,
    },
    interactions::take_bucket_picker_open_request,
    model::{BucketUsage, ProfileInventorySection},
    presentation::{
        InventoryPageKind, dismantle_class_label, dismantle_masterwork_label,
        dismantle_rarity_label, dismantle_rarity_summary, draw_schema_notice, draw_section_error,
        draw_unresolved_bucket_warning, picker_height,
    },
};

impl SundialApp {
    pub(in crate::app) fn draw_profile_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        let section_id = ui.make_persistent_id("profile-inventory-section");
        let mut section = ui.data_mut(|data| {
            data.get_temp::<ProfileInventorySection>(section_id)
                .unwrap_or_default()
        });
        let dismantle_rewards_available = account::dismantle_rewards_available(&self.document);
        if section == ProfileInventorySection::DismantleRewards && !dismantle_rewards_available {
            section = ProfileInventorySection::SharedItems;
        }

        ui.horizontal(|ui| {
            ui.heading("Profile Inventory");
            crate::ui_help::info(ui, "Items shared by the account and available to every character. Storage and limits follow the active Sunrise account source.");
        });
        if self.document.uses_json_account() {
            draw_schema_notice(ui, mode, InventoryPageKind::Profile);
        }
        ui.add_space(4.0);

        if dismantle_rewards_available {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::SharedItems,
                    "Shared Items",
                );
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::DismantleRewards,
                    "Dismantle Rewards",
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

    fn draw_dismantle_reward_section(&mut self, ui: &mut egui::Ui, _mode: SchemaMode) {
        if !account::dismantle_rewards_available(&self.document) {
            return;
        }
        let rewards = match account::dismantle_rewards(&self.document) {
            Ok(rewards) => rewards.unwrap_or_default(),
            Err(error) => {
                ui.strong("Dismantle Rewards");
                draw_section_error(ui, &error.to_string());
                return;
            }
        };
        let editable = account::dismantle_rewards_editable(&self.document);
        let capacity = account::dismantle_reward_capacity(&self.document);
        let account_ready = account::account_collection_ready(&self.document);
        let filtered = account::filtered_dismantle_rewards(&self.document);
        let combined_gear_class = account::supports_combined_dismantle_gear_class(&self.document);
        let has_room = capacity.is_some_and(|capacity| rewards.len() < capacity);
        let picker_key = "dismantle-rewards:add".to_owned();
        let mut picker_anchor = None;
        let mut open_picker = false;

        ui.horizontal_wrapped(|ui| {
            ui.strong("Dismantle Rewards");
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
                    "Dismantle-policy editing is unavailable for the active account source"
                } else if !account_ready {
                    "The active account source has no account collection to edit"
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
        if filtered {
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
                            random_item_builder_hash: None,
                            empty_message: "No profile material definitions match".to_owned(),
                        },
                    )
                })
                .inner;
            if let Some(ItemEditorAction::SetDefinition { hash }) = action
                && let Ok(hash) = u32::try_from(hash)
            {
                match account::add_dismantle_reward(&mut self.document, hash) {
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
        let (minimum_card_width, maximum_card_width) =
            self.preferences.item_card_width.dimensions();
        item_editor::draw_responsive_item_cards(
            ui,
            &rewards,
            minimum_card_width,
            maximum_card_width,
            |ui, reward| {
                if pending.is_none()
                    && let Some(action) = self.draw_dismantle_reward_card(
                        ui,
                        reward,
                        editable,
                        filtered,
                        combined_gear_class,
                    )
                {
                    pending = Some((reward.location, action));
                }
            },
        );
        if let Some((location, action)) = pending {
            let structural = matches!(action, DismantleRewardAction::Remove);
            match account::apply_dismantle_reward_action(&mut self.document, location, action) {
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
        filtered: bool,
        combined_gear_class: bool,
    ) -> Option<DismantleRewardAction> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let valid = resolved
            .as_ref()
            .is_some_and(|definition| definition.metadata.is_profile_items_candidate());
        let hash_hex_text = format_hash_hex(u64::from(snapshot.definition_hash));
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
                                "Dismantle Reward Policy Â· Row {}",
                                snapshot.location.index + 1
                            ),
                            instance_id: None,
                            authored_level: None,
                            flags: None,
                            plug_count: None,
                            plugs: None,
                            quantity: Some(i64::from(snapshot.quantity)),
                        }),
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

                            if filtered {
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

                        if filtered {
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
                                        if combined_gear_class {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut gear_class,
                                                    Some(DismantleGearClass::Both),
                                                    "Weapon + armor",
                                                )
                                                .changed();
                                        }
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
                                    random_item_builder_hash: None,
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

    fn draw_profile_items_section(&mut self, ui: &mut egui::Ui, _mode: SchemaMode) {
        let snapshots = match account::profile_items(&self.document) {
            Ok(items) => items,
            Err(error) => {
                draw_section_error(ui, &error.to_string());
                return;
            }
        };
        let items = snapshots.unwrap_or_default();
        let profile_item_count = items.len();
        let editable = account::profile_items_editable(&self.document);
        let capacity = account::profile_item_capacity(&self.document);

        ui.horizontal_wrapped(|ui| {
            ui.strong("Shared Items");
            let count = capacity.map_or_else(
                || format!("{} items", items.len()),
                |capacity| format!("{} / {capacity}", items.len()),
            );
            ui.label(egui::RichText::new(count).weak());
        });
        ui.label("Stackable profile-scoped definitions only.");

        let bucket_usage = self.profile_bucket_usage(&items);
        let account_ready = account::account_collection_ready(&self.document);
        if !editable {
            ui.label(
                egui::RichText::new(
                    "Profile-item editing is unavailable for the active account source.",
                )
                .weak(),
            );
        } else if capacity.is_some_and(|capacity| items.len() >= capacity) {
            ui.label(
                egui::RichText::new("The profile-item collection is full for this account source.")
                    .weak(),
            );
        } else if !account_ready {
            ui.label(
                egui::RichText::new(
                    "Add controls require an account collection in the active source; existing rows remain visible.",
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
                                    random_item_builder_hash: None,
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
                                account::add_profile_item(&mut self.document, hash, 1)
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
                let (minimum_card_width, maximum_card_width) =
                    self.preferences.item_card_width.dimensions();
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
            match account::apply_profile_item_action(&mut self.document, location, action) {
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
        let hash_hex_text = format_hash_hex(u64::from(snapshot.definition_hash));
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
                                "Profile Inventory Â· Item {}",
                                snapshot.location.index + 1
                            ),
                            instance_id: None,
                            authored_level: None,
                            flags: None,
                            plug_count: None,
                            plugs: None,
                            quantity: Some(i64::from(snapshot.quantity)),
                        }),
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
                                    random_item_builder_hash: None,
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
}
