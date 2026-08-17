use std::collections::HashMap;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{self, AbilityChoice, ItemDef, format_hash},
    game_settings,
};

use super::{
    ARMOR_SLOTS, ConfirmationDialog, ITEM_PICKER_MAX_HEIGHT, ITEM_PICKER_MIN_HEIGHT,
    PLUG_PICKER_MAX_HEIGHT, PLUG_PICKER_MIN_HEIGHT, PlugSelectionMode, SLOTS, SundialApp,
    WEAPON_SLOTS, settings::character_ability_issue,
};

#[path = "item_editor.rs"]
pub(super) mod item_editor;
pub(super) use item_editor::NativePlugDefault;
use item_editor::{
    ClearDefinitionChoice, DefinitionChoice, DefinitionPickerChoices, DefinitionSummary,
    ItemEditorAction, ItemHeader, NumericItemFields, PickerHeight, PlugChoice, PlugPickerSnapshot,
};

/// A tolerant, read-only view of one non-null character equipment slot.
///
/// Unlike the editable equipment UI, this snapshot deliberately retains malformed
/// rows so callers can still show the authored data and its issues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EquippedItemSnapshot {
    pub slot: &'static str,
    pub slot_label: &'static str,
    pub bucket_hash: u64,
    pub raw_item_text: String,
    pub definition_hash: Option<u64>,
    pub definition_text: String,
    pub instance_soid: Option<u64>,
    pub instance_soid_text: String,
    pub level: Option<i64>,
    pub quantity: Option<i64>,
    pub plugs: EquippedItemPlugs,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EquippedItemPlugs {
    NativeDefaults,
    Authored(Vec<EquippedPlugValue>),
    Missing,
    Malformed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EquippedPlugValue {
    Empty,
    Hash(u64),
    Malformed(String),
}

pub(super) struct EquipmentSlotCard<'a> {
    pub id_scope: &'static str,
    pub slot: &'static str,
    pub label: &'a str,
    pub bucket_hash: u64,
    pub class_type: u64,
    pub editable: bool,
    pub header_fill: Option<egui::Color32>,
    pub snapshot: Option<&'a EquippedItemSnapshot>,
}

impl SundialApp {
    fn equipment_mutation_allowed(&mut self) -> bool {
        if super::inventory::schema_mode(&self.document).can_mutate_equipment() {
            true
        } else {
            self.set_status(
                "Equipment editing is disabled for this settings schema",
                true,
            );
            false
        }
    }

    fn equipment_flags_mutation_allowed(&mut self) -> bool {
        if super::inventory::schema_mode(&self.document).can_mutate_equipment_flags() {
            true
        } else {
            self.set_status(
                format!(
                    "Equipment lock-state editing requires a writable settings schema {} or newer",
                    super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
                ),
                true,
            );
            false
        }
    }

    fn select_item(&mut self, character: usize, slot: &str, item: &ItemDef) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match equip_definition(
            &mut self.document,
            character,
            slot,
            item.hash,
            &item.default_plugs,
        ) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(format!("Equipped {}", item.name), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn select_subclass_item(&mut self, character: usize, item: &ItemDef) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match equip_subclass_with_default_abilities(&mut self.document, character, item) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(format!("Equipped {}", item.name), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn empty_weapon(&mut self, character: usize, slot: &str) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match set_weapon_slot_empty(&mut self.document, character, slot) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Set the {} slot to empty", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn select_plug(
        &mut self,
        character: usize,
        slot: &str,
        socket_index: usize,
        socket_label: &str,
        default_plugs: &[Option<String>],
        hash: Option<u64>,
    ) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        let Some(plugs_value) = self
            .characters_mut()
            .and_then(|chars| chars.get_mut(character))
            .and_then(|ch| ch.pointer_mut(&format!("/equipment/{slot}/plugs")))
        else {
            self.set_status(format!("Missing plugs value for {slot}"), true);
            return;
        };
        let Some(plugs) = materialize_authored_plugs(plugs_value, default_plugs) else {
            self.set_status(format!("Invalid plugs value for {slot}"), true);
            return;
        };
        while plugs.len() <= socket_index {
            plugs.push(Value::Null);
        }
        plugs[socket_index] = hash.map(format_hash).map_or(Value::Null, Value::String);
        self.dirty = true;
        self.set_status(format!("Updated {slot} {socket_label}"), false);
    }

