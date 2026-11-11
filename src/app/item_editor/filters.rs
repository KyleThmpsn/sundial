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

/// What the filters read from one row: a catalog item, or a row standing for one, such as a saved
/// recipe.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FilterFields<'a> {
    /// The row's weapon type, or `None` for a row that is not a weapon.
    pub(crate) weapon_type: Option<&'a str>,
    pub(crate) damage_type: Option<ItemDamageType>,
    pub(crate) ammo_type: Option<ItemWeaponAmmoType>,
    pub(crate) rarity: ItemRarity,
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
        self.matches_fields(FilterFields {
            weapon_type: None,
            damage_type: None,
            ammo_type: None,
            rarity: catalog
                .item_package_metadata(definition.hash)
                .map_or(ItemRarity::Unknown, |metadata| metadata.rarity),
        })
    }

    pub(crate) fn is_active(&self) -> bool {
        self.narrows() || self.include_dummy_weapons
    }

    /// Whether a choice other than the dummy-weapon option hides rows.
    fn narrows(&self) -> bool {
        self.weapon_type.is_some()
            || self.damage_type.is_some()
            || self.ammo_type.is_some()
            || self.rarity.is_some()
    }

    pub(crate) fn matches(&self, catalog: &Catalog, item: &ItemDef) -> bool {
        // Every row passes an unset filter, so a long list reads no metadata for it.
        if !self.narrows() {
            return true;
        }
        let metadata = catalog.item_package_metadata(item.hash);
        self.matches_fields(FilterFields {
            weapon_type: is_authorable_weapon_item(item).then_some(item.type_name.as_str()),
            damage_type: metadata.and_then(|metadata| metadata.damage_type),
            ammo_type: metadata.and_then(|metadata| metadata.weapon_ammo_type),
            rarity: metadata.map_or(ItemRarity::Unknown, |metadata| metadata.rarity),
        })
    }

    /// Whether a row with `fields` passes. A weapon type, damage or ammo choice passes weapons
    /// only, and a weapon type matches whatever its case.
    pub(crate) fn matches_fields(&self, fields: FilterFields<'_>) -> bool {
        if self
            .rarity
            .is_some_and(|selected| fields.rarity != selected)
        {
            return false;
        }
        let Some(weapon_type) = fields.weapon_type else {
            return self.weapon_type.is_none()
                && self.damage_type.is_none()
                && self.ammo_type.is_none();
        };
        self.weapon_type
            .as_deref()
            .is_none_or(|selected| weapon_type.trim().eq_ignore_ascii_case(selected.trim()))
            && self
                .damage_type
                .is_none_or(|selected| fields.damage_type == Some(selected))
            && self
                .ammo_type
                .is_none_or(|selected| fields.ammo_type == Some(selected))
    }
}

pub(crate) fn draw_item_filter_bar(
    ui: &mut egui::Ui,
    id_salt: impl Hash + std::fmt::Debug + Clone,
    scope: ItemFilterScope,
    candidates: &[&ItemDef],
    filter: &mut ItemFilter,
) -> bool {
    draw_filter_bar(
        ui,
        id_salt,
        scope,
        move || {
            candidates
                .iter()
                .copied()
                .filter(|item| is_authorable_weapon_item(item))
                .map(|item| item.type_name.as_str())
        },
        filter,
    )
}

/// The filter bar over rows whose weapon types `weapon_types` names. It is read only while the
/// Type list is open, since a long list takes a while to read.
pub(crate) fn draw_filter_bar<'a, I: IntoIterator<Item = &'a str>>(
    ui: &mut egui::Ui,
    id_salt: impl Hash + std::fmt::Debug + Clone,
    scope: ItemFilterScope,
    weapon_types: impl FnOnce() -> I,
    filter: &mut ItemFilter,
) -> bool {
    let mut option_clicked = false;
    let filter_style = ui.style().clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if matches!(
            scope,
            ItemFilterScope::Weapon | ItemFilterScope::WeaponDonor
        ) {
            draw_filter_group(ui, "Weapon type", 154.0, |ui, width| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "weapon-type"))
                    .selected_text(format!(
                        "Type: {}",
                        filter.weapon_type.as_deref().unwrap_or("Any")
                    ))
                    .width(width)
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_style(filter_style.clone());
                        ui.set_min_width(100.0);
                        option_clicked |= ui
                            .selectable_value(&mut filter.weapon_type, None, "Any")
                            .clicked();
                        for weapon_type in &distinct_weapon_types(weapon_types()) {
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
            draw_filter_group(ui, "Damage type", 126.0, |ui, width| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "damage-type"))
                    .selected_text(format!(
                        "Damage: {}",
                        filter.damage_type.map_or("Any", ItemDamageType::label)
                    ))
                    .width(width)
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
            draw_filter_group(ui, "Ammo type", 128.0, |ui, width| {
                egui::ComboBox::from_id_salt((id_salt.clone(), "ammo-type"))
                    .selected_text(format!(
                        "Ammo: {}",
                        filter.ammo_type.map_or("Any", ItemWeaponAmmoType::label)
                    ))
                    .width(width)
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
        draw_filter_group(ui, "Rarity", 138.0, |ui, width| {
            egui::ComboBox::from_id_salt((id_salt.clone(), "rarity"))
                .selected_text(format!(
                    "Rarity: {}",
                    filter.rarity.map_or("Any", ItemRarity::label)
                ))
                .width(width)
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

/// The weapon types `names` holds, each once whatever its case, in name order.
fn distinct_weapon_types<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut weapon_types = names
        .into_iter()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    weapon_types.sort_by_key(|name| name.to_ascii_lowercase());
    weapon_types.dedup_by(|first, second| first.eq_ignore_ascii_case(second));
    weapon_types
}

/// Adds one filter control `width` wide. A combo box is placed where the row stands and never
/// wraps by itself, so a control that would run past the edge starts the next row.
fn draw_filter_group(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    add_control: impl FnOnce(&mut egui::Ui, f32) -> egui::Response,
) {
    let row_used = ui.cursor().min.x > ui.max_rect().min.x;
    if row_used && ui.available_rect_before_wrap().width() < width {
        ui.end_row();
    }
    let response = add_control(ui, width);
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(label));
}
