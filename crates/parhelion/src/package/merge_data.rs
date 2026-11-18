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
    /// The stock entries the build changes and how, or `None` when it ships no merge data.
    pub(crate) touched: Option<Vec<Touch>>,
}

/// One stock entry a build changes, as Sunrise would merge it with another mod's change.
#[derive(Clone, Debug)]
pub(crate) struct Touch {
    pub(crate) tag: u32,
    /// The entry's class, which names what it holds when two members can't share it.
    pub(crate) class: u32,
    /// Sunrise serves the entry from the mod that loads last.
    pub(crate) opaque: bool,
    /// The aligned 8-byte words of the stock entry its diffs change, outside the ranges Sunrise
    /// rebuilds for every mod. Sunrise keeps the later mod's value of a word two mods change.
    pub(crate) words: Vec<Word>,
}

/// One aligned 8-byte word of a stock entry a build's diffs change.
#[derive(Clone, Debug)]
pub(crate) struct Word {
    pub(crate) index: u32,
    /// The word's built bytes, each `None` where Sunrise rebuilds it for every mod. `None` as a
    /// whole where a relocated site sits in the word: Sunrise shifts those bytes by load order,
    /// so equal built bytes there can still point at different rows.
    pub(crate) bytes: Option<[Option<u8>; 8]>,
}

impl Word {
    /// Whether Sunrise would keep only one build's value of a word both change. The loader takes
    /// a later copy's word whole where a byte an earlier copy changed differs in it, so two builds
    /// that write the same bytes share the word.
    fn clashes(&self, other: &Self) -> bool {
        self.index == other.index
            && match (&self.bytes, &other.bytes) {
                (Some(mine), Some(theirs)) => mine
                    .iter()
                    .zip(theirs)
                    .any(|pair| matches!(pair, (Some(a), Some(b)) if a != b)),
                _ => true,
            }
    }
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
            version: sidecar::OLDEST_VERSION,
            skipped: Some(&reason),
            pairs: &[],
            entries: &[],
        }),
        touched: None,
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
    let rebuilt = result
        .entries
        .iter()
        .filter_map(|entry| Some((u32::from_str_radix(&entry.tag, 16).ok()?, &entry.rebuilt)))
        .collect::<std::collections::BTreeMap<_, _>>();
    // Sunrise merges no rows while a pair group's tables gain different counts, so then every
    // entry is served whole from one mod.
    let unpaired = !result.pairs.is_empty();
    let mut touched = Vec::new();
    for (package, sidecar) in described.iter().zip(&result.sidecars) {
        if !package.installed {
            continue;
        }
        for entry in &sidecar.entries {
            let tag = TagHash::new(sidecar.package, entry.index as u16).0;
            let rebuilt = rebuilt.get(&tag).map_or(&[][..], |ranges| &ranges[..]);
            let is_rebuilt = |at: u32| {
                rebuilt
                    .iter()
                    .any(|&(begin, end)| (begin..end).contains(&at))
            };
            let mut indices = entry
                .diffs
                .iter()
                .flat_map(|diff| diff.offset..diff.offset + diff.old.len() as u32)
                .filter(|&at| !is_rebuilt(at))
                .map(|at| at / 8)
                .collect::<Vec<_>>();
            indices.dedup();
            let edited = build.edited.get(&tag);
            // Built bytes in stock coordinates. Bytes past the stock size belong to added blobs.
            let stock_region = edited.map_or(&[][..], |edited| {
                let end = edited.stock.len().min(edited.built.len());
                &edited.built[..end]
            });
            let words = indices
                .into_iter()
                .map(|index| {
                    let start = index * 8;
                    let relocated = entry.sites.iter().any(|site| {
                        site.offset < start + 8 && site.offset + u32::from(site.width) > start
                    });
                    let bytes = (!relocated && edited.is_some()).then(|| {
                        std::array::from_fn(|byte| {
                            let at = start + byte as u32;
                            (!is_rebuilt(at))
                                .then(|| stock_region.get(at as usize).copied())
                                .flatten()
                        })
                    });
                    Word { index, bytes }
                })
                .collect();
            touched.push(Touch {
                tag,
                class: edited.map_or(0, |edited| edited.class),
                opaque: unpaired || entry.kind == sidecar::EntryKind::Opaque,
                words,
            });
        }
    }
    let mut sidecars = Vec::with_capacity(files.len());
    // Each file takes the oldest version that holds it, and the report names the newest written.
    let mut version = sidecar::OLDEST_VERSION;
    for (&(name, _), sidecar) in files.iter().zip(&result.sidecars) {
        version = version.max(sidecar.version());
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
            version,
            skipped: None,
            pairs: &result.pairs,
            entries: &result.entries,
        }),
        touched: Some(touched),
    })
}

