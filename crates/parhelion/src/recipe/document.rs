//! Checked recipe parsing, canonical serialization and durable file replacement.

use super::*;
use crate::collection::armor::Origin;
use std::{fs, path::Path};

type Listing = (WeaponRecipe, Option<Origin>, Option<u32>);

#[cfg(feature = "d2-model-importer")]
fn oldest_import_revision(reference: &serde_json::Value) -> Option<u32> {
    reference.as_object()?;
    let own = reference["converter_revision"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    Some(
        reference["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(oldest_import_revision)
            .fold(own, u32::min),
    )
}

impl WeaponRecipe {
    pub fn from_json_str(encoded: &str) -> Result<Self, RecipeError> {
        Self::parse(encoded, true)
    }

    /// A gear recipe that embeds its imported model restores it to the local asset cache after
    /// checking its content pin. Without `restore_assets` the recipe reads with no imported model.
    fn parse(encoded: &str, restore_assets: bool) -> Result<Self, RecipeError> {
        Self::parse_details(encoded, restore_assets).map(|(recipe, _, _)| recipe)
    }

    fn parse_details(encoded: &str, restore_assets: bool) -> Result<Listing, RecipeError> {
        use sundial::package_authoring::parse_json_envelope;
        #[cfg(feature = "d2-model-importer")]
        let (mut recipe, origin, revision): (Self, _, _) = {
            let mut document = parse_json_envelope(encoded, RECIPE_NESTING)?;
            let origin = Origin::document(&document);
            let revision = oldest_import_revision(&document["overrides"]["imported_graph"]);
            if restore_assets {
                crate::imported::archive::expand(&mut document).map_err(RecipeError::Validation)?;
            } else {
                crate::imported::archive::skip(&mut document);
            }
            (serde_json::from_value(document)?, origin, revision)
        };
        #[cfg(not(feature = "d2-model-importer"))]
        let (mut recipe, origin, revision): (Self, Option<Origin>, Option<u32>) = {
            let _ = restore_assets;
            (parse_json_envelope(encoded, RECIPE_NESTING)?, None, None)
        };
        if recipe.schema != RECIPE_SCHEMA {
            return Err(RecipeError::Validation(format!(
                "Unsupported recipe schema {}; expected {RECIPE_SCHEMA}",
                recipe.schema
            )));
        }
        recipe.canonicalize_investment_stats();
        // A recipe saved before shared projectiles had to agree on a damage type builds as it did.
        if let Some(abilities) = &mut recipe.overrides.subclass_abilities {
            abilities.settle_shared_swap_damage();
        }
        #[cfg(feature = "d2-model-importer")]
        crate::imported::artwork::apply(&mut recipe).map_err(RecipeError::Validation)?;
        recipe.validate()?;
        let origin = origin.or_else(|| Origin::recipe(&recipe));
        Ok((recipe, origin, revision))
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
        Self::parse(&read(path.as_ref())?, true)
    }

    /// Loads a recipe for a list or a search of saved recipes, which read names, icons and
    /// sockets. A gear recipe's embedded model is not restored, so its `imported_graph` is
    /// empty. Restoring it decodes and hashes the whole model, and a library of imported armor
    /// held the window for seconds on every scan. Opening, editing and building use `load_json`.
    ///
    /// A large file is read once per version. Every save rescans the library, and an embedded
    /// model is tens of megabytes of text to parse again.
    pub(crate) fn load_listing(path: impl AsRef<Path>) -> Result<Self, RecipeError> {
        Self::load_listing_details(path).map(|(recipe, _, _)| recipe)
    }

    pub(crate) fn load_listing_details(path: impl AsRef<Path>) -> Result<Listing, RecipeError> {
        let path = path.as_ref();
        let before = listed::version(path);
        if let Some(recipe) = before
            .as_ref()
            .and_then(|version| listed::get(path, version))
        {
            return Ok(recipe);
        }
        let recipe = Self::parse_details(&read(path)?, false)?;
        // A file replaced while it was read keeps no entry, so the next read sees the new one.
        if let Some(version) =
            before.filter(|version| listed::version(path).as_ref() == Some(version))
        {
            listed::put(path, version, &recipe);
        }
        Ok(recipe)
    }

    /// The namespace and identity hashes a saved recipe claims, read without checking the rest of
    /// it, so a recipe this version refuses still reserves them.
    pub(crate) fn load_claim(path: impl AsRef<Path>) -> Result<(String, [u32; 12]), RecipeError> {
        #[derive(serde::Deserialize)]
        struct Claim {
            namespace: String,
            identity: WeaponIdentity,
        }
        let claim: Claim =
            sundial::package_authoring::parse_json_envelope(&read(path.as_ref())?, RECIPE_NESTING)?;
        let hashes = claim.identity.parsed_hashes(&claim.namespace)?;
        Ok((claim.namespace, hashes))
    }

    /// Parses a recipe as `load_listing` reads it, with an embedded model left unrestored.
    pub(crate) fn from_json_listing(encoded: &str) -> Result<Self, RecipeError> {
        Self::parse(encoded, false)
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

fn read(path: &Path) -> Result<String, RecipeError> {
    fs::read_to_string(path).map_err(|source| RecipeError::Io {
        operation: "read recipe",
        path: path.to_owned(),
        source,
    })
}

/// Listing reads of large recipe files, by path, size and modification time.
mod listed {
    use super::Listing;
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::{Mutex, PoisonError},
        time::SystemTime,
    };

    /// A recipe without an embedded model parses in well under a millisecond, so smaller files
    /// are read each time.
    const MIN_BYTES: u64 = 1 << 20;

    #[derive(Clone, PartialEq, Eq)]
    pub(super) struct Version {
        len: u64,
        modified: SystemTime,
    }

    static LISTED: Mutex<BTreeMap<PathBuf, (Version, Listing)>> = Mutex::new(BTreeMap::new());

    /// None for a small or unreadable file, which is never kept.
    pub(super) fn version(path: &Path) -> Option<Version> {
        let metadata = fs::metadata(path).ok()?;
        if metadata.len() < MIN_BYTES {
            return None;
        }
        Some(Version {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }

    pub(super) fn get(path: &Path, version: &Version) -> Option<Listing> {
        let listed = LISTED.lock().unwrap_or_else(PoisonError::into_inner);
        listed
            .get(path)
            .filter(|(kept, _)| kept == version)
            .map(|(_, recipe)| recipe.clone())
    }

    pub(super) fn put(path: &Path, version: Version, recipe: &Listing) {
        LISTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned(), (version, recipe.clone()));
    }
}
