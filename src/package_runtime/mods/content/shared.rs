//! Entries several mods change, as Sunrise f1e7c66's loader serves them
//! (`state/mods/packages/mod_package_serve.cpp`). Where the mods describe their changes in merge
//! data, the loader merges their rows into one entry. Elsewhere the mod that loads last wins.
use super::super::ID;
use super::super::merge::{self, EntryInputs, Input, Outcome, Status, recipes, sidecar::Sidecar};
use super::super::plaintext::{self, Unreadable};
use super::{Added, Claim, Stage};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
};
use tiger_pkg::TagHash;

/// One package file whose entries the merge reads.
struct Source<'a> {
    owner: usize,
    path: &'a Path,
    sidecar: Option<&'a Sidecar>,
    package: u16,
    /// Entries to read, sorted and unique.
    indices: Vec<u32>,
    /// One copy per index, read from this file alone.
    read: Result<Vec<Vec<u8>>, Unreadable>,
}

/// One entry the merge takes, with its sources in load order.
struct Planned {
    package: u16,
    entry: u32,
    sources: Vec<usize>,
}

/// Why a mod's merge data is ignored whole, as the first file that broke a rule says.
enum Unusable {
    /// A package file of a mod with merge data has none.
    Missing,
    /// It sits beside an older generation of a package the mod adds.
    Older,
    Unread(Unreadable),
}

/// How Parhelion's mod loads beside the others.
#[derive(Clone, Debug, Default)]
pub struct Coexistence {
    /// Why the loader wouldn't serve Parhelion's content intact beside the other mods.
    pub problems: Vec<String>,
    /// Entries Parhelion shares with other mods that the loader merges.
    pub merged: Vec<String>,
}

/// What the loader serves for the entries mods share.
#[derive(Default)]
pub(super) struct Merged {
    /// Entries served with merged rows or shifted row numbers.
    pub replacements: Vec<(TagHash, Vec<u8>)>,
    /// Entries several mods change that are served merged, so no mod's changes are lost.
    pub entries: HashSet<TagHash>,
    pub coexistence: Coexistence,
}

fn tag(package: u16, entry: u32) -> TagHash {
    TagHash::new(package, entry as u16)
}

/// The rows each package's patches share, and each new package's row numbers, merged as the
/// loader merges them. `groups` holds each package's patches in load order.
pub(super) fn merge(
    stage: &Stage,
    groups: &BTreeMap<u16, Vec<&Claim>>,
    notices: &mut Vec<String>,
) -> Merged {
    let mut sources = Vec::new();
    let mut planned = Vec::new();
    for (&package, group) in groups {
        plan_group(stage, package, group, &mut sources, &mut planned);
    }
    plan_added(stage, &mut sources, &mut planned);
    let mut unusable = partial(stage);
    for (index, source) in sources.iter_mut().enumerate() {
        source.indices = planned
            .iter()
            .filter(|item| item.sources.contains(&index))
            .map(|item| item.entry)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if source.indices.is_empty() {
            continue;
        }
        source.read = plaintext::read_entries(source.path, &source.indices);
        if let (Err(reason), Some(_)) = (&source.read, source.sidecar) {
            unusable
                .entry(source.owner)
                .or_insert((source.package, Unusable::Unread(*reason)));
        }
    }
    for source in &mut sources {
        if unusable.contains_key(&source.owner) {
            source.sidecar = None;
        }
    }
    let mut ignored = BTreeMap::new();
    for (&owner, (package, reason)) in &unusable {
        let reason = match reason {
            Unusable::Missing => format!("its file for package {package:04X} has none"),
            Unusable::Older => {
                format!("its data for package {package:04X} describes an older generation")
            }
            Unusable::Unread(reason) => {
                format!("its file for package {package:04X} {}", reason.text())
            }
        };
        notices.push(all_ignored(&stage.admitted[owner], &reason));
        ignored.insert(owner, reason);
    }
    let entries = inputs(&planned, &sources);
    let outcome = if planned.is_empty() {
        Outcome::default()
    } else {
        merge::merge(&entries)
    };
    for item in &outcome.ignored {
        let reason = format!("for package {:04X}, {}", item.package, item.reason.text());
        notices.push(all_ignored(&stage.admitted[item.owner], &reason));
        ignored.entry(item.owner).or_insert(reason);
    }
    let mut merged = report(stage, groups, &entries, &outcome, notices);
    merged.coexistence = coexistence(stage, groups, &merged.entries, &outcome, &ignored);
    merged
}

