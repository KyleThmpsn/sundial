//! What an ability or a path node authors over the stock one it starts from: its text, its icon,
//! the perks it grants and what it changes about the subclass's abilities. Abilities and nodes
//! share it, since the game presents both by a node record and grants both through a pool.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::layout;
use super::modifiers::{
    self, AbilityModifier, MOST_CHARGES, ParameterValue, RECHARGE_RANGE, StockModifier,
};
use super::palette::{EffectGrade, PaletteEdit, TintEdit};
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
    /// The tree node and existing HUD tile's sRGB color. None inherits the subclass theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Presentation of descendant abilities with their own HUD controller, scoped to this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attached_abilities: Vec<AttachedAbility>,
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
    /// A multiplier on the ability's recharge rate while it is selected, faster above 1, as the
    /// float's bits. Zero leaves the rate stock.
    #[serde(
        default,
        rename = "recharge",
        skip_serializing_if = "is_unset",
        with = "super::f32_bits"
    )]
    pub recharge_bits: u32,
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
    /// Color changes to color constants its effects draw with, on private copies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tints: Vec<TintEdit>,
    /// A grade over the final color of every effect, on private copies of their pixel programs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<EffectGrade>,
    /// Projectiles it fires in place of the stock ones its graphs spawn, each a private copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spawn_swaps: Vec<SpawnSwap>,
    /// Values of property rows of the ability's bank, changed in a private copy of the bank.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bank_values: Vec<BankValue>,
    /// The damage type every damage profile its graphs name deals, on private copies of the
    /// profiles. `None` keeps each profile's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<crate::recipe::RecipeDamageType>,
}

/// A traced lane of a property row of the ability's stock bank, by the row's key and place among
/// the bank's rows and the lane's offset in the row's modifier block, and the four bytes it
/// holds in place of the stock ones.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankValue {
    #[serde(with = "super::hex_hash")]
    pub key: u32,
    pub row: u16,
    pub lane: u16,
    pub bits: u32,
    /// Whether it is a script parameter's reset value, the value the bank's script reads while no
    /// applied key sets the parameter: `key` is then the parameter's name and `row` its row of
    /// the parameter table.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parameter: bool,
}

/// A projectile a graph of the ability spawns, and the stock projectile spawned in its place.
/// Every place the graph names the replaced projectile names a private copy of the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnSwap {
    /// The graph that spawns it: the ability's entity or a graph below it.
    #[serde(with = "super::hex_hash")]
    pub graph: u32,
    #[serde(with = "super::hex_hash")]
    pub replaced: u32,
    #[serde(with = "super::hex_hash")]
    pub replacement: u32,
    /// The damage type the replacement's copy deals through every damage profile its graphs
    /// name. `None` gives it the ability's damage type, or leaves its profiles as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<crate::recipe::RecipeDamageType>,
}

