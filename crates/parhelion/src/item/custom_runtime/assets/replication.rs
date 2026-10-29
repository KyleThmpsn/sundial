//! Validate private replication references before assigning package identities.
use super::*;
mod allocation;
use allocation::Allocation;

pub(in crate::item) fn target<'a, M>(
    nodes: &'a [Node<M>],
    node: &Node<M>,
    offset: usize,
) -> AuthoringResult<&'a Node<M>> {
    let mut patches = node.patches.iter().filter(|(at, _)| *at == offset);
    let Some((_, symbol)) = patches.next() else {
        return Err(invalid(format!(
            "Private asset {} has no symbolic reference at +0x{offset:X}",
            node.symbol
        )));
    };
    if patches.next().is_some() || read_u32(&node.payload, offset)? != u32::MAX {
        return Err(invalid("Private asset symbolic reference is invalid"));
    }
    nodes
        .iter()
        .find(|node| node.symbol == *symbol)
        .ok_or_else(|| invalid("Private asset symbolic dependency is missing"))
}

pub(in crate::item) fn validate_templates<M>(
    nodes: &[Node<M>],
    root: &Node<M>,
    manager: &PackageManager,
) -> AuthoringResult<()> {
    if manager
        .get_entry(TagHash(root.template))
        .is_none_or(|entry| entry.reference != 0x80809C0F)
    {
        return Err(invalid("Replication root template is not a native entity"));
    }
    let replication = target(nodes, root, 0x88)?;
    let class = |node: &Node<M>, expected| {
        node.reference.is_none()
            && node.storage == crate::NewTagStorageMode::InheritTemplate
            && manager
                .get_entry(TagHash(node.template))
                .is_some_and(|entry| entry.reference == expected)
    };
    if !class(replication, 0x80809BB6)
        || manager
            .get_entry(TagHash(replication.template))
            .is_none_or(|entry| entry.file_type != 16 || entry.file_subtype != 0)
    {
        return Err(invalid(
            "Private entity replication template has the wrong class",
        ));
    }
    let (rows, _, start, _) = array_at(&replication.payload, 0x30)?;
    let (_, _, owners, _) = array_at(&root.payload, 16)?;
    for index in 0..rows {
        let owner = target(nodes, root, owners + index * 12)?;
        Allocation::read(nodes, replication, start + index * 32 + 4)?
            .validate(nodes, owner, manager)?;
    }
    Ok(())
}

pub(in crate::item) struct Loading {
    pub(super) owner: String,
    source: crate::LoadingOwner,
    payload: Vec<u8>,
    pub bound: usize,
}

/// Retained native resources must follow the authored payload, not just its
/// template's graph. Read before linking so private layout slots remain symbolic.
pub(in crate::item) fn requirements<M>(
    nodes: &[Node<M>],
    manager: &PackageManager,
) -> AuthoringResult<Vec<TagHash>> {
    let mut required = BTreeSet::new();
    for node in nodes {
        if manager
            .get_entry(TagHash(node.template))
            .is_none_or(|entry| entry.file_type != 8 || entry.reference != 0x80809C36)
        {
            continue;
        }
        required.extend(
            sundial::package_authoring::sandbox_perk::entity::residency::owner_requirements(
                manager,
                node.template,
                &node.payload,
            )
            .map_err(|error| invalid(format!("Private component {}: {error}", node.symbol)))?
            .into_iter()
            .map(|requirement| TagHash(requirement.tag)),
        );
    }
    Ok(required.into_iter().collect())
}

impl Loading {
    pub fn all<M>(
        nodes: &[Node<M>],
        manager: &PackageManager,
        required: &[&str],
        resources: &[TagHash],
    ) -> AuthoringResult<Vec<Self>> {
        let mut result = Vec::new();
        for symbol in required {
            let root = nodes
                .iter()
                .find(|node| node.symbol == *symbol)
                .ok_or_else(|| invalid("Private entity root is missing"))?;
            let loading = Self::read(nodes, root, manager, resources)?;
            if !result
                .iter()
                .any(|previous: &Self| previous.owner == loading.owner)
            {
                result.push(loading);
            }
        }
        // A complete runtime can contain another entity, such as a vehicle's
        // projectile. Every replication owner needs its own enrolled companion.
        for node in nodes {
            if manager
                .get_entry(TagHash(node.template))
                .is_none_or(|entry| entry.file_type != 16 || entry.reference != 0x80809BB6)
                || result.iter().any(|loading| loading.owner == node.symbol)
            {
                continue;
            }
            let root = target(nodes, node, 8)?;
            if target(nodes, root, 0x88)?.symbol != node.symbol {
                return Err(invalid("Replication owner and entity root disagree"));
            }
            result.push(Self::read(nodes, root, manager, resources)?);
        }
        Ok(result)
    }

