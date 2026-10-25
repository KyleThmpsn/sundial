//! Compact, reusable catalog filters for equipment-definition pickers.

use std::hash::Hash;

use eframe::egui;

use crate::catalog::{
    Catalog, ItemDamageType, ItemDef, ItemRarity, ItemWeaponAmmoType, is_authorable_weapon_item,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ItemFilter {
    pub(crate) weapon_type: Option<String>,
    pub(crate) damage_type: Option<ItemDamageType>,
    pub(crate) ammo_type: Option<ItemWeaponAmmoType>,
    pub(crate) rarity: Option<ItemRarity>,
    /// Authoring picker option. Ordinary Sundial pickers use their existing dummy preference.
    pub(crate) include_dummy_weapons: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemFilterScope {
    Weapon,
    WeaponDonor,
    Armor,
}

impl ItemFilterScope {
    pub(crate) fn from_candidates(candidates: &[&ItemDef]) -> Self {
        if candidates
            .iter()
            .any(|item| is_authorable_weapon_item(item))
        {
            Self::Weapon
        } else {
            Self::Armor
        }
    }
}

impl ItemFilter {
    pub(crate) fn matches_inventory(
        &self,
        catalog: &Catalog,
        definition: crate::catalog::InventoryDefinition<'_>,
    ) -> bool {
        if let Some(item) = definition.item {
            return self.matches(catalog, item);
        }
        self.weapon_type.is_none()
            && self.damage_type.is_none()
            && self.ammo_type.is_none()
            && self.rarity.is_none_or(|rarity| {
                catalog
                    .item_package_metadata(definition.hash)
                    .map_or(ItemRarity::Unknown, |metadata| metadata.rarity)
                    == rarity
            })
    }

    pub(crate) fn is_active(&self) -> bool {
        self.weapon_type.is_some()
            || self.damage_type.is_some()
            || self.ammo_type.is_some()
            || self.rarity.is_some()
            || self.include_dummy_weapons
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
        if has_weapon_filter && !is_authorable_weapon_item(item) {
            return false;
        }
        if self
            .weapon_type
            .as_deref()
            .is_some_and(|selected| !item.type_name.trim().eq_ignore_ascii_case(selected.trim()))
        {
            return false;
        }
        if self
            .damage_type
            .is_some_and(|selected| catalog.item_damage_type(item.hash) != Some(selected))
        {
            return false;
        }
        if self.ammo_type.is_some_and(|selected| {
            catalog
                .item_package_metadata(item.hash)
                .and_then(|metadata| metadata.weapon_ammo_type)
                != Some(selected)
        }) {
            return false;
        }
        true
    }
}

pub(crate) fn draw_item_filter_bar(
    ui: &mut egui::Ui,
    id_salt: impl Hash + std::fmt::Debug + Clone,
    scope: ItemFilterScope,
    candidates: &[&ItemDef],
    filter: &mut ItemFilter,
) -> bool {
    let mut weapon_types = candidates
        .iter()
        .copied()
        .filter(|item| is_authorable_weapon_item(item))
        .map(|item| item.type_name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    weapon_types.sort_by_key(|name| name.to_ascii_lowercase());
    weapon_types.dedup_by(|first, second| first.eq_ignore_ascii_case(second));

    let mut option_clicked = false;
    let filter_style = ui.style().clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if matches!(
            scope,
            ItemFilterScope::Weapon | ItemFilterScope::WeaponDonor
        ) {
            draw_filter_group(ui, "Weapon type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "weapon-type"))
                    .selected_text(format!(
                        "Type: {}",
                        filter.weapon_type.as_deref().unwrap_or("Any")
                    ))
                    .width(154.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_style(filter_style.clone());
                        ui.set_min_width(100.0);
                        option_clicked |= ui
                            .selectable_value(&mut filter.weapon_type, None, "Any")
                            .clicked();
                        for weapon_type in &weapon_types {
                            option_clicked |= ui
                                .selectable_value(
                                    &mut filter.weapon_type,
                                    Some(weapon_type.clone()),
                                    weapon_type,
                                )
                                .clicked();
                        }
                    })
                    .response
            });
            draw_filter_group(ui, "Damage type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "damage-type"))
                    .selected_text(format!(
                        "Damage: {}",
                        filter.damage_type.map_or("Any", ItemDamageType::label)
                    ))
                    .width(126.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_style(filter_style.clone());
                        ui.set_min_width(100.0);
                        option_clicked |= ui
                            .selectable_value(&mut filter.damage_type, None, "Any")
                            .clicked();
                        for option in ItemDamageType::ALL {
                            option_clicked |= ui
                                .selectable_value(
                                    &mut filter.damage_type,
                                    Some(option),
                                    option.label(),
                                )
                                .clicked();
                        }
                    })
                    .response
            });
            draw_filter_group(ui, "Ammo type", |ui| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "ammo-type"))
                    .selected_text(format!(
                        "Ammo: {}",
                        filter.ammo_type.map_or("Any", ItemWeaponAmmoType::label)
                    ))
                    .width(128.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_style(filter_style.clone());
                        ui.set_min_width(100.0);
                        option_clicked |= ui
                            .selectable_value(&mut filter.ammo_type, None, "Any")
                            .clicked();
                        for option in ItemWeaponAmmoType::ALL {
                            option_clicked |= ui
                                .selectable_value(
                                    &mut filter.ammo_type,
                                    Some(option),
                                    option.label(),
                                )
                                .clicked();
                        }
                    })
                    .response
            });
        }
        draw_filter_group(ui, "Rarity", |ui| {
            egui::ComboBox::from_id_salt((id_salt.clone(), "rarity"))
                .selected_text(format!(
                    "Rarity: {}",
                    filter.rarity.map_or("Any", ItemRarity::label)
                ))
                .width(138.0)
                .truncate()
                .show_ui(ui, |ui| {
                    ui.set_style(filter_style.clone());
                    ui.set_min_width(100.0);
                    option_clicked |= ui
                        .selectable_value(&mut filter.rarity, None, "Any")
                        .clicked();
                    for option in ItemRarity::ALL {
                        option_clicked |= ui
                            .selectable_value(&mut filter.rarity, Some(option), option.label())
                            .clicked();
                    }
                })
                .response
        });
        if scope == ItemFilterScope::WeaponDonor {
            option_clicked |= ui
                .checkbox(&mut filter.include_dummy_weapons, "Include Dummy Weapons")
                .on_hover_text("Some lack the data Parhelion needs.")
                .changed();
        }
        if ui
            .add_enabled(filter.is_active(), egui::Button::new("Reset").small())
            .clicked()
        {
            *filter = ItemFilter::default();
            option_clicked = true;
        }
    });
    option_clicked
}

fn draw_filter_group(
    ui: &mut egui::Ui,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> egui::Response,
) {
    let response = add_control(ui);
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(label));
}
