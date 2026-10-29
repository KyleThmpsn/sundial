//! Allocate a private resource group in an asset package and resolve its cross-references.
//! Native loading owners get a cloned companion index so the client loads the group with
//! the owner instead of through the global runtime root.
use super::*;

#[cfg(test)]
mod materialization;

/// One private tag: a native template's metadata, an authored payload, and the words inside
/// it that name other members of the group by symbol.
pub(super) struct Node {
    pub symbol: String,
    pub template: TagHash,
    pub payload: Vec<u8>,
    /// Payload offsets holding `0xFFFFFFFF` that receive the named symbol's tag.
    pub patches: Vec<(usize, String)>,
    /// The entry-header reference (a buffer header naming its data), by symbol.
    pub reference: Option<String>,
    /// A cloned loading index for the named owner symbol, from that native parent's companion.
    pub companion: Option<Companion>,
    pub storage: crate::NewTagStorageMode,
    /// A texture's large buffer, which the client streams on demand. No loading group lists it,
    /// even though its header names it.
    pub streamed: bool,
    #[cfg(feature = "d2-model-importer")]
    pub audio: crate::item::custom_runtime::assets::audio::Metadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Companion {
    pub shared_owner: String,
    pub source_parent: u32,
    /// Further native loading owners whose dependencies the clone also loads, for a group that
    /// keeps native assets which only another native owner's index loads.
    pub inherited: Vec<u32>,
}

impl Companion {
    pub fn new(shared_owner: impl Into<String>, source_parent: u32) -> Self {
        Self {
            shared_owner: shared_owner.into(),
            source_parent,
            inherited: Vec::new(),
        }
    }

    fn native_parents(&self) -> impl Iterator<Item = u32> + '_ {
        std::iter::once(self.source_parent).chain(self.inherited.iter().copied())
    }
}

/// Ordered declarations used to choose final tags before payload conversion.
pub(super) struct Declaration {
    pub symbol: String,
    pub template: TagHash,
    pub bound: usize,
    pub companion: Option<Companion>,
}

impl Declaration {
    pub fn new(symbol: impl Into<String>, template: u32, bound: usize) -> Self {
        Self {
            symbol: symbol.into(),
            template: TagHash(template),
            bound,
            companion: None,
        }
    }
}

impl Node {
    pub fn new(symbol: impl Into<String>, template: u32, payload: Vec<u8>) -> Self {
        Self {
            symbol: symbol.into(),
            template: TagHash(template),
            payload,
            patches: Vec::new(),
            reference: None,
            companion: None,
            storage: crate::NewTagStorageMode::InheritTemplate,
            streamed: false,
            #[cfg(feature = "d2-model-importer")]
            audio: Default::default(),
        }
    }

    /// Blank the word at `offset` and record the symbol that fills it.
    pub fn patch(&mut self, offset: usize, symbol: impl Into<String>) -> AuthoringResult<()> {
        write_u32(&mut self.payload, offset, u32::MAX)?;
        self.patches.push((offset, symbol.into()));
        Ok(())
    }
}

pub(super) struct Linked {
    pub symbols: BTreeMap<String, TagHash>,
}

/// Resources an earlier group already committed that this group names by symbol, as a texture
/// shared with an earlier imported graph is. They are resolved rather than allocated, and they
/// join this group's loading closures.
#[derive(Default)]
pub(super) struct External {
    pub symbols: BTreeMap<String, TagHash>,
    pub resources: Vec<crate::LoadingResource>,
}

pub(super) use crate::shared_tag_dependency_index::native::{Companions, native_companion};

/// Reserve, allocate and link one group. `primary` names the owner that keeps every
/// resource nothing else references. `extra_groups` may widen the loading groups before the
/// companions are cloned (borrowed native materials, for instance).
#[allow(clippy::too_many_arguments)]
pub(super) fn link(
    directory: &Path,
    emission: &mut PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    nodes: Vec<Node>,
    primary: &str,
    companions: &mut Companions,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    link_with(
        directory,
        emission,
        manager,
        (nodes, &External::default()),
        primary,
        companions,
        extra_groups,
        debug,
    )
}

