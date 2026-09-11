use super::WeaponRecipe;
use std::collections::BTreeSet;

impl WeaponRecipe {
    pub(crate) fn unused_copy<'a>(
        &self,
        namespaces: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, String> {
        let namespaces: BTreeSet<_> = namespaces
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect();
        let mut copy = self.clone();
        for suffix in 1..=10_000 {
            let name = if suffix == 1 {
                format!("{} Copy", self.name)
            } else {
                format!("{} Copy {suffix}", self.name)
            };
            copy.rename_authored_item(&name)
                .map_err(|error| format!("Could not duplicate recipe: {error}"))?;
            if !namespaces.contains(&copy.namespace.to_ascii_lowercase()) {
                return Ok(copy);
            }
        }
        Err("Could not allocate an unused recipe copy identity".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_allocator_skips_case_insensitive_namespace_collisions() {
        let recipe =
            WeaponRecipe::new_named_weapon_for_donor("Signal Fire", 0x1234_5678, "Signal Donor")
                .unwrap();
        let copy = recipe
            .unused_copy(["parhelion.signal-fire-copy", "PARHELION.SIGNAL-FIRE-COPY-2"])
            .unwrap();

        assert_eq!(copy.name, "Signal Fire Copy 3");
        assert_eq!(copy.namespace, "parhelion.signal-fire-copy-3");
        assert_ne!(copy.identity, recipe.identity);
        assert_eq!(copy.donor, recipe.donor);
    }

    #[test]
    fn copy_allocator_preserves_recipe_mechanics() {
        let mut recipe = WeaponRecipe::every_end();
        recipe.flavor = "A carried-over story.".into();
        recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
        let before = recipe.clone();

        let copy = recipe.unused_copy(std::iter::empty()).unwrap();

        assert_eq!(copy.donor, before.donor);
        assert_eq!(copy.presentation_donor, before.presentation_donor);
        assert_eq!(copy.overrides, before.overrides);
        assert_eq!(copy.flavor, before.flavor);
        assert_eq!(copy.name, "Every End Copy");
        assert!(copy.identity_is_name_derived());
    }
}