    fn select_equipment_level(&mut self, character: usize, slot: &str, level: i64) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match set_equipment_item_level(&mut self.document, character, slot, level) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Updated {} power", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn select_equipment_flags(&mut self, character: usize, slot: &str, flags: Option<u8>) {
        if !self.equipment_flags_mutation_allowed() {
            return;
        }
        match set_equipment_item_flags(&mut self.document, character, slot, flags) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Updated {} lock state", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn draw_character_fields(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editable: bool,
    ) {
        let settings_schema = game_settings::schema_version(&self.document);
        let Some(character) = self.characters().and_then(|chars| chars.get(index)) else {
            return;
        };
        let soid = character
            .get("soid")
            .and_then(parse_unsigned_value)
            .map_or_else(|| "Unknown".to_owned(), format_hash);
        let mut race = character.get("race").and_then(Value::as_u64).unwrap_or(0);
        let mut gender = character.get("gender").and_then(Value::as_u64).unwrap_or(0);
        let mut class_type = character.get("class").and_then(Value::as_u64).unwrap_or(0);
        let mut movement = character
            .get("movement_ability")
            .and_then(Value::as_u64)
            .unwrap_or(4);
        let mut grenade = character
            .get("grenade_ability")
            .and_then(Value::as_u64)
            .unwrap_or(7);
        let mut super_ability = character
            .get("super_ability")
            .and_then(Value::as_u64)
            .unwrap_or(10);
        let mut melee = character
            .get("melee_ability")
            .and_then(Value::as_u64)
            .unwrap_or(11);
        let mut class_ability = character
            .get("class_ability")
            .and_then(Value::as_u64)
            .unwrap_or(2);
        let original_class_type = class_type;
        let mut current_subclass_hash = character
            .pointer("/equipment/subclass/definition_hash")
            .and_then(parse_unsigned_value);
        let mut abilities = current_subclass_hash
            .and_then(|hash| self.manifest.get_for_bucket(hash, 3_284_755_031))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let mut attunement_index = selected_attunement_index(&abilities, super_ability, melee);
        let all_subclasses: Vec<ItemDef> = self
            .manifest
            .items
            .iter()
            .filter(|item| item.bucket_hash == 3_284_755_031)
            .cloned()
            .collect();
        let mut subclasses: Vec<ItemDef> = all_subclasses
            .iter()
            .filter(|item| item.class_type == class_type)
            .cloned()
            .collect();
        let mut selected_subclass = None::<ItemDef>;
        let stored_warning = self
            .source_warning
            .as_deref()
            .filter(|warning| {
                warning.starts_with(&format!("Character {} ", index + 1))
                    && (warning.contains("ability") || warning.contains("super and melee"))
            })
            .map(str::to_owned);
        let ability_warning = character
            .as_object()
            .and_then(character_ability_issue)
            .or(stored_warning);

        ui.heading(format!("Character {}", index + 1));
        ui.label(egui::RichText::new(soid).monospace().weak());
        if let Some(warning) = ability_warning {
            ui.add_space(6.0);
            ui.colored_label(
                        ui.visuals().warn_fg_color,
                format!(
                    "Warning: {warning}. This can prevent Sunrise from loading the character. Choose supported abilities below and save before launching."
                ),
            );
        }
        ui.add_space(8.0);
        egui::Grid::new("character_fields")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.label("Class");
                combo_u64(
                    ui,
                    "class",
                    &mut class_type,
                    &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")],
                );
                if class_type != original_class_type {
                    subclasses = all_subclasses
                        .iter()
                        .filter(|item| item.class_type == class_type)
                        .cloned()
                        .collect();
                    if let Some(subclass) = subclasses
                        .iter()
                        .find(|item| item.name == default_subclass_name(class_type))
                        .cloned()
                        .or_else(|| subclasses.first().cloned())
                    {
                        current_subclass_hash = Some(subclass.hash);
                        abilities = subclass.abilities.clone();
                        (movement, grenade, super_ability, melee, class_ability) =
                            default_ability_values(class_type, &abilities, settings_schema);
                        attunement_index =
                            selected_attunement_index(&abilities, super_ability, melee);
                        selected_subclass = Some(subclass);
                    }
                }
                ui.end_row();
                ui.label("Race");
                combo_u64(
                    ui,
                    "race",
                    &mut race,
                    &[(0, "Human"), (1, "Awoken"), (2, "Exo")],
                );
                ui.end_row();
                ui.label("Gender");
                combo_u64(ui, "gender", &mut gender, &[(0, "Male"), (1, "Female")]);
                ui.end_row();
                ui.label("Subclass");
                let selected_name = current_subclass_hash
                    .and_then(|hash| subclasses.iter().find(|item| item.hash == hash))
                    .map_or("Unknown subclass", |item| item.name.as_str());
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected_name)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for subclass in &subclasses {
                            let selected = current_subclass_hash == Some(subclass.hash);
                            if ui.selectable_label(selected, &subclass.name).clicked() && !selected
                            {
                                current_subclass_hash = Some(subclass.hash);
                                abilities = subclass.abilities.clone();
                                (movement, grenade, super_ability, melee, class_ability) =
                                    default_ability_values(class_type, &abilities, settings_schema);
                                attunement_index =
                                    selected_attunement_index(&abilities, super_ability, melee);
                                selected_subclass = Some(subclass.clone());
                            }
                        }
                    });
                ui.end_row();
                ui.label("Attunement");
                let previous_attunement = attunement_index;
                let selected_attunement = abilities
                    .attunements
                    .get(attunement_index)
                    .map_or("No attunement data", |attunement| attunement.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected_attunement)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (choice_index, attunement) in abilities.attunements.iter().enumerate() {
                            ui.selectable_value(
                                &mut attunement_index,
                                choice_index,
                                &attunement.name,
                            );
                        }
                    });
                ui.end_row();
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    let current_pair_is_valid = attunement.melee.entry == melee
                        && attunement
                            .super_abilities
                            .iter()
                            .any(|choice| choice.entry == super_ability);
                    if attunement_index != previous_attunement || !current_pair_is_valid {
                        melee = attunement.melee.entry;
                        super_ability = attunement
                            .super_abilities
                            .first()
                            .map_or(10, |choice| choice.entry);
                    }
                }
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    ui.label("Attunement perks");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                attunement
                                    .perks
                                    .iter()
                                    .map(|choice| choice.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" • "),
                            )
                            .weak(),
                        )
                        .wrap(),
                    );
                    ui.end_row();
                }
                for (label, id, value, choices) in [
                    (
                        "Movement ability",
                        "movement_ability",
                        &mut movement,
                        &abilities.movement,
                    ),
                    (
                        "Grenade ability",
                        "grenade_ability",
                        &mut grenade,
                        &abilities.grenade,
                    ),
                ] {
                    ui.label(label);
                    ability_combo(ui, id, value, choices);
                    ui.end_row();
                }
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    ui.label("Super ability");
                    ui.label(
                        attunement
                            .super_abilities
                            .first()
                            .map_or("Unknown super", |choice| choice.name.as_str()),
                    );
                    ui.end_row();
                    ui.label("Melee ability");
                    ui.label(&attunement.melee.name);
                    ui.end_row();
                } else {
                    for (label, id, value, choices) in [
                        (
                            "Super ability",
                            "super_ability",
                            &mut super_ability,
                            &abilities.super_ability,
                        ),
                        (
                            "Melee ability",
                            "melee_ability",
                            &mut melee,
                            &abilities.melee,
                        ),
                    ] {
                        ui.label(label);
                        ability_combo(ui, id, value, choices);
                        ui.end_row();
                    }
                }
                ui.label("Class ability").on_hover_text(
                    "Dodge, Barricade, and Rift remain independent choices. Attunement perks may modify their behavior.",
                );
                ability_combo(
                    ui,
                    "class_ability",
                    &mut class_ability,
                    &abilities.class_ability,
                );
                ui.end_row();
            });

        // A disabled egui scope still executes this function. Do not let its fallback display
        // values materialize missing fields in a read-only schema.
        if !editable {
            return;
        }

        let mut changed = false;
        let selecting_subclass = selected_subclass.is_some();
        let armor_template = (class_type != original_class_type)
            .then(|| self.class_armor_defaults.get(&class_type).cloned())
            .flatten();
        {
            let Some(character) = self.characters_mut().and_then(|chars| chars.get_mut(index))
            else {
                return;
            };
            let Some(object) = character.as_object_mut() else {
                return;
            };
            for (key, new_value) in [("race", race), ("gender", gender), ("class", class_type)] {
                let old = object.get(key).and_then(Value::as_u64);
                if old != Some(new_value) {
                    object.insert(key.into(), Value::from(new_value));
                    changed = true;
                }
            }
            if !selecting_subclass {
                for (key, new_value) in [
                    ("movement_ability", movement),
                    ("grenade_ability", grenade),
                    ("super_ability", super_ability),
                    ("melee_ability", melee),
                    ("class_ability", class_ability),
                ] {
                    if object.get(key).and_then(Value::as_u64) != Some(new_value) {
                        object.insert(key.into(), Value::from(new_value));
                        changed = true;
                    }
                }
            }
            if let Some(template) = armor_template.as_ref() {
                changed |= restore_class_armor(object, template);
            }
        }
        self.dirty |= changed;
        if let Some(subclass) = selected_subclass {
            self.select_subclass_item(index, &subclass);
        }
    }

    pub(super) fn draw_item_safety_controls(&mut self, ui: &mut egui::Ui) {
        let mut requested_plug_selection_mode = self.plug_selection_mode;
        ui.horizontal_wrapped(|ui| {
            ui.label("Plug choices:");
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::Supported,
                "Supported only",
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::MatchingSocketType,
                "Matching socket type (unsafe)",
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::AnyPlug,
                "Any plug (really unsafe)",
            );
            ui.separator();
            ui.checkbox(&mut self.show_dummy_items, "Show dummy items")
                .on_hover_text(
                    "Includes display-only definitions that cannot normally be obtained in the game.",
                );
        });
        if requested_plug_selection_mode != self.plug_selection_mode {
            if requested_plug_selection_mode == PlugSelectionMode::AnyPlug
                && !self.really_unsafe_warning_acknowledged
            {
                self.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
            } else {
                self.plug_selection_mode = requested_plug_selection_mode;
            }
        }
        match self.plug_selection_mode {
            PlugSelectionMode::Supported => {}
            PlugSelectionMode::MatchingSocketType => {
                ui.colored_label(
                            ui.visuals().warn_fg_color,
                    "Warning: unsupported plug combinations may break items, corrupt the loadout, or crash Sunrise/Destiny 2.",
                );
            }
            PlugSelectionMode::AnyPlug => {
                ui.colored_label(
                            ui.visuals().error_fg_color,
                    "Danger: every discovered plug is available for every socket, greatly increasing the chance that the game will not load or will crash.",
                );
            }
        }
    }

    pub(super) fn draw_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        let editable = super::inventory::schema_mode(&self.document).can_mutate_equipment();
        let class_type = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.get("class"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        ui.add_space(14.0);
        ui.heading("Equipped loadout");
        ui.label("Search by item name or 0x hash. Choosing an item also installs its package-default plugs.");
        ui.label(
            egui::RichText::new(
                "Some character changes may leave the character-select preview appearing to load, while the in-game model still reflects them.",
            )
            .weak(),
        );
        ui.add_enabled_ui(editable, |ui| self.draw_item_safety_controls(ui));
        ui.add_space(6.0);

        for &(slot, label, bucket) in SLOTS {
            if slot == "subclass" {
                continue;
            }
            self.draw_equipment_slot_card(
                ui,
                character_index,
                EquipmentSlotCard {
                    id_scope: "characters-equipment",
                    slot,
                    label,
                    bucket_hash: bucket,
                    class_type,
                    editable,
                    header_fill: None,
                    snapshot: None,
                },
            );
            ui.add_space(5.0);
        }
    }

    pub(super) fn draw_equipment_slot_card(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        card: EquipmentSlotCard<'_>,
    ) {
        let EquipmentSlotCard {
            id_scope,
            slot,
            label,
            bucket_hash: bucket,
            class_type,
            editable,
            header_fill,
            snapshot,
        } = card;
        let equipped_value = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.pointer(&format!("/equipment/{slot}")))
            .cloned();
        let is_empty = equipped_value.as_ref().is_some_and(Value::is_null);
        let current_level = equipped_value
            .as_ref()
            .and_then(|item| item.get("level"))
            .and_then(Value::as_i64);
        let current_flags = equipped_value
            .as_ref()
            .and_then(|item| item.get("flags"))
            .and_then(parse_unsigned_value)
            .and_then(|flags| u8::try_from(flags).ok());
        let current_hash_value = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.pointer(&format!("/equipment/{slot}/definition_hash")))
            .cloned();
        let current_hash = current_hash_value.as_ref().and_then(parse_unsigned_value);
        let current_hash_text = current_hash.map_or_else(
            || {
                current_hash_value
                    .as_ref()
                    .and_then(Value::as_str)
                    .unwrap_or("<missing>")
                    .to_owned()
            },
            format_hash,
        );
        let current = current_hash
            .and_then(|hash| self.manifest.get_for_bucket(hash, bucket))
            .cloned();
        let definition_valid = is_empty
            || current.as_ref().is_some_and(|item| {
                item.bucket_hash == bucket
                    && (item.class_type == 3 || item.class_type == class_type)
            });
        let snapshot_valid = snapshot.is_none_or(|snapshot| snapshot.issues.is_empty());
        let valid = definition_valid && snapshot_valid;
        let guided_editable = editable && snapshot_valid;
        let flags_editable =
            super::inventory::schema_mode(&self.document).can_mutate_equipment_flags();
        ui.push_id((id_scope, character_index, slot), |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let definition = if is_empty {
                    DefinitionSummary::Empty
                } else if let Some(item) = &current {
                    DefinitionSummary::Known {
                        name: &item.name,
                        hash: &current_hash_text,
                    }
                } else {
                    DefinitionSummary::Unknown {
                        hash: &current_hash_text,
                    }
                };
                let header = ItemHeader {
                    label: Some(label),
                    definition,
                    valid,
                    invalid_message: if definition_valid {
                        "invalid equipped item"
                    } else {
                        "invalid for slot/class"
                    },
                };
                if let Some(fill) = header_fill {
                    egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            item_editor::draw_item_header(ui, header);
                        });
                } else {
                    item_editor::draw_item_header(ui, header);
                }

                if let Some(snapshot) = snapshot {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("SOID").weak());
                        ui.monospace(&snapshot.instance_soid_text);
                    });
                    if !snapshot.issues.is_empty() {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            snapshot.issues.join(" · "),
                        )
                        .on_hover_text(format!("Authored item: {}", snapshot.raw_item_text));
                        ui.label(
                            egui::RichText::new(
                                "Guided edits are disabled for this malformed equipped item.",
                            )
                            .weak(),
                        );
                    }
                }

                if !is_empty {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_enabled_ui(guided_editable, |ui| {
                            if let Some(level) = current_level {
                                for action in item_editor::draw_level_and_quantity(
                                    ui,
                                    ("equipment-numeric", character_index, slot),
                                    NumericItemFields {
                                        level: Some(level),
                                        quantity: None,
                                        quantity_max: None,
                                    },
                                ) {
                                    if let ItemEditorAction::SetLevel { level } = action {
                                        self.select_equipment_level(character_index, slot, level);
                                    }
                                }
                            } else {
                                ui.label("Power");
                                ui.label(egui::RichText::new("<invalid or missing>").weak());
                            }
                        });

                        ui.add_space(8.0);
                        ui.add_enabled_ui(guided_editable && flags_editable, |ui| {
                            let mut locked = current_flags.unwrap_or_default()
                                & super::inventory::INVENTORY_FLAG_LOCKED
                                != 0;
                            if ui.checkbox(&mut locked, "Locked").changed() {
                                self.select_equipment_flags(
                                    character_index,
                                    slot,
                                    super::inventory::set_inventory_locked_flag(
                                        current_flags,
                                        locked,
                                    ),
                                );
                            }
                        });
                    });
                }
                let key = format!("{id_scope}:{character_index}:{slot}");
                let picker_action = {
                    let manifest = &self.manifest;
                    let show_dummy_items = self.show_dummy_items;
                    let query = self.searches.entry(key.clone()).or_default();
                    ui.add_enabled_ui(guided_editable, |ui| {
                        item_editor::draw_definition_picker(
                            ui,
                            ("equipment-definition", id_scope, character_index, slot),
                            query,
                            PickerHeight {
                                min: ITEM_PICKER_MIN_HEIGHT,
                                max: ITEM_PICKER_MAX_HEIGHT,
                            },
                            |query_value| {
                                let candidates = if query_value.trim().is_empty() {
                                    manifest.browse(bucket, class_type, show_dummy_items)
                                } else {
                                    manifest.search(
                                        query_value,
                                        bucket,
                                        class_type,
                                        show_dummy_items,
                                    )
                                };
                                let needle = query_value.to_lowercase();
                                let definitions =
                                    equipment_definition_choices(candidates, query_value);
                                let show_empty_weapon = WEAPON_SLOTS.contains(&slot)
                                    && (query_value.trim().is_empty()
                                        || "empty weapon".contains(&needle));
                                DefinitionPickerChoices {
                                    definitions,
                                    clear: show_empty_weapon.then(|| ClearDefinitionChoice {
                                        label: "Empty weapon".to_owned(),
                                        tooltip: "Sets this equipment slot to empty.".to_owned(),
                                        selected: is_empty,
                                    }),
                                    empty_message: "No compatible installed items found".to_owned(),
                                }
                            },
                        )
                    })
                    .inner
                };
                match picker_action {
                    Some(ItemEditorAction::ClearDefinition) => {
                        self.empty_weapon(character_index, slot);
                        self.searches.insert(key.clone(), String::new());
                    }
                    Some(ItemEditorAction::SetDefinition { hash }) => {
                        if let Some(item) = self.manifest.get_for_bucket(hash, bucket).cloned() {
                            if slot == "subclass" {
                                self.select_subclass_item(character_index, &item);
                            } else {
                                self.select_item(character_index, slot, &item);
                            }
                            self.searches.insert(key.clone(), String::new());
                        }
                    }
                    _ => {}
                }

                if let Some(item) = &current {
                    let plugs_value = self
                        .characters()
                        .and_then(|chars| chars.get(character_index))
                        .and_then(|ch| ch.pointer(&format!("/equipment/{slot}/plugs")));
                    let (current_plugs, native_defaults) =
                        displayed_plugs(plugs_value, &item.default_plugs);
                    if !item.sockets.is_empty() || !current_plugs.is_empty() {
                        let title = if native_defaults {
                            format!("Plugs ({}, native defaults)", current_plugs.len())
                        } else {
                            format!("Plugs ({})", current_plugs.len())
                        };
                        egui::CollapsingHeader::new(title)
                            .id_salt(("equipment-plugs", id_scope, character_index, slot))
                            .show(ui, |ui| {
                                let socket_count = item.sockets.len().max(current_plugs.len());
                                // A plug's array index is part of the Sunrise save schema.
                                // Keep sockets in that exact order even when a label is unknown.
                                for socket_index in 0..socket_count {
                                    let current_hash = current_plugs
                                        .get(socket_index)
                                        .and_then(parse_unsigned_value);
                                    let native_default =
                                        native_plug_default(&item.default_plugs, socket_index);
                                    let allowed = item
                                        .sockets
                                        .get(socket_index)
                                        .map(|socket| match self.plug_selection_mode {
                                            PlugSelectionMode::Supported => {
                                                self.manifest.socket_options(socket)
                                            }
                                            PlugSelectionMode::MatchingSocketType => self
                                                .manifest
                                                .socket_type_options(socket.socket_type),
                                            PlugSelectionMode::AnyPlug => {
                                                self.manifest.all_plug_options()
                                            }
                                        })
                                        .unwrap_or_default();
                                    let current_label = current_hash.map_or_else(
                                        || "None".to_owned(),
                                        |hash| self.manifest.plug_label(hash),
                                    );
                                    let show_plug_types =
                                        self.plug_selection_mode == PlugSelectionMode::AnyPlug;
                                    let choices = allowed
                                        .iter()
                                        .map(|hash| PlugChoice {
                                            hash: *hash,
                                            label: self.manifest.plug_label(*hash),
                                            type_name: if show_plug_types {
                                                self.manifest
                                                    .plug_type_name(*hash)
                                                    .unwrap_or_default()
                                                    .to_owned()
                                            } else {
                                                String::new()
                                            },
                                        })
                                        .collect::<Vec<_>>();
                                    let searchable = choices.len() > 12;
                                    let plug_search_key = format!(
                                        "plug-search:{id_scope}:{character_index}:{slot}:{socket_index}"
                                    );
                                    let mut plug_query = self
                                        .plug_searches
                                        .get(&plug_search_key)
                                        .cloned()
                                        .unwrap_or_default();
                                    let socket_label = item.sockets.get(socket_index).map_or_else(
                                        || format!("Socket {}", socket_index + 1),
                                        |socket| socket.display_label(socket_index),
                                    );
                                    let snapshot = PlugPickerSnapshot {
                                        socket_index,
                                        socket_label,
                                        current_hash,
                                        current_label,
                                        native_default,
                                        native_default_label: match native_default {
                                            Some(NativePlugDefault::Plug(hash)) => {
                                                Some(self.manifest.plug_label(hash))
                                            }
                                            _ => None,
                                        },
                                        choices,
                                        show_types: show_plug_types,
                                    };
                                    let action = ui
                                        .add_enabled_ui(guided_editable, |ui| {
                                            item_editor::draw_plug_picker(
                                                ui,
                                                (
                                                    "equipment-plug",
                                                    id_scope,
                                                    character_index,
                                                    slot,
                                                    socket_index,
                                                ),
                                                &mut plug_query,
                                                &snapshot,
                                                PickerHeight {
                                                    min: PLUG_PICKER_MIN_HEIGHT,
                                                    max: PLUG_PICKER_MAX_HEIGHT,
                                                },
                                            )
                                        })
                                        .inner;
                                    if let Some(ItemEditorAction::SetPlug { socket_index, hash }) =
                                        action
                                    {
                                        self.select_plug(
                                            character_index,
                                            slot,
                                            socket_index,
                                            &snapshot.socket_label,
                                            &item.default_plugs,
                                            hash,
                                        );
                                    }
                                    if searchable {
                                        self.plug_searches.insert(plug_search_key, plug_query);
                                    } else {
                                        self.plug_searches.remove(&plug_search_key);
                                    }
                                }
                            });
                    }
                }
            });
        });
    }
}

