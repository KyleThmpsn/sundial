//! Editable examples materialized once in the default custom-perk library.

pub(super) const RECIPES: &[&str] = &[
    include_str!("../../recipes/perks/micro-missile-frame.perk.json"),
    include_str!("../../recipes/perks/chicken.perk.json"),
    include_str!("../../recipes/perks/hammer-of-sol-frame.perk.json"),
    include_str!("../../recipes/perks/storm-cannon-frame.perk.json"),
    include_str!("../../recipes/perks/void-nova-frame.perk.json"),
    include_str!("../../recipes/perks/deadeye-dividend.perk.json"),
    include_str!("../../recipes/perks/closed-circuit.perk.json"),
    include_str!("../../recipes/perks/chromatic-instinct.perk.json"),
    include_str!("../../recipes/perks/event-horizon.perk.json"),
    include_str!("../../recipes/perks/runaway-reactor.perk.json"),
    include_str!("../../recipes/perks/scavengers-rhythm.perk.json"),
    include_str!("../../recipes/perks/constellation.perk.json"),
    include_str!("../../recipes/perks/loaded-dice.perk.json"),
    include_str!("../../recipes/perks/scatter-matrix.perk.json"),
    include_str!("../../recipes/perks/borrowed-time.perk.json"),
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::RECIPES;
    use crate::perk::PerkRecipe;

    #[test]
    fn bundled_recipes_are_unique_and_valid() {
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for encoded in RECIPES {
            let recipe: PerkRecipe = serde_json::from_str(encoded).unwrap();
            recipe.validate().unwrap();
            assert!(ids.insert(recipe.id.clone()), "duplicate bundled perk ID");
            assert!(
                names.insert(recipe.name.clone()),
                "duplicate bundled perk name"
            );
        }
    }
}
