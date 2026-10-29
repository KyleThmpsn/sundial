//! Library recipes that earlier versions saved with their imported assets embedded. A sweep of
//! every item first saves each one again. Loading it restores its assets into its source item's
//! import folder, and saving names that folder, so the sweep keeps the restored assets.
use super::*;

/// The key an embedded imported graph carries.
const EMBEDDED: &[u8] = b"\"embedded_assets\"";

impl RecipeLibrary {
    /// Saves again every library recipe whose imported graph embeds its assets, and returns how
    /// many it saved. A recipe that cannot be restored or saved keeps its file.
    pub(super) fn link_embedded_recipes(&self, status: &Status, errors: &mut Vec<String>) -> usize {
        let paths = match self.recipe_paths() {
            Ok(paths) => paths,
            Err(error) => {
                errors.push(error);
                return 0;
            }
        };
        let embedded = paths
            .into_iter()
            .filter(|path| embeds(path))
            .collect::<Vec<_>>();
        if embedded.is_empty() {
            return 0;
        }
        status.start(Step::Linking, embedded.len());
        let mut linked = 0;
        for path in embedded {
            status.update(|progress| {
                progress.current = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
            });
            let saved = WeaponRecipe::load_json(&path)
                .map_err(|error| error.to_string())
                .and_then(|recipe| self.save_existing_if_unchanged(&path, &recipe, &recipe));
            match saved {
                Ok(()) => linked += 1,
                Err(error) => errors.push(format!(
                    "Could not save {} with linked assets: {error}",
                    path.display()
                )),
            }
            status.update(|progress| progress.done += 1);
        }
        linked
    }
}

/// Whether the recipe file at `path` embeds imported assets. A file that cannot be read is left to
/// the snapshot read, which reports it.
fn embeds(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.windows(EMBEDDED.len()).any(|key| key == EMBEDDED))
}
