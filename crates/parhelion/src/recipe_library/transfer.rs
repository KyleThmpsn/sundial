//! Local recipe sharing and copies with independent weapon identities.
use super::{Path, PathBuf, RecipeLibrary, WeaponRecipe};

#[derive(Default)]
pub(crate) struct ImportReport {
    pub paths: Vec<PathBuf>,
    pub errors: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RecipeBundle {
    format: String,
    version: u32,
    recipes: Vec<serde_json::Value>,
}

const BUNDLE_FORMAT: &str = "parhelion.recipe-bundle";
/// How deep a bundle holds each recipe: its object, then its `recipes` list.
const BUNDLE_ENVELOPE: usize = 2;
/// The most a file to import may hold.
const MAX_IMPORT_BYTES: u64 = 256 * 1024 * 1024;
/// The most recipes one bundle may hold.
const MAX_BUNDLE_RECIPES: usize = 1024;
/// The most decoded picture memory one file's pictures may ask for together, counted from their
/// headers before any is decoded. Each picture also has a limit of its own when it decodes.
const MAX_IMPORT_PICTURE_BYTES: u64 = 1024 * 1024 * 1024;

/// One file to import: its recipes, each parsed and checked on its own.
struct Import {
    bundle: bool,
    recipes: Vec<serde_json::Value>,
}

impl RecipeLibrary {
    pub fn duplicate(&self, recipe: &WeaponRecipe) -> Result<PathBuf, String> {
        let _lock = self.lock()?;
        self.duplicate_locked(recipe).map(|(path, _)| path)
    }

    pub(super) fn duplicate_locked(
        &self,
        recipe: &WeaponRecipe,
    ) -> Result<(PathBuf, WeaponRecipe), String> {
        let scan = self.scan()?;
        let copy = recipe.unused_copy(scan.entries.iter().map(|entry| entry.namespace.as_str()))?;
        self.save_new_locked(&copy).map(|path| (path, copy))
    }

    pub fn export(&self, recipe: &WeaponRecipe, path: &Path) -> Result<(), String> {
        self.validate_export_path(path)?;
        recipe.save_json(path).map_err(|error| error.to_string())
    }

    pub(crate) fn import_files(&self, sources: &[PathBuf]) -> ImportReport {
        let mut report = ImportReport::default();
        for source in sources {
            let import = match read_import(source) {
                Ok(import) => import,
                Err(error) => {
                    report.errors.push(format!("{}: {error}", source.display()));
                    continue;
                }
            };
            // One recipe's pictures are decoded at a time, and it is saved before the next.
            for (index, value) in import.recipes.into_iter().enumerate() {
                let recipe = match WeaponRecipe::from_json_str(&value.to_string()) {
                    Ok(recipe) => recipe,
                    Err(error) if import.bundle => {
                        report.errors.push(format!(
                            "{}: Recipe {} in bundle: {error}",
                            source.display(),
                            index + 1
                        ));
                        continue;
                    }
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", source.display()));
                        continue;
                    }
                };
                match self.save_new(&recipe) {
                    Ok(path) => report.paths.push(path),
                    Err(error) => report.errors.push(format!("{}: {error}", recipe.name)),
                }
            }
        }
        report
    }

