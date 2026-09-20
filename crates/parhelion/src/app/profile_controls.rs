//! Weapon profile controls; recipe mutation occurs on user actions.
use super::*;

pub(super) fn effective_power_cap_rows(
    overrides: &WeaponRecipeOverrides,
    inherited: &[u16],
) -> Vec<u16> {
    overrides.power_cap_groups.clone().unwrap_or_else(|| {
        inherited
            .iter()
            .map(|group| overrides.power_cap_group.unwrap_or(*group))
            .collect()
    })
}

const VARIABLE_DAMAGE_LABEL: &str = "Variable (Hold Reload)";

/// Changes one field without implicitly changing the other or the ammo override.
///
/// `variable_damage_available` says whether this weapon's family can switch damage at all.
/// `damage_locked` holds the control while a behavior graft owns the damage type, which happens
/// when the chosen source weapon switches damage itself.
pub(super) fn draw_combat_profile_control(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    donor: Option<&WeaponDonor>,
    select_slot: bool,
    variable_damage_available: bool,
    damage_locked: bool,
) -> bool {
    use crate::capabilities::{
        recipe_damage_type_from_catalog, recipe_inventory_slot_from_catalog,
    };
    use sundial::investment::WeaponDamageType;
    let field_label = if select_slot {
        "Equipment Slot"
    } else {
        "Damage Type"
    };
    let label = ui.horizontal(|ui| {
        let label = ui.label(field_label);
        draw_authoring_info_icon(ui, if select_slot {
            "Chooses the Kinetic, Energy, or Power slot. Damage type and ammo type are separate choices. Test unusual combinations in game."
        } else {
            "Changes the weapon's damage type. Slot and ammo type are separate choices. Kinetic conversion is unavailable for some elemental weapons. Variable damage steps the element while Reload is held, the way Hard Light and Borealis do, and wears one of them. Test unusual combinations in game."
        });
        label
    }).inner;
    let Some(donor) = donor else {
        ui.add_enabled(false, egui::Button::new("Load a Base Weapon"));
        return false;
    };
    let capabilities = weapon_authoring_capabilities(donor);
    let action = recipe_combat_profile_action(overrides, &donor.summary);
    let profile = match action {
        Some(CombatProfileAction::Set(profile)) => Some(profile),
        _ => donor
            .summary
            .inventory_slot
            .zip(donor.summary.damage_type)
            .map(
                |(inventory_slot, damage_type)| crate::capabilities::CombatProfile {
                    inventory_slot,
                    damage_type,
                },
            ),
    };
    let selected = if select_slot {
        profile
            .map(|p| p.inventory_slot.label())
            .or_else(|| donor.summary.inventory_slot.map(WeaponInventorySlot::label))
    } else {
        profile.map(|p| p.damage_type.label())
    }
    .unwrap_or("Unknown");
    let inherited = if select_slot {
        overrides.inventory_slot.is_none()
    } else {
        overrides.modern_damage_type.is_none()
    };
    let variable = !select_slot && overrides.variable_damage.is_some();
    let selected_text = if variable {
        VARIABLE_DAMAGE_LABEL.to_owned()
    } else if inherited {
        format!("{selected} (base weapon)")
    } else {
        selected.to_owned()
    };
    let slots = [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ];
    let damages = [
        WeaponDamageType::Kinetic,
        WeaponDamageType::Arc,
        WeaponDamageType::Solar,
        WeaponDamageType::Void,
    ];
    let mut changed = false;
    let variable_offered = !select_slot && (variable_damage_available || variable);
    let locked = damage_locked && !select_slot;
    ui.add_enabled_ui(
        !locked && capabilities.is_authorable() && (profile.is_some() || variable_offered),
        |ui| {
        egui::ComboBox::from_id_salt(if select_slot {
            "recipe_inventory_slot"
        } else {
            "recipe_damage_type"
        })
        .selected_text(selected_text)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            for index in 0..if select_slot {
                slots.len()
            } else {
                damages.len()
            } {
                let Some(mut candidate) = profile else { break };
                let text = if select_slot {
                    candidate.inventory_slot = slots[index];
                    slots[index].label()
                } else {
                    candidate.damage_type = damages[index];
                    damages[index].label()
                };
                let candidate_action = if donor.summary.inventory_slot
                    == Some(candidate.inventory_slot)
                    && donor.summary.damage_type == Some(candidate.damage_type)
                {
                    CombatProfileAction::Preserve
                } else {
                    CombatProfileAction::Set(candidate)
                };
                if ui
                    .add_enabled(
                        capabilities.supports(candidate_action),
                        egui::SelectableLabel::new(!variable && profile == Some(candidate), text),
                    )
                    .on_disabled_hover_text("This damage conversion has not been verified for the base weapon and cannot be selected.")
                    .clicked()
                {
                    if select_slot {
                        let value = (donor.summary.inventory_slot
                            != Some(candidate.inventory_slot))
                        .then(|| recipe_inventory_slot_from_catalog(candidate.inventory_slot));
                        changed = overrides.inventory_slot != value;
                        overrides.inventory_slot = value;
                    } else {
                        let value = (donor.summary.damage_type != Some(candidate.damage_type))
                            .then(|| recipe_damage_type_from_catalog(candidate.damage_type));
                        let was_variable = overrides.variable_damage.take().is_some();
                        changed = was_variable || overrides.modern_damage_type != value;
                        overrides.modern_damage_type = value;
                    }
                }
            }
            if variable_offered
                && ui
                    .add_enabled(
                        variable_damage_available || variable,
                        egui::SelectableLabel::new(variable, VARIABLE_DAMAGE_LABEL),
                    )
                    .on_hover_text("Steps the damage type through Void, Arc and Solar while Reload is held, the way Hard Light and Borealis do. The Fundamentals takes the first trait socket and the weapon keeps its own appearance. Kinetic damage can be authored separately with a Perk Workbench Set Damage Type action.")
                    .on_disabled_hover_text("This weapon family has no element-switch behavior to graft. Rifles and sniper rifles support it.")
                    .clicked()
                && !variable
            {
                overrides.variable_damage = Some(VariableDamageRecipe::all());
                changed = true;
            }
        })
        .response
        .on_disabled_hover_text(if locked {
            "The chosen Unique Weapon Behavior switches damage, so this follows it."
        } else {
            "This weapon cannot take a different damage type."
        })
        .labelled_by(label.id);
        },
    );
    if variable && (draw_variable_damage_elements(ui, overrides) || changed) {
        // The weapon rests on the first chosen element in selector order. A base weapon whose
        // damage cannot be converted, such as Hard Light itself, keeps its own marker.
        let resting = overrides
            .variable_damage
            .as_ref()
            .and_then(|variable| variable_damage_resting_type(&variable.elements));
        let slot = profile
            .map(|profile| profile.inventory_slot)
            .or(donor.summary.inventory_slot);
        overrides.modern_damage_type = resting.zip(slot).and_then(|(resting, slot)| {
            let damage_type = crate::capabilities::recipe_damage_type(resting);
            let native = donor.summary.inventory_slot == Some(slot)
                && donor.summary.damage_type == Some(damage_type);
            let action = if native {
                CombatProfileAction::Preserve
            } else {
                CombatProfileAction::Set(crate::capabilities::CombatProfile {
                    inventory_slot: slot,
                    damage_type,
                })
            };
            (capabilities.supports(action) && donor.summary.damage_type != Some(damage_type))
                .then_some(resting)
        });
        changed = true;
    }
    if !select_slot
        && capabilities.diagnostics.is_empty()
        && action.is_none_or(|action| !capabilities.supports(action))
        && ui.button("Use Base Weapon").clicked()
    {
        apply_combat_profile_action(overrides, &donor.summary, CombatProfileAction::Preserve);
        changed = true;
    }
    changed
}