/// Plans one package's patches: entries several of them change that merge data describes, and
/// entries one of them changes whose merge data names rows by number.
fn plan_group<'a>(
    stage: &'a Stage,
    package: u16,
    group: &[&'a Claim],
    sources: &mut Vec<Source<'a>>,
    planned: &mut Vec<Planned>,
) {
    let first = sources.len();
    for claim in group {
        sources.push(Source {
            owner: stage.owner(&claim.owner),
            path: &claim.path,
            sidecar: claim.sidecar.as_ref(),
            package,
            indices: Vec::new(),
            read: Ok(Vec::new()),
        });
    }
    let changed = group
        .iter()
        .flat_map(|claim| &claim.changed)
        .collect::<BTreeSet<_>>();
    for &changed in changed {
        let entry = u32::from(changed.entry_index());
        let mut item = Planned {
            package,
            entry,
            sources: Vec::new(),
        };
        let (mut described, mut sites) = (false, false);
        for (index, claim) in group.iter().enumerate() {
            if !claim.changed.contains(&changed) {
                continue;
            }
            item.sources.push(first + index);
            let spec = claim
                .sidecar
                .as_ref()
                .and_then(|sidecar| sidecar.entry(entry));
            described |= spec.is_some();
            sites |= spec.is_some_and(|spec| !spec.sites.is_empty());
        }
        let count = item.sources.len();
        if (count > 1 && described) || (count == 1 && sites) {
            planned.push(item);
        }
    }
}

/// Plans the entries of each new package whose merge data names rows by number. Entries a patch
/// changes are served from the patch, so they are left out.
fn plan_added<'a>(stage: &'a Stage, sources: &mut Vec<Source<'a>>, planned: &mut Vec<Planned>) {
    for added in &stage.added {
        let Some(sidecar) = added.sidecar.as_ref().filter(|_| stage.newest(added)) else {
            continue;
        };
        sources.push(Source {
            owner: stage.owner(&added.owner),
            path: &added.path,
            sidecar: Some(sidecar),
            package: added.package,
            indices: Vec::new(),
            read: Ok(Vec::new()),
        });
        for spec in &sidecar.entries {
            let changed = tag(added.package, spec.index);
            let claimed = stage
                .claims
                .iter()
                .any(|claim| claim.package == added.package && claim.changed.contains(&changed));
            if !spec.sites.is_empty() && !claimed {
                planned.push(Planned {
                    package: added.package,
                    entry: spec.index,
                    sources: vec![sources.len() - 1],
                });
            }
        }
    }
}

/// Mods whose merge data covers only some of their package files. Their rows may move, and a
/// file without merge data couldn't follow them.
fn partial(stage: &Stage) -> BTreeMap<usize, (u16, Unusable)> {
    let mut described = HashSet::new();
    for claim in stage.claims.iter().filter(|claim| claim.sidecar.is_some()) {
        described.insert(claim.owner.as_str());
    }
    for added in &stage.added {
        if added.sidecar.is_some() && stage.newest(added) {
            described.insert(added.owner.as_str());
        }
    }
    let mut unusable = BTreeMap::new();
    for claim in &stage.claims {
        if claim.sidecar.is_none() && described.contains(claim.owner.as_str()) {
            unusable
                .entry(stage.owner(&claim.owner))
                .or_insert((claim.package, Unusable::Missing));
        }
    }
    for added in &stage.added {
        let reason = match (stage.newest(added), added.sidecar.is_some()) {
            (true, false) if described.contains(added.owner.as_str()) => Unusable::Missing,
            (false, true) => Unusable::Older,
            _ => continue,
        };
        unusable
            .entry(stage.owner(&added.owner))
            .or_insert((added.package, reason));
    }
    unusable
}