fn equipment_definition_choices<'a>(
    candidates: impl IntoIterator<Item = &'a ItemDef>,
    query: &str,
) -> Vec<DefinitionChoice> {
    let needle = query.to_lowercase();
    candidates
        .into_iter()
        .filter(|item| {
            query.trim().is_empty()
                || item.name.to_lowercase().contains(&needle)
                || format_hash(item.hash).to_lowercase().contains(&needle)
        })
        .map(|item| DefinitionChoice {
            hash: item.hash,
            label: item.label(),
            group: None,
        })
        .collect()
}

pub(super) fn collect_class_armor_defaults(
    document: &Value,
) -> HashMap<u64, HashMap<String, Value>> {
    let mut defaults = HashMap::new();
    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return defaults;
    };
    for character in characters {
        let Some(class_type) = character.get("class").and_then(Value::as_u64) else {
            continue;
        };
        let Some(equipment) = character.get("equipment").and_then(Value::as_object) else {
            continue;
        };
        let armor = ARMOR_SLOTS
            .iter()
            .filter_map(|slot| {
                equipment
                    .get(*slot)
                    .cloned()
                    .map(|item| ((*slot).into(), item))
            })
            .collect();
        defaults.entry(class_type).or_insert(armor);
    }
    defaults
}

pub(super) fn restore_class_armor(
    character: &mut serde_json::Map<String, Value>,
    defaults: &HashMap<String, Value>,
) -> bool {
    let Some(equipment) = character
        .get_mut("equipment")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut changed = false;
    for &slot in ARMOR_SLOTS {
        let Some(replacement) = defaults.get(slot) else {
            continue;
        };
        let Some(replacement) = replacement.as_object() else {
            continue;
        };
        let Some(existing) = equipment.get(slot).and_then(Value::as_object) else {
            continue;
        };
        let mut merged = existing.clone();
        for (key, value) in replacement {
            if key != "instance_soid" {
                merged.insert(key.clone(), value.clone());
            }
        }
        let merged = Value::Object(merged);
        if equipment.get(slot) != Some(&merged) {
            equipment.insert(slot.into(), merged);
            changed = true;
        }
    }
    changed
}

