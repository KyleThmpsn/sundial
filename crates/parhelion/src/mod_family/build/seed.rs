//! The family decisions the last build of an install reached, kept in Parhelion's data folder
//! for the next build. Without them a family whose importers add rows to a shared table compiles
//! the main mod twice in every build: once before the importers' rows are known and once to write
//! them. The next build starts from the rows the main mod wrote for each importer and how many
//! entries the importers put before the stock entries of each Collections list, so its first pass
//! settles when nothing changed.
//!
//! The file is only a starting point. The first pass checks the seeded rows against what each
//! importer records like any other decision. Allocated rows require the same complete request,
//! stock inputs, resource fingerprints and package allocation. Older files without that identity
//! keep no rows. A failed first host can discard admitted rows once and restart, since settlement
//! cannot recover a failure that occurs before importer compilation.
//!
//! Coupling is not kept, because a seeded coupling would never let a review find that the
//! importer needs no main mod build, and the importer would never get its own mod back. A format
//! 2 file kept coupling, so the format changed and such a file is ignored. Planning couples the
//! importers whose recipes show a shared imported ability instead, in every build.
use super::*;

/// Raise when the kept decisions' meaning changes.
const FORMAT: u32 = 3;

/// Unknown fields are ignored, so a file that still holds the hosting build's flag, the tables
/// hosted for each importer or the Collections lists a member's entries yielded loads. The main
/// mod now builds whenever an importer has content and hosts every shared table, and every
/// member's entries go before the stock entries.
#[derive(Default, Deserialize, Serialize)]
pub(super) struct Seed {
    format: u32,
    /// Parhelion's version and the converter revision. The rows name each importer's assets as
    /// its build allocated them, which another version may allocate differently.
    version: String,
    /// Exact request and allocated identities for the hosted rows. Older seeds have no proof.
    #[serde(default)]
    inputs: Option<String>,
    contributions: BTreeMap<Importer, Contributions>,
    /// How many entries the importers put before the stock entries of each stock collectible
    /// array, as node, array offset and count. They count recipes, not allocated assets, so they
    /// hold across versions.
    #[serde(default)]
    fronts: Vec<(usize, usize, usize)>,
}

fn version() -> String {
    #[cfg(feature = "d2-model-importer")]
    let converter = parhelion_import::CONVERTER_REVISION;
    #[cfg(not(feature = "d2-model-importer"))]
    let converter = 0;
    format!("{}/{converter}", env!("CARGO_PKG_VERSION"))
}

/// A finished pass records the allocation it emitted. Planning compares the allocation retained
/// from the installed generation, so a staged but uninstalled seed cannot lend its tags to a new
/// allocation. The complete request also covers native members and private perk edits.
fn inputs(plan: &Plan, pass: &Pass, finished: bool) -> Option<String> {
    let allocations = plan
        .imports
        .iter()
        .map(|(owner, planned)| {
            let taken = pass.imports.get(owner)?;
            let own = if finished {
                &taken.state.own
            } else {
                &planned.previous
            };
            Some((owner, &planned.inputs, &taken.allocation, own))
        })
        .collect::<Option<Vec<_>>>()?;
    hash(&(&plan.seed_inputs, allocations)).ok()
}

/// Where the decisions for the install at `install_root` are kept.
pub(super) fn path(install_root: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let root = fs::canonicalize(install_root).unwrap_or_else(|_| install_root.to_owned());
    let key = Sha256::digest(root.to_string_lossy().to_lowercase().as_bytes());
    Some(
        sundial::package_authoring::parhelion_data_directory()?
            .join("cache/family")
            .join(format!("{}.json", hex::encode(&key[..16]))),
    )
}

impl Seed {
    /// The decisions in `path` for the importers `plan` builds, or none when the file is missing,
    /// unreadable or of another format. A file from another version keeps no rows.
    pub(super) fn load(path: &Path, plan: &Plan, pass: &Pass) -> Decisions {
        let Some(seed) = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|seed| seed.format == FORMAT)
        else {
            return Decisions::default();
        };
        let rows = seed.version == version()
            && seed.inputs.is_some()
            && seed.inputs == inputs(plan, pass, false);
        let planned = |importer: &Importer| plan.imports.contains_key(importer);
        // The main mod writes the importers' rows only in a pass where it builds, which it does
        // when some recipe or map is its own, or an importer's.
        let main_builds = plan.recipe_owners.contains(&None)
            || plan.map_owners.contains(&None)
            || plan.importers_have_content();
        let tables = hosted_tables(main_builds);
        let mut decisions = Decisions {
            fronts: seed
                .fronts
                .into_iter()
                .map(|(node, array, rows)| ((node, array), rows))
                .collect(),
            contributions: seed
                .contributions
                .into_iter()
                .filter(|(importer, _)| rows && planned(importer))
                .map(|(importer, contributions)| (importer, contributions.within(&tables)))
                .filter(|(_, contributions)| !contributions.is_empty())
                .collect(),
            ..Decisions::default()
        };
        decisions.seeded = !decisions.contributions.is_empty() || !decisions.fronts.is_empty();
        decisions
    }

    /// The decisions `current` settled on.
    pub(super) fn of(current: &Session) -> Self {
        let decisions = &current.decisions;
        Self {
            format: FORMAT,
            version: version(),
            inputs: inputs(&current.plan, &current.pass, true),
            contributions: decisions.contributions.clone(),
            fronts: decisions
                .fronts
                .iter()
                .map(|(&(node, array), &rows)| (node, array, rows))
                .collect(),
        }
    }

    /// Writes the decisions through a temporary file, so a reader never sees half of them. A
    /// failure only costs the next build its first pass, so it is ignored.
    pub(super) fn save(&self, path: &Path) {
        let Some(folder) = path.parent() else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(self) else {
            return;
        };
        let temporary = path.with_extension("json.tmp");
        let written = fs::create_dir_all(folder)
            .and_then(|()| fs::write(&temporary, bytes))
            .and_then(|()| fs::rename(&temporary, path));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }
}
