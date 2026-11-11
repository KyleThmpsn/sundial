//! Entries several mods change, as Sunrise f1e7c66's loader serves them
//! (`state/mods/packages/mod_package_serve.cpp`). Where the mods describe their changes in merge
//! data, the loader merges their rows into one entry. Elsewhere the mod that loads last wins.
use super::super::is_family_id;
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

/// How Parhelion's family loads beside the other mods and among its own members.
#[derive(Clone, Debug, Default)]
pub struct Coexistence {
    /// Why the loader wouldn't serve each member's content intact beside the other mods and
    /// members.
    pub problems: Vec<String>,
    /// Entries a member shares with other mods or members that the loader merges.
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
    merged.coexistence = coexistence(stage, groups, &merged.entries, &entries, &outcome, &ignored);
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

/// Whether a family member's merge data names every reference to the rows it adds to one array,
/// so the loader can move them when earlier mods add rows there. Node display rows run beside
/// the presentation nodes, and subclass socket entry lists beside item art's list rows. Parhelion
/// pairs each with its companion in one pair group, so the rows follow each other when the loader
/// moves both by the same rows. That holds when every earlier mod that adds one adds the other
/// too, as every family member does. Studio's recipe pairs neither, so an earlier Studio mod's
/// nodes would come without displays and leave the member's displays behind its nodes. Studio
/// adds no socket entry lists or item art lists.
fn follows(item: &merge::Shifted, shifted: &[merge::Shifted], entries: &[EntryInputs<'_>]) -> bool {
    // The native planner records earlier additions even for a member that keeps this array's
    // stock count. That member has no new row to keep beside a companion table.
    let unchanged = entries
        .iter()
        .find(|entry| tag(entry.package, entry.entry).0 == item.table)
        .and_then(|entry| {
            entry
                .inputs
                .iter()
                .find(|input| input.owner == item.owner)
                .and_then(|input| input.sidecar?.entry(entry.entry))
        })
        .and_then(|entry| {
            entry.ops.iter().find(|op| {
                op.parent_op == merge::sidecar::TOP_LEVEL_OP
                    && op.kind == merge::sidecar::OpKind::Append
                    && op.descriptor == item.descriptor
            })
        })
        .is_some_and(|op| op.mod_count == op.stock_count);
    if unchanged {
        return true;
    }
    let rows = |table: u32, descriptor: Option<u32>| {
        shifted
            .iter()
            .find(|other| {
                other.owner == item.owner
                    && other.table == table
                    && descriptor.is_none_or(|descriptor| other.descriptor == descriptor)
            })
            .map_or(0, |other| other.rows)
    };
    match (item.table, item.descriptor) {
        (recipes::ITEM_ART, 72) => rows(recipes::SOCKET_ENTRY_LISTS, None) == item.rows,
        (recipes::SOCKET_ENTRY_LISTS, _) => rows(recipes::ITEM_ART, Some(72)) == item.rows,
        (recipes::PRESENTATION_NODES, 8) => rows(recipes::NODE_DISPLAYS, None) == item.rows,
        (recipes::NODE_DISPLAYS, _) => rows(recipes::PRESENTATION_NODES, Some(8)) == item.rows,
        _ => true,
    }
}

/// The mods before `owner` in load order that change entry `table`.
fn earlier(stage: &Stage, groups: &BTreeMap<u16, Vec<&Claim>>, owner: usize, table: u32) -> String {
    let changed = TagHash(table);
    let names = groups
        .get(&changed.pkg_id())
        .into_iter()
        .flatten()
        .filter(|claim| stage.owner(&claim.owner) < owner && claim.changed.contains(&changed))
        .map(|claim| stage.label(stage.owner(&claim.owner)))
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
    for item in outcome
        .shifted
        .iter()
        .filter(|item| is_family_id(name(item.owner)) && !follows(item, &outcome.shifted, entries))
    {
        notices.push(format!(
            "{} loads before {own} and adds rows to the {}, so Sunrise renumbers {own}'s rows there. A table beside it keeps {own}'s rows where they were, so its items may show the wrong content.",
            earlier(stage, groups, item.owner, item.table),
            table_name(item.table),
            own = stage.label(item.owner),
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

/// Whether every family member's content loads intact beside the other mods and the other
/// members: every entry it shares with another mod or member merges, no stock bytes they both
/// change keep only one mod's value, and its own rows keep their numbers. Members of one build
/// are held to the same rules as any other mods, since Sunrise loads each as a mod of its own.
fn coexistence(
    stage: &Stage,
    groups: &BTreeMap<u16, Vec<&Claim>>,
    merged: &HashSet<TagHash>,
    inputs: &[EntryInputs<'_>],
    outcome: &Outcome,
    ignored: &BTreeMap<usize, String>,
) -> Coexistence {
    let mut result = Coexistence::default();
    let owned = |owner: usize| is_family_id(&stage.admitted[owner]);
    if !(0..stage.admitted.len()).any(owned) {
        return result;
    }
    let label = |owner: usize| stage.label(owner);
    let mut sharing = Sharing {
        stage,
        merged,
        fallbacks: outcome
            .fallbacks
            .iter()
            .map(|item| (tag(item.package, item.entry), item.reason))
            .collect(),
        shares: BTreeSet::new(),
        joint: BTreeMap::new(),
        problems: Vec::new(),
    };
    for (&package, group) in groups {
        for (index, ours) in group.iter().enumerate() {
            if !is_family_id(&ours.owner) {
                continue;
            }
            // Each other mod once, and each pair of members once.
            for (_, other) in group
                .iter()
                .enumerate()
                .filter(|&(later, claim)| !is_family_id(&claim.owner) || later > index)
            {
                sharing.pair(package, ours, other);
            }
        }
    }
    result.problems = sharing.problems;
    for ((own, theirs), packages) in sharing.joint {
        let total = packages.iter().map(|(_, count)| count).sum::<usize>();
        let place = if let [(package, _)] = packages.as_slice() {
            format!("package {package:04X}")
        } else {
            let mut names = packages
                .iter()
                .map(|(package, count)| format!("{package:04X} ({count})"))
                .collect::<Vec<_>>();
            let last = names.pop().unwrap_or_default();
            format!("packages {} and {last}", names.join(", "))
        };
        result.merged.push(format!(
            "Sunrise merges {total} {} that {} and {} both change, in {place}.",
            entries(total),
            label(own),
            label(theirs),
        ));
    }
    for owner in sharing.shares {
        if let Some(reason) = ignored.get(&owner) {
            result.problems.push(format!(
                "Sunrise would ignore {}'s merge data: {reason}. Build again to write it.",
                label(owner)
            ));
        }
    }
    for item in &outcome.conflicts {
        // The other mod is named first, as in the lines above.
        let (other, own) = match (owned(item.earlier), owned(item.later)) {
            (true, _) => (item.later, item.earlier),
            (_, true) => (item.earlier, item.later),
            _ => continue,
        };
        let bytes = if item.bytes == 1 { "byte" } else { "bytes" };
        result.problems.push(format!(
            "{} and {} change the same {} {bytes} of entry 0x{:08X} differently, and Sunrise keeps {}'s. {}",
            label(other),
            label(own),
            item.bytes,
            tag(item.package, item.entry).0,
            label(item.later),
            remedy(owned(other)),
        ));
    }
    for item in outcome
        .shifted
        .iter()
        .filter(|item| owned(item.owner) && !follows(item, &outcome.shifted, inputs))
    {
        result.problems.push(format!(
            "{} loads before {own} and adds rows to the {} that {own} adds to as well. Sunrise would renumber {own}'s rows there, but a table beside it would keep them where they were. Remove the overlapping edits from one of them.",
            earlier(stage, groups, item.owner, item.table),
            table_name(item.table),
            own = label(item.owner),
        ));
    }
    result
}

/// What makes two mods load together where their edits collide. Two members of one build share
/// one mod once the build couples them.
fn remedy(member: bool) -> &'static str {
    if member {
        "Build again to keep both in one mod."
    } else {
        "Remove the overlapping edits from one of them."
    }
}

/// What the family shares with the other mods and among its members, gathered pair by pair.
struct Sharing<'a> {
    stage: &'a Stage,
    merged: &'a HashSet<TagHash>,
    fallbacks: HashMap<TagHash, Status>,
    /// The members that share entries with another mod or member.
    shares: BTreeSet<usize>,
    /// Merged entry counts per member and other mod or member, and package, reported as one
    /// line per pair.
    joint: BTreeMap<(usize, usize), Vec<(u16, usize)>>,
    problems: Vec<String>,
}

impl Sharing<'_> {
    /// What member claim `ours` shares with `other` on `package`: the entries Sunrise merges, and
    /// a problem line per table for those it can't combine.
    fn pair(&mut self, package: u16, ours: &Claim, other: &Claim) {
        let own = self.stage.owner(&ours.owner);
        let theirs = self.stage.owner(&other.owner);
        let member = is_family_id(&other.owner);
        let (joined, apart): (Vec<TagHash>, Vec<TagHash>) = ours
            .changed
            .intersection(&other.changed)
            .partition(|changed| self.merged.contains(changed));
        if !joined.is_empty() || !apart.is_empty() {
            self.shares.insert(own);
            if member {
                self.shares.insert(theirs);
            }
        }
        if !joined.is_empty() {
            self.joint
                .entry((own, theirs))
                .or_default()
                .push((package, joined.len()));
        }
        // One line per table, naming its entries, why Sunrise can't combine them and what makes
        // the two mods load together.
        let mut tables = BTreeMap::<String, Vec<TagHash>>::new();
        for changed in &apart {
            tables
                .entry(table_name(changed.0))
                .or_default()
                .push(*changed);
        }
        for (table, changed) in tables {
            let reason = match self.fallbacks.get(&changed[0]) {
                Some(status) => status.text().to_owned(),
                None => "neither mod describes it in merge data".to_owned(),
            };
            let list = changed
                .iter()
                .map(|tag| format!("0x{:08X}", tag.0))
                .collect::<Vec<_>>()
                .join(", ");
            let fix = if member {
                remedy(true).to_owned()
            } else if changed
                .iter()
                .any(|tag| tag.0 == recipes::PRESENTATION_NODES)
            {
                format!(
                    "Build again with {} installed, and on the Collections tab set Position to Automatic or Last on items set to First.",
                    other.owner
                )
            } else {
                format!(
                    "Build again with {} installed, or remove the overlapping edits from one of them.",
                    other.owner
                )
            };
            self.problems.push(format!(
                "{} and {} both change the {table} (package {package:04X}, {} {list}), which Sunrise can't combine: {reason}. {fix}",
                self.stage.label(theirs),
                self.stage.label(own),
                entries(changed.len()),
            ));
        }
    }
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
    /// How a notice names an admitted mod: a family member by its name, which tells the members
    /// apart, and another mod by its id.
    fn label(&self, owner: usize) -> &str {
        if is_family_id(&self.admitted[owner]) {
            &self.names[owner]
        } else {
            &self.admitted[owner]
        }
    }

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
