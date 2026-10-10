//! Validate imported replication references before assigning package identities.
use super::*;

fn target<'a>(nodes: &'a [Node], node: &Node, offset: usize) -> AuthoringResult<&'a Node> {
    let mut patches = node.patches.iter().filter(|(at, _)| *at == offset);
    let Some((_, symbol)) = patches.next() else {
        return Err(invalid("Imported projectile replication is incomplete"));
    };
    if patches.next().is_some() || read_u32(&node.payload, offset)? != u32::MAX {
        return Err(invalid(
            "Imported projectile replication reference is invalid",
        ));
    }
    nodes
        .iter()
        .find(|node| node.symbol == *symbol)
        .ok_or_else(|| invalid("Imported projectile replication dependency is missing"))
}

pub(super) fn validate_templates(
    nodes: &[Node],
    root: &Node,
    manager: &PackageManager,
) -> AuthoringResult<()> {
    let replication = target(nodes, root, 0x88)?;
    let class = |node: &Node, expected| {
        node.reference.is_none()
            && node.storage == crate::NewTagStorageMode::InheritTemplate
            && manager
                .get_entry(TagHash(node.template))
                .is_some_and(|entry| entry.reference == expected)
    };
    if !class(replication, 0x80809BB6)
        || manager
            .get_entry(TagHash(replication.template))
            .is_none_or(|entry| entry.file_type != 16)
    {
        return Err(invalid(
            "Imported projectile replication template has the wrong class",
        ));
    }
    let (rows, _, start, _) = array_at(&replication.payload, 0x30)?;
    for index in 0..rows {
        let allocation = target(nodes, replication, start + index * 32 + 4)?;
        if !class(allocation, 0x80809BBB) {
            return Err(invalid(
                "Imported projectile replication allocation has the wrong class",
            ));
        }
    }
    Ok(())
}

pub(super) struct Loading {
    owner: String,
    source: crate::LoadingOwner,
    payload: Vec<u8>,
    pub bound: usize,
}

impl Loading {
    pub fn read(nodes: &[Node], root: &Node, manager: &PackageManager) -> AuthoringResult<Self> {
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
        Ok(Self {
            owner: replication.symbol.clone(),
            source: crate::LoadingOwner {
                owner: TagHash(replication.template),
                companion: *companion,
            },
            // The private group stays within one package. One extra block covers
            // all of its entry indices, descriptors and alignment.
            bound: payload
                .len()
                .checked_add(crate::format::BLOCK_SIZE)
                .ok_or_else(|| invalid("Replication loading size overflow"))?,
            payload: payload.clone(),
        })
    }

    pub fn append(
        &self,
        nodes: &[Node],
        symbols: &BTreeMap<String, TagHash>,
        package: &mut crate::asset_packages::AssetPackage,
    ) -> AuthoringResult<()> {
        let identity = crate::LoadingOwner {
            owner: symbols[&self.owner],
            companion: AppendedTagAllocator::new(package.id, 0).assigned_tag(
                package.tags.len(),
                "Replication loading companion",
                "companion",
            )?,
        };
        let replication = nodes
            .iter()
            .find(|node| node.symbol == self.owner)
            .ok_or_else(|| invalid("Replication loading owner is missing"))?;
        let (rows, _, start, _) = array_at(&replication.payload, 0x30)?;
        let mut resources = vec![identity.owner, identity.companion];
        for index in 0..rows {
            let allocation = target(nodes, replication, start + index * 32 + 4)?;
            resources.push(symbols[&allocation.symbol]);
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
        package.tags.push(NewTagSpec {
            template_tag: self.source.companion,
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        Ok(())
    }
}

pub(super) fn validate(nodes: &[Node], root: &Node) -> AuthoringResult<()> {
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
            let owner_allocation = target(nodes, owner, 0x44)?;
            let allocation = target(nodes, replication, row + 4)?;
            let instance = crate::tag_payload::bounded_relative_target(
                &owner.payload,
                16,
                "Replication instance",
            )?;
            if instance < 4
                || read_u32(payload, row)? != read_u32(&owner.payload, instance - 4)?
                || allocation.symbol != owner_allocation.symbol
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
    check().map_err(|error| {
        invalid(format!(
            "Imported projectile replication is invalid: {error}. Import the weapon again"
        ))
    })
}