/// Each planned entry's copies. A file that couldn't be read gives an empty copy without merge
/// data, so its entry can't merge and the mod that loads last wins.
fn inputs<'a>(planned: &[Planned], sources: &'a [Source<'a>]) -> Vec<EntryInputs<'a>> {
    planned
        .iter()
        .map(|item| EntryInputs {
            package: item.package,
            entry: item.entry,
            inputs: item
                .sources
                .iter()
                .map(|&index| {
                    let source = &sources[index];
                    match &source.read {
                        Ok(copies) => {
                            let copy = source
                                .indices
                                .binary_search(&item.entry)
                                .expect("a planned entry was read from each of its sources");
                            Input {
                                owner: source.owner,
                                sidecar: source.sidecar,
                                bytes: &copies[copy],
                            }
                        }
                        Err(_) => Input {
                            owner: source.owner,
                            sidecar: None,
                            bytes: &[],
                        },
                    }
                })
                .collect(),
        })
        .collect()
}

/// A table's name in a notice.
fn table_name(table: u32) -> String {
    recipes::table(table).map_or_else(
        || format!("entry 0x{table:08X}"),
        |recipe| format!("{} table", recipe.name),
    )
}

/// The mods before `owner` in load order that change entry `table`.
fn earlier(stage: &Stage, groups: &BTreeMap<u16, Vec<&Claim>>, owner: usize, table: u32) -> String {
    let changed = TagHash(table);
    let names = groups
        .get(&changed.pkg_id())
        .into_iter()
        .flatten()
        .filter(|claim| stage.owner(&claim.owner) < owner && claim.changed.contains(&changed))
        .map(|claim| claim.owner.as_str())
        .collect::<Vec<_>>();
    if names.is_empty() {
        "Another mod".to_owned()
    } else {
        names.join(" and ")
    }
}

/// The merge's notices, and what the loader serves.
fn report(
    stage: &Stage,
    groups: &BTreeMap<u16, Vec<&Claim>>,
    entries: &[EntryInputs<'_>],
    outcome: &Outcome,
    notices: &mut Vec<String>,
) -> Merged {
    let name = |owner: usize| stage.admitted[owner].as_str();
    for item in &outcome.conflicts {
        let bytes = if item.bytes == 1 { "byte" } else { "bytes" };
        notices.push(format!(
            "{later} changes {} {bytes} of entry 0x{:08X} that {} also changes. Sunrise uses {later}'s values.",
            item.bytes,
            tag(item.package, item.entry).0,
            name(item.earlier),
            later = name(item.later),
        ));
    }
    let reason = outcome
        .rows_off
        .map_or(Status::Unsupported, |(reason, _)| reason);
    for item in &outcome.dropped {
        notices.push(format!(
            "{}'s rows added to entry 0x{:08X} aren't merged: {}. Its references to them may name other rows.",
            name(item.owner),
            item.table,
            reason.text(),
        ));
    }
    // Parhelion's merge data names the references to its rows that recipes describe, which
    // aren't yet all of them.
    for item in outcome.shifted.iter().filter(|item| name(item.owner) == ID) {
        notices.push(format!(
            "{} loads before Parhelion and adds rows to the {}, so Sunrise renumbers Parhelion's rows there. Parhelion's merge data can't follow every reference to them yet, so its items may show the wrong content.",
            earlier(stage, groups, item.owner, item.table),
            table_name(item.table),
        ));
    }
    let fallbacks = outcome
        .fallbacks
        .iter()
        .map(|item| (item.package, item.entry))
        .collect::<HashSet<_>>();
    Merged {
        replacements: outcome
            .replacements
            .iter()
            .map(|item| (tag(item.package, item.entry), item.bytes.clone()))
            .collect(),
        entries: entries
            .iter()
            .filter(|entry| {
                entry.inputs.len() > 1 && !fallbacks.contains(&(entry.package, entry.entry))
            })
            .map(|entry| tag(entry.package, entry.entry))
            .collect(),
        coexistence: Coexistence::default(),
    }
}

fn entries(count: usize) -> &'static str {
    if count == 1 { "entry" } else { "entries" }
}

/// Whether Parhelion's content loads intact beside the other mods: every entry it shares with
/// another mod merges, no stock bytes they both change keep only one mod's value, and its own
/// rows keep their numbers.
fn coexistence(
    stage: &Stage,
    groups: &BTreeMap<u16, Vec<&Claim>>,
    merged: &HashSet<TagHash>,
    outcome: &Outcome,
    ignored: &BTreeMap<usize, String>,
) -> Coexistence {
    let mut result = Coexistence::default();
    let Some(own) = stage.admitted.iter().position(|id| id == ID) else {
        return result;
    };
    let fallbacks = outcome
        .fallbacks
        .iter()
        .map(|item| (tag(item.package, item.entry), item.reason))
        .collect::<HashMap<_, _>>();
    let mut shares = false;
    for (&package, group) in groups {
        let Some(ours) = group.iter().find(|claim| claim.owner == ID) else {
            continue;
        };
        for other in group.iter().filter(|claim| claim.owner != ID) {
            let (joined, apart): (Vec<TagHash>, Vec<TagHash>) = ours
                .changed
                .intersection(&other.changed)
                .partition(|changed| merged.contains(changed));
            shares |= !joined.is_empty() || !apart.is_empty();
            if !joined.is_empty() {
                result.merged.push(format!(
                    "Sunrise merges {} {} of package {package:04X} that Parhelion and {} both change.",
                    joined.len(),
                    entries(joined.len()),
                    other.owner,
                ));
            }
            let Some(first) = apart.first() else {
                continue;
            };
            let reason = match fallbacks.get(first) {
                Some(status) => status.text().to_owned(),
                None => "neither mod describes them in merge data".to_owned(),
            };
            result.problems.push(format!(
                "{} and Parhelion both change {} {} of package {package:04X} that Sunrise can't merge: {reason}. Remove the overlapping edits from one of them.",
                other.owner,
                apart.len(),
                entries(apart.len()),
            ));
        }
    }
    if shares && let Some(reason) = ignored.get(&own) {
        result.problems.push(format!(
            "Sunrise would ignore Parhelion's merge data: {reason}. Build again to write it."
        ));
    }
    let name = |owner: usize| stage.admitted[owner].as_str();
    for item in &outcome.conflicts {
        let other = match (item.earlier == own, item.later == own) {
            (true, _) => item.later,
            (_, true) => item.earlier,
            _ => continue,
        };
        let bytes = if item.bytes == 1 { "byte" } else { "bytes" };
        result.problems.push(format!(
            "{} and Parhelion change the same {} {bytes} of entry 0x{:08X} differently, and Sunrise keeps {}'s. Remove the overlapping edits from one of them.",
            name(other),
            item.bytes,
            tag(item.package, item.entry).0,
            name(item.later),
        ));
    }
    for item in outcome.shifted.iter().filter(|item| item.owner == own) {
        result.problems.push(format!(
            "{} loads before Parhelion and adds rows to the {} that Parhelion adds to as well. Sunrise would renumber Parhelion's rows there, which its merge data can't follow yet. Remove the overlapping edits from one of them.",
            earlier(stage, groups, own, item.table),
            table_name(item.table),
        ));
    }
    result
}

/// Says a mod's merge data is ignored whole, and which package file broke a rule.
fn all_ignored(owner: &str, reason: &str) -> String {
    format!("All of {owner}'s merge data was ignored: {reason}.")
}

/// Says why a package file's merge data was ignored when it was read.
pub(super) fn ignored(owner: &str, package: u16, reason: Status) -> String {
    format!(
        "{owner}'s merge data for package {package:04X} was ignored: {}.",
        reason.text()
    )
}

impl Stage {
    /// The index the merge names an admitted mod by.
    fn owner(&self, id: &str) -> usize {
        self.admitted
            .iter()
            .position(|admitted| admitted == id)
            .expect("only admitted mods stage files")
    }

    /// Whether `added` is the newest generation of its package, the one the loader serves.
    fn newest(&self, added: &Added) -> bool {
        self.bases
            .get(&added.package)
            .and_then(|base| base.generations.last())
            .is_some_and(|newest| *newest == added.path)
    }
}