/// [`link`] for a group that also names `external` resources an earlier group committed.
#[allow(clippy::too_many_arguments)]
pub(super) fn link_with(
    directory: &Path,
    emission: &mut PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    (nodes, external): (Vec<Node>, &External),
    primary: &str,
    companions: &mut Companions,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    let mut declarations = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let size = match &node.companion {
            // Account for a sparse descriptor and index per resource across packages,
            // in addition to every retained native index.
            Some(companion) => {
                let mut size = (nodes.len() + external.resources.len())
                    .checked_mul(64)
                    .and_then(|bytes| bytes.checked_add(crate::format::BLOCK_SIZE))
                    .ok_or_else(|| invalid("Companion size overflow"))?;
                for parent in companion.native_parents() {
                    size = native_companion(directory, manager, parent, companions)?
                        .1
                        .len()
                        .checked_add(size)
                        .ok_or_else(|| invalid("Companion size overflow"))?;
                }
                size
            }
            None => node.payload.len(),
        };
        let mut declaration = Declaration::new(node.symbol.clone(), node.template.0, size);
        declaration.companion = node.companion.clone();
        declarations.push(declaration);
    }
    materialize(
        directory,
        &mut emission.asset_packages,
        manager,
        (declarations, external),
        primary,
        companions,
        |_symbols| {
            #[cfg(feature = "d2-model-importer")]
            let nodes = {
                use crate::item::custom_runtime::assets::audio;
                let symbols = _symbols;
                let views = nodes
                    .iter()
                    .map(|node| audio::Node {
                        symbol: &node.symbol,
                        payload: &node.payload,
                        storage: node.storage,
                        patched: !node.patches.is_empty(),
                        metadata: &node.audio,
                    })
                    .collect::<Vec<_>>();
                let mut bound = audio::bind(
                    &views,
                    symbols,
                    &format!("parhelion/{:08x}/appearance-audio", symbols[primary].0),
                )?;
                let mut nodes = nodes;
                for node in &mut nodes {
                    if let Some(payload) = bound.remove(&node.symbol) {
                        node.payload = payload;
                    }
                }
                nodes
            };
            Ok(nodes)
        },
        extra_groups,
        debug,
    )
}

fn validate_materialized(nodes: &[Node], declarations: &[Declaration]) -> AuthoringResult<()> {
    if nodes.len() != declarations.len() {
        return Err(invalid(
            "Materialized asset count differs from its declaration",
        ));
    }
    for (node, declaration) in nodes.iter().zip(declarations) {
        if node.symbol != declaration.symbol
            || node.template != declaration.template
            || node.companion != declaration.companion
            || node.payload.len() > declaration.bound
            || (node.companion.is_none() && node.payload.is_empty())
        {
            return Err(invalid(
                "Materialized asset order, template, companion or bound differs",
            ));
        }
        if node.companion.is_some() && !node.patches.is_empty() {
            return Err(invalid(
                "A cloned companion cannot patch a discarded input payload",
            ));
        }
        let mut bytes = BTreeSet::new();
        for (offset, _) in &node.patches {
            let end = offset
                .checked_add(4)
                .ok_or_else(|| invalid("Asset patch offset overflow"))?;
            if end > node.payload.len() || (*offset..end).any(|at| !bytes.insert(at)) {
                return Err(invalid(
                    "Asset patch is out of bounds or overlaps another patch",
                ));
            }
        }
    }
    Ok(())
}

/// Check symbols and read the native companions before planning physical packages.
fn check_declarations(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    (declarations, primary): (&[Declaration], &str),
    companions: &mut Companions,
) -> AuthoringResult<()> {
    let mut names = BTreeSet::new();
    for declaration in declarations {
        if declaration.symbol.is_empty() || !names.insert(declaration.symbol.as_str()) {
            return Err(invalid("Private asset symbols must be nonempty and unique"));
        }
        if let Some(companion) = &declaration.companion {
            for parent in companion.native_parents() {
                native_companion(directory, manager, parent, companions)?;
            }
        }
    }
    if !names.contains(primary) {
        return Err(invalid("The primary private asset symbol is missing"));
    }
    Ok(())
}

