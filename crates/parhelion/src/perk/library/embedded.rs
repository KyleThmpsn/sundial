//! Materialize embedded weapon perks once, without linking subsequent library edits.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub struct ImportReport {
    pub added: usize,
    pub errors: Vec<String>,
}

impl Library {
    /// Receives fully resolved perks, including inherited stock effects and stats.
    /// Content identities ignore document IDs and socket positions. A durable history
    /// keeps an imported perk from returning after the reader edits or deletes it.
    pub fn import_embedded(
        &self,
        recipes: impl IntoIterator<Item = PerkRecipe>,
    ) -> Result<ImportReport, String> {
        let _lock = self.lock()?;
        let history_path = self.root.join("imported-weapon-perks.json");
        let mut history: BTreeSet<String> = match fs::read(&history_path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("Could not read embedded perk import history: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => return Err(error.to_string()),
        };
        let before = history.clone();
        let scan = self.scan()?;
        // Do not risk duplicating a library entry that could not be read.
        if !scan.errors.is_empty() {
            return Err(format!(
                "Embedded perk import paused until the library can be read:\n{}",
                scan.errors.join("\n")
            ));
        }
        let mut existing = scan
            .entries
            .iter()
            .map(|entry| identity(&entry.recipe))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut report = ImportReport::default();
        for mut recipe in recipes {
            let result = (|| {
                recipe.validate()?;
                let key = identity(&recipe)?;
                if history.contains(&key) {
                    return Ok(());
                }
                if !existing.contains(&key) {
                    recipe.id = key.clone();
                    let path = self.root.join(format!("{key}.perk.json"));
                    let mut bytes =
                        serde_json::to_vec_pretty(&recipe).map_err(|error| error.to_string())?;
                    bytes.push(b'\n');
                    // Never overwrite a file, even if its name matches the content key.
                    sundial::storage::create_file(&path, &bytes)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    existing.insert(key.clone());
                    report.added += 1;
                }
                history.insert(key);
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                report
                    .errors
                    .push(format!("Could not import {}: {error}", recipe.name));
            }
        }
        if history != before {
            let bytes = serde_json::to_vec_pretty(&history).map_err(|error| error.to_string())?;
            sundial::package_authoring::replace_authoring_file(&history_path, &bytes)
                .map_err(|error| format!("Could not save embedded perk import history: {error}"))?;
        }
        Ok(report)
    }
}

fn identity(recipe: &PerkRecipe) -> Result<String, String> {
    let mut content = recipe.clone();
    content.id.clear();
    let bytes = serde_json::to_vec(&content).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests;
