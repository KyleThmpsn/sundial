//! Link explicitly selected imported assets into private package allocations.
use super::*;
use parhelion_import::d2_mot::arrays;
use serde_json::Value;
mod bounds;
mod character;
mod clips;
mod crosshair;
mod dyes;
mod gear;
mod human_actions;
mod inspect;
mod loading;
mod ornament_collections;
mod ornament_icon;
mod ornaments;
mod shader;
mod sharing;
mod streaming;

struct Shared {
    companions: linking::Companions,
    characters: character::Profiles,
    identities: sharing::Identities,
    /// The gear art and dye keys the build's graphs hold so far.
    keys: BTreeSet<u64>,
    timings: Timings,
    /// The runtime the build is for, whose watermark imported ornament icons take. The packages
    /// a build reads can be a filtered view outside the install, so it is not read from them.
    branding: crate::branding::Branding,
}

/// Where linking the imported graphs spent its time, summed over every graph.
#[derive(Default)]
pub(super) struct Timings {
    graphs: usize,
    /// Reading and parsing each graph's description.
    read: std::time::Duration,
    /// Binding materials and reading and preparing each node's payload.
    nodes: std::time::Duration,
    /// Replication companions, ornament icon bounds and shader edits.
    companions: std::time::Duration,
    /// Allocating, converting and linking the nodes.
    link: std::time::Duration,
    /// Everything else: the item tables each graph edits afterwards.
    tables: std::time::Duration,
}

impl std::fmt::Display for Timings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = |duration: std::time::Duration| duration.as_secs_f64();
        write!(
            f,
            "{} graphs: read {:.3}s, nodes {:.3}s, companions {:.3}s, link {:.3}s, tables {:.3}s",
            self.graphs,
            seconds(self.read),
            seconds(self.nodes),
            seconds(self.companions),
            seconds(self.link),
            seconds(self.tables)
        )
    }
}

/// What linking one recipe's graphs changed that the graphs linked after it read, so a recipe
/// whose graph fails is undone and the others still link. A pass with a failed recipe is never
/// packaged: the build compiles again without every recipe that failed. The undo therefore only
/// has to keep later recipes' results honest, not restore every byte.
///
/// - The asset packages only grow, so their lengths, reservations and shared clips restore them.
/// - The shared tables the recipe replaced take their earlier payloads back.
/// - The character profiles go back, or a later item could join the gate of a profile whose
///   selector edit was undone.
/// - The dependency index goes back, since the recipe enrolled entries that no longer exist.
///
/// The recipe's own item definition and Collections rows keep its edits, which no other recipe
/// reads. Resource sharing needs no undo: a later graph reuses a committed pair only when the
/// entry still holds the same bytes, and an undone entry does not.
struct Rollback {
    packages: Vec<(usize, usize)>,
    reservations: usize,
    shared_clips: BTreeMap<
        (
            Option<sundial::package_authoring::mod_content::Importer>,
            [u8; 32],
        ),
        TagHash,
    >,
    runtime_dependencies: Option<Vec<u8>>,
    characters: character::Profiles,
    /// Each shared table the recipe replaced, with the payload it had before the recipe.
    replaced: BTreeMap<u32, Option<Vec<u8>>>,
}

impl Rollback {
    fn take(emission: &PackageEmission, shared: &Shared) -> Self {
        let assets = &emission.asset_packages;
        Self {
            packages: assets
                .packages
                .iter()
                .map(|package| (package.tags.len(), package.references.len()))
                .collect(),
            reservations: assets.reservations.len(),
            shared_clips: assets.shared_clips.clone(),
            runtime_dependencies: emission.runtime_dependencies.clone(),
            characters: shared.characters.clone(),
            replaced: BTreeMap::new(),
        }
    }

    /// Replaces the shared table `tag` with `payload`, remembering what the recipe found there.
    fn replace(&mut self, replacements: &mut BTreeMap<u32, Vec<u8>>, tag: u32, payload: Vec<u8>) {
        let previous = replacements.insert(tag, payload);
        self.replaced.entry(tag).or_insert(previous);
    }

    fn apply(
        self,
        emission: &mut PackageEmission,
        shared: &mut Shared,
        replacements: &mut BTreeMap<u32, Vec<u8>>,
    ) {
        let assets = &mut emission.asset_packages;
        assets.packages.truncate(self.packages.len());
        for (package, (tags, references)) in assets.packages.iter_mut().zip(self.packages) {
            package.tags.truncate(tags);
            package.references.truncate(references);
        }
        assets.reservations.truncate(self.reservations);
        assets.shared_clips = self.shared_clips;
        assets.owner = None;
        emission.runtime_dependencies = self.runtime_dependencies;
        shared.characters = self.characters;
        for (tag, previous) in self.replaced {
            match previous {
                Some(payload) => {
                    replacements.insert(tag, payload);
                }
                None => {
                    replacements.remove(&tag);
                }
            }
        }
    }
}

/// An independently placed private part, either source geometry or marker-only geometry.
struct Kept {
    assignment: u32,
    key: u32,
    parent: String,
    region: Option<Region>,
}

struct Region {
    selector: u64,
    positions: Vec<usize>,
    native: bool,
}