pub(super) fn combo_u64(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[(u64, &str)]) {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(160.0)
        .show_ui(ui, |ui| {
            for &(candidate, name) in choices {
                ui.selectable_value(value, candidate, name);
            }
        });
}

pub(super) fn ability_combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u64,
    choices: &[AbilityChoice],
) {
    let selected = choices
        .iter()
        .find(|choice| choice.entry == *value)
        .map_or_else(
            || format!("Unknown entry {}", *value),
            |choice| choice.name.clone(),
        );
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(260.0)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(value, choice.entry, &choice.name);
            }
            if choices.is_empty() {
                ui.label("No named choices found for this subclass");
            }
        });
}

pub(super) const fn default_subclass_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Sunbreaker",
        1 => "Nightstalker",
        2 => "Dawnblade",
        _ => "",
    }
}

pub(super) fn selected_attunement_index(
    abilities: &catalog::AbilityOptions,
    super_ability: u64,
    melee: u64,
) -> usize {
    let paths = &abilities.attunements;
    paths
        .iter()
        .position(|path| {
            path.melee.entry == melee
                && path
                    .super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
        })
        .or_else(|| {
            if super_ability == 10 {
                None
            } else {
                paths.iter().position(|path| {
                    path.super_abilities
                        .iter()
                        .chain(path.perks.iter())
                        .any(|choice| choice.entry == super_ability)
                })
            }
        })
        .or_else(|| paths.iter().position(|path| path.melee.entry == melee))
        .or_else(|| {
            paths.iter().position(|path| {
                path.super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
            })
        })
        .unwrap_or(0)
}