/// A recipe damage type as the client encodes it in a damage profile.
#[must_use]
pub fn damage_mode(damage: crate::recipe::RecipeDamageType) -> u8 {
    use crate::recipe::RecipeDamageType;
    use sundial::package_authoring::ability_damage::{ARC, KINETIC, SOLAR, VOID};
    match damage {
        RecipeDamageType::Kinetic => KINETIC,
        RecipeDamageType::Arc => ARC,
        RecipeDamageType::Solar => SOLAR,
        RecipeDamageType::Void => VOID,
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u8) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_unset(bits: &u32) -> bool {
    *bits == 0
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

/// A descendant ability's HUD presentation. Its graph identity survives recipe reloads and
/// allocation changes. Missing values preserve the original icon and inherited HUD color.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttachedAbility {
    #[serde(with = "super::hex_hash")]
    pub graph: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<EntryIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}

impl EntryIcon {
    fn validate(&self, context: &str) -> Result<(), String> {
        match self {
            Self::Ability { entry, .. } if usize::from(*entry) >= layout::ENTRY_COUNT => Err(
                format!("{context} takes the icon of entry {entry}, which no subclass has"),
            ),
            Self::Artwork { artwork } => artwork
                .validate()
                .map_err(|error| format!("{context}: {error}")),
            _ => Ok(()),
        }
    }
}

impl EntryEdits {
    /// The override for an attached graph, or its inherited presentation.
    #[must_use]
    pub fn attached(&self, graph: u32) -> AttachedAbility {
        self.attached_abilities
            .iter()
            .find(|edit| edit.graph == graph)
            .cloned()
            .unwrap_or(AttachedAbility {
                graph,
                icon: None,
                color: None,
            })
    }

    /// Set one attached graph's presentation. Restoring both fields removes the saved override.
    pub fn set_attached(&mut self, edit: AttachedAbility) {
        self.attached_abilities
            .retain(|each| each.graph != edit.graph);
        if edit.icon.is_some() || edit.color.is_some() {
            self.attached_abilities.push(edit);
            self.attached_abilities.sort_by_key(|each| each.graph);
        }
    }
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

    /// Whether it leaves the ability's entity as it is: no value, color, projectile or damage
    /// type changes.
    #[must_use]
    pub fn keeps_entity(&self) -> bool {
        self.ability_values.is_empty()
            && !self.recolors()
            && self.spawn_swaps.is_empty()
            && self.bank_values.is_empty()
            && self.damage_type.is_none()
            && self.attached_abilities.is_empty()
    }

    /// The damage type it sets, as the client encodes it in a damage profile.
    #[must_use]
    pub fn damage_mode(&self) -> Option<u8> {
        self.damage_type.map(damage_mode)
    }

    /// The damage type the projectile spawned in place of `replaced` where `graph` names it
    /// deals, when a swap there names one.
    #[must_use]
    pub fn swap_damage(
        &self,
        graph: u32,
        replaced: u32,
    ) -> Option<crate::recipe::RecipeDamageType> {
        self.spawn_swaps
            .iter()
            .find(|swap| swap.graph == graph && swap.replaced == replaced)
            .and_then(|swap| swap.damage_type)
    }

    /// Gives the projectile spawned in place of `replaced` where `graph` names it the damage type
    /// `damage`, or with `None` the ability's. Nothing without a swap there.
    pub fn set_swap_damage(
        &mut self,
        graph: u32,
        replaced: u32,
        damage: Option<crate::recipe::RecipeDamageType>,
    ) {
        if let Some(swap) = self
            .spawn_swaps
            .iter_mut()
            .find(|swap| swap.graph == graph && swap.replaced == replaced)
        {
            swap.damage_type = damage;
        }
    }

    /// The bits it gives a bank row's lane, if any.
    #[must_use]
    pub fn bank_value(&self, key: u32, row: u16, lane: u16) -> Option<u32> {
        self.bank_values
            .iter()
            .find(|each| !each.parameter && (each.key, each.row, each.lane) == (key, row, lane))
            .map(|each| each.bits)
    }

    /// Gives a bank row's lane `bits`, or with `None` its stock bits again.
    pub fn set_bank_value(&mut self, key: u32, row: u16, lane: u16, bits: Option<u32>) {
        self.bank_values
            .retain(|each| each.parameter || (each.key, each.row, each.lane) != (key, row, lane));
        if let Some(bits) = bits {
            self.bank_values.push(BankValue {
                key,
                row,
                lane,
                bits,
                parameter: false,
            });
        }
    }

    /// Gives script parameter `name`, the `row`th of the bank's parameter table, the reset value
    /// `value`, or with `None` its stock one again.
    pub fn set_parameter_default(&mut self, name: u32, row: u16, value: Option<f32>) {
        self.bank_values
            .retain(|each| !(each.parameter && (each.key, each.row) == (name, row)));
        if let Some(value) = value {
            self.bank_values.push(BankValue {
                key: name,
                row,
                lane: sundial::package_authoring::ability_movement::PARAMETER_RESET,
                bits: value.to_bits(),
                parameter: true,
            });
        }
    }

    /// The projectile spawned in place of `replaced` where `graph` names it, if any.
    #[must_use]
    pub fn swap(&self, graph: u32, replaced: u32) -> Option<u32> {
        self.spawn_swaps
            .iter()
            .find(|swap| swap.graph == graph && swap.replaced == replaced)
            .map(|swap| swap.replacement)
    }

    /// Spawns `replacement` in place of `replaced` where `graph` names it, or with `None` the
    /// stock one again. Values of the replaced graph go with it, since nothing spawns it now,
    /// and a damage type set on the place stays with the projectile now spawned there.
    pub fn set_swap(&mut self, graph: u32, replaced: u32, replacement: Option<u32>) {
        let damage_type = self.swap_damage(graph, replaced);
        self.spawn_swaps
            .retain(|swap| !(swap.graph == graph && swap.replaced == replaced));
        if let Some(replacement) = replacement.filter(|replacement| *replacement != replaced) {
            self.ability_values
                .retain(|value| value.locator.graph_tag.map(|tag| tag.get()) != Some(replaced));
            self.spawn_swaps.push(SpawnSwap {
                graph,
                replaced,
                replacement,
                damage_type,
            });
        }
    }

    /// The multiplier it gives its ability's recharge rate, if any.
    #[must_use]
    pub fn recharge(&self) -> Option<f32> {
        (self.recharge_bits != 0).then(|| f32::from_bits(self.recharge_bits))
    }

    /// Multiplies its ability's recharge rate, or with `None` or 1 leaves the rate stock.
    pub fn set_recharge(&mut self, multiplier: Option<f32>) {
        self.recharge_bits = multiplier
            .filter(|multiplier| multiplier.is_finite() && *multiplier != 1.0)
            .map_or(0, f32::to_bits);
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

    /// Sets a tint's change, or with a stock one leaves the color as it is.
    pub fn set_tint(&mut self, edit: TintEdit) {
        self.tints.retain(|each| each.color != edit.color);
        if !edit.is_stock() {
            self.tints.push(edit);
        }
    }

    /// The change it makes to the stock color `rgb`, which is none for one it leaves stock.
    #[must_use]
    pub fn tint(&self, rgb: [f32; 3]) -> Option<TintEdit> {
        self.tints
            .iter()
            .find(|each| each.starts_from(rgb))
            .copied()
            .or_else(|| TintEdit::new(rgb))
    }

    /// Whether it changes any of its effects' colors.
    #[must_use]
    pub fn recolors(&self) -> bool {
        !self.palettes.is_empty() || !self.tints.is_empty() || self.grade.is_some()
    }

    /// Sets the grade over every effect, or with a stock one leaves their colors as they are.
    pub fn set_grade(&mut self, grade: EffectGrade) {
        self.grade = (!grade.is_stock()).then_some(grade);
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

    pub(super) fn validate(&self, context: &str, source_entry: u8) -> Result<(), String> {
        if self.extra_charges > MOST_CHARGES {
            return Err(format!(
                "{context} takes at most {MOST_CHARGES} extra charges"
            ));
        }
        if let Some(multiplier) = self.recharge()
            && !RECHARGE_RANGE.contains(&multiplier)
        {
            return Err(format!(
                "{context} multiplies recharge by {} to {}",
                RECHARGE_RANGE.start(),
                RECHARGE_RANGE.end()
            ));
        }
        if (self.extra_charges > 0
            || self.recharge().is_some()
            || !self.parameters.is_empty()
            || !self.keeps_entity())
            && !modifiers::holds_ability(source_entry)
        {
            return Err(format!("{context} holds no ability of its own"));
        }
        // A projectile two places spawn is copied once, so the places must agree on its type.
        if let Some(swap) = self.spawn_swaps.iter().find(|swap| {
            self.spawn_swaps.iter().any(|other| {
                other.replacement == swap.replacement
                    && other
                        .damage_type
                        .zip(swap.damage_type)
                        .is_some_and(|(theirs, own)| theirs != own)
            })
        }) {
            return Err(format!(
                "{context} fires projectile 0x{:08X} with two damage types",
                swap.replacement
            ));
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
        if let Some(icon) = &self.icon {
            icon.validate(context)?;
        }
        let mut attached = BTreeSet::new();
        for edit in &self.attached_abilities {
            if !attached.insert(edit.graph) {
                return Err(format!("{context} changes one attached ability twice"));
            }
            if edit.icon.is_none() && edit.color.is_none() {
                return Err(format!("{context} has an empty attached ability edit"));
            }
            if let Some(icon) = &edit.icon {
                icon.validate(context)?;
            }
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
        let tints = self
            .tints
            .iter()
            .map(|edit| edit.color)
            .collect::<std::collections::HashSet<_>>();
        if tints.len() != self.tints.len() {
            return Err(format!("{context} changes one tint twice"));
        }
        for edit in &self.tints {
            edit.validate(context)?;
        }
        if let Some(grade) = &self.grade {
            grade.validate(context)?;
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
