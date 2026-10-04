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
    #[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
    pub package_index: usize,
}

/// Native companion cache: parent relation tag to (companion tag, payload).
pub(super) type Companions = BTreeMap<u32, (TagHash, Vec<u8>)>;

pub(super) fn native_companion<'a>(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    source_parent: u32,
    companions: &'a mut Companions,
) -> AuthoringResult<&'a (TagHash, Vec<u8>)> {
    if !companions.contains_key(&source_parent) {
        let chain = crate::chain::discover_patch_chain(directory, TagHash(source_parent).pkg_id())?;
        let bytes = fs::read(&chain.latest().path).map_err(|e| invalid(e.to_string()))?;
        let layout = crate::format::PackageLayout::parse(&bytes)?;
        // Enrollment belongs to the package. Cache every row once instead of
        // reopening the same native package for each model and dye owner.
        for row in layout.shared_tag_enrollment_rows(&bytes)?.chunks_exact(8) {
            companions.entry(read_u32(row, 0)?).or_insert_with(|| {
                (
                    TagHash(u32::from_le_bytes(row[4..8].try_into().unwrap())),
                    Vec::new(),
                )
            });
        }
    }
    let (companion, payload) = companions
        .get_mut(&source_parent)
        .ok_or_else(|| invalid("Native parent has no shared-tag enrollment"))?;
    if payload.is_empty() {
        *payload = manager
            .read_tag(*companion)
            .map_err(|e| invalid(e.to_string()))?;
    }
    companions
        .get(&source_parent)
        .ok_or_else(|| invalid("Native companion cache missing"))
}

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
    extra_bounds: usize,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    let mut declarations = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let size = match &node.companion {
            // One native package contains at most 8192 tags. Its added bitmap,
            // index list and descriptors fit within this conservative extra block.
            // A merged index is no larger than the indexes it merges.
            Some(companion) => {
                let mut size = crate::format::BLOCK_SIZE;
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
        declarations,
        primary,
        companions,
        extra_bounds,
        |_| Ok(nodes),
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

/// Checks that the declarations fit one package with `extra_bounds` more entries, that their
/// symbols are nonempty and unique and include `primary`, and reads each native companion
/// parent they need.
fn check_declarations(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    (declarations, primary, extra_bounds): (&[Declaration], &str, usize),
    companions: &mut Companions,
) -> AuthoringResult<()> {
    if declarations
        .len()
        .checked_add(extra_bounds)
        .is_none_or(|count| count > crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT)
    {
        return Err(invalid(
            "Private asset declarations exceed one package's entry capacity",
        ));
    }
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
    declarations: Vec<Declaration>,
    primary: &str,
    companions: &mut Companions,
    extra_bounds: usize,
    materializer: impl FnOnce(&BTreeMap<String, TagHash>) -> AuthoringResult<Vec<Node>>,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    check_declarations(
        directory,
        manager,
        (&declarations, primary, extra_bounds),
        companions,
    )?;
    let mut bounds: Vec<_> = declarations.iter().map(|d| d.bound).collect();
    bounds.extend(std::iter::repeat_n(crate::format::BLOCK_SIZE, extra_bounds));
    let plan = assets.plan_group(bounds)?;
    let allocator = AppendedTagAllocator::new(plan.package_id, 0);
    let base = plan.base;
    let mut symbols = BTreeMap::new();
    for (i, declaration) in declarations.iter().enumerate() {
        symbols.insert(
            declaration.symbol.clone(),
            allocator.assigned_tag(base + i, "Private asset", "asset")?,
        );
    }
    let mut nodes = materializer(&symbols)?;
    validate_materialized(&nodes, &declarations)?;
    let symbol = |s: &str| -> AuthoringResult<TagHash> {
        symbols
            .get(s)
            .copied()
            .ok_or_else(|| invalid(format!("Missing symbol {s}")))
    };
    let mut loading_owners = Vec::new();
    let mut resources = Vec::new();
    for node in &nodes {
        let mut references = node
            .patches
            .iter()
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
    let mut loading_groups =
        crate::partition_loading_resources(&resources, &loading_owners, symbol(primary)?)?;
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
            let committed = tag
                .pkg_id()
                .checked_sub(crate::package_profile::PARHELION_ASSET_PACKAGE_ID)
                .and_then(|index| assets.packages.get(index as usize))
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
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        if let Some(target) = &node.reference {
            let ordinal = symbol(target)?.entry_index() as usize;
            references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: base + i,
                reference: crate::NewTagReference::Appended(ordinal),
            });
        }
    }
    let package_index = assets.commit_group(plan, prepared, references)?;
    Ok(Linked {
        symbols,
        package_index,
    })
}