pub(super) fn default_ability_values(
    class_type: u64,
    abilities: &catalog::AbilityOptions,
    settings_schema: Option<u64>,
) -> (u64, u64, u64, u64, u64) {
    let pick = |choices: &[AbilityChoice], preferred: u64| {
        choices
            .iter()
            .find(|choice| choice.entry == preferred)
            .or_else(|| choices.first())
            .map_or(preferred, |choice| choice.entry)
    };
    let movement = match class_type {
        0 if settings_schema.is_some_and(|version| version >= 3) => 6,
        0 | 2 => 5,
        1 => 6,
        _ => 4,
    };
    (
        pick(&abilities.movement, movement),
        pick(&abilities.grenade, 7),
        pick(&abilities.super_ability, 10),
        pick(&abilities.melee, 11),
        pick(&abilities.class_ability, 2),
    )
}

pub(super) const fn class_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Invalid class",
    }
}

pub(super) fn parse_hash(text: &str) -> Option<u64> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 16 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

pub(super) fn parse_unsigned_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_hash))
}

/// Returns the present, non-null equipment rows for one character in [`SLOTS`] order.
///
/// A malformed row is represented by an [`EquippedItemSnapshot`] with issues instead
/// of being discarded. Errors are reserved for an unusable character/equipment path.
pub(super) fn equipped_item_snapshots(
    document: &Value,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    let characters = document
        .pointer("/state/characters")
        .ok_or("Missing /state/characters")?
        .as_array()
        .ok_or("/state/characters must be an array")?;
    let character = characters
        .get(character_index)
        .ok_or_else(|| format!("Missing character at index {character_index}"))?
        .as_object()
        .ok_or_else(|| format!("Character {character_index} must be an object"))?;
    let Some(equipment_value) = character.get("equipment") else {
        return Ok(Vec::new());
    };
    let equipment = equipment_value
        .as_object()
        .ok_or_else(|| format!("Character {character_index} equipment must be an object"))?;

    Ok(SLOTS
        .iter()
        .filter_map(|&(slot, slot_label, bucket_hash)| {
            let value = equipment.get(slot)?;
            (!value.is_null()).then(|| equipped_item_snapshot(slot, slot_label, bucket_hash, value))
        })
        .collect())
}

fn equipped_item_snapshot(
    slot: &'static str,
    slot_label: &'static str,
    bucket_hash: u64,
    value: &Value,
) -> EquippedItemSnapshot {
    const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

    let raw_item_text = compact_json_text(value);
    let Some(item) = value.as_object() else {
        return EquippedItemSnapshot {
            slot,
            slot_label,
            bucket_hash,
            raw_item_text: raw_item_text.clone(),
            definition_hash: None,
            definition_text: "<missing>".to_owned(),
            instance_soid: None,
            instance_soid_text: "<missing>".to_owned(),
            level: None,
            quantity: None,
            plugs: EquippedItemPlugs::Malformed(raw_item_text),
            issues: vec!["equipment row must be an object".to_owned()],
        };
    };

    let mut issues = Vec::new();
    const KNOWN_MEMBERS: &[&str] = &[
        "instance_soid",
        "definition_hash",
        "level",
        "quantity",
        "plugs",
        "flags",
    ];
    for member in item.keys() {
        if !KNOWN_MEMBERS.contains(&member.as_str()) {
            issues.push(format!("unknown item member {member}"));
        }
    }

    let definition_value = item.get("definition_hash");
    let definition_hash = definition_value.and_then(parse_unsigned_value);
    let definition_text =
        definition_hash.map_or_else(|| field_display_text(definition_value), format_hash);
    match (definition_value, definition_hash) {
        (None, _) => issues.push("missing definition_hash".to_owned()),
        (Some(_), None) => {
            issues.push("definition_hash must be an unsigned integer or a 0x hex string".to_owned())
        }
        (_, Some(hash)) if u32::try_from(hash).is_err() => {
            issues.push("definition_hash must fit in an unsigned 32-bit value".to_owned());
        }
        (_, Some(NO_DEFINITION_HASH)) => {
            issues.push("definition_hash is the engine no-definition sentinel".to_owned());
        }
        _ => {}
    }

    let soid_value = item.get("instance_soid");
    let instance_soid = soid_value.and_then(parse_unsigned_value);
    let instance_soid_text = instance_soid.map_or_else(
        || field_display_text(soid_value),
        |soid| format!("0x{soid:016X}"),
    );
    match (soid_value, instance_soid) {
        (None, _) => issues.push("missing instance_soid".to_owned()),
        (Some(_), None) => {
            issues.push("instance_soid must be an unsigned integer or a 0x hex string".to_owned());
        }
        (_, Some(0)) => issues.push("instance_soid must not be zero".to_owned()),
        _ => {}
    }

    let level = item.get("level").and_then(Value::as_i64);
    match item.get("level") {
        None => issues.push("missing level".to_owned()),
        Some(_) if level.is_none() => {
            issues.push("level must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !level.is_some_and(|value| (0..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("level must be a non-negative signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let quantity = item.get("quantity").and_then(Value::as_i64);
    match item.get("quantity") {
        None => issues.push("missing quantity".to_owned()),
        Some(_) if quantity.is_none() => {
            issues.push("quantity must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !quantity.is_some_and(|value| (1..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("quantity must be a positive signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let plugs = equipped_item_plugs(item.get("plugs"), &mut issues, NO_DEFINITION_HASH);

    if let Some(flags) = item.get("flags")
        && parse_unsigned_value(flags)
            .is_none_or(|flags| flags > u64::from(super::inventory::INVENTORY_FLAG_MASK))
    {
        issues.push(format!(
            "flags must be between 0 and {}",
            super::inventory::INVENTORY_FLAG_MASK
        ));
    }

    EquippedItemSnapshot {
        slot,
        slot_label,
        bucket_hash,
        raw_item_text,
        definition_hash,
        definition_text,
        instance_soid,
        instance_soid_text,
        level,
        quantity,
        plugs,
        issues,
    }
}

fn equipped_item_plugs(
    value: Option<&Value>,
    issues: &mut Vec<String>,
    no_definition_hash: u64,
) -> EquippedItemPlugs {
    let Some(value) = value else {
        issues.push("missing plugs".to_owned());
        return EquippedItemPlugs::Missing;
    };
    if value.is_null() {
        return EquippedItemPlugs::NativeDefaults;
    }
    let Some(plugs) = value.as_array() else {
        let raw = compact_json_text(value);
        issues.push("plugs must be null or an array".to_owned());
        return EquippedItemPlugs::Malformed(raw);
    };
    if plugs.len() > super::inventory::MAX_ITEM_PLUGS {
        issues.push(format!(
            "plugs cannot contain more than {} entries",
            super::inventory::MAX_ITEM_PLUGS
        ));
    }
    let values = plugs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return EquippedPlugValue::Empty;
            }
            let Some(hash) = parse_unsigned_value(value) else {
                let raw = compact_json_text(value);
                issues.push(format!(
                    "plug {index} must be null, an unsigned integer, or a 0x hex string"
                ));
                return EquippedPlugValue::Malformed(raw);
            };
            if u32::try_from(hash).is_err() {
                issues.push(format!(
                    "plug {index} hash must fit in an unsigned 32-bit value"
                ));
            } else if hash == no_definition_hash {
                issues.push(format!(
                    "plug {index} hash is the engine no-definition sentinel"
                ));
            }
            EquippedPlugValue::Hash(hash)
        })
        .collect();
    EquippedItemPlugs::Authored(values)
}

fn field_display_text(value: Option<&Value>) -> String {
    match value {
        None => "<missing>".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => compact_json_text(value),
    }
}

fn compact_json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

pub(super) fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
    defaults
        .iter()
        .map(|plug| plug.clone().map_or(Value::Null, Value::String))
        .collect()
}

pub(super) fn equipment_slot_label(slot: &str) -> &str {
    SLOTS
        .iter()
        .find_map(|(name, label, _)| (*name == slot).then_some(*label))
        .unwrap_or(slot)
}

pub(super) fn next_instance_soid(document: &Value) -> Option<u64> {
    super::inventory::allocate_instance_soid(document).ok()
}

pub(super) fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| {
            equipment
                .values()
                .filter_map(|item| {
                    item.get("level")
                        .and_then(Value::as_i64)
                        .filter(|level| (1..=i64::from(i32::MAX)).contains(level))
                })
                .max()
        })
        .unwrap_or(106)
}

