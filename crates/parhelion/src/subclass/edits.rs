//! What an ability or a path node authors over the stock one it starts from: its text, its icon,
//! the perks it grants and what it changes about the subclass's abilities. Abilities and nodes
//! share it, since the game presents both by a node record and grants both through a pool.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::modifiers::{self, AbilityModifier, MOST_CHARGES, ParameterValue, StockModifier};
use super::palette::PaletteEdit;
use super::{Place, layout};
use crate::perk::{Icon, PerkRecipe};
use sundial::package_authoring::runtime::WeaponRuntimeValueOverride;

/// Longest authored name, and description.
pub(super) const NAME_LIMIT: usize = 64;
const DESCRIPTION_LIMIT: usize = 1_024;

pub(super) fn text_is_valid(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit && !text.contains('\0')
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryEdits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<EntryIcon>,
    /// Sandbox perks it grants beyond its source's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_perks: Vec<u16>,
    /// Its source's sandbox perks it leaves out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_perks: Vec<u16>,
    /// Perks from the perk workbench. Each effect becomes a sandbox perk of the entry's own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_perks: Vec<PerkRecipe>,
    /// Charges the ability gets beyond its own while it is selected.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub extra_charges: u8,
    /// Changes it makes to abilities of its subclass while it is selected, beyond its stock
    /// ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<AbilityModifier>,
    /// Its stock pool's modifiers it leaves out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_modifiers: Vec<StockModifier>,
    /// Script parameters of the ability's own bank, set to values of its own while it is
    /// selected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterValue>,
    /// Values of the ability's own entity, and of the entity graphs it spawns, changed on
    /// copies that only this entry uses. Each value names its graph.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ability_values: Vec<WeaponRuntimeValueOverride>,
    /// Color changes to palettes its effects draw with, on private copies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palettes: Vec<PaletteEdit>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// The icon an entry shows in place of its source's.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryIcon {
    /// The icon of a stock subclass's entry.
    Ability {
        #[serde(with = "super::hex_hash")]
        subclass: u32,
        entry: u8,
    },
    /// Artwork of its own, as a custom perk takes.
    Artwork { artwork: Icon },
}

impl EntryEdits {
    /// Whether it leaves its source as it is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Its source's perks less the removed ones, then the added ones.
    #[must_use]
    pub fn perks(&self, stock: &[u16]) -> Vec<u16> {
        stock
            .iter()
            .filter(|perk| !self.removed_perks.contains(perk))
            .chain(self.added_perks.iter().filter(|perk| !stock.contains(perk)))
            .copied()
            .collect()
    }

    /// Takes `perk` away: an added one leaves the list, a stock one is removed.
    pub fn remove_perk(&mut self, perk: u16) {
        if self.added_perks.contains(&perk) {
            self.added_perks.retain(|added| *added != perk);
        } else if !self.removed_perks.contains(&perk) {
            self.removed_perks.push(perk);
        }
    }

    /// Grants `perk`: a removed stock one comes back, any other is added.
    pub fn add_perk(&mut self, perk: u16) {
        if self.removed_perks.contains(&perk) {
            self.removed_perks.retain(|removed| *removed != perk);
        } else if !self.added_perks.contains(&perk) {
            self.added_perks.push(perk);
        }
    }

    /// Puts `perk` in place of the custom perk with its id, or after the others.
    pub fn set_custom_perk(&mut self, perk: PerkRecipe) {
        match self
            .custom_perks
            .iter_mut()
            .find(|existing| existing.id == perk.id)
        {
            Some(existing) => *existing = perk,
            None => self.custom_perks.push(perk),
        }
    }

    /// Takes a stock modifier away, or brings a removed one back.
    pub fn toggle_stock_modifier(&mut self, stock: StockModifier) {
        if self.removed_modifiers.contains(&stock) {
            self.removed_modifiers.retain(|removed| *removed != stock);
        } else {
            self.removed_modifiers.push(stock);
        }
    }