fn kept_region(part: &Value) -> AuthoringResult<Option<Region>> {
    let native = part["native_region"] == true;
    if !native && part["source_region"] != true {
        return Ok(None);
    }
    let selector = part["selector"]
        .as_u64()
        .ok_or_else(|| invalid("Kept art selector missing"))?;
    let selector = if native {
        selector
    } else {
        parhelion_import::d2_mot::artwork::native_selector(selector)
            .map_err(|error| invalid(error.to_string()))?
    };
    let position = usize::try_from(
        part["position"]
            .as_u64()
            .ok_or_else(|| invalid("Kept art position missing"))?,
    )
    .map_err(|_| invalid("Kept art position overflow"))?;
    let positions = if let Some(values) = part.get("positions") {
        let values = values
            .as_array()
            .ok_or_else(|| invalid("Kept art positions missing"))?;
        let positions = values
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| invalid("Kept art position overflow"))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        if positions.first() != Some(&position) || positions.windows(2).any(|p| p[0] >= p[1]) {
            return Err(invalid("Kept art positions are empty or unordered"));
        }
        positions
    } else {
        vec![position]
    };
    Ok(Some(Region {
        selector,
        positions,
        native,
    }))
}

fn place_region(
    slot: &mut Vec<u32>,
    positions: BTreeMap<usize, u32>,
    native: bool,
) -> AuthoringResult<()> {
    if native {
        // A retained native alternative can occupy any source position.
        // Empty positions remain empty and existing selector widths survive.
        let width = positions
            .last_key_value()
            .unwrap()
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Kept art position overflow"))?;
        slot.try_reserve(width.saturating_sub(slot.len()))
            .map_err(|_| invalid("Kept art alternatives exceed available capacity"))?;
        slot.resize(
            slot.len().max(width),
            parhelion_import::d2_mot::artwork::EMPTY,
        );
        for (position, key) in positions {
            slot[position] = key;
        }
    } else {
        if !positions.keys().copied().eq(0..positions.len()) {
            return Err(invalid("Source art alternatives are discontinuous"));
        }
        *slot = positions.into_values().collect();
    }
    Ok(())
}

fn place_regions(slots: &mut Vec<(u64, Vec<u32>)>, kept: &[Kept]) -> AuthoringResult<()> {
    struct Group {
        native: bool,
        positions: BTreeMap<usize, u32>,
    }
    let mut regions = BTreeMap::<u64, Group>::new();
    for part in kept {
        let Some(region) = &part.region else {
            continue;
        };
        let group = regions.entry(region.selector).or_insert_with(|| Group {
            native: region.native,
            positions: BTreeMap::new(),
        });
        if group.native != region.native {
            return Err(invalid("Conflicting kept art position"));
        }
        for &position in &region.positions {
            if group.positions.insert(position, part.key).is_some() {
                return Err(invalid("Conflicting kept art position"));
            }
        }
    }
    for (selector, group) in regions {
        let slot = if let Some(index) = slots.iter().position(|slot| slot.0 == selector) {
            &mut slots[index].1
        } else {
            slots.push((selector, Vec::new()));
            &mut slots.last_mut().unwrap().1
        };
        place_region(slot, group.positions, group.native)?;
    }
    Ok(())
}

fn kept_parts(graph: &Value) -> AuthoringResult<Vec<Kept>> {
    graph["kept_parts"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(graph["source_parts"].as_array().into_iter().flatten())
        .map(|part| {
            let word = |name: &str| {
                part[name]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| invalid(format!("Kept part {name} missing")))
            };
            Ok(Kept {
                assignment: word("assignment")?,
                key: word("key")?,
                parent: part["parent"]
                    .as_str()
                    .ok_or_else(|| invalid("Kept part parent missing"))?
                    .to_owned(),
                region: kept_region(part)?,
            })
        })
        .collect()
}

/// Bind the assembled graph to its body slot, replacing the native carrier's assignment, and
/// each kept donor part to its private copy.
fn replace(
    data: &mut Vec<u8>,
    row: usize,
    key: u32,
    donor_hash: u32,
    source_key: u32,
    kept: &[Kept],
) -> AuthoringResult<()> {
    let (count, _, rows, _) =
        sundial::package_authoring::native_payload::native_array_at(data, 8).map_err(invalid)?;
    let donor = (0..count)
        .map(|i| rows + i * 32)
        .find(|&o| read_u32(data, o).ok() == Some(donor_hash))
        .ok_or_else(|| invalid("Sword art donor missing"))?;
    art::rewrite(data, row, donor, |singles, slots| {
        for (_, keys) in slots.iter_mut() {
            for slot_key in keys {
                if *slot_key == source_key {
                    *slot_key = key;
                } else if let Some(part) = kept
                    .iter()
                    .find(|part| part.region.is_none() && part.assignment == *slot_key)
                {
                    *slot_key = part.key;
                }
            }
        }
        // The source assignment identifies the carrier, not its final placement.
        if !singles.contains(&source_key) && !slots.iter().any(|(_, keys)| keys.contains(&key)) {
            return Err(invalid("Selected native assignment missing"));
        }
        let keys = kept.iter().map(|part| part.key).collect::<Vec<_>>();
        parhelion_import::d2_mot::artwork::assembled(singles, slots, key, &keys)
            .map_err(|error| invalid(error.to_string()))?;
        place_regions(slots, kept)
    })
}
/// One recipe's imported graph to check: its kind, item, graph and name.
type Checked<'a> = (ItemKind, u32, &'a parhelion_import::GraphReference, &'a str);