pub(super) fn equip_definition(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if u32::try_from(definition_hash).is_err() {
        return Err(format!(
            "Cannot equip an invalid definition hash in the {} slot",
            equipment_slot_label(slot)
        ));
    }
    let current = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| equipment.get(slot));
    let replacement = match current {
        Some(Value::Object(_)) => None,
        Some(Value::Null) | None => {
            let instance_soid = next_instance_soid(document)
                .ok_or("Could not allocate a unique instance SOID for the selected item")?;
            Some(serde_json::json!({
                "instance_soid": format!("0x{instance_soid:016X}"),
                "definition_hash": format_hash(definition_hash),
                "level": inferred_item_level(document, character_index),
                "quantity": 1,
                "plugs": default_plug_values(default_plugs),
            }))
        }
        Some(_) => {
            return Err(format!(
                "The {} slot must be an object or null before it can be changed",
                equipment_slot_label(slot)
            ));
        }
    };

    let equipment = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    if let Some(replacement) = replacement {
        equipment.insert(slot.into(), replacement);
        return Ok(());
    }
    let equipped = equipment
        .get_mut(slot)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Missing equipment slot: {slot}"))?;
    equipped.insert(
        "definition_hash".into(),
        Value::String(format_hash(definition_hash)),
    );
    equipped.insert(
        "plugs".into(),
        Value::Array(default_plug_values(default_plugs)),
    );
    Ok(())
}

/// Equips a subclass and resets the character's coordinated ability fields as one edit.
///
/// Work is performed on a clone so a malformed equipment or character path cannot leave
/// the subclass and ability selections out of sync.
pub(super) fn equip_subclass_with_default_abilities(
    document: &mut Value,
    character_index: usize,
    item: &ItemDef,
) -> Result<(), String> {
    let subclass_bucket = SLOTS
        .iter()
        .find_map(|(slot, _, bucket)| (*slot == "subclass").then_some(*bucket))
        .expect("SLOTS must contain the subclass slot");
    if item.bucket_hash != subclass_bucket {
        return Err("The selected definition is not a subclass".to_owned());
    }
    let class_type = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("class"))
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Character {} has no valid class", character_index + 1))?;
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let mut candidate = document.clone();
    let defaults = default_ability_values(
        class_type,
        &item.abilities,
        game_settings::schema_version(&candidate),
    );

    equip_definition(
        &mut candidate,
        character_index,
        "subclass",
        item.hash,
        &item.default_plugs,
    )?;
    let character = candidate
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Character {} must be an object", character_index + 1))?;
    for (field, value) in [
        ("movement_ability", defaults.0),
        ("grenade_ability", defaults.1),
        ("super_ability", defaults.2),
        ("melee_ability", defaults.3),
        ("class_ability", defaults.4),
    ] {
        character.insert(field.to_owned(), Value::from(value));
    }
    *document = candidate;
    Ok(())
}

pub(super) fn set_equipment_item_level(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    level: i64,
) -> Result<(), String> {
    if !(0..=i64::from(i32::MAX)).contains(&level) {
        return Err("Equipment level must be a non-negative signed 32-bit integer".to_owned());
    }
    equipment_item_object_mut(document, character_index, slot)?
        .insert("level".to_owned(), Value::from(level));
    Ok(())
}

pub(super) fn set_equipment_item_flags(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    if !super::inventory::schema_mode(document).can_mutate_equipment_flags() {
        return Err(format!(
            "Equipment flags require a writable settings schema {} or newer",
            super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
        ));
    }
    if flags.is_some_and(|flags| flags > super::inventory::INVENTORY_FLAG_MASK) {
        return Err(format!(
            "Equipment flags must be between 0 and {}",
            super::inventory::INVENTORY_FLAG_MASK
        ));
    }
    let item = equipment_item_object_mut(document, character_index, slot)?;
    if let Some(flags) = flags {
        item.insert("flags".to_owned(), Value::from(flags));
    } else {
        item.remove("flags");
    }
    Ok(())
}

fn equipment_item_object_mut<'a>(
    document: &'a mut Value,
    character_index: usize,
    slot: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    if !SLOTS.iter().any(|(known_slot, _, _)| *known_slot == slot) {
        return Err(format!("Unknown equipment slot: {slot}"));
    }
    document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .and_then(|equipment| equipment.get_mut(slot))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!(
                "The {} slot must contain an item object before it can be edited",
                equipment_slot_label(slot)
            )
        })
}

pub(super) fn set_weapon_slot_empty(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty; {} was not changed",
            equipment_slot_label(slot)
        ));
    }
    let equipment = document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .ok_or("The selected character has no equipment object")?;
    match equipment.get(slot) {
        Some(Value::Object(_) | Value::Null) | None => {
            equipment.insert(slot.into(), Value::Null);
            Ok(())
        }
        Some(_) => Err(format!(
            "The {} slot contains unexpected data and was not changed",
            equipment_slot_label(slot)
        )),
    }
}

