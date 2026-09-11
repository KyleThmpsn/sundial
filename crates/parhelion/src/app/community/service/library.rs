//! Apply community downloads through the existing recipe library's checked writes.
use super::{Entry, checksum, slug};
use crate::{RecipeLibrary, WeaponRecipe};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub(crate) struct Downloaded {
    pub entry: Entry,
    pub recipe: WeaponRecipe,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Receipt {
    pub schema: u32,
    pub id: String,
    pub file_name: String,
    pub version: u32,
    pub sha256: String,
    pub baseline: WeaponRecipe,
}

impl Downloaded {
    pub fn checked(entry: Entry, bytes: &[u8]) -> Result<Self, String> {
        entry.listing.validate()?;
        if bytes.len() != entry.bytes || checksum(bytes) != entry.sha256 {
            return Err("This download does not match the catalog checksum. Refresh the catalog and try again".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "The recipe is not valid UTF-8")?;
        let recipe = WeaponRecipe::from_json_str(text)
            .map_err(|error| format!("This recipe is not supported: {error}"))?;
        if recipe.namespace != entry.namespace || recipe.name != entry.name {
            return Err("The downloaded recipe does not match its catalog listing".into());
        }
        Ok(Self { entry, recipe })
    }
}

fn receipt_path(library: &RecipeLibrary, id: &str) -> Result<PathBuf, String> {
    if !slug(id) {
        return Err("Invalid community recipe ID".into());
    }
    // Direct sidecar files avoid directory traversal and do not match the recipe suffix.
    Ok(library.root().join(format!("community-{id}.receipt")))
}

pub(crate) fn load_receipt(library: &RecipeLibrary, id: &str) -> Result<Option<Receipt>, String> {
    let path = receipt_path(library, id)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not inspect community download history: {error}"
            ));
        }
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err("Community download history is not a regular file".into());
        }
        Ok(metadata) if metadata.len() > (super::MAX_RECIPE_BYTES * 2) as u64 => {
            return Err("Community download history exceeds the supported size".into());
        }
        Ok(_) => {}
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("Could not read community download history: {error}"))?;
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not read community download history: {error}"))?;
    let mut parts = Path::new(&receipt.file_name).components();
    if receipt.schema != 1
        || receipt.id != id
        || !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || !receipt.file_name.ends_with(".parhelion.json")
    {
        return Err("Invalid community download history".into());
    }
    Ok(Some(receipt))
}

fn save_receipt(
    library: &RecipeLibrary,
    downloaded: &Downloaded,
    destination: &Path,
) -> Result<(), String> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid library filename")?
        .to_owned();
    let receipt = Receipt {
        schema: 1,
        id: downloaded.entry.listing.id.clone(),
        file_name,
        version: downloaded.entry.listing.version,
        sha256: downloaded.entry.sha256.clone(),
        baseline: downloaded.recipe.clone(),
    };
    let path = receipt_path(library, &receipt.id)?;
    let bytes = serde_json::to_vec(&receipt).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(library.root()).map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(&path)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// A dirty open document is checked again by the UI immediately before this function.
pub(crate) fn install(library: &RecipeLibrary, downloaded: &Downloaded) -> Result<PathBuf, String> {
    let existing = load_receipt(library, &downloaded.entry.listing.id)?;
    let destination = match existing {
        Some(receipt) => update(library, downloaded, &receipt)?,
        None => add(library, downloaded)?,
    };
    save_receipt(library, downloaded, &destination).map_err(|error| format!(
        "The recipe is saved at {} but its update history could not be saved: {error}. Keep this file and retry Add To Library to recover tracking", destination.display()))?;
    Ok(destination)
}

fn add(library: &RecipeLibrary, downloaded: &Downloaded) -> Result<PathBuf, String> {
    let scan = library.scan()?;
    if !scan.errors.is_empty() {
        return Err("Repair unreadable local recipes before adding community recipes".into());
    }
    if let Some(existing) = scan
        .entries
        .iter()
        .find(|entry| entry.namespace == downloaded.recipe.namespace)
    {
        let recipe = WeaponRecipe::load_json(&existing.path).map_err(|error| error.to_string())?;
        if recipe == downloaded.recipe {
            return Ok(existing.path.clone());
        }
        return Err(
            "A different local recipe already uses this identity. Make a remix to keep both".into(),
        );
    }
    library.save_new(&downloaded.recipe)
}

fn update(
    library: &RecipeLibrary,
    downloaded: &Downloaded,
    receipt: &Receipt,
) -> Result<PathBuf, String> {
    let path = library.root().join(&receipt.file_name);
    if downloaded.entry.listing.version < receipt.version {
        return Err("The catalog revision is older than your downloaded recipe".into());
    }
    if downloaded.recipe.namespace != receipt.baseline.namespace
        || downloaded.recipe.identity != receipt.baseline.identity
    {
        return Err(
            "This update changes the weapon identity. It must be published as a separate recipe"
                .into(),
        );
    }
    if fs::symlink_metadata(&path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return library.save_new(&downloaded.recipe);
    }
    let current = WeaponRecipe::load_json(&path)
        .map_err(|error| format!("Could not check your downloaded recipe: {error}"))?;
    // Recover tracking after a previous successful recipe write and failed receipt write.
    if current == downloaded.recipe {
        return Ok(path);
    }
    if downloaded.entry.listing.version == receipt.version
        && downloaded.entry.sha256 != receipt.sha256
    {
        return Err(
            "The author changed a published revision without increasing its version".into(),
        );
    }
    if current != receipt.baseline {
        return Err("You have edited this recipe locally. Make a remix of the community version to keep both".into());
    }
    library.save_existing_if_unchanged(&path, &receipt.baseline, &downloaded.recipe)?;
    Ok(path)
}

pub(crate) fn remix(
    library: &RecipeLibrary,
    downloaded: &Downloaded,
    name: &str,
) -> Result<PathBuf, String> {
    let mut recipe = downloaded.recipe.clone();
    recipe
        .rename_authored_item(name.trim())
        .map_err(|error| error.to_string())?;
    if recipe.namespace == downloaded.recipe.namespace {
        return Err("Give the remix a different name to create its own weapon identity".into());
    }
    let path = library.save_new(&recipe)?;
    let origin_path = library
        .root()
        .join(format!("remix-{}.receipt", recipe.namespace));
    let origin = serde_json::json!({"schema": 1, "namespace": recipe.namespace, "original": downloaded.entry.listing.id});
    sundial::package_authoring::replace_authoring_file(&origin_path, origin.to_string().as_bytes())
        .map_err(|error| {
            format!(
                "The remix was saved at {} but its creator credit could not be recorded: {error}",
                path.display()
            )
        })?;
    Ok(path)
}

pub(crate) fn remix_origin(
    library: &RecipeLibrary,
    namespace: &str,
) -> Result<Option<String>, String> {
    crate::recipe::validate_parhelion_namespace(namespace)?;
    let path = library.root().join(format!("remix-{namespace}.receipt"));
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err("Invalid remix credit file".into());
    }
    let origin: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let id = origin["original"]
        .as_str()
        .filter(|id| slug(id))
        .ok_or("Invalid remix credit")?;
    if origin["schema"] != 1 || origin["namespace"] != namespace {
        return Err("Invalid remix credit".into());
    }
    Ok(Some(id.into()))
}
