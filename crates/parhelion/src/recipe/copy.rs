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
