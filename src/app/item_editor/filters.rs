//! Compact, reusable catalog filters for equipment-definition pickers.

use std::hash::Hash;

use eframe::egui;

use crate::catalog::{Catalog, ItemDamageType, ItemDef, ItemRarity};

const KINETIC_BUCKET: u64 = 1_498_876_634;
const ENERGY_BUCKET: u64 = 2_465_295_065;
const POWER_BUCKET: u64 = 953_998_645;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AmmoType {
    Primary,
    Special,
    Heavy,
}

impl AmmoType {
    const ALL: [Self; 3] = [Self::Primary, Self::Special, Self::Heavy];

    const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Special => "Special",
            Self::Heavy => "Heavy",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ItemFilter {
    pub(crate) weapon_type: Option<String>,
    pub(crate) damage_type: Option<ItemDamageType>,
    pub(crate) ammo_type: Option<AmmoType>,
    pub(crate) rarity: Option<ItemRarity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemFilterScope {
    Weapon,
    Armor,
}

impl ItemFilter {
    pub(crate) fn is_active(&self) -> bool {
        self.weapon_type.is_some()
            || self.damage_type.is_some()
            || self.ammo_type.is_some()
            || self.rarity.is_some()
    }

    pub(crate) fn matches(&self, catalog: &Catalog, item: &ItemDef) -> bool {
        if self.rarity.is_some_and(|selected| {
            catalog
                .item_package_metadata(item.hash)
                .map_or(ItemRarity::Unknown, |metadata| metadata.rarity)
                != selected
        }) {
            return false;
        }

        let has_weapon_filter =
            self.weapon_type.is_some() || self.damage_type.is_some() || self.ammo_type.is_some();
        if has_weapon_filter && !is_weapon(item) {
            return false;
        }
        if self
            .weapon_type
            .as_deref()
            .is_some_and(|selected| item.type_name.trim() != selected)
        {
            return false;
        }
        if self
            .damage_type
            .is_some_and(|selected| catalog.item_damage_type(item.hash) != Some(selected))
        {
            return false;
        }
        if self
            .ammo_type
            .is_some_and(|selected| weapon_ammo_type(item) != Some(selected))
        {
            return false;
        }
        true
    }
}

pub(crate) fn draw_item_filter_bar(
    ui: &mut egui::Ui,
    id_salt: impl Hash + Clone,
    scope: ItemFilterScope,
    candidates: &[&ItemDef],
    filter: &mut ItemFilter,
) {
    let mut weapon_types = candidates
        .iter()
        .copied()
        .filter(|item| is_weapon(item))
        .map(|item| item.type_name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    weapon_types.sort_by_key(|name| name.to_ascii_lowercase());
    weapon_types.dedup_by(|first, second| first.eq_ignore_ascii_case(second));

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if scope == ItemFilterScope::Weapon {
            draw_filter_group(ui, "Weapon type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "weapon-type"))
                    .selected_text(filter.weapon_type.as_deref().unwrap_or("Any"))
                    .width(122.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut filter.weapon_type, None, "Any");
                        for weapon_type in &weapon_types {
                            ui.selectable_value(
                                &mut filter.weapon_type,
                                Some(weapon_type.clone()),
                                weapon_type,
                            );
                        }
                    });
            });
            draw_filter_group(ui, "Damage type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "damage-type"))
                    .selected_text(filter.damage_type.map_or("Any", ItemDamageType::label))
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut filter.damage_type, None, "Any");
                        for option in ItemDamageType::ALL {
                            ui.selectable_value(
                                &mut filter.damage_type,
                                Some(option),
                                option.label(),
                            );
                        }
                    });
            });
            draw_filter_group(ui, "Ammo type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "ammo-type"))
                    .selected_text(filter.ammo_type.map_or("Any", AmmoType::label))
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut filter.ammo_type, None, "Any");
                        for option in AmmoType::ALL {
                            ui.selectable_value(
                                &mut filter.ammo_type,
                                Some(option),
                                option.label(),
                            );
                        }
                    });
            });
        }
        draw_filter_group(ui, "Rarity", |ui| {
            egui::ComboBox::from_id_salt((id_salt.clone(), "rarity"))
                .selected_text(filter.rarity.map_or("Any", ItemRarity::label))
                .width(86.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.rarity, None, "Any");
                    for option in ItemRarity::ALL {
                        ui.selectable_value(&mut filter.rarity, Some(option), option.label());
                    }
                });
        });
        if ui
            .add_enabled(filter.is_active(), egui::Button::new("Reset").small())
            .clicked()
        {
            *filter = ItemFilter::default();
        }
    });
}

fn draw_filter_group(ui: &mut egui::Ui, label: &str, add_control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(label);
        add_control(ui);
    });
}

fn is_weapon(item: &ItemDef) -> bool {
    matches!(
        item.bucket_hash,
        KINETIC_BUCKET | ENERGY_BUCKET | POWER_BUCKET
    )
}

fn weapon_ammo_type(item: &ItemDef) -> Option<AmmoType> {
    if !is_weapon(item) {
        return None;
    }
    if item.bucket_hash == POWER_BUCKET {
        return Some(AmmoType::Heavy);
    }

    let name = item.name.trim();
    if name.eq_ignore_ascii_case("Fighting Lion") {
        return Some(AmmoType::Primary);
    }
    if name.eq_ignore_ascii_case("Eriana's Vow") {
        return Some(AmmoType::Special);
    }

    match item.type_name.trim().to_ascii_lowercase().as_str() {
        "shotgun"
        | "sniper rifle"
        | "fusion rifle"
        | "trace rifle"
        | "grenade launcher"
        | "linear fusion rifle" => Some(AmmoType::Special),
        "auto rifle" | "hand cannon" | "pulse rifle" | "scout rifle" | "sidearm" | "smg"
        | "submachine gun" | "submachinegun" | "bow" | "combat bow" => Some(AmmoType::Primary),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{AbilityOptions, SocketDef};

    fn weapon(hash: u64, name: &str, type_name: &str, bucket_hash: u64) -> ItemDef {
        ItemDef {
            hash,
            name: name.into(),
            type_name: type_name.into(),
            bucket_hash,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets: Vec::<SocketDef>::new(),
            abilities: AbilityOptions::default(),
        }
    }

    #[test]
    fn ammo_rules_keep_reference_exceptions_and_power_weapons() {
        assert_eq!(
            weapon_ammo_type(&weapon(
                3_549_153_978,
                "Fighting Lion",
                "Grenade Launcher",
                ENERGY_BUCKET
            )),
            Some(AmmoType::Primary)
        );
        assert_eq!(
            weapon_ammo_type(&weapon(1, "Ordinary shotgun", "Shotgun", ENERGY_BUCKET)),
            Some(AmmoType::Special)
        );
        assert_eq!(
            weapon_ammo_type(&weapon(2, "Ordinary shotgun", "Shotgun", POWER_BUCKET)),
            Some(AmmoType::Heavy)
        );
    }
}
