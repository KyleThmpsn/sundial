//! Local recipe sharing and copies with independent weapon identities.
use super::{BTreeSet, Path, PathBuf, RecipeLibrary, WeaponRecipe};

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

impl RecipeLibrary {
    pub fn duplicate(&self, recipe: &WeaponRecipe) -> Result<PathBuf, String> {
        let namespaces: BTreeSet<_> = self
            .scan()?
            .entries
            .into_iter()
            .map(|entry| entry.namespace.to_ascii_lowercase())
            .collect();
        for suffix in 1..=10_000 {
            let name = if suffix == 1 {
                format!("{} Copy", recipe.name)
            } else {
                format!("{} Copy {suffix}", recipe.name)
            };
            let mut copy = recipe.clone();
            copy.rename_authored_item(&name)
                .map_err(|error| format!("Could not duplicate recipe: {error}"))?;
            if !namespaces.contains(&copy.namespace.to_ascii_lowercase()) {
                return self.save_new(&copy);
            }
        }
        Err("Could not allocate an unused recipe copy identity".into())
    }

    pub fn export(&self, recipe: &WeaponRecipe, path: &Path) -> Result<(), String> {
        self.validate_export_path(path)?;
        recipe.save_json(path).map_err(|error| error.to_string())
    }

    pub(crate) fn import_files(&self, sources: &[PathBuf]) -> ImportReport {
        let mut report = ImportReport::default();
        for source in sources {
            let recipes = match read_import(source) {
                Ok(recipes) => recipes,
                Err(error) => {
                    report.errors.push(format!("{}: {error}", source.display()));
                    continue;
                }
            };
            for recipe in recipes {
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

fn read_import(path: &Path) -> Result<Vec<WeaponRecipe>, String> {
    let encoded = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: serde_json::Value =
        sundial::package_authoring::parse_json(&encoded).map_err(|error| error.to_string())?;
    if document.get("format").is_none() {
        return WeaponRecipe::from_json_str(&encoded)
            .map(|recipe| vec![recipe])
            .map_err(|error| error.to_string());
    }
    let bundle: RecipeBundle =
        serde_json::from_value(document).map_err(|error| error.to_string())?;
    if bundle.format != BUNDLE_FORMAT || bundle.version != 1 {
        return Err("Unsupported recipe bundle format or version".into());
    }
    if bundle.recipes.is_empty() {
        return Err("This recipe bundle is empty".into());
    }
    bundle
        .recipes
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            WeaponRecipe::from_json_str(&value.to_string())
                .map_err(|error| format!("Recipe {} in bundle: {error}", index + 1))
        })
        .collect()
}
