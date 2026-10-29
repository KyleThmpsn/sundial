//! Conversions made before every conversion kept its graphs in `graph` and `ornaments/<HASH>/graph`
//! (see `parhelion_import::place_graphs`). A sweep of every item moves a library recipe's graphs
//! into their places and saves the recipe naming them there, once nothing else needs the old
//! folders. A conversion that another saved file names stays where it is, since moving it would
//! break that file: a recipe backup, a staged run, a kept install or package backup, a map, a
//! second library recipe such as a duplicate, or the recipe open in the editor. It moves in a
//! later session once those files are gone.
use super::*;

impl RecipeLibrary {
    /// Arranges the conversions of every library recipe that alone names them, from `read`, the
    /// snapshots this sweep read, and returns how many recipes it saved.
    pub(super) fn arrange_conversions(
        &self,
        roots: &Roots,
        read: &BTreeMap<PathBuf, Snapshot>,
        open: &[serde_json::Value],
        status: &Status,
        errors: &mut Vec<String>,
    ) -> usize {
        let library: BTreeSet<PathBuf> = match self.recipe_paths() {
            Ok(paths) => paths.into_iter().collect(),
            Err(error) => {
                errors.push(error);
                return 0;
            }
        };
        let mut held = BTreeSet::new();
        let mut library_names = BTreeMap::<&String, usize>::new();
        for (path, snapshot) in read {
            if library.contains(path) {
                for name in &snapshot.names {
                    *library_names.entry(name).or_default() += 1;
                }
            } else {
                held.extend(snapshot.names.iter().cloned());
            }
        }
        held.extend(
            library_names
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .map(|(name, _)| name.clone()),
        );
        for document in open {
            names(document, roots, &mut held);
        }
        let pending = library
            .iter()
            .filter(|path| {
                read.get(*path).is_some_and(|snapshot| {
                    !snapshot.names.is_empty()
                        && snapshot.names.iter().all(|name| !held.contains(name))
                }) && !arranged(path)
            })
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return 0;
        }
        status.start(Step::Arranging, pending.len());
        let mut saved = 0;
        for path in pending {
            status.update(|progress| {
                progress.current = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
            });
            match self.arrange(path) {
                Ok(true) => saved += 1,
                Ok(false) => {}
                Err(error) => errors.push(format!(
                    "Could not arrange the conversions of {}: {error}",
                    path.display()
                )),
            }
            status.update(|progress| progress.done += 1);
        }
        saved
    }

    /// Moves the library recipe at `path`'s graphs into their places and saves it naming them
    /// there. A save that fails, such as for a recipe changed meanwhile, moves them back. Returns
    /// whether it saved.
    fn arrange(&self, path: &Path) -> Result<bool, String> {
        let baseline = WeaponRecipe::load_json(path).map_err(|error| error.to_string())?;
        let Some(reference) = &baseline.overrides.imported_graph else {
            return Ok(false);
        };
        let Some(conversion) = parhelion_import::conversion_folder(&reference.directory) else {
            return Ok(false);
        };
        let owner = baseline
            .identity
            .item_hash
            .parse_u32()
            .map_err(|error| error.to_string())?;
        let (placed, moves) = parhelion_import::place_graphs(reference, owner, &conversion)
            .map_err(|error| format!("{error:#}"))?;
        if moves.is_empty() {
            return Ok(false);
        }
        let mut updated = baseline.clone();
        updated.overrides.imported_graph = Some(placed);
        if let Err(error) = self.save_existing_if_unchanged(path, &baseline, &updated) {
            return Err(match parhelion_import::unplace_graphs(&moves) {
                Ok(()) => error,
                Err(undo) => format!("{error}. Its graphs could not move back: {undo:#}"),
            });
        }
        parhelion_import::marathon::browse::renote(&moves);
        Ok(true)
    }
}

/// Whether the recipe file at `path` names its graphs in their places already, or names none.
/// Only the reference is read, so most recipes are passed over without loading them whole.
fn arranged(path: &Path) -> bool {
    let reference = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|recipe| {
            serde_json::from_value::<parhelion_import::GraphReference>(
                recipe["overrides"]["imported_graph"].clone(),
            )
            .ok()
        });
    reference.is_none_or(|reference| parhelion_import::graphs_in_place(&reference))
}
