//! Allocation identities can stay native only while the component retains its template layout.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Allocation<'a> {
    Private(&'a str),
    Native(TagHash),
}

impl<'a> Allocation<'a> {
    pub fn read<M>(nodes: &'a [Node<M>], node: &Node<M>, offset: usize) -> AuthoringResult<Self> {
        if node.patches.iter().any(|(at, _)| *at == offset) {
            return Ok(Self::Private(&target(nodes, node, offset)?.symbol));
        }
        let tag = read_u32(&node.payload, offset)?;
        if matches!(tag, 0 | u32::MAX) {
            return Err(invalid(format!(
                "Private asset {} has no allocation at +0x{offset:X}",
                node.symbol
            )));
        }
        Ok(Self::Native(TagHash(tag)))
    }

    pub fn validate<M>(
        self,
        nodes: &[Node<M>],
        owner: &Node<M>,
        manager: &PackageManager,
    ) -> AuthoringResult<()> {
        let template = match self {
            Self::Private(symbol) => {
                let allocation = nodes
                    .iter()
                    .find(|node| node.symbol == symbol)
                    .ok_or_else(|| invalid("Private allocation dependency is missing"))?;
                if allocation.reference.is_some()
                    || allocation.storage != crate::NewTagStorageMode::InheritTemplate
                {
                    return Err(invalid(
                        "Private allocation overrides its native storage class",
                    ));
                }
                TagHash(allocation.template)
            }
            Self::Native(tag) => tag,
        };
        if manager
            .get_entry(template)
            .is_none_or(|entry| entry.reference != 0x80809BBB || entry.file_type != 8)
        {
            return Err(invalid(format!(
                "Replication allocation {template} is not a native allocation resource"
            )));
        }
        if let Self::Native(tag) = self {
            if owner.reference.is_some()
                || owner.storage != crate::NewTagStorageMode::InheritTemplate
                || manager
                    .get_entry(TagHash(owner.template))
                    .is_none_or(|entry| entry.reference != 0x80809C36 || entry.file_type != 8)
            {
                return Err(invalid(
                    "Retained allocation requires a native component template",
                ));
            }
            let original = read_tag(
                manager,
                TagHash(owner.template),
                "replication component template",
            )?;
            let pointer = |payload: &[u8], at| {
                crate::tag_payload::bounded_relative_target(payload, at, "Replication component")
            };
            let live = pointer(&owner.payload, 16)?;
            let original_live = pointer(&original, 16)?;
            if live < 4
                || original_live < 4
                || live != original_live
                || pointer(&owner.payload, 24)? != pointer(&original, 24)?
                || owner.payload.len() != original.len()
                || read_u32(&original, 0x44)? != tag.0
                || read_u32(&owner.payload, live - 4)? != read_u32(&original, original_live - 4)?
                || read_u64(&owner.payload, live + 8)? != read_u64(&original, original_live + 8)?
            {
                return Err(invalid(format!(
                    "Component {} cannot retain an allocation from a different native layout",
                    owner.symbol
                )));
            }
            // A class-qualified entry must also be readable. The existing owner-requirements
            // walk enrolls this retained allocation with the authored graph's eager resources.
            read_tag(manager, tag, "retained replication allocation")?;
        }
        Ok(())
    }

    pub fn resolve(self, symbols: &BTreeMap<String, TagHash>) -> AuthoringResult<TagHash> {
        match self {
            Self::Native(tag) => Ok(tag),
            Self::Private(symbol) => symbols
                .get(symbol)
                .copied()
                .ok_or_else(|| invalid("Private allocation has no assigned identity")),
        }
    }
}
