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
            let suffix = if suffix == 1 {
                " Copy".to_owned()
            } else {
                format!(" Copy {suffix}")
            };
            let name = self.copy_name(&suffix);
            copy.rename_authored_item(&name)
                .map_err(|error| format!("Could not duplicate recipe: {error}"))?;
            if !namespaces.contains(&copy.namespace.to_ascii_lowercase()) {
                return Ok(copy);
            }
        }
        Err("Could not allocate an unused recipe copy identity".into())
    }

    fn copy_name(&self, suffix: &str) -> String {
        // Keep the longest display-name prefix whose new identity fits. Character
        // boundaries preserve Unicode, and the suffix remains intact for later copies.
        let max_characters = usize::from(u16::MAX) - suffix.chars().count();
        let boundaries: Vec<_> = self
            .name
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(self.name.len()))
            .take(max_characters + 1)
            .collect();
        let name_at = |end| format!("{}{suffix}", self.name[..end].trim_end());
        let end = boundaries
            .partition_point(|&end| super::namespace_for_weapon_name(&name_at(end)).is_ok());
        name_at(boundaries[end.saturating_sub(1)])
    }
}