pub(super) fn draw_optional_locale_text_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<String>,
    fallback: &str,
    multiline: bool,
) {
    let mut enabled = value.is_some();
    if ui
        .checkbox(&mut enabled, label)
        .on_hover_text("Translate this field.")
        .changed()
    {
        *value = enabled.then(|| fallback.to_owned());
    }
    if let Some(value) = value {
        if multiline {
            ui.add(
                egui::TextEdit::multiline(value)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2),
            );
        } else {
            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
        }
    } else {
        ui.add(egui::Label::new(egui::RichText::new(fallback).weak()).truncate())
            .on_hover_text(fallback);
    }
    ui.add_space(4.0);
}

/// Element checkboxes for a variable-damage weapon. Returns whether the set changed.
fn draw_variable_damage_elements(ui: &mut egui::Ui, overrides: &mut WeaponRecipeOverrides) -> bool {
    use crate::recipe::RecipeDamageType;
    use crate::weapon::variable_damage::{SELECTOR_ORDER, element_label};
    let Some(variable) = overrides.variable_damage.as_mut() else {
        return false;
    };
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for element in SELECTOR_ORDER.into_iter().map(RecipeDamageType::from) {
            let mut on = variable.elements.contains(&element);
            let last_two = on && variable.elements.len() <= 2;
            if ui
                .add_enabled(
                    !last_two,
                    egui::Checkbox::new(&mut on, element_label(element.into())),
                )
                .on_disabled_hover_text("Variable damage needs at least two elements.")
                .changed()
            {
                if on {
                    variable.elements.push(element);
                } else {
                    variable.elements.retain(|chosen| *chosen != element);
                }
                changed = true;
            }
        }
        draw_authoring_info_icon(
            ui,
            "Each Reload hold steps Void, Arc, then Solar. A step without a chosen element keeps the current one. Kinetic damage can be authored separately with a Perk Workbench Set Damage Type action.",
        );
    });
    changed
}