/// Checks every imported graph against the fingerprint taken when it was selected. Each
/// graph hashes tens of megabytes of its own files, so the graphs are checked side by side.
/// The build log names each recipe's model after `verb` in recipe order as the check waits on
/// it. Every graph that changed is reported with its recipe, so the build leaves them all out
/// in one pass. The build checks before [`apply`] links them and again after.
pub(super) fn validate_graphs(
    weapons: &[WeaponCloneSpec],
    progress: &mut crate::item::build::Progress<'_>,
    verb: &str,
) -> AuthoringResult<()> {
    let graphs = weapons
        .iter()
        .filter_map(|weapon| {
            weapon.overrides.imported_graph.as_ref().map(|graph| {
                let name = weapon.text.name.as_str();
                (weapon.kind, weapon.identity.item_hash, graph, name)
            })
        })
        .collect::<Vec<Checked<'_>>>();
    // The recipe of each checked graph, in the same order, to name the ones that changed.
    let recipes = weapons
        .iter()
        .filter(|weapon| weapon.overrides.imported_graph.is_some())
        .collect::<Vec<_>>();
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8)
        .min(graphs.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let stopped = || invalid("An imported graph check stopped unexpectedly");
    std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let handles = (0..workers)
            .map(|_| {
                let sender = sender.clone();
                let (graphs, next) = (&graphs, &next);
                scope.spawn(move || check_graphs(graphs, next, &sender))
            })
            .collect::<Vec<_>>();
        drop(sender);
        let mut finished = BTreeMap::new();
        let mut failures = Vec::new();
        let checked = graphs
            .iter()
            .enumerate()
            .try_for_each(|(index, &(.., name))| {
                progress.start(&format!("{verb} Model for {name}"));
                while !finished.contains_key(&index) {
                    let (done, result) = receiver.recv().map_err(|_| stopped())?;
                    finished.insert(done, result);
                }
                if let Err(error) = finished.remove(&index).ok_or_else(stopped)? {
                    failures.push(recipes[index].in_recipe(invalid(error)));
                }
                Ok(())
            });
        // A worker still checking stops at its next result.
        drop(receiver);
        // Every worker is joined, even after one fails.
        let mut joined = true;
        for handle in handles {
            joined &= handle.join().is_ok();
        }
        if !joined {
            return Err(stopped());
        }
        checked?;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(crate::AuthoringError::several(failures))
        }
    })
}

