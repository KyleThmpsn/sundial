//! The family decisions the last build of an install reached, kept in Parhelion's data folder
//! for the next build. Without them a family whose importers share a table compiles every member
//! twice in every build: once to find the importers' rows and once to write them in the main mod.
//! The next build starts from the tables the main mod hosted, the rows it wrote for each importer
//! and how many entries the importers put before the stock entries of each Collections list, so
//! its first pass settles when nothing changed.
//!
//! The file is only a starting point. The first pass checks the seeded rows against what each
//! importer records like any other decision, so a missing, stale or unreadable file costs the
//! second pass and nothing else. A file from another Parhelion version or converter revision
//! keeps its decisions and loses only its rows, since the rows name assets as that version
//! allocated them. Until 2026-11-10 such a file was ignored whole, so a build after a converter
//! change wrote the shared tables in each importer and took a pass more to move them back.
//!
//! Coupling is not kept, because a seeded coupling would never let a review find that the
//! importer needs no main mod build, and the importer would never get its own mod back. A format
//! 2 file kept coupling, so the format changed and such a file is ignored.
use super::*;

/// Raise when the kept decisions' meaning changes.
const FORMAT: u32 = 3;

/// Unknown fields are ignored, so a file that still holds the hosting build's flag or the
/// Collections lists a member's entries yielded loads. The main mod now builds whenever an
/// importer has content, and every member's entries go before the stock entries.
#[derive(Default, Deserialize, Serialize)]
pub(super) struct Seed {
    format: u32,
    /// Parhelion's version and the converter revision. The rows name each importer's assets as
    /// its build allocated them, which another version may allocate differently.
    version: String,
    hosting: BTreeMap<Importer, BTreeSet<Shared>>,
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
    pub(super) fn load(path: &Path, plan: &Plan) -> Decisions {
        let Some(seed) = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|seed| seed.format == FORMAT)
        else {
            return Decisions::default();
        };
        let rows = seed.version == version();
        let planned = |importer: &Importer| plan.imports.contains_key(importer);
        let mut decisions = Decisions {
            hosting: seed
                .hosting
                .into_iter()
                .filter(|(importer, _)| planned(importer))
                .collect(),
            fronts: seed
                .fronts
                .into_iter()
                .map(|(node, array, rows)| ((node, array), rows))
                .collect(),
            ..Decisions::default()
        };
        // The main mod writes bank rows and Collections counts only in a pass where it builds,
        // which it does when some recipe or map is its own, or an importer's whose rows it hosts.
        let main_builds = plan.recipe_owners.contains(&None)
            || plan.map_owners.contains(&None)
            || plan.importers_have_content();
        decisions.contributions = seed
            .contributions
            .into_iter()
            .filter(|(importer, _)| rows && planned(importer))
            .map(|(importer, contributions)| {
                let tables = hosted_tables(&decisions, main_builds, importer);
                (importer, contributions.within(&tables))
            })
            .filter(|(_, contributions)| !contributions.is_empty())
            .collect();
        decisions
    }

    /// The decisions `current` settled on, without a hosted table an importer no longer adds rows
    /// to or builds on.
    pub(super) fn of(current: &Session) -> Self {
        let decisions = &current.decisions;
        let pass = &current.pass;
        let hosting = decisions
            .hosting
            .iter()
            .map(|(importer, tables)| {
                let rows = decisions.contributions.get(importer);
                let builds_on = pass.needs_main.get(importer);
                let kept = tables
                    .iter()
                    .copied()
                    .filter(|table| {
                        rows.is_some_and(|rows| !rows.within(&BTreeSet::from([*table])).is_empty())
                            || builds_on.is_some_and(|tables| tables.contains(table))
                    })
                    .collect::<BTreeSet<_>>();
                (*importer, kept)
            })
            .filter(|(_, tables)| !tables.is_empty())
            .collect::<BTreeMap<_, _>>();
        Self {
            format: FORMAT,
            version: version(),
            hosting,
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
