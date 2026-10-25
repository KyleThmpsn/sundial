//! The Magazine row's reading: the count the inspect screen shows for the weapon as built. The stat
//! group's display curve does not give it, so the row replays the weapon's own stat translator with
//! the stats the table shows.
use super::contribution::{Contributions, total};
use super::*;
use crate::weapon::magazine::{Magazine, Shown};

/// The count at the weapon's own stats and with its starting plugs, and how the second comes about.
pub(in crate::app) struct Reading {
    pub(super) base: i64,
    pub(in crate::app) now: i64,
    pub(super) details: String,
}

impl PackageAuthoringApp {
    /// The Magazine row's reading for a weapon whose stats the table shows from `donor` and
    /// `contributions`. None for another kind of item, while the weapon is read and where its count
    /// is unknown, which leave the row on the display curve.
    pub(in crate::app) fn magazine_reading(
        &mut self,
        ctx: &egui::Context,
        donor: &WeaponDonor,
        contributions: &Contributions,
    ) -> Option<Reading> {
        if !self.recipe.kind.is_weapon() {
            return None;
        }
        let Some(Ok(Some(defaults))) = self.barrel_reading(ctx, Some(donor)) else {
            return None;
        };
        let magazine = defaults.magazine.as_ref().ok()?;
        let base = self.shown(magazine, donor, None)?;
        let now = self.shown(magazine, donor, Some(contributions))?;
        Some(Reading {
            base: base.count,
            now: now.count,
            details: self.breakdown(magazine, &now, donor),
        })
    }

    /// The count at the stats the table shows: each one's own value, or 0 once removed, with its
    /// starting plugs' amounts and perks where `contributions` gives them.
    fn shown(
        &self,
        magazine: &Magazine,
        donor: &WeaponDonor,
        contributions: Option<&Contributions>,
    ) -> Option<Shown> {
        let overrides = &self.recipe.overrides;
        let stat = |index: u16| {
            let own = if overrides.removed_investment_stats.contains(&index) {
                0
            } else {
                overrides
                    .investment_stats
                    .iter()
                    .find(|value| value.definition_index == index)
                    .map(|value| value.value)
                    .or_else(|| {
                        donor
                            .investment_stats
                            .iter()
                            .find(|stat| stat.definition_index == index)
                            .map(|stat| stat.value)
                    })
                    .unwrap_or(0)
            };
            let sources = contributions
                .and_then(|contributions| contributions.get(&index))
                .map_or(&[][..], Vec::as_slice);
            total(own, sources)
        };
        let bullets = overrides
            .barrel
            .as_ref()
            .and_then(|edits| edits.bullets_per_shot);
        magazine.shown(stat, bullets, contributions.is_some())
    }

    /// The capacity, each starting perk's change and the division, line by line.
    fn breakdown(&self, magazine: &Magazine, shown: &Shown, donor: &WeaponDonor) -> String {
        let perks = starting_perks(self.catalog.as_ref(), &self.recipe, donor);
        let capacity = egui::emath::format_with_decimals_in_range(f64::from(shown.capacity), 0..=4);
        let mut lines = vec![format!("Capacity {capacity}")];
        for modifier in &magazine.modifiers {
            let name = perks
                .names
                .get(&modifier.perk)
                .cloned()
                .unwrap_or_else(|| format!("Perk {}", modifier.perk));
            let amount = if modifier.multiplies {
                format!("×{}", modifier.amount)
            } else {
                format!("{:+}", modifier.amount)
            };
            lines.push(if modifier.capacity() {
                format!("{name} {amount}")
            } else {
                format!("{name} {amount} bullets per shot")
            });
        }
        if let Some(per_shot) = shown.per_shot {
            lines.push(format!("Divided by {per_shot} bullets per shot"));
        }
        if perks.private {
            lines.push("Private perk runtime is not counted".to_owned());
        }
        lines.join("\n")
    }
}
