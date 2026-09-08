//! Allocate complete resource groups into owned packages before assigning their tags.
use crate::{
    AuthoringResult, NewTagReferenceOverride, NewTagSpec,
    appended_tags::MAX_PACKAGE_ENTRY_COUNT,
    error::{invalid, validation},
    format::{BLOCK_SIZE, MAX_BLOCK_COUNT},
    package_profile::{MAX_AUTHORED_STANDALONE_PACKAGE_ID, PARHELION_ASSET_PACKAGE_ID},
};

pub(crate) struct AssetPackage {
    pub id: u16,
    pub tags: Vec<NewTagSpec>,
    pub references: Vec<NewTagReferenceOverride>,
}

pub(crate) struct AssetPackages {
    pub packages: Vec<AssetPackage>,
}

#[derive(Clone, Copy)]
struct Budget {
    entries: usize,
    blocks: usize,
}

impl Budget {
    fn for_lengths(lengths: impl IntoIterator<Item = usize>) -> AuthoringResult<Self> {
        lengths.into_iter().try_fold(
            Self {
                entries: 0,
                blocks: 0,
            },
            |budget, size| {
                if size == 0 {
                    return Err(invalid("An authored asset cannot be empty"));
                }
                Ok(Self {
                    entries: budget
                        .entries
                        .checked_add(1)
                        .ok_or_else(|| invalid("Asset count overflow"))?,
                    // This conservative bound remains valid with either shared or individual blocks.
                    blocks: budget
                        .blocks
                        .checked_add(size.div_ceil(BLOCK_SIZE))
                        .ok_or_else(|| invalid("Asset block count overflow"))?,
                })
            },
        )
    }

    fn fits(self, added: Self) -> bool {
        self.entries
            .checked_add(added.entries)
            .is_some_and(|n| n <= MAX_PACKAGE_ENTRY_COUNT)
            && self
                .blocks
                .checked_add(added.blocks)
                .is_some_and(|n| n <= MAX_BLOCK_COUNT)
    }
}

impl AssetPackages {
    pub(crate) fn primary(
        tags: Vec<NewTagSpec>,
        references: Vec<NewTagReferenceOverride>,
    ) -> AuthoringResult<Self> {
        let mut assets = Self {
            packages: Vec::new(),
        };
        let index = assets.reserve_group(tags.iter().map(|tag| tag.payload.len()))?;
        assets.packages[index].tags = tags;
        assets.packages[index].references = references;
        Ok(assets)
    }

    /// The caller supplies upper bounds for payloads that grow while linking the group.
    /// It then appends that group to the returned package before reserving another.
    pub(crate) fn reserve_group(
        &mut self,
        lengths: impl IntoIterator<Item = usize>,
    ) -> AuthoringResult<usize> {
        let added = Budget::for_lengths(lengths)?;
        if added.entries == 0
            || !(Budget {
                entries: 0,
                blocks: 0,
            })
            .fits(added)
        {
            return Err(invalid(
                "A resource group exceeds one native asset package's entry or block capacity",
            ));
        }
        if let Some(last) = self.packages.last() {
            let current = Budget::for_lengths(last.tags.iter().map(|tag| tag.payload.len()))?;
            if current.fits(added) {
                return Ok(self.packages.len() - 1);
            }
        }
        let index = self.packages.len();
        let id = u16::try_from(index)
            .ok()
            .and_then(|n| PARHELION_ASSET_PACKAGE_ID.checked_add(n))
            .filter(|id| *id <= MAX_AUTHORED_STANDALONE_PACKAGE_ID)
            .ok_or_else(|| invalid("The authored asset package id range is exhausted"))?;
        self.packages.push(AssetPackage {
            id,
            tags: Vec::new(),
            references: Vec::new(),
        });
        Ok(index)
    }

    pub(crate) fn validate(&self) -> AuthoringResult<()> {
        for (index, package) in self.packages.iter().enumerate() {
            let budget = Budget::for_lengths(package.tags.iter().map(|tag| tag.payload.len()))?;
            if package.id as usize != PARHELION_ASSET_PACKAGE_ID as usize + index
                || budget.entries == 0
                || !(Budget {
                    entries: 0,
                    blocks: 0,
                })
                .fits(budget)
            {
                return Err(validation(
                    "Linked assets exceeded their reserved package capacity",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewTagStorageMode;
    use tiger_pkg::TagHash;

    fn tag(size: usize) -> NewTagSpec {
        NewTagSpec {
            template_tag: TagHash(0x80800001),
            payload: vec![0; size],
            storage: NewTagStorageMode::InheritTemplate,
        }
    }

    #[test]
    fn entry_capacity_starts_the_next_owned_package_without_reusing_tags() {
        let mut assets = AssetPackages::primary(
            (0..MAX_PACKAGE_ENTRY_COUNT).map(|_| tag(1)).collect(),
            vec![],
        )
        .unwrap();
        let next = assets.reserve_group([1, 16]).unwrap();
        assert_eq!(next, 1);
        assert_eq!(assets.packages[next].id, PARHELION_ASSET_PACKAGE_ID + 1);
        assert!(assets.packages[next].tags.is_empty());
        assets.packages[next].tags.extend([tag(1), tag(16)]);
        assets.validate().unwrap();
        assert_eq!(assets.reserve_group([1]).unwrap(), 1);
    }

    #[test]
    fn block_capacity_and_oversized_groups_are_checked_without_allocating_payloads() {
        let full = Budget::for_lengths([BLOCK_SIZE * MAX_BLOCK_COUNT]).unwrap();
        let one = Budget::for_lengths([1]).unwrap();
        assert!(!full.fits(one));
        let mut assets = AssetPackages { packages: vec![] };
        assert!(
            assets
                .reserve_group([BLOCK_SIZE * MAX_BLOCK_COUNT + 1])
                .is_err()
        );
        assert!(
            assets
                .reserve_group(std::iter::repeat_n(1, MAX_PACKAGE_ENTRY_COUNT + 1))
                .is_err()
        );
        assert!(assets.packages.is_empty());
    }
}