pub(super) fn draw_combat_profile_diagnostics(
    ui: &mut egui::Ui,
    overrides: &WeaponRecipeOverrides,
    donor: Option<&WeaponDonor>,
) {
    let Some(donor) = donor else {
        return;
    };
    if let Some(variable) = &overrides.variable_damage {
        ui.weak("Hold Reload in game to step the element. The weapon keeps its own appearance.");
        if variable.elements.len() < 2 {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Variable damage needs at least two elements.",
            );
        }
        if let Some(resting) = overrides.modern_damage_type
            && !variable.elements.contains(&resting)
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The resting damage type is not one of the variable elements. Pick the damage type again.",
            );
        }
    }
    if let Some(CombatProfileAction::Set(profile)) =
        recipe_combat_profile_action(overrides, &donor.summary)
    {
        use sundial::investment::WeaponDamageType;
        let kinetic_damage = profile.damage_type == WeaponDamageType::Kinetic;
        if kinetic_damage != (profile.inventory_slot == WeaponInventorySlot::Kinetic) {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Experimental slot and damage combination. Check equipping, damage, and ammo in game.",
            );
        }
    }
    if matches!(
        donor.summary.damage_profile,
        WeaponDamageProfile::PlugOrEmptyAmbiguous(Some(_))
    ) {
        ui.weak("Kinetic damage is not yet verified for this weapon's damage socket.");
    }
    let capabilities = weapon_authoring_capabilities(donor);
    if capabilities.diagnostics.is_empty() && capabilities.combat_profiles.len() == 1 {
        ui.weak("This base weapon has unresolved or dynamic damage. Slot and damage changes are unavailable.");
    }
    for diagnostic in &capabilities.diagnostics {
        ui.colored_label(ui.visuals().error_fg_color, &diagnostic.message);
    }
    if capabilities.diagnostics.is_empty()
        && recipe_combat_profile_action(overrides, &donor.summary)
            .is_none_or(|action| !capabilities.supports(action))
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "The imported slot/element pair is not supported by the gameplay donor. Reset it before building.",
        );
    }
}

pub(super) fn draw_ammo_type_control(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    gameplay_donor: Option<&WeaponDonor>,
) {
    let label = ui.horizontal(|ui| {
        let label = ui.label("Ammo Type");
        draw_authoring_info_icon(
            ui,
            "Sets the ammo type for the weapon and its perk variants. Magazine size, reserve capacity, and equipment slot are separate settings.",
        );
        label
    }).inner;
    let inherited = gameplay_donor
        .and_then(|donor| donor.summary.ammo_type)
        .map_or("unknown", WeaponAmmoType::label);
    let selected_text = overrides.ammo_type.map_or_else(
        || format!("{inherited} (base weapon)"),
        |ammo_type| recipe_ammo_type_label(ammo_type).to_owned(),
    );
    if gameplay_donor.is_none_or(|donor| donor.summary.weapon_pattern_index.is_none()) {
        ui.label(selected_text);
        ui.weak("Choose a gameplay donor with a native runtime before changing ammo type.");
        return;
    }
    egui::ComboBox::from_id_salt("recipe_ammo_type")
        .selected_text(selected_text)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            ui.selectable_value(
                &mut overrides.ammo_type,
                None,
                format!("{inherited} (base weapon)"),
            );
            for ammo_type in [
                RecipeAmmoType::Primary,
                RecipeAmmoType::Special,
                RecipeAmmoType::Heavy,
            ] {
                ui.selectable_value(
                    &mut overrides.ammo_type,
                    Some(ammo_type),
                    recipe_ammo_type_label(ammo_type),
                );
            }
        })
        .response
        .labelled_by(label.id);
}

pub(super) const fn recipe_ammo_type_label(ammo_type: RecipeAmmoType) -> &'static str {
    match ammo_type {
        RecipeAmmoType::Primary => "Primary",
        RecipeAmmoType::Special => "Special",
        RecipeAmmoType::Heavy => "Heavy",
    }
}

