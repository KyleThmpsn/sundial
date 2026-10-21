//! Merge data beside each package a mod-folder build writes. Sunrise reads it to merge the rows
//! Parhelion adds to installed tables with the rows other mods add, and to renumber the rows of a
//! mod that loads later.
//!
//! The data is read back from the build's own package bytes, so an entry the loader would read
//! from a compressed block fails here rather than in the game. A build whose merge data can't be
//! written ships none, as builds did before, and its report says why.
use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};
use sundial::package_authoring::{
    PackageManager,
    mod_content::{
        merge::{describe, recipes, sidecar},
        plaintext,
    },
    open_shadowkeep_package_manager,
};
use tiger_pkg::TagHash;

/// The build's merge data report, beside its manifest.
pub(crate) const REPORT_FILE_NAME: &str = "merge-data.json";

/// Each package's merge data file by name, and the build's report.
pub(crate) struct MergeData {
    pub(crate) sidecars: Vec<(String, Vec<u8>)>,
    pub(crate) report: Vec<u8>,
}

#[derive(Serialize)]
struct Report<'a> {
    version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<&'a str>,
    pairs: &'a [String],
    entries: &'a [describe::Description],
}

/// The build's source packages, read for stock entries.
struct Stock<'a>(&'a PackageManager);

impl describe::Stock for Stock<'_> {
    fn has(&mut self, tag: u32) -> bool {
        self.0.get_entry(TagHash(tag)).is_some()
    }

    fn read(&mut self, tag: u32) -> Option<Vec<u8>> {
        self.0.read_tag(TagHash(tag)).ok()
    }
}

/// Merge data for every package of a mod-folder build, by file name and bytes, from the source
/// packages the build read.
pub(crate) fn describe(packages: &Path, files: &[(&str, &[u8])]) -> MergeData {
    describe_all(packages, files).unwrap_or_else(|reason| MergeData {
        sidecars: Vec::new(),
        report: encode(&Report {
            version: sidecar::VERSION,
            skipped: Some(&reason),
            pairs: &[],
            entries: &[],
        }),
    })
}

fn encode(report: &Report<'_>) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(report).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn describe_all(packages: &Path, files: &[(&str, &[u8])]) -> Result<MergeData, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut build = describe::Build::default();
    let mut described = Vec::with_capacity(files.len());
    for &(name, bytes) in files {
        let unreadable = |reason: plaintext::Unreadable| format!("{name} {}", reason.text());
        let records = plaintext::records_in(bytes).map_err(unreadable)?;
        let base = manager.package_paths.get(&records.package);
        if let Some(base) = base {
            // A patch changes the entries whose records differ from its stock base, as the
            // loader finds them.
            let stock = plaintext::records(Path::new(&base.path))
                .map_err(|reason| format!("{} {}", base.filename, reason.text()))?;
            if stock.rows.len() != records.rows.len() {
                return Err(format!("{name} changes its package's entry count"));
            }
            let changed = (0..records.rows.len())
                .filter(|&index| records.rows[index] != stock.rows[index])
                .map(|index| index as u32)
                .collect::<Vec<_>>();
            let built = plaintext::read_entries_in(bytes, &changed).map_err(unreadable)?;
            for (&index, built) in changed.iter().zip(built) {
                let tag = TagHash::new(records.package, index as u16);
                let stock = manager
                    .read_tag(tag)
                    .map_err(|error| format!("stock entry {tag} can't be read: {error}"))?;
                let class = records.class(index as usize);
                build.edited.insert(
                    tag.0,
                    describe::Edited {
                        class,
                        stock,
                        built,
                    },
                );
            }
        } else {
            let read = (0..records.rows.len())
                .filter(|&index| recipes::has_class_rules(records.class(index)))
                .map(|index| index as u32)
                .collect::<Vec<_>>();
            let built = plaintext::read_entries_in(bytes, &read).map_err(unreadable)?;
            for (&index, built) in read.iter().zip(built) {
                let tag = TagHash::new(records.package, index as u16);
                let class = records.class(index as usize);
                build.added.insert(tag.0, describe::Added { class, built });
            }
        }
        described.push(describe::Package {
            id: records.package,
            patch: records.patch,
            digest: records.digest,
            installed: base.is_some(),
        });
    }
    let result = describe::describe(&described, &build, &mut Stock(&manager))?;
    let mut sidecars = Vec::with_capacity(files.len());
    for (&(name, _), sidecar) in files.iter().zip(&result.sidecars) {
        let stem = name
            .strip_suffix(".pkg")
            .ok_or_else(|| format!("{name} isn't a package file name"))?;
        let bytes = sidecar
            .write()
            .map_err(|status| format!("{name}'s merge data is invalid: {}", status.text()))?;
        sidecars.push((format!("{stem}.{}", sidecar::EXTENSION), bytes));
    }
    Ok(MergeData {
        sidecars,
        report: encode(&Report {
            version: sidecar::VERSION,
            skipped: None,
            pairs: &result.pairs,
            entries: &result.entries,
        }),
    })
}

/// Writes the merge data beside the staged packages and returns the merge data files.
pub(crate) fn stage(directory: &Path, data: &MergeData) -> Result<Vec<PathBuf>, String> {
    write_new(&directory.join(REPORT_FILE_NAME), &data.report)?;
    data.sidecars
        .iter()
        .map(|(name, bytes)| {
            if let Some(path) = crate::mod_family::build::kept_path(name) {
                if std::fs::read(&path).map_err(|e| e.to_string())? != *bytes {
                    return Err(format!("Kept merge data {name} changed with this build"));
                }
                return Ok(path);
            }
            let path = directory.join(name);
            write_new(&path, bytes)?;
            Ok(path)
        })
        .collect()
}

/// A staging run is new and exclusively leased, so its files are written once, in place.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}
