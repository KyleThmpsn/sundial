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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemFilterScope {
    Weapon,
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
    id_salt: impl Hash + Clone,
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
        if scope == ItemFilterScope::Weapon {
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
        if ui
            .add_enabled(filter.is_active(), egui::Button::new("Reset").small())
            .clicked()
        {
            *filter = ItemFilter::default();
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
    fn long_filter_values_fit_and_controls_have_accessible_labels() {
        let item = weapon(
            1,
            "Test",
            "A deliberately long custom weapon type",
            1_498_876_634,
        );
        for width in [360.0, 640.0, 900.0] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut filter = ItemFilter {
                weapon_type: Some(item.type_name.clone()),
                ..Default::default()
            };
            let mut overflow = 0.0;
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1024.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(width);
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
                            let right = ui.max_rect().right();
                            draw_item_filter_bar(
                                ui,
                                "test",
                                ItemFilterScope::Weapon,
                                &[&item],
                                &mut filter,
                            );
                            overflow = ui.min_rect().right() - right;
                        });
                    },
                );
            }
            assert!(overflow <= 1.0, "{width}px filter overflow: {overflow}");
            assert_eq!(filter.weapon_type.as_deref(), Some(item.type_name.as_str()));
            let tree = output.platform_output.accesskit_update.unwrap();
            for label in ["Weapon type", "Damage type", "Ammo type", "Rarity"] {
                let ids: Vec<_> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.value() == Some(label) || node.label() == Some(label))
                    .map(|(id, _)| *id)
                    .collect();
                assert!(!ids.is_empty(), "{label} needs an accessible label");
            }
        }
    }

    #[test]
    fn weapon_type_matching_ignores_case_and_surrounding_whitespace() {
        let catalog = Catalog::for_test(Vec::new(), Default::default());
        let filter = ItemFilter {
            weapon_type: Some("Auto Rifle".into()),
            ..Default::default()
        };
        assert!(filter.matches(&catalog, &weapon(1, "Rifle", " auto RIFLE ", 1_498_876_634)));
        assert!(!filter.matches(&catalog, &weapon(2, "Sidearm", "Sidearm", 1_498_876_634)));
    }

    #[test]
    fn candidate_scope_only_enables_weapon_controls_for_weapon_buckets() {
        let weapon_item = weapon(1, "Test rifle", "Auto Rifle", 1_498_876_634);
        let ornament = weapon(2, "Test ornament", "Weapon Ornament", 1_498_876_634);
        let armor = weapon(3, "Test helmet", "Helmet", 3_448_274_439);

        assert_eq!(
            ItemFilterScope::from_candidates(&[&weapon_item]),
            ItemFilterScope::Weapon
        );
        assert_eq!(
            ItemFilterScope::from_candidates(&[&armor]),
            ItemFilterScope::Armor
        );
        assert_eq!(
            ItemFilterScope::from_candidates(&[&ornament]),
            ItemFilterScope::Armor
        );
        assert_eq!(
            ItemFilterScope::from_candidates(&[]),
            ItemFilterScope::Armor
        );
    }
}
