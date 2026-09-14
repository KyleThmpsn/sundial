//! Discover installed projectiles independently of whether a perk references them.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::*;
mod identity;
mod source_names;
pub use source_names::NameEvidence;
pub mod knowledge;
mod usage;
use crate::{
    package_runtime::{index_cache, parallel, tft},
    sandbox_perk::dependencies,
    weapon_entity::{weapon_component_binding_hashes, weapon_component_bindings},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub graph: u32,
    pub kind: Kind,
    /// The native object type byte, so an entity can be described even without a name.
    #[serde(default)]
    pub object_type: u8,
    pub owners: Vec<u32>,
    pub package: String,
    pub native_name: Option<String>,
    pub native_paths: Vec<String>,
    pub contexts: Vec<Context>,
    pub perk_indices: Vec<u16>,
    /// Common operation and timing of decoded nodes that directly reference this asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hint: Option<String>,
}

impl Entry {
    /// Useful discovery evidence, separate from structural validity or gameplay testing.
    /// A global reference, package membership or a tag alone does not identify an effect.
    pub fn has_discovery_identity(&self) -> bool {
        self.has_discovery_identity_with(|_| None, |_| None)
    }

    pub fn has_discovery_identity_with(
        &self,
        perk_name: impl FnMut(u16) -> Option<String>,
        item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> bool {
        self.discovery_name_with(perk_name, item_name).is_some()
            && (self.kind != Kind::Entity
                || self
                    .source_hint
                    .as_deref()
                    .is_some_and(identity::meaningful_role))
    }
    /// A direct native identity, followed by explicitly qualified usage context.
    /// A parent's filename must never be presented as this projectile's own name.
    pub fn label(&self) -> String {
        self.label_with_perks(|_| None)
    }

    pub fn label_with_perks(&self, perk_name: impl FnMut(u16) -> Option<String>) -> String {
        self.label_with(perk_name, |_| None)
    }

    /// The label with stock perk names and weapon item names supplied by the caller. A
    /// weapon name applies when the nearest named ancestor is a sandbox pattern entity.
    /// Shared assets use a common role or weapon type rather than listing source names.
    pub fn label_with(
        &self,
        perk_name: impl FnMut(u16) -> Option<String>,
        item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> String {
        match self.identity(perk_name, item_name) {
            Identity::Shared(hint) => format!("{} · {hint}", self.kind.label()),
            Identity::Path(path) => tft::asset_label(&path),
            Identity::Name(name) => name,
            Identity::Context {
                relation, names, ..
            } => format!("{} · {relation} {}", self.kind.label(), listed(&names)),
            Identity::Perks(names) => format!("{} · Used by {}", self.kind.label(), listed(&names)),
            Identity::None => format!("{} 0x{:08X}", self.kind_label(), self.graph),
        }
    }

    /// The heading this asset sorts under when a picker groups its rows: the folder of a
    /// native path, the ancestor or perk that names it, or its package when nothing does.
    /// Assets fired by several weapons of one type sit under that type, so the shared
    /// projectiles of every hand cannon read as one row. Headings that start with
    /// "Unnamed" sort last.
    pub fn source_group(
        &self,
        perk_name: impl FnMut(u16) -> Option<String>,
        item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> String {
        match self.identity(perk_name, item_name) {
            Identity::Shared(hint) => hint,
            Identity::Path(path) => tft::asset_folder(&path),
            Identity::Name(_) => "Named Assets".to_owned(),
            Identity::Context {
                relation,
                names,
                kinds,
            } => {
                let first = names.first().expect("a context names something");
                match (names.len(), kinds.first()) {
                    (2.., Some(kind)) if kinds.len() == 1 => format!("{relation} {kind}s"),
                    _ => format!("{relation} {first}"),
                }
            }
            Identity::Perks(names) => {
                format!("Used by {}", names.first().expect("a perk names something"))
            }
            Identity::None => format!("Unnamed · {}", self.package),
        }
    }

    /// The strongest name evidence this entry has, resolved with the caller's names.
    fn identity(
        &self,
        mut perk_name: impl FnMut(u16) -> Option<String>,
        mut item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> Identity {
        if let Some(path) = self
            .native_paths
            .iter()
            .find(|path| !path.trim().is_empty() && !shared_metadata_path(path))
        {
            return Identity::Path(path.clone());
        }
        if let Some(name) = self
            .native_name
            .as_ref()
            .filter(|name| !name.trim().is_empty() && !shared_metadata_path(name))
        {
            return Identity::Name(name.clone());
        }
        if let Some(hint) = &self.source_hint {
            return Identity::Shared(hint.clone());
        }
        if let Some(identity) = self.context_identity(&mut perk_name, &mut item_name) {
            return identity;
        }
        let perks = self
            .perk_indices
            .iter()
            .map(|&index| {
                perk_name(index)
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| format!("Effect {index}"))
            })
            .collect::<BTreeSet<_>>();
        if self.perk_indices.len() > 1 {
            Identity::Shared("Shared Perk Asset".into())
        } else if perks.is_empty() {
            Identity::None
        } else {
            Identity::Perks(perks)
        }
    }

    fn context_identity(
        &self,
        mut perk_name: impl FnMut(u16) -> Option<String>,
        mut item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> Option<Identity> {
        let nearest = self
            .contexts
            .iter()
            .filter(|context| context.names_something())
            .map(|context| context.depth)
            .min();
        if let Some(depth) = nearest {
            let mut names = BTreeSet::new();
            let mut kinds = BTreeSet::new();
            let mut count = 0;
            let mut all_typed_items = true;
            let mut all_perks = true;
            let mut common_folder: Option<Vec<&str>> = None;
            for context in self
                .contexts
                .iter()
                .filter(|context| context.depth == depth && context.names_something())
            {
                count += 1;
                all_perks &= context.perk.is_some();
                all_typed_items &= context.item.is_some();
                if !context.path.is_empty() {
                    let mut parts = context.path.split(['/', '\\']).collect::<Vec<_>>();
                    parts.pop();
                    if let Some(common) = &mut common_folder {
                        let length = common
                            .iter()
                            .zip(&parts)
                            .take_while(|(a, b)| a == b)
                            .count();
                        common.truncate(length);
                    } else {
                        common_folder = Some(parts);
                    }
                }
                names.insert(match (context.item, context.perk) {
                    (Some(item), _) => match item_name(item) {
                        Some(item) => {
                            all_typed_items &= !item.kind.trim().is_empty();
                            if !item.kind.trim().is_empty() {
                                kinds.insert(item.kind);
                            }
                            item.name
                        }
                        None => {
                            all_typed_items = false;
                            format!("weapon 0x{item:08X}")
                        }
                    },
                    (None, Some(perk)) => {
                        perk_name(perk).unwrap_or_else(|| format!("Effect {perk}"))
                    }
                    (None, None) => tft::asset_label(&context.path),
                });
            }
            if count > 1 {
                let hint = if all_typed_items && kinds.len() == 1 {
                    format!("Shared by {}s", kinds.first().unwrap())
                } else if all_perks {
                    "Shared Perk Asset".into()
                } else if let Some(folder) = common_folder.filter(|parts| parts.len() > 2) {
                    format!("Shared in {}", folder.join("\\"))
                } else {
                    "Shared Asset, Role Unmapped".into()
                };
                return Some(Identity::Shared(hint));
            }
            // A direct reference is worded as one. A deeper ancestor, such as the weapon
            // pattern that fires a projectile, is where the asset comes from.
            return Some(Identity::Context {
                relation: if depth <= 1 { "Referenced by" } else { "From" },
                names,
                kinds,
            });
        }
        None
    }

    /// The kind, with the client's object type name for an entity that is not a projectile
    /// or emitter.
    #[must_use]
    pub fn kind_label(&self) -> String {
        match self.kind {
            Kind::Entity | Kind::Object | Kind::Pickup => format!(
                "{} · {}",
                self.kind.label(),
                object_type_label(self.object_type)
            ),
            kind => kind.label().to_owned(),
        }
    }

    pub fn label_rank(&self) -> u8 {
        if self
            .native_paths
            .iter()
            .any(|path| !path.trim().is_empty() && !shared_metadata_path(path))
            || self
                .native_name
                .as_ref()
                .is_some_and(|name| !name.trim().is_empty() && !shared_metadata_path(name))
        {
            0
        } else if self.contexts.iter().any(Context::names_something) {
            1
        } else if !self.perk_indices.is_empty() {
            2
        } else {
            3
        }
    }
}

/// A weapon item as the caller can name it: its display name and its weapon type, such as
/// "Hand Cannon". The type is empty when the caller does not know it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemName {
    pub name: String,
    pub kind: String,
}

impl ItemName {
    #[must_use]
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: kind.into(),
        }
    }
}

/// The name evidence behind an entry's label, strongest first.
enum Identity {
    Shared(String),
    /// The entry's own native content path.
    Path(String),
    /// The entry's own misc tag name.
    Name(String),
    /// The nearest named ancestors, with how they relate and the weapon types among them.
    Context {
        relation: &'static str,
        names: BTreeSet<String>,
        kinds: BTreeSet<String>,
    },
    /// Stock perks that reference the entry directly.
    Perks(BTreeSet<String>),
    None,
}

/// How many names a label spells out before counting the rest.
const LISTED_NAMES: usize = 3;

/// The first few names, then " (+n)" for the ones left unlisted.
fn listed(names: &BTreeSet<String>) -> String {
    let shown = names
        .iter()
        .take(LISTED_NAMES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > LISTED_NAMES {
        format!("{shown} (+{})", names.len() - LISTED_NAMES)
    } else {
        shown
    }
}

/// A named resource reaches this asset through references. It describes where the asset
/// sits, not the asset's own identity or a guarantee about a native execution path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// The named ancestor.
    pub graph: u32,
    /// The resource through which the ancestor was reached, or the ancestor itself when
    /// it refers to the asset directly.
    pub owner: u32,
    /// Retained for older callers. The reference walk does not record byte offsets.
    #[serde(default)]
    pub offset: usize,
    /// The ancestor's content path or misc tag name. Empty when `item` names it instead.
    pub path: String,
    /// An entity name recovered by an exact hash match to installed native vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_evidence: Option<NameEvidence>,
    /// The weapon item whose sandbox pattern entity is the ancestor, when the ancestor has
    /// no path of its own. The caller resolves the item's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u32>,
    /// A stock perk that references the ancestor graph directly, when nothing names the
    /// ancestor itself. Ability graphs are reached this way, so an ability's projectile is
    /// named after the ability perk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perk: Option<u16>,
    /// How many reference steps separate the ancestor from the asset. One is a direct
    /// reference.
    #[serde(default = "one")]
    pub depth: usize,
}

impl Context {
    /// Whether this context carries a name at all.
    #[must_use]
    pub fn names_something(&self) -> bool {
        self.item.is_some()
            || self.perk.is_some()
            || (!self.path.trim().is_empty() && !shared_metadata_path(&self.path))
    }
}

/// A label registry names labels shared by hundreds of behaviors, not the resource that
/// happens to reference it. Keep its path in the native index, outside asset identity.
fn shared_metadata_path(path: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("label_globals.label_globals.tft"))
}