/// One worker's share of [`validate_graphs`]: graphs taken in recipe order, so the one the log
/// waits on is checked first, until none remain or the log stops waiting.
fn check_graphs(
    graphs: &[Checked<'_>],
    next: &std::sync::atomic::AtomicUsize,
    sender: &std::sync::mpsc::Sender<(usize, Result<(), String>)>,
) {
    loop {
        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(&(kind, item, graph, _)) = graphs.get(index) else {
            return;
        };
        let result = match crate::imported::kind(kind) {
            Some(kind) => graph.validate_reusable(kind),
            None => graph.validate(item),
        }
        .map_err(|e| e.to_string());
        if sender.send((index, result)).is_err() {
            return;
        }
    }
}

pub(super) fn apply(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
    branding: crate::branding::Branding,
    progress: &mut crate::item::build::Progress<'_>,
) -> AuthoringResult<(Vec<ReplacementSpec>, Timings)> {
    let mut timings = Timings::default();
    let started = std::time::Instant::now();
    // Native runtime cache replay can append whole packages without reserving new groups.
    // Claim that complete native prefix before any importer or map chooses its package IDs.
    if crate::package_profile::host().is_standalone() {
        for package in &emission.asset_packages.packages {
            crate::mod_family::build::native(package.id);
        }
    }
    validate_graph_kinds(weapons)?;
    let folders = weapons
        .iter()
        .filter_map(|weapon| {
            weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(|graph| (weapon, graph))
        })
        .flat_map(|(weapon, graph)| {
            let owner = weapon.text.name.as_str();
            std::iter::once(Target {
                folder: graph.directory.as_path(),
                owner,
                spec: Some(weapon),
                recipe: weapon,
            })
            .chain(graph.attachments.iter().map(move |attachment| Target {
                folder: attachment.directory.as_path(),
                owner,
                spec: (weapon.kind == ItemKind::Armor).then_some(weapon),
                recipe: weapon,
            }))
        })
        .collect::<Vec<_>>();
    timings.read = started.elapsed();
    let abilities = human_actions::directories(weapons);
    // The main mod's build writes the importers' rows of the shared tables it hosts.
    let hosted = crate::mod_family::build::contributions();
    if folders.is_empty() && abilities.is_empty() && hosted.is_empty() {
        return Ok((Vec::new(), timings));
    }
    let mut shared = Shared {
        companions: linking::Companions::new(),
        characters: character::Profiles::default(),
        identities: sharing::Identities::default(),
        keys: BTreeSet::new(),
        timings,
        branding,
    };
    let mut replacements = BTreeMap::new();
    // Custom shader dyes already extend the dye table, so imported dyes follow them.
    if let Some(table) = &emission.dye_table {
        replacements.insert(table.tag.0, table.payload.clone());
    }
    let accounted =
        |timings: &Timings| timings.read + timings.nodes + timings.companions + timings.link;
    // A recipe's graphs, its own and its attachments', link one after another. One that fails
    // undoes the recipe's graphs, and the others still link, so the pass names every recipe
    // whose graphs fail and the build leaves them out together.
    let mut failures = Vec::new();
    let mut failed = BTreeSet::new();
    let mut undo: Option<(&str, Rollback)> = None;
    for target in folders {
        let folder = target.folder;
        let recipe = target.recipe;
        let namespace = recipe.namespace.as_str();
        if failed.contains(namespace) {
            continue;
        }
        if undo.as_ref().is_none_or(|(owner, _)| *owner != namespace) {
            undo = Some((namespace, Rollback::take(emission, &shared)));
        }
        let started = std::time::Instant::now();
        let before = accounted(&shared.timings);
        let applied = apply_one(
            directory,
            emission,
            target,
            &replacements,
            manager,
            &mut shared,
            progress,
        );
        let staged = accounted(&shared.timings) - before;
        shared.timings.tables += started.elapsed().saturating_sub(staged);
        shared.timings.graphs += 1;
        match applied {
            Ok(applied) => {
                let (_, rollback) = undo.as_mut().expect("the recipe's undo is taken");
                for replacement in applied {
                    rollback.replace(&mut replacements, replacement.tag.0, replacement.payload);
                }
            }
            Err(error) => {
                let error = error.context(format!("Imported graph {}", folder.display()));
                // The graph belongs to one recipe, so the build can name it and go on without it.
                let error = if error.recipe_namespace().is_some() {
                    error
                } else {
                    recipe.in_recipe(error)
                };
                let (_, rollback) = undo.take().expect("the recipe's undo is taken");
                rollback.apply(emission, &mut shared, &mut replacements);
                failed.insert(namespace);
                progress.diagnostic(format!(
                    "Linking {} failed. Linking the other items before reporting it.",
                    recipe.text.name
                ));
                failures.push(error);
            }
        }
    }
    // The build compiles again without the failed recipes, and the hosted tables and abilities
    // link in that pass.
    if !failures.is_empty() {
        return Err(crate::AuthoringError::several(failures));
    }
    let mut added_actions = BTreeMap::new();
    host(
        manager,
        &hosted,
        &mut replacements,
        &mut shared,
        &mut added_actions,
    )?;
    // Imported abilities add their private human actions on top of the graphs' additions.
    for folder in &abilities {
        for replacement in
            human_actions::apply(manager, emission, folder, &replacements, &mut added_actions)
                .map_err(|error| error.context(format!("Imported ability {}", folder.display())))?
        {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
    }
    let identities = &shared.identities;
    if identities.reused + identities.within > 0 {
        eprintln!(
            "Shared {} resources with earlier imported graphs and {} within their own, {:.1} MB not repeated",
            identities.reused,
            identities.within,
            identities.bytes as f64 / 1048576.0
        );
    }
    share(manager, &mut replacements)?;
    let replacements = replacements
        .into_iter()
        .map(|(tag, payload)| ReplacementSpec {
            tag: TagHash(tag),
            payload,
        })
        .collect();
    Ok((replacements, shared.timings))
}

/// Adds the importers' rows of the shared tables the main mod's build hosts, after its own:
/// crosshair rows naming the importers' scenes, and character profiles and human actions with the
/// clips the importers' builds allocated. `added_actions` records the human actions added.
fn host(
    manager: &sundial::package_authoring::PackageManager,
    hosted: &[crate::mod_family::build::Contributions],
    replacements: &mut BTreeMap<u32, Vec<u8>>,
    shared: &mut Shared,
    added_actions: &mut BTreeMap<u32, [u8; 32]>,
) -> AuthoringResult<()> {
    use crate::weapon::crosshair::{TABLE, Table};
    let rows = hosted
        .iter()
        .flat_map(|contributions| &contributions.crosshair)
        .collect::<Vec<_>>();
    if !rows.is_empty() {
        let original = match replacements.get(&TABLE.0) {
            Some(data) => data.clone(),
            None => manager
                .read_tag(TABLE)
                .map_err(|error| invalid(error.to_string()))?,
        };
        let mut table = Table::parse(&original)?;
        // A row for a bucket and key an earlier member already added stays that member's.
        for row in rows {
            table.insert(row.clone())?;
        }
        replacements.insert(TABLE.0, table.serialize()?);
    }
    for profile in hosted
        .iter()
        .flat_map(|contributions| &contributions.characters)
    {
        for replacement in character::host(manager, profile, replacements, &mut shared.characters)?
        {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
    }
    for actions in hosted
        .iter()
        .flat_map(|contributions| &contributions.human_actions)
    {
        for replacement in human_actions::host(manager, actions, replacements, added_actions)? {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
    }
    Ok(())
}

/// Records the shared tables the member's build changed, and for an importer what it adds to
/// them. The tables the main mod's build hosts for the importer leave its own build.
fn share(
    manager: &sundial::package_authoring::PackageManager,
    replacements: &mut BTreeMap<u32, Vec<u8>>,
) -> AuthoringResult<()> {
    use crate::mod_family::build::{Shared as Table, contribute, hosts, produced, produced_by};
    use crate::weapon::crosshair::{TABLE, Table as Crosshair};
    if let Some(table) = replacements.get(&TABLE.0) {
        produced(Table::Crosshair, [TABLE.0]);
        if crate::package_profile::member().is_some() {
            let stock = Crosshair::parse(
                &manager
                    .read_tag(TABLE)
                    .map_err(|error| invalid(error.to_string()))?,
            )?;
            let rows = Crosshair::parse(table)?
                .rows()
                .iter()
                .filter(|row| !stock.has(row.bucket, row.key))
                .cloned()
                .collect::<Vec<_>>();
            contribute(|contributions| contributions.crosshair = rows);
            if hosts(Table::Crosshair) {
                replacements.remove(&TABLE.0);
            }
        }
    }
    if hosts(Table::Character) {
        for (tag, table) in produced_by(crate::package_profile::member()) {
            if table == Table::Character {
                replacements.remove(&tag);
            }
        }
    }
    Ok(())
}
/// One node of an imported asset graph, with its payload read from `folder` and prepared:
/// model draw indices declared and moved markers applied.
fn graph_node(
    folder: &Path,
    n: &Value,
    model: bool,
    single_detail: bool,
    spec: Option<&WeaponCloneSpec>,
    bound: &mut BTreeMap<String, Vec<u8>>,
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<linking::Node> {
    let name = n["symbol"]
        .as_str()
        .ok_or_else(|| invalid("Asset symbol missing"))?;
    let template = n["template"]
        .as_u64()
        .ok_or_else(|| invalid("Template missing"))? as u32;
    let is_companion = name == "parent-companion" || n["shared_owner"].is_string();
    let mut payload = if is_companion {
        Vec::new()
    } else if let Some(bytes) = bound.remove(name) {
        bytes
    } else {
        fs::read(
            folder.join(
                n["file"]
                    .as_str()
                    .ok_or_else(|| invalid("Payload missing"))?,
            ),
        )
        .map_err(|e| invalid(e.to_string()))?
    };
    if model {
        bounds::repair(manager, TagHash(template), &mut payload)?;
        if single_detail {
            parhelion_import::tiger::draws::cover_single_detail(&mut payload)
                .map_err(|e| invalid(format!("Imported model detail coverage: {e}")))?;
        }
        parhelion_import::d2_mot::audit::draws::declare_model_draw_indices(&mut payload)
            .map_err(|e| invalid(format!("Imported model draw indices: {e}")))?;
    }
    // Moved markers move in the imported model's own marker sets, matched by name.
    if let Some(spec) = spec
        && !spec.overrides.marker_offsets.is_empty()
        && sundial::package_authoring::gear_markers::is_marker_set(&payload)
    {
        sundial::package_authoring::gear_markers::offset_markers(
            &mut payload,
            &spec.overrides.marker_offsets,
        )
        .map_err(|error| invalid(format!("Imported marker set {name}: {error}")))?;
    }
    let mut patches = n["patches"]
        .as_array()
        .ok_or_else(|| invalid("Fixups missing"))?
        .clone();
    if !is_companion
        && manager
            .get_entry(TagHash(template))
            .is_some_and(|entry| entry.reference == 0x80809c36)
        && let Some(fixed) = parhelion_import::tiger::instance::upgrade_owner(&payload, &patches)
            .map_err(|error| invalid(format!("Imported component {name}: {error:#}")))?
    {
        payload = fixed.payload;
        patches = fixed.patches;
    }
    let mut node = linking::Node::new(name, template, payload);
    node.audio = serde_json::from_value(n.clone())
        .map_err(|error| invalid(format!("Imported graph audio: {error}")))?;
    node.storage = match n["storage"].as_str() {
        None => crate::NewTagStorageMode::InheritTemplate,
        Some("audio_media") => crate::NewTagStorageMode::AudioMedia,
        Some("audio_bank") => crate::NewTagStorageMode::AudioBank,
        Some("gpu_hull") => crate::NewTagStorageMode::GpuHull,
        Some("gpu_domain") => crate::NewTagStorageMode::GpuDomain,
        Some(_) => return Err(invalid("Unsupported imported asset storage")),
    };
    for p in &patches {
        let offset = p["offset"]
            .as_u64()
            .ok_or_else(|| invalid("Offset missing"))? as usize;
        let target = p["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Fixup symbol missing"))?;
        node.patches.push((offset, target.to_owned()));
    }
    node.reference = n["reference"].as_str().map(str::to_owned);
    if is_companion {
        let mut companion = linking::Companion::new(
            n["shared_owner"].as_str().unwrap_or("parent"),
            n["source_parent"].as_u64().unwrap_or(0x80EC272A) as u32,
        );
        for parent in n["inherited"].as_array().into_iter().flatten() {
            companion.inherited.push(
                parent
                    .as_u64()
                    .and_then(|tag| u32::try_from(tag).ok())
                    .ok_or_else(|| invalid("Inherited loading owner is not a tag"))?,
            );
        }
        node.companion = Some(companion);
    }
    Ok(node)
}

/// Make a weapon model graph a mod carries, an imported weapon ornament, the mod's own plug, with
/// the art, dye and kept part keys an imported model copy takes from its item. Renaming the mod
/// changes its identity, and keys hashed from the whole item are far less likely to repeat another
/// import's than keys from its low 16 bits.
fn bind_mod(graph: &mut Value, spec: &WeaponCloneSpec) {
    let item = spec.identity.item_hash;
    graph["item_hash"] = item.into();
    rekey(graph, &format!("{item:08X}"));
}

/// The graph rows that hold a key, and the field each holds it in.
const KEYED_ROWS: [(&str, &str, &str); 3] = [
    ("dyes", "manifest", "dye"),
    ("kept_parts", "key", "kept"),
    ("source_parts", "key", "source-part"),
];

/// Gives the graph the art, dye, kept part and source part keys an imported model copy takes,
/// FNV-1 of `parhelion/imported-model/{seed}/{role}`. A copy seeds them with its item hash.
fn rekey(graph: &mut Value, seed: &str) {
    let key = |role: &str| {
        let hash = fnv1_name_hash(&format!("parhelion/imported-model/{seed}/{role}"));
        if [0, u32::MAX, 0x811C9DC5].contains(&hash) {
            hash ^ 0x10000
        } else {
            hash
        }
    };
    graph["art_key"] = key("art").into();
    for (field, target, role) in KEYED_ROWS {
        for (index, row) in graph[field]
            .as_array_mut()
            .into_iter()
            .flatten()
            .enumerate()
        {
            row[target] = key(&format!("{role}-{index}")).into();
        }
    }
}

/// The keys a graph adds to the build's gear art and dye maps.
fn graph_keys(graph: &Value) -> Vec<u64> {
    std::iter::once(&graph["art_key"])
        .chain(KEYED_ROWS.iter().flat_map(|(field, target, _)| {
            graph[*field]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |row| &row[*target])
        }))
        .filter_map(Value::as_u64)
        .collect()
}

/// Two recipes can carry graphs with the same keys, as a library copy imported again from its
/// source does, since an import takes its keys from the source item. A graph holding a key an
/// earlier graph of the build or the gear art map already holds takes keys hashed from its own
/// item instead, as a copy does, so both build. The first graph keeps its keys.
fn keep_keys_apart(
    graph: &mut Value,
    recipe: &WeaponCloneSpec,
    taken: &mut BTreeSet<u64>,
    table: &[u8],
) -> AuthoringResult<()> {
    let assigned = art::assignments(table)?;
    let clash = |graph: &Value, taken: &BTreeSet<u64>| {
        graph_keys(graph).into_iter().any(|key| {
            taken.contains(&key) || u32::try_from(key).is_ok_and(|key| assigned.contains_key(&key))
        })
    };
    if clash(graph, taken) {
        let item = graph["item_hash"]
            .as_u64()
            .and_then(|item| u32::try_from(item).ok())
            .unwrap_or(recipe.identity.item_hash);
        let mut seed = format!("{item:08X}");
        for attempt in 1.. {
            rekey(graph, &seed);
            if !clash(graph, taken) {
                break;
            }
            if attempt == 16 {
                return Err(invalid(format!(
                    "Imported graph keys for item 0x{item:08X} collide with other imports"
                )));
            }
            seed = format!("{item:08X}/{attempt}");
        }
    }
    taken.extend(graph_keys(graph));
    Ok(())
}

/// One graph to link: its folder, the name of the recipe it belongs to, and that recipe when the
/// graph is the recipe's own or an armor appearance. A weapon appearance with gear art of its
/// own binds to its socket choice through `recipe`, the recipe it belongs to, and is otherwise
/// linked without it.
struct Target<'a> {
    folder: &'a Path,
    owner: &'a str,
    spec: Option<&'a WeaponCloneSpec>,
    recipe: &'a WeaponCloneSpec,
}

/// The build log's line while a graph links: an appearance by its ornament name, else its
/// recipe's model.
/// The nodes a graph with a runtime presentation links into its appearance: its own nodes and the
/// runtime nodes they reach, such as the model's materials and the driver seat. A vehicle graph
/// lists its whole runtime among its nodes, and the runtime takes a resident group of its own, so
/// linking the rest here would load a second copy whenever the item's appearance loads. `None`
/// keeps every node of a graph without a runtime presentation.
fn appearance_symbols(graph: &Value) -> Option<BTreeSet<String>> {
    let runtime = graph
        .get("runtime_presentation")?
        .get("nodes")?
        .as_array()?
        .iter()
        .filter_map(|node| node["symbol"].as_str())
        .collect::<BTreeSet<_>>();
    let nodes = graph["nodes"]
        .as_array()?
        .iter()
        .filter_map(|node| Some((node["symbol"].as_str()?, node)))
        .collect::<BTreeMap<_, _>>();
    let mut keep = BTreeSet::new();
    let mut pending = nodes
        .keys()
        .copied()
        .filter(|symbol| !runtime.contains(symbol))
        .collect::<Vec<_>>();
    while let Some(symbol) = pending.pop() {
        if !keep.insert(symbol.to_owned()) {
            continue;
        }
        let Some(node) = nodes.get(symbol) else {
            continue;
        };
        pending.extend(
            node["patches"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|patch| patch["symbol"].as_str()),
        );
        pending.extend(node["reference"].as_str());
    }
    Some(keep)
}

fn linking_label(graph: &Value, owner: &str) -> String {
    match graph.get("ornament") {
        Some(link) => match link["name"].as_str() {
            Some(name) => format!("Linking {name} for {owner}"),
            None => format!("Linking Appearance for {owner}"),
        },
        None => format!("Linking Model for {owner}"),
    }
}

fn apply_one(
    directory: &Path,
    emission: &mut PackageEmission,
    target: Target<'_>,
    previous: &BTreeMap<u32, Vec<u8>>,
    manager: &sundial::package_authoring::PackageManager,
    shared: &mut Shared,
    progress: &mut crate::item::build::Progress<'_>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let Target {
        folder,
        owner,
        spec,
        recipe,
    } = target;
    let started = std::time::Instant::now();
    let mut graph: Value = serde_json::from_slice(
        &fs::read(folder.join("asset-graph.json")).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    shared.timings.read += started.elapsed();
    progress.start(&linking_label(&graph, owner));
    if graph["source"] == "d1_roi"
        && graph["kind"] == "armor"
        && graph["converter_revision"].as_u64().unwrap_or(0) < 48
    {
        return Err(invalid(
            "Reimport this D1 armor and its ornaments. Older prepared assets use experimental shader color bindings",
        ));
    }
    if graph["source"] == "halo_reach"
        && graph["animation"]["equipment_skeleton"]["source_parts"]["placement"] == "Trailing"
        && (graph["animation"]["equipment_skeleton"]["source_parts"]["layout"].is_null()
            || graph["animation"]["equipment_skeleton"]["source_parts"]["layout"]
                == "trailing procedural range merged into its FK section")
    {
        return Err(invalid(
            "Import this Reach weapon again. Its donor's moving parts lost their procedural bones",
        ));
    }
    if graph["kind"] == "shader" {
        shader::bind(
            &mut graph,
            spec.ok_or_else(|| invalid("Source shader recipe missing"))?,
        )?;
    } else if graph.get("gear_art").is_some() {
        gear::bind(&mut graph, recipe, emission)?;
    } else if let Some(spec) = spec.filter(|spec| spec.kind == ItemKind::Mod) {
        bind_mod(&mut graph, spec);
    }
    // Shader and gear graphs take keys from their recipes as they bind, so only a model graph's
    // keys can repeat another graph's. Every graph's keys count as taken.
    if graph["kind"] != "shader" && graph.get("gear_art").is_none() {
        let table = match previous.get(&art::ASSIGNMENT_TABLE.0) {
            Some(table) => table.clone(),
            None => manager
                .read_tag(art::ASSIGNMENT_TABLE)
                .map_err(|error| invalid(error.to_string()))?,
        };
        keep_keys_apart(&mut graph, recipe, &mut shared.keys, &table)?;
    } else {
        shared.keys.extend(graph_keys(&graph));
    }
    let json_nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| invalid("Asset nodes missing"))?;
    let appearance = appearance_symbols(&graph);
    let json_nodes = &json_nodes
        .iter()
        .filter(|node| {
            appearance.as_ref().is_none_or(|keep| {
                node["symbol"]
                    .as_str()
                    .is_some_and(|symbol| keep.contains(symbol))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let branding = shared.branding;
    let shader = graph["kind"] == "shader";
    let origin = crate::package_profile::host()
        .is_standalone()
        .then(|| crate::mod_family::build::graph_origin(&graph, spec))
        .flatten();
    let cache_key = crate::mod_family::build::group_key(folder, spec);
    let reused = origin.and_then(|origin| crate::mod_family::build::group(origin, &cache_key));
    let linked = if let Some(symbols) = reused {
        let owner = origin.unwrap();
        let ids = symbols
            .values()
            .map(|tag| tag.pkg_id())
            .collect::<BTreeSet<_>>();
        for id in ids {
            crate::mod_family::build::use_id(owner, id);
            if !emission
                .asset_packages
                .packages
                .iter()
                .any(|package| package.id == id)
            {
                emission
                    .asset_packages
                    .packages
                    .push(crate::mod_family::build::restore(id).map_err(invalid)?);
            }
        }
        if symbols.is_empty() {
            return Err(invalid("Cached imported graph has no symbols"));
        }
        linking::Linked { symbols }
    } else {
        emission.asset_packages.owner = origin;
        let linked = (|| {
            let started = std::time::Instant::now();
            let mut nodes = Vec::with_capacity(json_nodes.len());
            let mut bound = parhelion_import::tiger::material::bind(&graph, folder)
                .map_err(|e| invalid(format!("Imported model bindings: {e:#}")))?;
            let inputs = spec
                .and_then(|spec| spec.overrides.imported_graph.as_ref())
                .filter(|reference| reference.directory == folder)
                .and_then(|reference| reference.material_inputs.as_ref());
            parhelion_import::marathon::inputs::bind(&graph, folder, inputs, &mut bound)
                .map_err(|e| invalid(format!("Imported material inputs: {e:#}")))?;
            parhelion_import::marathon::restore_source_eye(&graph, folder, &mut bound)
                .map_err(|e| invalid(format!("Imported sight eye: {e:#}")))?;
            let models =
                parhelion_import::model_symbols(&graph).map_err(|e| invalid(e.to_string()))?;
            for n in json_nodes {
                let model = n["symbol"]
                    .as_str()
                    .is_some_and(|name| models.contains(name));
                nodes.push(graph_node(
                    folder,
                    n,
                    model,
                    graph["source"] == "halo_reach",
                    spec,
                    &mut bound,
                    manager,
                )?);
            }
            streaming::split(&mut nodes, manager)?;
            shared.timings.nodes += started.elapsed();
            let started = std::time::Instant::now();
            loading::replication_companions(
                manager,
                directory,
                &mut nodes,
                &mut shared.companions,
            )?;
            let borrowed = if shader {
                shader::edit(
                    &mut nodes,
                    &graph,
                    spec.ok_or_else(|| invalid("Source shader recipe missing"))?,
                    manager,
                )?
            } else {
                BTreeMap::new()
            };
            let primary = if shader {
                graph["dyes"]
                    .as_array()
                    .and_then(|dyes| dyes.first())
                    .and_then(|dye| dye["parent"].as_str())
                    .ok_or_else(|| invalid("Imported shader has no dye parent"))?
            } else {
                graph["parent"].as_str().unwrap_or("parent")
            };
            let reuse =
                shared
                    .identities
                    .reuse(origin, &mut nodes, &emission.asset_packages, manager)?;
            shared.timings.companions += started.elapsed();
            let started = std::time::Instant::now();
            let mut linked = linking::link_with(
                directory,
                emission,
                manager,
                (nodes, &reuse.external),
                primary,
                &mut shared.companions,
                |symbols, groups| {
                    loading::include_native_resources(
                        manager, folder, json_nodes, symbols, groups,
                    )?;
                    for (scope, resources) in &borrowed {
                        let tag = symbols
                            .get(scope)
                            .ok_or_else(|| invalid("Source shader scope was not allocated"))?;
                        for group in groups.values_mut().filter(|g| g.contains(tag)) {
                            group.extend(resources.iter().copied());
                            group.sort();
                            group.dedup();
                        }
                    }
                    Ok(())
                },
                // Linked payloads are written only on request, to compare with staged packages.
                std::env::var_os("PARHELION_KEEP_LINKED")
                    .is_some()
                    .then(|| folder.join("linked"))
                    .as_deref(),
            )?;
            shared
                .identities
                .register(origin, reuse, &mut linked.symbols)?;
            shared.timings.link += started.elapsed();
            Ok::<_, crate::AuthoringError>(linked)
        })();
        emission.asset_packages.owner = None;
        let linked = linked?;
        if let Some(origin) = origin {
            crate::mod_family::build::remember_group(origin, cache_key, &linked.symbols);
        }
        linked
    };
    let linking::Linked { symbols } = linked;
    if shader {
        let replacement = dyes::apply(manager, emission, &symbols, &graph, previous)?
            .ok_or_else(|| invalid("Imported shader has no dye rows"))?;
        return Ok(vec![replacement]);
    }
    gear::collections(emission, &graph, spec)?;
    if graph.get("gear_art").is_some() {
        let mut result = gear::apply(manager, emission, &symbols, &graph, previous)?;
        ornaments::apply(emission, &graph)?;
        ornament_icon::apply(manager, emission, &graph, folder, branding)?;
        // A weapon ornament drawn from gear art, as Rise of Iron's are, joins Collections as one
        // drawn from an art row does.
        ornament_collections::apply(manager, emission, &graph)?;
        result.extend(character::apply(
            manager,
            emission,
            &graph,
            folder,
            &symbols,
            previous,
            &mut shared.characters,
        )?);
        return Ok(result);
    }
    let symbol = |s: &str| -> AuthoringResult<TagHash> {
        symbols
            .get(s)
            .copied()
            .ok_or_else(|| invalid(format!("Missing symbol {s}")))
    };
    let nodes = json_nodes;
    let read = |tag| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|e| invalid(e.to_string()))
    };
    let item = graph["item_hash"]
        .as_u64()
        .ok_or_else(|| invalid("Item hash missing"))? as u32;
    let key = graph["art_key"]
        .as_u64()
        .ok_or_else(|| invalid("Art key missing"))? as u32;
    let source_key = graph["native_assignment"].as_u64().unwrap_or(0xCFBE7264) as u32;
    let (row_index, native_art_item) = art::prepare_row(
        manager,
        emission,
        item,
        graph["native_item"].as_u64().unwrap_or(0x02222CBF) as u32,
        source_key,
    )?;
    let (_, _, start, _) =
        sundial::package_authoring::native_payload::native_array_at(&emission.item_metadata, 8)
            .map_err(invalid)?;
    let row = start + row_index * 32;
    let kept = kept_parts(&graph)?;
    replace(
        &mut emission.item_metadata,
        row,
        key,
        native_art_item,
        source_key,
        &kept,
    )?;
    let ordinal = definition_ordinal(emission, item)?;
    let definition = &mut emission
        .host_new_tags
        .get_mut(ordinal)
        .ok_or_else(|| invalid("Authored definition missing"))?
        .payload;
    let translation = crate::tag_payload::relative_target(definition, 0x88)?;
    let (art_count, _, art_start, _) =
        sundial::package_authoring::native_payload::native_array_at(definition, translation)
            .map_err(invalid)?;
    if art_count != 1 {
        return Err(invalid("Expected one class-neutral art row"));
    }
    definition[art_start] = 255;
    definition[art_start + 2..art_start + 4].copy_from_slice(
        &u16::try_from(row_index)
            .map_err(|_| invalid("Art index overflow"))?
            .to_le_bytes(),
    );
    let table_tag = art::ASSIGNMENT_TABLE;
    let original = match previous.get(&table_tag.0) {
        Some(data) => data.clone(),
        None => read(table_tag.0)?,
    };
    let mut assignments = vec![(key, symbol("parent")?)];
    for part in &kept {
        assignments.push((part.key, symbol(&part.parent)?));
    }
    let table = art::insert_assignments(&original, &assignments)?;
    if origin.is_none() || !crate::mod_family::build::is_kept(origin.unwrap()) {
        fs::write(
            folder.join("allocated.json"),
            serde_json::to_vec_pretty(
                &symbols
                    .iter()
                    .map(|(k, v)| (k, v.0))
                    .collect::<BTreeMap<_, _>>(),
            )
            .map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(|e| invalid(e.to_string()))?;
    }
    eprintln!(
        "Imported {} private asset tags; art index {}; parent {}",
        nodes.len(),
        row_index,
        symbol("parent")?
    );
    let mut result = vec![table];
    if let Some(replacement) = dyes::apply(manager, emission, &symbols, &graph, previous)? {
        result.push(replacement);
    }
    let crosshairs = crosshair::apply(manager, emission, &symbols, &graph, previous, item)?;
    // The inspect scene row joins the table as the crosshair step left it.
    let table = crosshairs
        .as_ref()
        .map(|replacement| replacement.payload.as_slice())
        .or_else(|| {
            previous
                .get(&crate::weapon::crosshair::TABLE.0)
                .map(Vec::as_slice)
        });
    let inspect = if spec.is_some_and(|spec| spec.kind == ItemKind::Weapon) {
        inspect::apply(
            (directory, manager),
            emission,
            (&graph, folder),
            table,
            item,
            &mut shared.companions,
        )?
    } else {
        None
    };
    result.extend(inspect.or(crosshairs));
    let ordinal = definition_ordinal(emission, item)?;
    let definition = &mut emission.host_new_tags[ordinal].payload;
    arrays::add_array_markers(definition).map_err(invalid)?;

    ornaments::apply(emission, &graph)?;
    ornament_icon::apply(manager, emission, &graph, folder, branding)?;
    ornament_collections::apply(manager, emission, &graph)?;
    result.extend(character::apply(
        manager,
        emission,
        &graph,
        folder,
        &symbols,
        previous,
        &mut shared.characters,
    )?);
    Ok(result)
}

fn validate_graph_kinds(weapons: &[WeaponCloneSpec]) -> AuthoringResult<()> {
    for spec in weapons {
        if let Some(reference) = &spec.overrides.imported_graph {
            let graph: Value = serde_json::from_slice(
                &fs::read(reference.directory.join("asset-graph.json"))
                    .map_err(|e| invalid(e.to_string()))?,
            )
            .map_err(|e| invalid(e.to_string()))?;
            if (graph["kind"] == "shader") != (spec.kind == ItemKind::Shader) {
                return Err(invalid(
                    "Imported asset kind does not match its item recipe",
                ));
            }
            if let Some(kind) = crate::imported::kind(spec.kind) {
                if graph["kind"] != kind {
                    return Err(invalid("Imported asset kind does not match its recipe"));
                }
            } else if graph.get("gear_art").is_some()
                && !(spec.kind == ItemKind::Weapon && graph["kind"] == "weapon")
            {
                return Err(invalid("Imported gear cannot supply a weapon model"));
            }
        }
    }
    Ok(())
}