/// Supply final private IDs to conversion, then commit the fully checked group.
#[allow(clippy::too_many_arguments)]
pub(super) fn materialize(
    directory: &Path,
    assets: &mut crate::asset_packages::AssetPackages,
    manager: &sundial::package_authoring::PackageManager,
    (declarations, external): (Vec<Declaration>, &External),
    primary: &str,
    companions: &mut Companions,
    materializer: impl FnOnce(&BTreeMap<String, TagHash>) -> AuthoringResult<Vec<Node>>,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    check_declarations(directory, manager, (&declarations, primary), companions)?;
    let names: BTreeMap<_, _> = declarations
        .iter()
        .enumerate()
        .map(|(index, declaration)| (declaration.symbol.as_str(), index))
        .collect();
    let together = declarations
        .iter()
        .enumerate()
        .filter_map(|(index, declaration)| {
            declaration
                .companion
                .as_ref()
                .map(|companion| (index, companion))
        })
        .map(|(index, companion)| {
            names
                .get(companion.shared_owner.as_str())
                .copied()
                .map(|owner| (owner, index))
                .ok_or_else(|| invalid("Loading owner declaration is missing"))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let plan = assets.plan_graph(declarations.iter().map(|d| d.bound).collect(), &together)?;
    let mut symbols = BTreeMap::new();
    for (i, declaration) in declarations.iter().enumerate() {
        symbols.insert(declaration.symbol.clone(), plan.tag(i)?);
    }
    for (symbol, tag) in &external.symbols {
        if symbols.insert(symbol.clone(), *tag).is_some() {
            return Err(invalid(format!(
                "Shared asset symbol {symbol} is also allocated"
            )));
        }
    }
    let mut nodes = materializer(&symbols)?;
    validate_materialized(&nodes, &declarations)?;
    let symbol = |s: &str| -> AuthoringResult<TagHash> {
        symbols
            .get(s)
            .copied()
            .ok_or_else(|| invalid(format!("Missing symbol {s}")))
    };
    let streamed: BTreeSet<&str> = nodes
        .iter()
        .filter(|node| node.streamed)
        .map(|node| node.symbol.as_str())
        .collect();
    let mut loading_owners = Vec::new();
    let mut resources = Vec::new();
    for node in nodes.iter().filter(|node| !node.streamed) {
        let mut references = node
            .patches
            .iter()
            .filter(|(_, target)| !streamed.contains(target.as_str()))
            .map(|(_, target)| symbol(target))
            .collect::<AuthoringResult<Vec<_>>>()?;
        if let Some(target) = &node.reference {
            references.push(symbol(target)?);
        }
        resources.push(crate::LoadingResource {
            tag: symbol(&node.symbol)?,
            references,
        });
        if let Some(companion) = &node.companion {
            loading_owners.push(crate::LoadingOwner {
                owner: symbol(&companion.shared_owner)?,
                companion: symbol(&node.symbol)?,
            });
        }
    }
    resources.extend(external.resources.iter().cloned());
    let eager = eager_loading_owners(manager, &nodes, &symbols)?;
    let mut loading_groups = crate::shared_tag_dependency_index::partition::partition_with_eager(
        &resources,
        &loading_owners,
        symbol(primary)?,
        &eager,
    )?;
    let owners: BTreeSet<_> = loading_groups.keys().copied().collect();
    let required: Vec<_> = loading_groups
        .iter()
        .flat_map(|(owner, group)| group.iter().map(|tag| (*owner, *tag)))
        .collect();
    extra_groups(&symbols, &mut loading_groups)?;
    if loading_groups.keys().copied().collect::<BTreeSet<_>>() != owners {
        return Err(invalid("Loading group expansion changed its owners"));
    }
    if required
        .iter()
        .any(|(owner, tag)| !loading_groups[owner].contains(tag))
    {
        return Err(invalid("Loading group expansion removed a required asset"));
    }
    let private: BTreeSet<_> = symbols.values().copied().collect();
    for group in loading_groups.values() {
        for tag in group {
            let committed = assets
                .packages
                .iter()
                .find(|package| package.id == tag.pkg_id())
                .is_some_and(|package| {
                    package.id == tag.pkg_id()
                        && package.tags.get(tag.entry_index() as usize).is_some()
                });
            if !private.contains(tag) && !committed && manager.get_entry(*tag).is_none() {
                return Err(invalid("Loading group references an unavailable asset"));
            }
        }
    }
    let mut prepared = Vec::with_capacity(nodes.len());
    let mut references = Vec::new();
    for (i, node) in nodes.iter_mut().enumerate() {
        let mut template = node.template;
        let mut data = std::mem::take(&mut node.payload);
        for (offset, target) in &node.patches {
            if read_u32(&data, *offset)? != u32::MAX {
                return Err(invalid("Fixup is not an unresolved placeholder"));
            }
            write_u32(&mut data, *offset, symbol(target)?.0)?;
        }
        if let Some(companion) = &node.companion {
            let mut inherited = Vec::with_capacity(companion.inherited.len());
            for &parent in &companion.inherited {
                let (native, payload) = native_companion(directory, manager, parent, companions)?;
                inherited.push((
                    payload.clone(),
                    crate::LoadingOwner {
                        companion: *native,
                        owner: TagHash(parent),
                    },
                ));
            }
            let (native, payload) =
                native_companion(directory, manager, companion.source_parent, companions)?;
            template = *native;
            data = crate::clone_scoped_dependencies(
                (
                    payload,
                    crate::LoadingOwner {
                        companion: template,
                        owner: TagHash(companion.source_parent),
                    },
                ),
                crate::LoadingOwner {
                    companion: symbol(&node.symbol)?,
                    owner: symbol(&companion.shared_owner)?,
                },
                loading_groups
                    .get(&symbol(&companion.shared_owner)?)
                    .ok_or_else(|| invalid("Loading group missing"))?,
                &inherited
                    .iter()
                    .map(|(payload, owner)| (payload.as_slice(), *owner))
                    .collect::<Vec<_>>(),
            )?;
        }
        if manager.get_entry(template).is_none() {
            return Err(invalid(format!(
                "Private asset template {template} is unavailable"
            )));
        }
        if data.is_empty() || data.len() > declarations[i].bound {
            return Err(invalid("Linked asset exceeded its declared payload bound"));
        }
        if let Some(debug) = debug {
            if Path::new(&node.symbol).components().count() != 1
                || !matches!(
                    Path::new(&node.symbol).components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                return Err(invalid("Private asset debug symbol is not a file name"));
            }
            fs::create_dir_all(debug).map_err(|e| invalid(e.to_string()))?;
            fs::write(debug.join(format!("{}.bin", node.symbol)), &data)
                .map_err(|e| invalid(e.to_string()))?;
        }
        prepared.push(NewTagSpec {
            template_tag: template,
            payload: data,
            storage: node.storage,
        });
        if let Some(target) = &node.reference {
            references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: i,
                reference: crate::NewTagReference::Tag(symbol(target)?),
            });
        }
    }
    assets.commit_graph(plan, prepared, references)?;
    Ok(Linked { symbols })
}

fn eager_loading_owners(
    manager: &sundial::package_authoring::PackageManager,
    nodes: &[Node],
    symbols: &BTreeMap<String, TagHash>,
) -> AuthoringResult<BTreeSet<TagHash>> {
    let mut eager = BTreeSet::new();
    for companion in nodes.iter().filter_map(|node| node.companion.as_ref()) {
        let owner = nodes
            .iter()
            .find(|node| node.symbol == companion.shared_owner)
            .ok_or_else(|| invalid("Loading owner node is missing"))?;
        let entry = manager
            .get_entry(owner.template)
            .ok_or_else(|| invalid("Loading owner template is missing"))?;
        if !crate::shared_tag_dependency_index::scoped::deferred_owner(
            entry.file_type,
            entry.reference,
        )? {
            eager.insert(symbols[&owner.symbol]);
        }
    }
    Ok(eager)
}
