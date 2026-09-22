//! Allocate a private resource group in an asset package and resolve its cross-references.
//! Native loading owners get a cloned companion index so the client loads the group with
//! the owner instead of through the global runtime root.
use super::*;

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

pub(super) struct Companion {
    pub shared_owner: String,
    pub source_parent: u32,
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
    mut nodes: Vec<Node>,
    primary: &str,
    companions: &mut Companions,
    extra_bounds: usize,
    extra_groups: impl FnOnce(
        &BTreeMap<String, TagHash>,
        &mut BTreeMap<TagHash, Vec<TagHash>>,
    ) -> AuthoringResult<()>,
    debug: Option<&Path>,
) -> AuthoringResult<Linked> {
    let mut bounds = Vec::with_capacity(nodes.len() + 8);
    for node in &nodes {
        let size = match &node.companion {
            // One native package contains at most 8192 tags. Its added bitmap,
            // index list and descriptors fit within this conservative extra block.
            Some(companion) => {
                native_companion(directory, manager, companion.source_parent, companions)?
                    .1
                    .len()
                    .checked_add(crate::format::BLOCK_SIZE)
                    .ok_or_else(|| invalid("Companion size overflow"))?
            }
            None => node.payload.len(),
        };
        bounds.push(size);
    }
    bounds.extend(std::iter::repeat_n(crate::format::BLOCK_SIZE, extra_bounds));
    let package_index = emission.asset_packages.reserve_group(bounds)?;
    let package_id = emission.asset_packages.packages[package_index].id;
    let base = emission.asset_packages.packages[package_index].tags.len();
    let allocator = AppendedTagAllocator::new(package_id, 0);
    let mut symbols = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        if symbols
            .insert(
                node.symbol.clone(),
                allocator.assigned_tag(base + i, "Private asset", "asset")?,
            )
            .is_some()
        {
            return Err(invalid(format!("Duplicate asset symbol {}", node.symbol)));
        }
    }
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
    extra_groups(&symbols, &mut loading_groups)?;
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
                &[],
            )?;
        }
        if let Some(debug) = debug {
            fs::create_dir_all(debug).map_err(|e| invalid(e.to_string()))?;
            fs::write(debug.join(format!("{}.bin", node.symbol)), &data)
                .map_err(|e| invalid(e.to_string()))?;
        }
        emission.asset_packages.packages[package_index]
            .tags
            .push(NewTagSpec {
                template_tag: template,
                payload: data,
                storage: crate::NewTagStorageMode::InheritTemplate,
            });
        if let Some(target) = &node.reference {
            let ordinal = symbol(target)?.entry_index() as usize;
            emission.asset_packages.packages[package_index]
                .references
                .push(crate::NewTagReferenceOverride {
                    new_tag_ordinal: base + i,
                    reference: crate::NewTagReference::Appended(ordinal),
                });
        }
    }
    Ok(Linked {
        symbols,
        package_index,
    })
}