const fn one() -> usize {
    1
}

/// A name the reference walk can stop at.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Named {
    Symbol(NameEvidence),
    /// A content path or misc tag name.
    Path(String),
    /// A sandbox pattern entity, identified by the weapon items whose patterns share it.
    /// Reissued weapons keep the entity of the original, so one entity can carry several.
    Items(Vec<u32>),
    /// A graph a stock perk references, identified by the finished perk index.
    Perks(Vec<u16>),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub errors: Vec<String>,
    /// Native component resources, retained instead of reducing their identity to a count.
    #[serde(default)]
    pub owners: BTreeMap<u32, Owner>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Owner {
    pub package: String,
    pub names: Vec<String>,
    pub components: Vec<OwnerComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnerComponent {
    pub binding: u32,
    pub class: u32,
}

impl Catalog {
    /// The packages that carry an asset some stock perk references. Assets from other
    /// packages exist in the installation but are unlikely candidates for a weapon perk.
    #[must_use]
    pub fn perk_packages(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter(|entry| !entry.perk_indices.is_empty())
            .map(|entry| entry.package.clone())
            .collect()
    }
}

fn owners(payload: &[u8]) -> Result<BTreeMap<u32, Vec<OwnerComponent>>, String> {
    let mut owners = BTreeMap::<u32, Vec<OwnerComponent>>::new();
    for binding in weapon_component_binding_hashes(payload)? {
        for resource in weapon_component_bindings(payload, binding)? {
            owners
                .entry(resource.owner_tag)
                .or_default()
                .push(OwnerComponent {
                    binding,
                    class: resource.concrete_class,
                });
        }
    }
    Ok(owners)
}

fn perk_references(
    index: &dependencies::Index,
) -> (BTreeMap<u32, Vec<u16>>, BTreeMap<u32, Vec<u16>>) {
    let mut references = BTreeMap::<u32, Vec<u16>>::new();
    for perk in &index.perks {
        if let Ok(perk_index) = u16::try_from(perk.index) {
            for graph in &perk.graphs {
                references.entry(graph.tag).or_default().push(perk_index);
            }
        }
    }
    let mut perk_graphs = references.clone();
    for perk in &index.perks {
        if let (Ok(perk_index), Some(action)) = (u16::try_from(perk.index), perk.action) {
            perk_graphs.entry(action).or_default().push(perk_index);
        }
    }
    (references, perk_graphs)
}

pub fn inspect(
    manager: &PackageManager,
    index: &dependencies::Index,
    names: &tft::Index,
) -> Result<Catalog, String> {
    let mut catalog = Catalog::default();
    let native_names = names.names();
    let symbols = source_names::index(&names.paths);
    let mut recovered = BTreeMap::new();
    catalog.errors.extend(names.errors.iter().cloned());
    let (mut references, perk_graphs) = perk_references(index);
    // A weapon's pattern entity rarely carries a content path, but the items that own the
    // pattern have names, and the projectiles it fires sit one component below it.
    let mut pattern_items = HashMap::<u32, Vec<u32>>::new();
    for pattern in &index.patterns {
        if matches!(pattern.item_hash, 0 | 0x811C_9DC5) {
            continue;
        }
        if let Some(entity) = &pattern.entity {
            let items = pattern_items.entry(entity.tag).or_default();
            if !items.contains(&pattern.item_hash) {
                items.push(pattern.item_hash);
            }
        }
    }
    // Every graph's component resources, including graphs the picker does not offer, so
    // the reference walk can climb from a resource to the graph that binds it.
    let mut resource_graph = HashMap::<u32, Vec<u32>>::new();
    for (tag, scan) in read_graphs(manager) {
        let (object_type, owners, kind) = match scan {
            Scan::Unreadable(error) => {
                catalog.errors.push(format!("{tag}: {error}"));
                continue;
            }
            Scan::Empty => continue,
            Scan::Graph {
                object_type,
                name_hash,
                owners,
                kind,
            } => {
                if let Some(evidence) = symbols.get(&name_hash) {
                    recovered.insert(tag.0, evidence.clone());
                }
                (object_type, owners, kind)
            }
        };
        for (&owner, components) in &owners {
            resource_graph.entry(owner).or_default().push(tag.0);
            let detail = catalog.owners.entry(owner).or_insert_with(|| Owner {
                package: manager
                    .package_paths
                    .get(&TagHash(owner).pkg_id())
                    .map(|package| package.name.clone())
                    .unwrap_or_default(),
                names: native_names
                    .get(&owner)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .chain(manager.get_tag_name(TagHash(owner)))
                    .collect(),
                components: Vec::new(),
            });
            detail.components.extend(components.iter().cloned());
            detail.components.sort_unstable();
            detail.components.dedup();
        }
        let kind = match kind {
            Some(Ok(kind)) => kind,
            Some(Err(error)) => {
                catalog.errors.push(format!("{tag}: {error}"));
                continue;
            }
            None => continue,
        };
        catalog.entries.push(Entry {
            graph: tag.0,
            kind,
            object_type,
            owners: owners.into_keys().collect(),
            package: manager
                .package_paths
                .get(&tag.pkg_id())
                .map(|package| package.name.clone())
                .unwrap_or_default(),
            native_name: manager.get_tag_name(tag),
            native_paths: native_names.get(&tag.0).cloned().unwrap_or_default(),
            contexts: Vec::new(),
            perk_indices: references.remove(&tag.0).unwrap_or_default(),
            source_hint: None,
        });
    }
    attach_context(
        manager,
        &mut catalog,
        names,
        SourceNames {
            paths: &native_names,
            symbols: &recovered,
        },
        &resource_graph,
        &pattern_items,
        &perk_graphs,
    );
    usage::annotate(manager, index, &mut catalog);
    Ok(catalog)
}

/// What one entity graph contributes to the catalog, read off its payload on a worker.
enum Scan {
    Unreadable(String),
    /// Too short to carry an object type.
    Empty,
    Graph {
        object_type: u8,
        name_hash: u32,
        /// Component resources the graph binds. A component map that does not parse only
        /// means the reference walk cannot climb through this graph, so it is empty then.
        owners: BTreeMap<u32, Vec<OwnerComponent>>,
        /// `None` for an object type the picker never offers. Any other attachable type is
        /// offered as a plain entity, including graphs no stock perk references.
        kind: Option<Result<Kind, String>>,
    },
}

fn scan_graph(manager: &PackageManager, tag: TagHash) -> Scan {
    let payload = match manager.read_tag(tag) {
        Ok(payload) => payload,
        Err(error) => return Scan::Unreadable(error.to_string()),
    };
    let Some(&object_type) = payload.get(OBJECT_TYPE_OFFSET) else {
        return Scan::Empty;
    };
    let offered =
        ATTACHED_OBJECT_TYPES.contains(&object_type) || matches!(object_type, 1..=8 | 11 | 18..=21);
    let owners = match owners(&payload) {
        Ok(owners) => owners,
        Err(error) => return Scan::Unreadable(error),
    };
    Scan::Graph {
        object_type,
        name_hash: crate::package_payload::u32_at(&payload, 8).unwrap_or_default(),
        owners,
        kind: offered.then(|| spawn_kind(&payload).map(|kind| kind.unwrap_or(Kind::Entity))),
    }
}

/// Every live entity graph in tag order, read one package per worker. Tens of thousands of
/// graphs read one after another dominate a catalog build, and each package has its own
/// reader, so packages are the natural unit of parallelism.
fn read_graphs(manager: &PackageManager) -> Vec<(TagHash, Scan)> {
    let mut tags = manager
        .get_all_by_reference(WEAPON_ENTITY_CLASS)
        .into_iter()
        .filter(|(_, entry)| entry.file_type == 8)
        .map(|(tag, _)| tag)
        .collect::<Vec<_>>();
    tags.sort_by_key(|tag| tag.0);
    let jobs = tags
        .chunk_by(|a, b| a.pkg_id() == b.pkg_id())
        .collect::<Vec<_>>();
    parallel::map_jobs(&jobs, |package| {
        package
            .iter()
            .map(|&tag| (tag, scan_graph(manager, tag)))
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect()
}

/// How many reference steps the walk climbs before giving up on a name.
const MAX_CLIMB: usize = 6;
/// Caps one level of the walk, so a graph referenced from everywhere stays cheap.
const MAX_FRONTIER: usize = 4096;

struct SourceNames<'a> {
    paths: &'a BTreeMap<u32, Vec<String>>,
    symbols: &'a BTreeMap<u32, NameEvidence>,
}

/// Names each unnamed entry after the nearest resources that reach it through references.
fn attach_context(
    manager: &PackageManager,
    catalog: &mut Catalog,
    names: &tft::Index,
    sources: SourceNames<'_>,
    resource_graph: &HashMap<u32, Vec<u32>>,
    pattern_items: &HashMap<u32, Vec<u32>>,
    perk_graphs: &BTreeMap<u32, Vec<u16>>,
) {
    let mut parents = HashMap::<u32, Vec<u32>>::new();
    for reference in &names.entity_references {
        parents
            .entry(reference.target)
            .or_default()
            .push(reference.source);
    }
    let own_paths = names.own_paths();
    let name_of = |tag: u32| -> Option<Named> {
        sources
            .paths
            .get(&tag)
            .and_then(|paths| {
                paths
                    .iter()
                    .find(|path| !shared_metadata_path(path))
                    .cloned()
            })
            .or_else(|| manager.get_tag_name(TagHash(tag)))
            .filter(|path| !shared_metadata_path(path))
            .map(Named::Path)
            .or_else(|| sources.symbols.get(&tag).cloned().map(Named::Symbol))
            .or_else(|| {
                own_paths
                    .get(&tag)
                    .and_then(|paths| {
                        paths
                            .iter()
                            .find(|path| !shared_metadata_path(path))
                            .cloned()
                    })
                    .map(Named::Path)
            })
            .or_else(|| pattern_items.get(&tag).cloned().map(Named::Items))
            .or_else(|| perk_graphs.get(&tag).cloned().map(Named::Perks))
    };
    for entry in &mut catalog.entries {
        if entry.label_rank() == 0 {
            continue;
        }
        entry.contexts = climb(entry.graph, &parents, resource_graph, name_of);
        if let Some(evidence) = sources
            .symbols
            .get(&entry.graph)
            .filter(|evidence| identity::source_name(&evidence.name).is_some())
        {
            entry.contexts.insert(
                0,
                Context {
                    graph: entry.graph,
                    owner: entry.graph,
                    offset: 8,
                    path: evidence.name.clone(),
                    name_evidence: Some(evidence.clone()),
                    item: None,
                    perk: None,
                    depth: 0,
                },
            );
        }
    }
}

/// Breadth-first walk from `graph` through the resources that refer to it and the graphs
/// that bind those resources, stopping at the first level with a named ancestor.
fn climb(
    graph: u32,
    parents: &HashMap<u32, Vec<u32>>,
    resource_graph: &HashMap<u32, Vec<u32>>,
    mut name_of: impl FnMut(u32) -> Option<Named>,
) -> Vec<Context> {
    let mut seen = HashSet::from([graph]);
    let mut frontier = vec![graph];
    for depth in 1..=MAX_CLIMB {
        let mut next = Vec::new();
        let mut found = Vec::new();
        for &via in &frontier {
            let ups = parents
                .get(&via)
                .into_iter()
                .chain(resource_graph.get(&via))
                .flatten()
                .copied();
            for up in ups {
                if !seen.insert(up) {
                    continue;
                }
                let context = |path: String, item: Option<u32>, perk: Option<u16>| Context {
                    graph: up,
                    owner: via,
                    offset: 0,
                    path,
                    name_evidence: None,
                    item,
                    perk,
                    depth,
                };
                match name_of(up) {
                    Some(Named::Symbol(evidence)) => {
                        let mut named = context(evidence.name.clone(), None, None);
                        named.name_evidence = Some(evidence);
                        found.push(named);
                    }
                    Some(Named::Path(path)) if shared_metadata_path(&path) => next.push(up),
                    Some(Named::Path(path)) => found.push(context(path, None, None)),
                    // Every weapon that shares the pattern entity fires this asset.
                    Some(Named::Items(items)) => {
                        found.extend(
                            items
                                .into_iter()
                                .map(|item| context(String::new(), Some(item), None)),
                        );
                    }
                    Some(Named::Perks(perks)) => found.extend(
                        perks
                            .into_iter()
                            .map(|perk| context(String::new(), None, Some(perk))),
                    ),
                    None => next.push(up),
                }
            }
        }
        if !found.is_empty() {
            found.sort_by(|a, b| {
                (&a.path, a.item, a.perk, a.graph).cmp(&(&b.path, b.item, b.perk, b.graph))
            });
            found.dedup_by(|a, b| a.path == b.path && a.item == b.item && a.perk == b.perk);
            return found;
        }
        next.truncate(MAX_FRONTIER);
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    Vec::new()
}

static CACHE: index_cache::Cache<Catalog> = index_cache::Cache::new();

/// The on-disk cache name. Bump it whenever an entry's contents change.
const CACHE_VERSION: &str = "projectiles-v13";

/// The catalog already cached for this installation, without building one. The build uses
/// it to refuse assets that only load with an activity.
pub fn cached_only(packages: &Path) -> Result<Option<Arc<Catalog>>, String> {
    index_cache::cached_only(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        CACHE_VERSION,
        &CACHE,
    )
}

/// Keep repeat parameter opens fast and invalidate after any package changes.
pub fn cached(packages: &Path, manager: &PackageManager) -> Result<Arc<Catalog>, String> {
    index_cache::cached(
        packages,
        crate::sandbox_perk::CACHE_DIRECTORY,
        CACHE_VERSION,
        &CACHE,
        || {
            let dependencies = dependencies::cached(packages, manager, |_, _| {})?;
            let names = tft::cached(packages, manager, |_, _| {})?;
            inspect(manager, &dependencies, &names)
        },
        // Preserve incomplete-discovery warnings along with usable results.
        // Compiling a selected effect still reads and validates its live tags.
        |_| true,
    )
}

#[cfg(test)]
mod tests;
