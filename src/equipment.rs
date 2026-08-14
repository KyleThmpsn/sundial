use crate::catalog;

use super::*;

impl SundialApp {
    fn select_item(&mut self, character: usize, slot: &str, item: &ItemDef) {
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

    fn empty_weapon(&mut self, character: usize, slot: &str) {
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
        default_plugs: &[Option<String>],
        hash: Option<u64>,
    ) {
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
        self.set_status(format!("Updated {slot} socket {}", socket_index + 1), false);
    }

    pub(super) fn draw_character_fields(&mut self, ui: &mut egui::Ui, index: usize) {
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

        ui.heading(format!("Character {}", index + 1));
        ui.label(egui::RichText::new(soid).monospace().weak());
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

        let mut changed = false;
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
            if let Some(template) = armor_template.as_ref() {
                changed |= restore_class_armor(object, template);
            }
        }
        self.dirty |= changed;
        if let Some(subclass) = selected_subclass {
            self.select_item(index, "subclass", &subclass);
        }
    }

    pub(super) fn draw_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        let class_type = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.get("class"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        ui.add_space(14.0);
        ui.heading("Equipped loadout");
        ui.label("Search by item name or 0x hash. Choosing an item also installs its package-default plugs.");
        ui.checkbox(
            &mut self.allow_unsafe_plugs,
            "Allow any plug matching the socket type (unsafe)",
        );
        ui.checkbox(&mut self.show_dummy_items, "Show dummy items")
            .on_hover_text(
                "Includes display-only definitions that cannot normally be obtained in the game.",
            );
        if self.allow_unsafe_plugs {
            ui.colored_label(
                egui::Color32::from_rgb(255, 190, 80),
                "Warning: unsupported plug combinations may break items, corrupt the loadout, or crash Sunrise/Destiny 2.",
            );
        }
        ui.add_space(6.0);

        for &(slot, label, bucket) in SLOTS {
            if slot == "subclass" {
                continue;
            }
            let equipped_value = self
                .characters()
                .and_then(|chars| chars.get(character_index))
                .and_then(|ch| ch.pointer(&format!("/equipment/{slot}")))
                .cloned();
            let is_empty = equipped_value.as_ref().is_some_and(Value::is_null);
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
            let valid = is_empty
                || current.as_ref().is_some_and(|item| {
                    item.bucket_hash == bucket
                        && (item.class_type == 3 || item.class_type == class_type)
                });
            ui.push_id((character_index, slot), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(label).strong());
                        ui.add_space(6.0);
                        if is_empty {
                            ui.label(egui::RichText::new("Empty").weak());
                        } else {
                            match &current {
                                Some(item) => {
                                    ui.label(&item.name);
                                    ui.label(
                                        egui::RichText::new(&current_hash_text).monospace().weak(),
                                    );
                                }
                                None => {
                                    ui.colored_label(
                                        egui::Color32::LIGHT_RED,
                                        format!("Unknown item {current_hash_text}"),
                                    );
                                }
                            }
                        }
                        if !valid {
                            ui.colored_label(egui::Color32::LIGHT_RED, "invalid for slot/class");
                        }
                    });
                    let key = format!("{character_index}:{slot}");
                    let picker_response = {
                        let query = self.searches.entry(key.clone()).or_default();
                        ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Click to browse, or type an item name or hex hash…")
                                .desired_width(ui.available_width()),
                        )
                    };
                    if picker_response.clicked() || picker_response.changed() {
                        self.browsing.insert(key.clone());
                    }
                    let query_value = self.searches.get(&key).cloned().unwrap_or_default();
                    let is_browsing = self.browsing.contains(&key);
                    if !query_value.trim().is_empty() || is_browsing {
                        let candidates = if is_browsing {
                            self.manifest
                                .browse(bucket, class_type, self.show_dummy_items)
                        } else {
                            self.manifest.search(
                                &query_value,
                                bucket,
                                class_type,
                                self.show_dummy_items,
                            )
                        };
                        let needle = query_value.to_lowercase();
                        let results: Vec<ItemDef> = candidates
                            .into_iter()
                            .filter(|item| {
                                query_value.trim().is_empty()
                                    || item.name.to_lowercase().contains(&needle)
                                    || format_hash(item.hash).to_lowercase().contains(&needle)
                            })
                            .take(500)
                            .cloned()
                            .collect();
                        let show_empty_weapon = WEAPON_SLOTS.contains(&slot)
                            && (query_value.trim().is_empty() || "empty weapon".contains(&needle));
                        if results.is_empty() && !show_empty_weapon {
                            ui.label(
                                egui::RichText::new("No compatible installed items found").weak(),
                            );
                        } else {
                            egui::Frame::popup(ui.style())
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    egui::ScrollArea::vertical()
                                        .max_height(400.0)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            if show_empty_weapon {
                                                if ui
                                                    .selectable_label(is_empty, "Empty weapon")
                                                    .on_hover_text(
                                                        "Sets this equipment slot to empty.",
                                                    )
                                                    .clicked()
                                                {
                                                    self.empty_weapon(character_index, slot);
                                                    self.searches
                                                        .insert(key.clone(), String::new());
                                                    self.browsing.remove(&key);
                                                }
                                                ui.separator();
                                            }
                                            for item in results {
                                                if ui
                                                    .selectable_label(false, item.label())
                                                    .clicked()
                                                {
                                                    self.select_item(character_index, slot, &item);
                                                    self.searches
                                                        .insert(key.clone(), String::new());
                                                    self.browsing.remove(&key);
                                                }
                                            }
                                        });
                                });
                        }
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
                            ui.collapsing(title, |ui| {
                                let socket_count = item.sockets.len().max(current_plugs.len());
                                for socket_index in 0..socket_count {
                                    let current_hash = current_plugs
                                        .get(socket_index)
                                        .and_then(parse_unsigned_value);
                                    let allowed = item
                                        .sockets
                                        .get(socket_index)
                                        .map(|socket| {
                                            if self.allow_unsafe_plugs {
                                                self.manifest
                                                    .socket_type_options(socket.socket_type)
                                                    .to_vec()
                                            } else {
                                                self.manifest.socket_options(socket).to_vec()
                                            }
                                        })
                                        .unwrap_or_default();
                                    let current_label = current_hash.map_or_else(
                                        || "None".to_owned(),
                                        |hash| self.manifest.plug_label(hash),
                                    );
                                    let plug_search_key = format!(
                                        "plug-search:{character_index}:{slot}:{socket_index}"
                                    );
                                    let mut plug_query = self
                                        .plug_searches
                                        .get(&plug_search_key)
                                        .cloned()
                                        .unwrap_or_default();
                                    let searchable = allowed.len() > 12;
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Socket {}", socket_index + 1));
                                        let popup_id = ui.make_persistent_id(format!(
                                            "plug-browser:{character_index}:{slot}:{socket_index}"
                                        ));
                                        let button = ui.add_sized(
                                            [500.0, ui.spacing().interact_size.y],
                                            egui::Button::new(current_label),
                                        );
                                        if button.clicked() {
                                            ui.memory_mut(|memory| memory.toggle_popup(popup_id));
                                        }
                                        let mut selection = None::<Option<u64>>;
                                        egui::popup::popup_below_widget(
                                            ui,
                                            popup_id,
                                            &button,
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            |ui| {
                                                ui.set_min_width(500.0);
                                                if searchable {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut plug_query)
                                                            .hint_text(
                                                                "Search plug name or hex hash…",
                                                            )
                                                            .desired_width(480.0),
                                                    );
                                                    ui.separator();
                                                }
                                                egui::ScrollArea::vertical()
                                                    .min_scrolled_height(PLUG_PICKER_MIN_HEIGHT)
                                                    .max_height(PLUG_PICKER_MAX_HEIGHT)
                                                    .show(ui, |ui| {
                                                        if ui
                                                            .selectable_label(
                                                                current_hash.is_none(),
                                                                "None",
                                                            )
                                                            .clicked()
                                                        {
                                                            selection = Some(None);
                                                        }
                                                        if let Some(hash) = current_hash {
                                                            if !allowed.contains(&hash)
                                                                && ui
                                                                    .selectable_label(
                                                                        true,
                                                                        format!(
                                                                            "{}  (custom/current)",
                                                                            self.manifest
                                                                                .plug_label(hash)
                                                                        ),
                                                                    )
                                                                    .clicked()
                                                            {
                                                                selection = Some(Some(hash));
                                                            }
                                                        }
                                                        let needle =
                                                            plug_query.trim().to_lowercase();
                                                        let mut visible = 0usize;
                                                        for &hash in &allowed {
                                                            let label =
                                                                self.manifest.plug_label(hash);
                                                            if !needle.is_empty()
                                                                && !label
                                                                    .to_lowercase()
                                                                    .contains(&needle)
                                                                && !format_hash(hash)
                                                                    .to_lowercase()
                                                                    .contains(&needle)
                                                            {
                                                                continue;
                                                            }
                                                            visible += 1;
                                                            if ui
                                                                .selectable_label(
                                                                    current_hash == Some(hash),
                                                                    label,
                                                                )
                                                                .clicked()
                                                            {
                                                                selection = Some(Some(hash));
                                                            }
                                                        }
                                                        if searchable && visible == 0 {
                                                            ui.label(
                                                                egui::RichText::new(
                                                                    "No matching plugs found",
                                                                )
                                                                .weak(),
                                                            );
                                                        }
                                                    });
                                            },
                                        );
                                        if let Some(hash) = selection {
                                            self.select_plug(
                                                character_index,
                                                slot,
                                                socket_index,
                                                &item.default_plugs,
                                                hash,
                                            );
                                            ui.memory_mut(egui::Memory::close_popup);
                                        }
                                    });
                                    if searchable {
                                        self.plug_searches.insert(plug_search_key, plug_query);
                                    }
                                }
                            });
                        }
                    }
                });
            });
            ui.add_space(5.0);
        }
    }
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
        0 => 5,
        1 => 6,
        2 => 5,
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

pub(super) fn format_hash(hash: u64) -> String {
    format!("0x{hash:08X}")
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
    let mut used = HashSet::new();
    if let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    {
        for character in characters {
            let Some(equipment) = character.get("equipment").and_then(Value::as_object) else {
                continue;
            };
            for item in equipment.values().filter_map(Value::as_object) {
                if let Some(soid) = item.get("instance_soid").and_then(parse_unsigned_value) {
                    used.insert(soid);
                }
            }
        }
    }

    let mut candidate = GENERATED_INSTANCE_SOID_START;
    loop {
        if !used.contains(&candidate) {
            return Some(candidate);
        }
        candidate = candidate.checked_add(1)?;
    }
}

pub(super) fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| {
            equipment.values().find_map(|item| {
                item.get("level")
                    .and_then(Value::as_i64)
                    .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
            })
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
        Some(Value::Object(_)) | Some(Value::Null) | None => {
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
