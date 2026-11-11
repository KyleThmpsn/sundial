//! The item pickers' filters for Parhelion lists whose rows are not catalog items.
use std::hash::Hash;

use eframe::egui;

use crate::investment::{WeaponAmmoType, WeaponDamageType, WeaponRarity};

use crate::app::item_editor::{FilterFields, ItemFilter, ItemFilterScope, draw_filter_bar};

/// The item pickers' Type, Damage, Ammo and Rarity filters for rows that are not catalog items,
/// such as saved recipes. Each row gives what the filters read as [`ItemChoiceFields`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemChoiceFilter(ItemFilter);

/// What the item filters read from one row.
#[derive(Clone, Copy, Debug)]
pub struct ItemChoiceFields<'a> {
    /// The row's weapon type, or `None` for a row that is not a weapon. Only weapons pass a
    /// Type, Damage or Ammo choice.
    pub weapon_type: Option<&'a str>,
    pub damage_type: Option<WeaponDamageType>,
    pub ammo_type: Option<WeaponAmmoType>,
    pub rarity: WeaponRarity,
}

impl ItemChoiceFilter {
    /// Whether a choice hides rows.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.0.is_active()
    }

    /// Whether a row with `fields` passes.
    #[must_use]
    pub fn passes(&self, fields: ItemChoiceFields<'_>) -> bool {
        self.0.matches_fields(FilterFields {
            weapon_type: fields.weapon_type,
            damage_type: fields.damage_type.map(Into::into),
            ammo_type: fields.ammo_type.map(Into::into),
            rarity: fields.rarity.into(),
        })
    }

    /// Draws the filter bar the item pickers show. `weapons` offers Type, Damage and Ammo beside
    /// Rarity. A list without weapons clears those choices, so none stays set out of sight.
    /// `weapon_types` names the Type choices and is read only while that list is open. Returns
    /// whether a control was used.
    pub fn draw<'a, I: IntoIterator<Item = &'a str>>(
        &mut self,
        ui: &mut egui::Ui,
        id_salt: impl Hash + std::fmt::Debug + Clone,
        weapons: bool,
        weapon_types: impl FnOnce() -> I,
    ) -> bool {
        let scope = if weapons {
            ItemFilterScope::Weapon
        } else {
            self.0.weapon_type = None;
            self.0.damage_type = None;
            self.0.ammo_type = None;
            ItemFilterScope::Armor
        };
        draw_filter_bar(ui, id_salt, scope, weapon_types, &mut self.0)
    }
}
