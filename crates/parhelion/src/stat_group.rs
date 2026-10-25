//! Stat display groups of a recipe's own.
//!
//! An item's strings name a stat display group: the stats its inspect screen shows, each with a
//! curve from the stat's investment value to the value shown, and the value every stat is capped
//! at. A recipe may describe a group of its own instead of selecting a stock one. The build
//! appends it to the stat group table (`item::stat_group`) and points the item at it. The group
//! only changes what the inspect screen shows. The weapon's stat translator still turns each
//! investment value into its gameplay effect.
use serde::{Deserialize, Serialize};

/// The most stats one group shows.
pub const MAXIMUM_STATS: usize = 32;
/// The most points in one stat's display curve.
pub const MAXIMUM_POINTS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomStatGroup {
    /// The highest investment value any stat in the group takes.
    pub maximum_value: i32,
    /// The stats the group shows, in display order.
    pub stats: Vec<CustomScaledStat>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomScaledStat {
    /// The stat definition's index, as investment stats name it.
    pub definition_index: u16,
    /// Shown as a number rather than a bar, as Rounds Per Minute and Magazine are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub display_as_numeric: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_linear: bool,
    /// The display curve, as pairs of investment value and shown value, by rising investment
    /// value. An empty curve leaves the stat unshown, as stock groups do for hidden stats.
    pub display: Vec<[i32; 2]>,
}

impl CustomStatGroup {
    /// Checks what the stat group table can hold: a positive maximum, at most
    /// [`MAXIMUM_STATS`] distinct stats whose definition indices fit a byte, and curves of at
    /// most [`MAXIMUM_POINTS`] points with investment values rising within 0 to the maximum.
    pub fn validate(&self) -> Result<(), String> {
        if self.maximum_value <= 0 {
            return Err("A custom stat group needs a maximum above zero".into());
        }
        if self.stats.is_empty() || self.stats.len() > MAXIMUM_STATS {
            return Err(format!(
                "A custom stat group shows between 1 and {MAXIMUM_STATS} stats"
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for stat in &self.stats {
            if stat.definition_index > u16::from(u8::MAX) {
                return Err(format!(
                    "Stat definition {} does not fit a stat group row",
                    stat.definition_index
                ));
            }
            if !seen.insert(stat.definition_index) {
                return Err(format!(
                    "Stat definition {} appears twice in the custom stat group",
                    stat.definition_index
                ));
            }
            if stat.display.len() > MAXIMUM_POINTS {
                return Err(format!(
                    "Stat definition {} has more than {MAXIMUM_POINTS} display points",
                    stat.definition_index
                ));
            }
            for (index, [value, _]) in stat.display.iter().enumerate() {
                if !(0..=self.maximum_value).contains(value)
                    || index > 0 && stat.display[index - 1][0] >= *value
                {
                    return Err(format!(
                        "Stat definition {}'s display curve must rise from 0 to {}",
                        stat.definition_index, self.maximum_value
                    ));
                }
            }
        }
        Ok(())
    }

    /// The range an investment value of `definition_index` takes in this group, from its curve's
    /// first point to the group maximum, as stock groups bound their stats. `None` when the
    /// group does not show the stat.
    #[must_use]
    pub fn value_range(&self, definition_index: u16) -> Option<(i32, i32)> {
        let stat = self
            .stats
            .iter()
            .find(|stat| stat.definition_index == definition_index)?;
        let minimum = stat.display.first().map_or(0, |[value, _]| *value);
        Some((minimum, self.maximum_value))
    }

    /// Gives `stats` this group's display: its curve, flags and bounds for the stats it shows,
    /// and no display for the rest, as the inspect screen will show them.
    pub(crate) fn apply(&self, stats: &mut [sundial::investment::WeaponInvestmentStat]) {
        for stat in stats {
            let shown = self
                .stats
                .iter()
                .find(|shown| shown.definition_index == stat.definition_index);
            stat.display_as_numeric = shown.is_some_and(|shown| shown.display_as_numeric);
            stat.is_linear = shown.is_some_and(|shown| shown.is_linear);
            stat.display_interpolation = shown
                .map(|shown| {
                    shown
                        .display
                        .iter()
                        .map(
                            |[value, display]| sundial::investment::WeaponStatDisplayPoint {
                                investment_value: *value,
                                display_value: *display,
                            },
                        )
                        .collect()
                })
                .unwrap_or_default();
            stat.minimum_value = self
                .value_range(stat.definition_index)
                .map(|(minimum, _)| minimum);
            stat.maximum_value = Some(self.maximum_value);
        }
    }
}
