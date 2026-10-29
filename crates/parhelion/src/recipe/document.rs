//! Checked recipe parsing, canonical serialization and durable file replacement.

use super::*;
use crate::collection::armor::Origin;
use std::{fs, path::Path};

type Listing = (WeaponRecipe, Option<Origin>, Option<Imported>);

/// What a listing reads of a recipe's imported graph, which a portable recipe's listing otherwise
/// leaves out with its embedded assets. Only the importer reads imported graphs.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
pub(crate) struct Imported {
    /// The oldest converter revision of the graph and its appearance attachments.
    pub revision: u32,
    /// Whether the graph carries appearance attachments, such as ornaments.
    pub ornaments: bool,
}

#[cfg(feature = "d2-model-importer")]
fn imported(reference: &serde_json::Value) -> Option<Imported> {
    Some(Imported {
        revision: oldest_import_revision(reference)?,
        ornaments: reference["attachments"]
            .as_array()
            .is_some_and(|attachments| !attachments.is_empty()),
    })
}

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
        Self::parse_details(encoded, restore_assets).map(|((recipe, _, _), _)| recipe)
    }

    /// The recipe, and without `restore_assets`, the text of a recipe that embeds a model with the
    /// model's files left out. That text lists as the whole recipe does.
    fn parse_details(
        encoded: &str,
        restore_assets: bool,
    ) -> Result<(Listing, Option<String>), RecipeError> {
        use sundial::package_authoring::parse_json_envelope;
        #[cfg(feature = "d2-model-importer")]
        let (mut recipe, origin, revision, stripped): (Self, _, _, _) = {
            let mut document = parse_json_envelope(encoded, RECIPE_NESTING)?;
            let origin = Origin::document(&document);
            let revision = imported(&document["overrides"]["imported_graph"]);
            let mut stripped = None;
            if restore_assets {
                crate::imported::archive::expand(&mut document).map_err(RecipeError::Validation)?;
            } else {
                if crate::imported::archive::strip(&mut document) {
                    stripped = Some(serde_json::to_string(&document)?);
                }
                crate::imported::archive::skip(&mut document);
            }
            (
                serde_json::from_value(document)?,
                origin,
                revision,
                stripped,
            )
        };
        #[cfg(not(feature = "d2-model-importer"))]
        let (mut recipe, origin, revision, stripped): (
            Self,
            Option<Origin>,
            Option<Imported>,
            _,
        ) = {
            let _ = restore_assets;
            (
                parse_json_envelope(encoded, RECIPE_NESTING)?,
                None,
                None,
                None,
            )
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
        Ok(((recipe, origin, revision), stripped))
    }

    pub fn to_json_pretty(&self) -> Result<String, RecipeError> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.canonicalize_investment_stats();
        #[cfg(feature = "d2-model-importer")]
        crate::imported::artwork::apply(&mut canonical).map_err(RecipeError::Validation)?;
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
                // A recipe saved with embedded assets restores the same pinned bytes to a folder
                // of its own, which its migrated copy names.
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
    /// model is tens of megabytes of text to parse again. A recipe that embeds one is read in
    /// later sessions from its text without the model's files, which is kept beside its library.
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
        let saved = before.as_ref().and_then(|version| {
            listed::saved(path, version, |text| Self::parse_details(text, false))
        });
        let (recipe, stripped) = match saved {
            Some(listing) => listing,
            None => Self::parse_details(&read(path)?, false)?,
        };
        // A file replaced while it was read keeps no entry, so the next read sees the new one.
        if let Some(version) =
            before.filter(|version| listed::version(path).as_ref() == Some(version))
        {
            listed::put(path, version, &recipe, stripped.as_deref());
        }
        Ok(recipe)
    }

    /// Forgets the texts kept for recipes of the library folder `root` other than `paths`, which
    /// were deleted or renamed.
    pub(crate) fn keep_listings(root: &Path, paths: &[std::path::PathBuf]) {
        listed::keep_only(root, paths);
    }

    /// The namespace and identity hashes a saved recipe claims, read without checking the rest of
    /// it, so a recipe this version refuses still reserves them.
    pub(crate) fn load_claim(path: impl AsRef<Path>) -> Result<(String, [u32; 12]), RecipeError> {
        #[derive(serde::Deserialize)]
        struct Claim {
            namespace: String,
            identity: WeaponIdentity,
        }
        let path = path.as_ref();
        let parse = |encoded: &str| -> Result<_, RecipeError> {
            let claim: Claim =
                sundial::package_authoring::parse_json_envelope(encoded, RECIPE_NESTING)?;
            let hashes = claim.identity.parsed_hashes(&claim.namespace)?;
            Ok((claim.namespace, hashes))
        };
        // A recipe that embeds a model claims the same in its text without the model's files.
        if let Some(claim) =
            listed::version(path).and_then(|version| listed::saved(path, &version, parse))
        {
            return Ok(claim);
        }
        parse(&read(path)?)
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
///
/// A recipe that embeds a model also keeps its text without the model's files, in a cache folder
/// beside its library folder as the library state is, under the recipe's file name. A library of
/// imported armor was gigabytes of model text, read again on the first scan of every session.
mod listed {
    use super::{Listing, RecipeError};
    use serde::{Deserialize, Serialize};
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::{Path, PathBuf},
        sync::{Mutex, PoisonError},
        time::{SystemTime, UNIX_EPOCH},
    };

    /// A recipe without an embedded model parses in well under a millisecond, so smaller files
    /// are read each time.
    const MIN_BYTES: u64 = 1 << 20;

    /// The kept texts' folder, beside the library folder.
    const FOLDER: &str = "cache/recipe-listings";

    /// Raised when a kept text leaves out something new, so older ones are read again.
    const FORMAT: u32 = 1;

    #[derive(Clone, PartialEq, Eq)]
    pub(super) struct Version {
        len: u64,
        modified: SystemTime,
    }

    impl Version {
        fn stamp(&self) -> Option<(u64, u32)> {
            let since = self.modified.duration_since(UNIX_EPOCH).ok()?;
            Some((since.as_secs(), since.subsec_nanos()))
        }
    }

    /// A recipe's text without its model's files, and the version of the file it came from.
    #[derive(Serialize, Deserialize)]
    struct Saved {
        format: u32,
        source: PathBuf,
        len: u64,
        modified: (u64, u32),
        document: String,
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

    /// Keeps the listing for this session and `stripped`, the recipe's text without its model's
    /// files, for later ones. Keeping that text is only a speedup, so a failure is not reported.
    pub(super) fn put(path: &Path, version: Version, recipe: &Listing, stripped: Option<&str>) {
        if let (Some(document), Some(saved), Some(modified), Ok(source)) = (
            stripped,
            saved_path(path),
            version.stamp(),
            fs::canonicalize(path),
        ) {
            let saved_text = serde_json::to_vec(&Saved {
                format: FORMAT,
                source,
                len: version.len,
                modified,
                document: document.to_owned(),
            });
            if let (Ok(bytes), Some(folder)) = (saved_text, saved.parent())
                && fs::create_dir_all(folder).is_ok()
            {
                let _ = sundial::package_authoring::replace_authoring_file(&saved, &bytes);
            }
        }
        LISTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned(), (version, recipe.clone()));
    }

    /// Reads kept text only for its own source file and version. A rejected cache is a miss,
    /// so callers still report errors from the saved recipe rather than its derived text.
    pub(super) fn saved<T>(
        path: &Path,
        version: &Version,
        parse: impl FnOnce(&str) -> Result<T, RecipeError>,
    ) -> Option<T> {
        let cached = saved_path(path)?;
        let saved: Saved = serde_json::from_slice(&fs::read(&cached).ok()?).ok()?;
        if saved.format != FORMAT
            || saved.source != fs::canonicalize(path).ok()?
            || saved.len != version.len
            || Some(saved.modified) != version.stamp()
        {
            return None;
        }
        match parse(&saved.document) {
            Ok(value) => Some(value),
            Err(_) => {
                let _ = fs::remove_file(cached);
                None
            }
        }
    }

    /// Removes the texts kept for recipes of the library folder `root` other than `paths`.
    pub(super) fn keep_only(root: &Path, paths: &[PathBuf]) {
        let Some(folder) = root.parent().map(|parent| parent.join(FOLDER)) else {
            return;
        };
        let Ok(entries) = fs::read_dir(&folder) else {
            return;
        };
        let names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .collect::<BTreeSet<_>>();
        for entry in entries.flatten() {
            if !names.contains(entry.file_name().as_os_str()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Library recipes are the library folder's own files, so a file name is unique there.
    fn saved_path(path: &Path) -> Option<PathBuf> {
        Some(
            path.parent()?
                .parent()?
                .join(FOLDER)
                .join(path.file_name()?),
        )
    }
}