pub(super) fn displayed_plugs(
    plugs: Option<&Value>,
    defaults: &[Option<String>],
) -> (Vec<Value>, bool) {
    match plugs {
        Some(Value::Array(plugs)) => (plugs.clone(), false),
        Some(Value::Null) => (default_plug_values(defaults), true),
        _ => (Vec::new(), false),
    }
}

pub(super) fn materialize_authored_plugs<'a>(
    plugs: &'a mut Value,
    defaults: &[Option<String>],
) -> Option<&'a mut Vec<Value>> {
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    plugs.as_array_mut()
}

#[cfg(test)]
pub(super) fn picker_list_height(
    row_count: usize,
    row_height: f32,
    min_height: f32,
    max_height: f32,
) -> f32 {
    item_editor::picker_list_height(row_count, row_height, min_height, max_height)
}

pub(super) fn native_plug_default(
    defaults: &[Option<String>],
    socket_index: usize,
) -> Option<NativePlugDefault> {
    match defaults.get(socket_index)? {
        Some(hash) => parse_hash(hash).map(NativePlugDefault::Plug),
        None => Some(NativePlugDefault::Empty),
    }
}

#[cfg(test)]
mod snapshot_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn equipment_picker_choice_assembly_keeps_more_than_five_hundred_items() {
        let items = (0_u64..620)
            .map(|index| ItemDef {
                hash: 10_000 + index,
                name: format!("Browse item {index:04}"),
                type_name: "Test weapon".into(),
                bucket_hash: 1_498_876_634,
                class_type: 3,
                default_plugs: Vec::new(),
                sockets: Vec::new(),
                abilities: catalog::AbilityOptions::default(),
            })
            .collect::<Vec<_>>();