pub(super) fn draw_rarity_control(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    gameplay_donor: Option<&WeaponDonor>,
) {
    let label = ui.horizontal(|ui| {
        let label = ui.label("Rarity");
        draw_authoring_info_icon(
            ui,
            "Exotic weapons appear under Exotics in Collections. Other rarities use their weapon-type page. Both appear on your runtime’s badge. Choose perks separately below. Trace rifles require Exotic rarity because this game version has no other Collections page for them.",
        );
        label
    }).inner;
    let inherited = gameplay_donor.map_or(WeaponRarity::Unknown, |donor| donor.summary.rarity);
    let selected_text = overrides.rarity.map_or_else(
        || format!("{} (base weapon)", inherited.label()),
        |rarity| recipe_rarity_label(rarity).to_owned(),
    );
    egui::ComboBox::from_id_salt("recipe_rarity")
        .selected_text(selected_text)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            if ui
                .add_enabled(
                    rarity_is_supported(gameplay_donor, None),
                    egui::SelectableLabel::new(
                        overrides.rarity.is_none(),
                        format!("{} (base weapon)", inherited.label()),
                    ),
                )
                .clicked()
            {
                overrides.rarity = None;
            }
            for rarity in [
                RecipeRarity::Common,
                RecipeRarity::Uncommon,
                RecipeRarity::Rare,
                RecipeRarity::Legendary,
                RecipeRarity::Exotic,
            ] {
                if ui
                    .add_enabled(
                        rarity_is_supported(gameplay_donor, Some(rarity)),
                        egui::SelectableLabel::new(
                            overrides.rarity == Some(rarity),
                            recipe_rarity_label(rarity),
                        ),
                    )
                    .on_disabled_hover_text(
                        "Trace rifles require Exotic rarity in this game version.",
                    )
                    .clicked()
                {
                    overrides.rarity = Some(rarity);
                }
            }
        })
        .response
        .labelled_by(label.id);
    if gameplay_donor.is_some() && !rarity_is_supported(gameplay_donor, overrides.rarity) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Choose Exotic rarity. This weapon family has no non-Exotic Collections page.",
        );
    }
}

pub(super) fn rarity_is_supported(
    donor: Option<&WeaponDonor>,
    rarity: Option<RecipeRarity>,
) -> bool {
    let Some(donor) = donor else {
        return false;
    };
    !donor.summary.type_name.eq_ignore_ascii_case("Trace Rifle")
        || rarity.map_or(donor.summary.rarity == WeaponRarity::Exotic, |value| {
            value == RecipeRarity::Exotic
        })
}

pub(super) const fn recipe_rarity_label(rarity: RecipeRarity) -> &'static str {
    match rarity {
        RecipeRarity::Common => "Common",
        RecipeRarity::Uncommon => "Uncommon",
        RecipeRarity::Rare => "Rare",
        RecipeRarity::Legendary => "Legendary",
        RecipeRarity::Exotic => "Exotic",
    }
}

pub(super) fn draw_power_cap_control(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    donor: Option<&WeaponDonor>,
    catalog: Option<&InvestmentCatalog>,
) {
    let label = ui
        .horizontal(|ui| {
            let label = ui.label("Power Cap");
            draw_authoring_info_icon(
                ui,
                "Sets the weapon's infusion limit. Current Power is edited separately in Sundial.",
            );
            label
        })
        .inner;
    let inherited_cap = donor
        .and_then(|donor| donor.summary.power_cap)
        .map_or_else(|| "unresolved".to_owned(), |cap| cap.to_string());
    let choices = catalog.map_or_else(Vec::new, InvestmentCatalog::power_cap_choices);
    let selected_cap = overrides.power_cap_groups.as_ref().map_or_else(
        || {
            overrides.power_cap_group.map_or_else(
                || format!("{inherited_cap} (base weapon)"),
                |group| {
                    choices
                        .iter()
                        .find(|choice| choice.authoring_version_group == group)
                        .map_or_else(
                            || format!("Version group {group}"),
                            |choice| format!("{} power", choice.power_cap),
                        )
                },
            )
        },
        |groups| format!("Advanced per-version rows ({})", groups.len()),
    );
    egui::ComboBox::from_id_salt("recipe_power_cap")
        .selected_text(selected_cap)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            if ui
                .selectable_label(
                    overrides.power_cap_group.is_none() && overrides.power_cap_groups.is_none(),
                    format!("{inherited_cap} (base weapon)"),
                )
                .clicked()
            {
                overrides.power_cap_group = None;
                overrides.power_cap_groups = None;
            }
            // Equal caps can have different native identities. The everyday picker
            // offers each value once; the advanced editor retains every table row.
            let selected_power = overrides.power_cap_group.and_then(|index| {
                choices
                    .iter()
                    .find(|choice| choice.authoring_version_group == index)
                    .map(|choice| choice.power_cap)
            });
            let by_power = choices
                .iter()
                .map(|choice| (choice.power_cap, choice))
                .collect::<std::collections::BTreeMap<_, _>>();
            for choice in by_power.values() {
                if ui
                    .selectable_label(
                        overrides.power_cap_groups.is_none()
                            && selected_power == Some(choice.power_cap),
                        format!("{} power", choice.power_cap),
                    )
                    .clicked()
                {
                    overrides.power_cap_group = Some(choice.authoring_version_group);
                    overrides.power_cap_groups = None;
                }
            }
        })
        .response
        .labelled_by(label.id);
}