/// What an entry Sunrise can't merge holds, for a reason the user reads.
fn shared(class: u32) -> &'static str {
    match class {
        0x8080_9C36 => "an ability bank",
        0x8080_47BD => "the crosshair table",
        0x8080_8EE1 => "an animation parameter dictionary",
        0x8080_3465 | 0x8080_36F6 | 0x8080_345F | 0x8080_3478 | 0x8080_348E | 0x8080_34EB => {
            "the character animation tables"
        }
        0x8080_9A88 | 0x8080_9A8A => "HUD status names",
        0x8080_76F0 | 0x8080_5E0A => "the activity tables",
        0x8080_7571 | 0x8080_59B9 => "the private match tables",
        0x8080_775B => "the Collections counts",
        _ => "game data",
    }
}

/// Why an importer's build can't ship beside the other members' as it is.
#[derive(Debug, Default)]
pub(crate) struct Conflict {
    /// Sentences saying why none of the importer's entries can be described, so no item can be
    /// named for them.
    pub(crate) whole: std::collections::BTreeSet<String>,
    /// Each stock entry the importer's build changes that another member's also changes in a way
    /// Sunrise wouldn't merge, with what the entry holds and each such member's name.
    pub(crate) entries:
        std::collections::BTreeMap<u32, std::collections::BTreeSet<(&'static str, &'static str)>>,
}

impl Conflict {
    /// Every reason, as sentences.
    pub(crate) fn reasons(&self) -> Vec<String> {
        let shares = self
            .entries
            .values()
            .flatten()
            .collect::<std::collections::BTreeSet<_>>();
        self.whole
            .iter()
            .cloned()
            .chain(
                shares
                    .into_iter()
                    .map(|(what, with)| format!("Shares {what} with {with}.")),
            )
            .collect()
    }
}

/// The importers whose builds change a stock entry another member's build also changes in a
/// way Sunrise wouldn't merge, or whose merge data can't be written. `builds` pairs each member's
/// importer, or `None` for the main mod, with its merge data.
pub(crate) fn conflicts(
    builds: &[(
        Option<sundial::package_authoring::mod_content::Importer>,
        &MergeData,
    )],
) -> std::collections::BTreeMap<sundial::package_authoring::mod_content::Importer, Conflict> {
    use std::collections::BTreeMap;
    let name = |owner: Option<sundial::package_authoring::mod_content::Importer>| {
        owner.map_or("Parhelion", |importer| importer.name())
    };
    let mut found = BTreeMap::<_, Conflict>::new();
    // Merge data a member can't ship leaves Sunrise to serve every shared entry from one mod.
    let mut entries = BTreeMap::<u32, Vec<(Option<_>, &Touch)>>::new();
    for &(owner, data) in builds {
        match &data.touched {
            Some(touched) => {
                for touch in touched {
                    entries.entry(touch.tag).or_default().push((owner, touch));
                }
            }
            None => {
                if let Some(owner) = owner {
                    found
                        .entry(owner)
                        .or_default()
                        .whole
                        .insert("Its merge data couldn't be written.".into());
                }
            }
        }
    }
    // Installation takes merge data for every package of a build or for none, so without the
    // main mod's every importer builds with it.
    if builds
        .iter()
        .any(|&(owner, data)| owner.is_none() && data.touched.is_none())
    {
        for &(owner, _) in builds {
            if let Some(owner) = owner {
                found
                    .entry(owner)
                    .or_default()
                    .whole
                    .insert("Parhelion's merge data couldn't be written.".into());
            }
        }
    }
    for (tag, members) in entries {
        if members.len() < 2 {
            continue;
        }
        for (index, &(owner, touch)) in members.iter().enumerate() {
            let Some(owner) = owner else {
                continue;
            };
            for (other, &(them, theirs)) in members.iter().enumerate() {
                if other != index
                    && (touch.opaque
                        || theirs.opaque
                        || touch
                            .words
                            .iter()
                            .any(|word| theirs.words.iter().any(|other| word.clashes(other))))
                {
                    found
                        .entry(owner)
                        .or_default()
                        .entries
                        .entry(tag)
                        .or_default()
                        .insert((shared(touch.class), name(them)));
                }
            }
        }
    }
    found
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