        let choices = equipment_definition_choices(items.iter(), "");
        assert_eq!(choices.len(), 620);
        assert_eq!(choices.first().unwrap().hash, 10_000);
        assert_eq!(choices.last().unwrap().hash, 10_619);
    }

    #[test]
    fn equipped_snapshots_follow_slot_order_and_skip_missing_or_null_rows() {
        let document = json!({
            "state": {
                "characters": [{
                    "equipment": {
                        "emote": true,
                        "subclass": {
                            "instance_soid": "0x0000000000000004",
                            "definition_hash": "0x00000005",
                            "level": 75,
                            "quantity": 1,
                            "plugs": []
                        },
                        "energy": null,
                        "helmet": {
                            "instance_soid": 3,
                            "definition_hash": 4,
                            "level": 76,
                            "quantity": 1,
                            "plugs": [null, "0x00000006", 7, "not-a-hash"]
                        },
                        "kinetic": {
                            "instance_soid": "0x0000000000000001",
                            "definition_hash": "0x00000002",
                            "level": 75,
                            "quantity": 1,
                            "plugs": null
                        }
                    }
                }]
            }
        });

        let snapshots = equipped_item_snapshots(&document, 0).unwrap();
        assert_eq!(
            snapshots
                .iter()
                .map(|snapshot| snapshot.slot)
                .collect::<Vec<_>>(),
            ["kinetic", "helmet", "subclass", "emote"]
        );

        let kinetic = &snapshots[0];
        assert_eq!(kinetic.slot_label, "Kinetic");
        assert_eq!(kinetic.bucket_hash, 1_498_876_634);
        assert_eq!(kinetic.definition_hash, Some(2));
        assert_eq!(kinetic.definition_text, "0x00000002");
        assert_eq!(kinetic.instance_soid, Some(1));
        assert_eq!(kinetic.instance_soid_text, "0x0000000000000001");
        assert_eq!(kinetic.level, Some(75));
        assert_eq!(kinetic.quantity, Some(1));
        assert_eq!(kinetic.plugs, EquippedItemPlugs::NativeDefaults);
        assert!(kinetic.issues.is_empty());

        assert_eq!(
            snapshots[1].plugs,
            EquippedItemPlugs::Authored(vec![
                EquippedPlugValue::Empty,
                EquippedPlugValue::Hash(6),
                EquippedPlugValue::Hash(7),
                EquippedPlugValue::Malformed("\"not-a-hash\"".to_owned()),
            ])
        );
        assert!(
            snapshots[1]
                .issues
                .iter()
                .any(|issue| issue.contains("plug 3"))
        );

        assert_eq!(snapshots[2].slot, "subclass");
        assert_eq!(snapshots[3].raw_item_text, "true");
        assert_eq!(snapshots[3].definition_hash, None);
        assert!(matches!(
            snapshots[3].plugs,
            EquippedItemPlugs::Malformed(ref raw) if raw == "true"
        ));
        assert_eq!(snapshots[3].issues, ["equipment row must be an object"]);
    }

    #[test]
    fn equipped_snapshots_retain_invalid_fields_and_report_issues() {
        let document = json!({
            "state": {
                "characters": [{
                    "equipment": {
                        "kinetic": {
                            "instance_soid": 0,
                            "definition_hash": "invalid",
                            "level": -1,
                            "quantity": 0,
                            "plugs": {"unexpected": true}
                        }
                    }
                }]
            }
        });

        let snapshots = equipped_item_snapshots(&document, 0).unwrap();
        let snapshot = &snapshots[0];
        assert_eq!(snapshot.definition_hash, None);
        assert_eq!(snapshot.definition_text, "invalid");
        assert_eq!(snapshot.instance_soid, Some(0));
        assert_eq!(snapshot.level, Some(-1));
        assert_eq!(snapshot.quantity, Some(0));
        assert_eq!(
            snapshot.plugs,
            EquippedItemPlugs::Malformed("{\"unexpected\":true}".to_owned())
        );
        assert_eq!(snapshot.issues.len(), 5);
    }

    #[test]
    fn equipped_snapshots_reject_an_unusable_equipment_path() {
        let missing_characters = json!({"state": {}});
        assert!(equipped_item_snapshots(&missing_characters, 0).is_err());

        let missing_character = json!({"state": {"characters": []}});
        assert!(equipped_item_snapshots(&missing_character, 0).is_err());

        let missing_equipment = json!({"state": {"characters": [{}]}});
        assert_eq!(
            equipped_item_snapshots(&missing_equipment, 0).unwrap(),
            Vec::new()
        );

        let malformed_equipment = json!({"state": {"characters": [{"equipment": []}]}});
        assert!(equipped_item_snapshots(&malformed_equipment, 0).is_err());
    }

    #[test]
    fn inferred_item_level_uses_the_highest_positive_equipped_level() {
        let document = json!({
            "state": {
                "characters": [{
                    "equipment": {
                        "ghost": {"level": 0},
                        "kinetic": {"level": 75},
                        "energy": {"level": 106},
                        "helmet": {"level": -1}
                    }
                }]
            }
        });
        assert_eq!(inferred_item_level(&document, 0), 106);

        let unpowered = json!({
            "state": {"characters": [{"equipment": {"ghost": {"level": 0}}}]}
        });
        assert_eq!(inferred_item_level(&unpowered, 0), 106);
    }

    #[test]
    fn semantic_equipment_field_edits_do_not_touch_stored_inventory() {
        let mut document = json!({
            "version": 6,
            "state": {
                "characters": [{
                    "equipment": {
                        "kinetic": {"level": 200, "flags": 2},
                        "energy": {"level": 106}
                    },
                    "inventory": [{"level": 75, "flags": 1}]
                }]
            }
        });

        set_equipment_item_level(&mut document, 0, "kinetic", 75).unwrap();
        let locked = super::super::inventory::set_inventory_locked_flag(Some(2), true);
        set_equipment_item_flags(&mut document, 0, "kinetic", locked).unwrap();

        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/level"),
            Some(&json!(75))
        );
        assert_eq!(
            document.pointer("/state/characters/0/equipment/kinetic/flags"),
            Some(&json!(3))
        );
        assert_eq!(
            document.pointer("/state/characters/0/equipment/energy/level"),
            Some(&json!(106))
        );
        assert_eq!(
            document.pointer("/state/characters/0/inventory/0"),
            Some(&json!({"level": 75, "flags": 1}))
        );

        let unchanged = document.clone();
        assert!(set_equipment_item_level(&mut document, 0, "future_slot", 75).is_err());
        assert_eq!(document, unchanged);
        assert!(set_equipment_item_flags(&mut document, 0, "kinetic", Some(4)).is_err());
        assert_eq!(document, unchanged);
    }

    #[test]
    fn equipment_flag_mutation_follows_schema_introduction_and_is_atomic() {
        for version in 2..=6 {
            let mut document = json!({
                "version": version,
                "state": {
                    "characters": [{
                        "equipment": {
                            "kinetic": {
                                "level": 106,
                                "flags": 2,
                                "future": {"preserved": true}
                            }
                        }
                    }]
                }
            });
            let before = document.clone();
            let result = set_equipment_item_flags(&mut document, 0, "kinetic", Some(3));

            if version < super::super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION {
                assert!(
                    result.is_err(),
                    "schema {version} unexpectedly allowed flags"
                );
                assert_eq!(document, before);
            } else {
                result.unwrap();
                assert_eq!(
                    document.pointer("/state/characters/0/equipment/kinetic/flags"),
                    Some(&json!(3))
                );
                assert_eq!(
                    document.pointer("/state/characters/0/equipment/kinetic/future/preserved"),
                    Some(&Value::Bool(true))
                );
            }
        }
    }

    #[test]
    fn subclass_equipping_updates_definition_and_default_abilities_atomically() {
        let mut document = json!({
            "version": 6,
            "state": {
                "characters": [{
                    "class": 0,
                    "movement_ability": 99,
                    "grenade_ability": 99,
                    "super_ability": 99,
                    "melee_ability": 99,
                    "class_ability": 99,
                    "equipment": {
                        "subclass": {
                            "instance_soid": "0x0000000000000001",
                            "definition_hash": "0x00000001",
                            "level": 0,
                            "quantity": 1,
                            "plugs": null
                        }
                    }
                }]
            }
        });
        let choice = |entry, name: &str| AbilityChoice {
            entry,
            name: name.to_owned(),
        };
        let item = ItemDef {
            hash: 42,
            name: "Test subclass".to_owned(),
            type_name: "Subclass".to_owned(),
            bucket_hash: 3_284_755_031,
            class_type: 0,
            default_plugs: vec![Some("0x0000000A".to_owned()), None],
            sockets: Vec::new(),
            abilities: catalog::AbilityOptions {
                movement: vec![choice(6, "Lift")],
                grenade: vec![choice(7, "Grenade")],
                super_ability: vec![choice(10, "Super")],
                melee: vec![choice(11, "Melee")],
                class_ability: vec![choice(2, "Barricade")],
                attunements: Vec::new(),
            },
        };

        equip_subclass_with_default_abilities(&mut document, 0, &item).unwrap();
        assert_eq!(
            document.pointer("/state/characters/0/equipment/subclass/definition_hash"),
            Some(&json!("0x0000002A"))
        );
        assert_eq!(
            document.pointer("/state/characters/0/equipment/subclass/plugs"),
            Some(&json!(["0x0000000A", null]))
        );
        for (field, expected) in [
            ("movement_ability", 6),
            ("grenade_ability", 7),
            ("super_ability", 10),
            ("melee_ability", 11),
            ("class_ability", 2),
        ] {
            assert_eq!(
                document.pointer(&format!("/state/characters/0/{field}")),
                Some(&json!(expected))
            );
        }

        let unchanged = document.clone();
        let mut wrong_bucket = item.clone();
        wrong_bucket.bucket_hash = 0;
        assert!(equip_subclass_with_default_abilities(&mut document, 0, &wrong_bucket).is_err());
        assert_eq!(document, unchanged);

        let mut wrong_class = item.clone();
        wrong_class.class_type = 1;
        assert!(equip_subclass_with_default_abilities(&mut document, 0, &wrong_class).is_err());
        assert_eq!(document, unchanged);

        let mut malformed = json!({
            "version": 6,
            "state": {"characters": [{"class": 0, "equipment": []}]}
        });
        let original = malformed.clone();
        assert!(equip_subclass_with_default_abilities(&mut malformed, 0, &item).is_err());
        assert_eq!(malformed, original);
    }

    #[test]
    fn arcstrider_and_sentinel_subclass_edits_keep_the_base_super_lane() {
        let choice = |entry, name: &str| AbilityChoice {
            entry,
            name: name.to_owned(),
        };
        for (hash, class_type, name) in
            [(0x4F91_DC97, 1, "Arcstrider"), (0xC99B_33E9, 0, "Sentinel")]
        {
            let mut document = json!({
                "version": 6,
                "state": {
                    "characters": [{
                        "soid": "0x9EAA300200100100",
                        "class": class_type,
                        "movement_ability": 4,
                        "grenade_ability": 9,
                        "super_ability": 20,
                        "melee_ability": 21,
                        "class_ability": 3,
                        "equipment": {
                            "subclass": {
                                "instance_soid": "0x4000000000000001",
                                "definition_hash": "0x00000001",
                                "level": 0,
                                "quantity": 1,
                                "plugs": null
                            }
                        }
                    }]
                }
            });
            let item = ItemDef {
                hash,
                name: name.to_owned(),
                type_name: "Subclass".to_owned(),
                bucket_hash: 3_284_755_031,
                class_type,
                default_plugs: Vec::new(),
                sockets: Vec::new(),
                abilities: catalog::AbilityOptions {
                    movement: vec![choice(4, "Movement"), choice(6, "Preferred movement")],
                    grenade: vec![choice(7, "Grenade")],
                    // Put the Forsaken middle-path entries first to prove the helper still
                    // selects the base-super/base-melee pair for these guard subclasses.
                    super_ability: vec![choice(20, "Guard"), choice(10, "Base super")],
                    melee: vec![choice(21, "Middle melee"), choice(11, "Base melee")],
                    class_ability: vec![choice(2, "Class ability")],
                    attunements: Vec::new(),
                },
            };

            equip_subclass_with_default_abilities(&mut document, 0, &item).unwrap();
            assert_eq!(
                document.pointer("/state/characters/0/super_ability"),
                Some(&json!(10)),
                "{name} selected the wrong super lane"
            );
            assert_eq!(
                document.pointer("/state/characters/0/melee_ability"),
                Some(&json!(11)),
                "{name} selected the wrong melee lane"
            );
            assert_eq!(
                super::super::settings::validate_characters(&document),
                Ok(()),
                "{name} produced an invalid character"
            );
        }
    }
}
