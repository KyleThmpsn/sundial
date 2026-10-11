//! Checked recipe parsing, canonical serialization and durable file replacement.

use super::*;
use std::{fs, path::Path};

impl WeaponRecipe {
    pub fn from_json_str(encoded: &str) -> Result<Self, RecipeError> {
        use sundial::package_authoring::parse_json_envelope;
        #[cfg(feature = "d2-model-importer")]
        let mut recipe: Self = {
            let mut document = parse_json_envelope(encoded, RECIPE_NESTING)?;
            crate::imported::archive::expand(&mut document).map_err(RecipeError::Validation)?;
            serde_json::from_value(document)?
        };
        #[cfg(not(feature = "d2-model-importer"))]
        let mut recipe: Self = parse_json_envelope(encoded, RECIPE_NESTING)?;
        if recipe.schema != RECIPE_SCHEMA {
            return Err(RecipeError::Validation(format!(
                "Unsupported recipe schema {}; expected {RECIPE_SCHEMA}",
                recipe.schema
            )));
        }
        recipe.canonicalize_investment_stats();
        #[cfg(feature = "d2-model-importer")]
        crate::imported::artwork::apply(&mut recipe).map_err(RecipeError::Validation)?;
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn to_json_pretty(&self) -> Result<String, RecipeError> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.canonicalize_investment_stats();
        #[cfg(feature = "d2-model-importer")]
        crate::imported::artwork::apply(&mut canonical).map_err(RecipeError::Validation)?;
        #[cfg(feature = "d2-model-importer")]
        if crate::imported::kind(canonical.kind).is_some()
            && canonical.overrides.imported_graph.is_some()
        {
            return crate::imported::archive::encode(&canonical).map_err(RecipeError::Validation);
        }
        Ok(serde_json::to_string_pretty(&canonical)?)
    }

    /// Compare the content saved by this format. Stat and locale ordering is normalized
    /// by both load and save, and must not be mistaken for an external edit.
    pub(crate) fn same_saved_content(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        let mut left = self.clone();
        let mut right = other.clone();
        left.canonicalize_investment_stats();
        right.canonicalize_investment_stats();
        #[cfg(feature = "d2-model-importer")]
        for recipe in [&mut left, &mut right] {
            if crate::imported::artwork::apply(recipe).is_err() {
                return false;
            }
            if crate::imported::kind(recipe.kind).is_some()
                && let Some(reference) = &mut recipe.overrides.imported_graph
            {
                // Portable recipes restore the same pinned bytes to a different local folder.
                reference.directory.clear();
            }
        }
        left == right
    }

    pub fn load_json(path: impl AsRef<Path>) -> Result<Self, RecipeError> {
        let path = path.as_ref();
        let encoded = fs::read_to_string(path).map_err(|source| RecipeError::Io {
            operation: "read recipe",
            path: path.to_owned(),
            source,
        })?;
        Self::from_json_str(&encoded)
    }

    pub fn save_json(&self, path: impl AsRef<Path>) -> Result<(), RecipeError> {
        let path = path.as_ref();
        let mut encoded = self.to_json_pretty()?;
        encoded.push('\n');
        sundial::package_authoring::replace_authoring_file(path, encoded.as_bytes()).map_err(
            |source| RecipeError::Io {
                operation: "write recipe",
                path: path.to_owned(),
                source,
            },
        )
    }

    pub(super) fn canonicalize_investment_stats(&mut self) {
        for variant in &mut self.overrides.socket_plug_variants {
            variant
                .investment_stats
                .sort_unstable_by_key(|stat| stat.definition_index);
        }
        self.overrides
            .investment_stats
            .sort_unstable_by_key(|stat| stat.definition_index);
        self.overrides.removed_investment_stats.sort_unstable();
        self.locale_overrides
            .sort_unstable_by_key(|locale| locale.locale_index);
    }
}