    pub(crate) fn export_bundle(
        &self,
        recipes: &[WeaponRecipe],
        path: &Path,
    ) -> Result<(), String> {
        self.validate_export_path(path)?;
        if recipes.is_empty() {
            return Err("Select at least one recipe to export".into());
        }
        let recipes = recipes
            .iter()
            .map(|recipe| {
                let encoded = recipe.to_json_pretty().map_err(|error| error.to_string())?;
                serde_json::from_str(&encoded).map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let bundle = RecipeBundle {
            format: BUNDLE_FORMAT.into(),
            version: 1,
            recipes,
        };
        let mut encoded =
            serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
        encoded.push('\n');
        // A bundle is written only when it imports again as it stands.
        for (index, value) in parse_import(&encoded)?.recipes.into_iter().enumerate() {
            WeaponRecipe::from_json_str(&value.to_string()).map_err(|error| {
                format!(
                    "Recipe {} would not import from this bundle: {error}",
                    index + 1
                )
            })?;
        }
        sundial::package_authoring::replace_authoring_file(path, encoded.as_bytes())
            .map_err(|error| error.to_string())
    }

    fn validate_export_path(&self, path: &Path) -> Result<(), String> {
        use sundial::package_authoring::{path_is_within, resolve_path_for_comparison};

        let target = resolve_path_for_comparison(path).map_err(|error| error.to_string())?;
        let root = resolve_path_for_comparison(self.root()).map_err(|error| error.to_string())?;
        if path_is_within(&target, &root) {
            return Err("Choose an export location outside the recipe library. Use Duplicate to create a library copy.".into());
        }
        Ok(())
    }
}

fn read_import(path: &Path) -> Result<Import, String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut encoded = String::new();
    file.take(MAX_IMPORT_BYTES + 1)
        .read_to_string(&mut encoded)
        .map_err(|error| error.to_string())?;
    if encoded.len() as u64 > MAX_IMPORT_BYTES {
        return Err(format!(
            "This file is larger than the {} MiB an import reads",
            MAX_IMPORT_BYTES >> 20
        ));
    }
    parse_import(&encoded)
}

/// A recipe or a bundle of them, checked as a whole before any recipe's pictures are decoded.
fn parse_import(encoded: &str) -> Result<Import, String> {
    // The bundle's own containers do not count against each recipe's nesting limit. Each recipe
    // is parsed again on its own with that limit when it is imported.
    let document: serde_json::Value = sundial::package_authoring::parse_json_envelope(
        encoded,
        BUNDLE_ENVELOPE + crate::recipe::RECIPE_NESTING,
    )
    .map_err(|error| error.to_string())?;
    let pictures = picture_bytes(&document)?;
    if pictures > MAX_IMPORT_PICTURE_BYTES {
        return Err(format!(
            "The pictures in this file would take {} MiB to open, more than the {} MiB an import may use",
            pictures >> 20,
            MAX_IMPORT_PICTURE_BYTES >> 20
        ));
    }
    if document.get("format").is_none() {
        return Ok(Import {
            bundle: false,
            recipes: vec![document],
        });
    }
    let bundle: RecipeBundle =
        serde_json::from_value(document).map_err(|error| error.to_string())?;
    if bundle.format != BUNDLE_FORMAT || bundle.version != 1 {
        return Err("Unsupported recipe bundle format or version".into());
    }
    if bundle.recipes.is_empty() {
        return Err("This recipe bundle is empty".into());
    }
    if bundle.recipes.len() > MAX_BUNDLE_RECIPES {
        return Err(format!(
            "This bundle holds {} recipes, more than the {MAX_BUNDLE_RECIPES} an import takes",
            bundle.recipes.len()
        ));
    }
    Ok(Import {
        bundle: true,
        recipes: bundle.recipes,
    })
}

/// The decoded memory every embedded picture in `value` asks for, from the headers alone.
fn picture_bytes(value: &serde_json::Value) -> Result<u64, String> {
    use serde_json::Value;
    match value {
        Value::Object(members) => {
            let mut total = 0_u64;
            if let Some(Value::String(encoded)) = members.get("png_base64") {
                let (width, height) = crate::image_import::embedded_dimensions(encoded)?;
                total = u64::from(width) * u64::from(height) * 4;
            }
            members.values().try_fold(total, |total, member| {
                Ok(total.saturating_add(picture_bytes(member)?))
            })
        }
        Value::Array(items) => items.iter().try_fold(0_u64, |total, item| {
            Ok(total.saturating_add(picture_bytes(item)?))
        }),
        _ => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shipped default goes out in one bundle and comes back with the same content. Hammer
    /// Time nests as deep as a recipe may, so the bundle's own containers must not count against
    /// it.
    #[test]
    fn every_bundled_default_survives_one_shared_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let defaults = super::super::BUNDLED_RECIPES
            .iter()
            .map(|(_, json)| WeaponRecipe::from_json_str(json).unwrap())
            .collect::<Vec<_>>();
        let bundle = directory.path().join("defaults.parhelion-bundle.json");
        library.export_bundle(&defaults, &bundle).unwrap();
        let import = read_import(&bundle).unwrap();
        assert!(import.bundle);
        assert_eq!(import.recipes.len(), defaults.len());
        for (value, original) in import.recipes.iter().zip(&defaults) {
            let recipe = WeaponRecipe::from_json_str(&value.to_string())
                .unwrap_or_else(|error| panic!("{}: {error}", original.name));
            assert!(recipe.same_saved_content(original), "{}", original.name);
        }
    }
}
