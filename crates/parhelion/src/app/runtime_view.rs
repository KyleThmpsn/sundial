//! Advanced native runtime and sandbox editing.
use super::*;

mod barrel;
mod base_perks;
mod components;
mod inventory;
mod patches;
mod projectile;
mod values;

pub(in crate::app) use barrel::BarrelControls;
pub(in crate::app) use projectile::FiredProjectile;

/// The recipe's component splices, which both the Barrel controls and the Projectile section read
/// so they show the components the build makes.
impl PackageAuthoringApp {
    /// The recipe's component splices as binding, donor pattern and donor item. A splice whose
    /// hashes do not parse is an error.
    fn component_splice_list(&self) -> Result<Vec<(u32, Option<u16>, u32)>, String> {
        self.recipe
            .overrides
            .component_splices
            .iter()
            .map(|splice| {
                let item = splice
                    .donor
                    .item_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let binding = splice
                    .binding_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let pattern = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == item)
                    .and_then(|donor| donor.weapon_pattern_index);
                Ok((binding, pattern, item))
            })
            .collect()
    }

    /// The splices that parse, for a reader that shows what the weapon fires. A splice that does
    /// not parse fails the build, which reports it.
    fn component_splice_sources(&self) -> Vec<(u32, Option<u16>, u32)> {
        self.component_splice_list().unwrap_or_default()
    }
}