    pub fn read<M>(
        nodes: &[Node<M>],
        root: &Node<M>,
        manager: &PackageManager,
        resources: &[TagHash],
    ) -> AuthoringResult<Self> {
        validate(nodes, root)?;
        validate_templates(nodes, root, manager)?;
        let replication = target(nodes, root, 0x88)?;
        let mut cache = crate::shared_tag_dependency_index::native::Companions::new();
        let (companion, payload) = crate::shared_tag_dependency_index::native::native_companion(
            &manager.package_dir,
            manager,
            replication.template,
            &mut cache,
        )?;
        if manager
            .get_entry(*companion)
            .is_none_or(|entry| entry.file_type != 8 || entry.reference != 0x80809EF9)
        {
            return Err(invalid("Replication loading companion has the wrong class"));
        }
        let payload = crate::shared_tag_dependency_index::enroll_dependencies(
            payload,
            *companion,
            TagHash(replication.template),
            resources,
        )?;
        Ok(Self {
            owner: replication.symbol.clone(),
            source: crate::LoadingOwner {
                owner: TagHash(replication.template),
                companion: *companion,
            },
            // Include sparse package descriptors as well as indices for the private graph.
            bound: payload
                .len()
                .checked_add(crate::format::BLOCK_SIZE)
                .and_then(|size| {
                    nodes
                        .len()
                        .checked_mul(64)
                        .and_then(|extra| size.checked_add(extra))
                })
                .ok_or_else(|| invalid("Replication loading size overflow"))?,
            payload,
        })
    }

    pub fn materialize<M>(
        &self,
        nodes: &[Node<M>],
        symbols: &BTreeMap<String, TagHash>,
        companion: TagHash,
    ) -> AuthoringResult<NewTagSpec> {
        let identity = crate::LoadingOwner {
            owner: symbols[&self.owner],
            companion,
        };
        let replication = nodes
            .iter()
            .find(|node| node.symbol == self.owner)
            .ok_or_else(|| invalid("Replication loading owner is missing"))?;
        let (rows, _, start, _) = array_at(&replication.payload, 0x30)?;
        let mut resources = vec![identity.owner, identity.companion];
        for index in 0..rows {
            resources.push(
                Allocation::read(nodes, replication, start + index * 32 + 4)?.resolve(symbols)?,
            );
        }
        let payload = crate::clone_scoped_dependencies(
            (&self.payload, self.source),
            identity,
            &resources,
            &[],
        )?;
        if payload.len() > self.bound {
            return Err(invalid("Replication companion exceeded its reserved size"));
        }
        Ok(NewTagSpec {
            template_tag: self.source.companion,
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        })
    }
}

pub(in crate::item) fn validate<M>(nodes: &[Node<M>], root: &Node<M>) -> AuthoringResult<()> {
    let check = || -> AuthoringResult<()> {
        let replication = target(nodes, root, 0x88)?;
        let back = target(nodes, replication, 8)?;
        if back.symbol != root.symbol || replication.reference.is_some() {
            return Err(invalid("Replication does not own this entity"));
        }
        let entity = &root.payload;
        let payload = &replication.payload;
        let (owners, _, owner_start, owner_class) = array_at(entity, 16)?;
        let (rows, _, row_start, row_class) = array_at(payload, 0x30)?;
        if owners == 0
            || owners > 64
            || owners != rows
            || owner_class != 0x80809C04
            || row_class != 0x80809BB8
            || owner_start
                .checked_add(owners * 12)
                .is_none_or(|end| end > entity.len())
            || row_start
                .checked_add(rows * 32)
                .is_none_or(|end| end > payload.len())
        {
            return Err(invalid("Replication component rows differ from the entity"));
        }
        if read_u64(payload, 0)? != payload.len() as u64
            || payload.get(12..48) != Some(&[0u8; 36][..])
        {
            return Err(invalid(
                "Replication header contains serialized runtime state",
            ));
        }
        for index in 0..owners {
            let row = row_start + index * 32;
            let owner = target(nodes, root, owner_start + index * 12)?;
            let owner_allocation = Allocation::read(nodes, owner, 0x44)?;
            let allocation = Allocation::read(nodes, replication, row + 4)?;
            let instance = crate::tag_payload::bounded_relative_target(
                &owner.payload,
                16,
                "Replication instance",
            )?;
            if instance < 4
                || read_u32(payload, row)? != read_u32(&owner.payload, instance - 4)?
                || allocation != owner_allocation
            {
                return Err(invalid(
                    "Replication instance schema or allocation differs from its component",
                ));
            }
            if read_u64(payload, row + 8)? != 0 || read_u64(payload, row + 16)? != 0 {
                return Err(invalid(
                    "Replication component contains a serialized runtime field array",
                ));
            }
        }
        Ok(())
    };
    check().map_err(|error| invalid(format!("Private entity replication is invalid: {error}")))
}