    /// Sets a parameter of the ability's bank, or with `None` leaves it stock.
    pub fn set_parameter(&mut self, parameter: u32, value: Option<f32>) {
        self.parameters.retain(|each| each.parameter != parameter);
        if let Some(value) = value {
            self.parameters.push(ParameterValue {
                parameter,
                value_bits: value.to_bits(),
            });
        }
    }

    /// The value it sets a parameter to, if any.
    #[must_use]
    pub fn parameter(&self, parameter: u32) -> Option<f32> {
        self.parameters
            .iter()
            .find(|each| each.parameter == parameter)
            .map(|each| each.value())
    }

    /// Sets a palette's change, or with a stock one leaves the palette as it is.
    pub fn set_palette(&mut self, edit: PaletteEdit) {
        self.palettes.retain(|each| each.palette != edit.palette);
        if !edit.is_stock() {
            self.palettes.push(edit);
        }
    }

    /// The change it makes to a palette, which is none for one it leaves stock.
    #[must_use]
    pub fn palette(&self, palette: u32) -> PaletteEdit {
        self.palettes
            .iter()
            .find(|each| each.palette == palette)
            .copied()
            .unwrap_or(PaletteEdit::new(palette))
    }

    pub(super) fn validate(&self, context: &str, place: Place) -> Result<(), String> {
        if self.extra_charges > MOST_CHARGES {
            return Err(format!(
                "{context} takes at most {MOST_CHARGES} extra charges"
            ));
        }
        if (self.extra_charges > 0 || !self.parameters.is_empty() || !self.palettes.is_empty())
            && !modifiers::holds_ability(modifiers::place_entry(place))
        {
            return Err(format!("{context} holds no ability of its own"));
        }
        modifiers::validate(
            context,
            (&self.modifiers, &self.removed_modifiers, &self.parameters),
        )?;
        if self
            .name
            .as_deref()
            .is_some_and(|name| !text_is_valid(name, NAME_LIMIT))
            || self
                .description
                .as_deref()
                .is_some_and(|text| !text_is_valid(text, DESCRIPTION_LIMIT))
        {
            return Err(format!(
                "{context} has an empty or overlong name or description"
            ));
        }
        let added = self.added_perks.iter().collect::<BTreeSet<_>>();
        let removed = self.removed_perks.iter().collect::<BTreeSet<_>>();
        if added.len() != self.added_perks.len()
            || removed.len() != self.removed_perks.len()
            || !added.is_disjoint(&removed)
            || added.contains(&u16::MAX)
        {
            return Err(format!("{context} repeats a perk"));
        }
        match &self.icon {
            Some(EntryIcon::Ability { entry, .. })
                if usize::from(*entry) >= layout::ENTRY_COUNT =>
            {
                return Err(format!(
                    "{context} takes the icon of entry {entry}, which no subclass has"
                ));
            }
            Some(EntryIcon::Artwork { artwork }) => {
                artwork
                    .validate()
                    .map_err(|error| format!("{context}: {error}"))?;
            }
            _ => {}
        }
        let palettes = self
            .palettes
            .iter()
            .map(|edit| edit.palette)
            .collect::<BTreeSet<_>>();
        if palettes.len() != self.palettes.len() {
            return Err(format!("{context} changes one palette twice"));
        }
        for edit in &self.palettes {
            edit.validate(context)?;
        }
        let ids = self
            .custom_perks
            .iter()
            .map(|perk| perk.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != self.custom_perks.len() {
            return Err(format!("{context} has one custom perk twice"));
        }
        for perk in &self.custom_perks {
            let name = &perk.name;
            perk.validate()
                .map_err(|error| format!("{context}, {name}: {error}"))?;
            if perk.effects.is_empty() {
                return Err(format!("{context}, {name}: add an effect"));
            }
            if !perk.stats.is_empty() {
                return Err(format!("{context}, {name}: abilities take no stats"));
            }
        }
        Ok(())
    }
}
