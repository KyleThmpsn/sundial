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

#[derive(Default)]
pub(crate) struct AssetPackages {
    pub packages: Vec<AssetPackage>,
    /// Allocation decisions, retained so a cached fragment can replay its resource groups.
    pub reservations: Vec<Reservation>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Reservation {
    pub package_index: usize,
    pub base: usize,
    pub bounds: Vec<usize>,
}

/// A pure package choice. No empty package or tag slot is inserted until commit.
pub(crate) struct GroupPlan {
    pub package_index: usize,
    pub package_id: u16,
    pub base: usize,
    bounds: Vec<usize>,
}

impl GroupPlan {
    pub(crate) fn reservation(&self) -> Reservation {
        Reservation {
            package_index: self.package_index,
            base: self.base,
            bounds: self.bounds.clone(),
        }
    }
}

/// Allocation-only candidate state. It borrows no payloads and copies no existing assets.
pub(crate) struct Layout {
    count: usize,
    last: Option<(u16, Budget)>,
}

impl Layout {
    pub(crate) fn append(
        &mut self,
        bounds: &[usize],
        lengths: impl IntoIterator<Item = usize>,
    ) -> AuthoringResult<GroupPlan> {
        let added = Budget::for_lengths(lengths)?;
        let reserved = Budget::for_lengths(bounds.iter().copied())?;
        // reserve_group callers may emit clips and audio in a different order than their
        // upper bounds, or omit shared clips. The native entry and block budgets matter.
        if added.entries > reserved.entries || added.blocks > reserved.blocks {
            return Err(validation(
                "Cached assets exceeded their recorded group bounds",
            ));
        }
        let plan = choose(self.count, self.last, bounds.to_vec())?;
        let mut budget = if plan.package_index == self.count {
            self.count += 1;
            Budget {
                entries: 0,
                blocks: 0,
            }
        } else {
            self.last.expect("existing last package").1
        };
        budget.entries += added.entries;
        budget.blocks += added.blocks;
        self.last = Some((plan.package_id, budget));
        Ok(plan)
    }
}

fn choose(
    count: usize,
    last: Option<(u16, Budget)>,
    bounds: Vec<usize>,
) -> AuthoringResult<GroupPlan> {
    let added = Budget::for_lengths(bounds.iter().copied())?;
    if added.entries == 0 || !added.within_capacity() {
        return Err(invalid(
            "A resource group exceeds one native asset package's entry or block capacity",
        ));
    }
    if let Some((id, current)) = last
        && current.fits(added)
    {
        return Ok(GroupPlan {
            package_index: count - 1,
            package_id: id,
            base: current.entries,
            bounds,
        });
    }
    let id = u16::try_from(count)
        .ok()
        .and_then(|n| PARHELION_ASSET_PACKAGE_ID.checked_add(n))
        .filter(|id| *id <= MAX_AUTHORED_STANDALONE_PACKAGE_ID)
        .ok_or_else(|| invalid("The authored asset package id range is exhausted"))?;
    Ok(GroupPlan {
        package_index: count,
        package_id: id,
        base: 0,
        bounds,
    })
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

    fn within_capacity(self) -> bool {
        self.entries <= MAX_PACKAGE_ENTRY_COUNT && self.blocks <= MAX_BLOCK_COUNT
    }
}

impl AssetPackages {
    pub(crate) fn primary(
        tags: Vec<NewTagSpec>,
        references: Vec<NewTagReferenceOverride>,
    ) -> AuthoringResult<Self> {
        let mut assets = Self {
            packages: Vec::new(),
            reservations: Vec::new(),
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
        let plan = self.plan_group(lengths)?;
        if plan.package_index == self.packages.len() {
            self.packages.push(AssetPackage {
                id: plan.package_id,
                tags: Vec::new(),
                references: Vec::new(),
            });
        }
        self.reservations.push(plan.reservation());
        Ok(plan.package_index)
    }

    pub(crate) fn plan_group(
        &self,
        lengths: impl IntoIterator<Item = usize>,
    ) -> AuthoringResult<GroupPlan> {
        let mut bounds = Vec::new();
        for length in lengths {
            if bounds.len() == MAX_PACKAGE_ENTRY_COUNT {
                return Err(invalid(
                    "A resource group exceeds one native asset package's entry capacity",
                ));
            }
            bounds.push(length);
        }
        let layout = self.layout()?;
        choose(layout.count, layout.last, bounds)
    }

    pub(crate) fn layout(&self) -> AuthoringResult<Layout> {
        Ok(Layout {
            count: self.packages.len(),
            last: self
                .packages
                .last()
                .map(|package| {
                    Budget::for_lengths(package.tags.iter().map(|tag| tag.payload.len()))
                        .map(|budget| (package.id, budget))
                })
                .transpose()?,
        })
    }

    /// Validate the decision and the complete group before one append operation.
    pub(crate) fn commit_group(
        &mut self,
        plan: GroupPlan,
        tags: Vec<NewTagSpec>,
        references: Vec<NewTagReferenceOverride>,
    ) -> AuthoringResult<usize> {
        let current = self.plan_group(plan.bounds.iter().copied())?;
        if (current.package_index, current.package_id, current.base)
            != (plan.package_index, plan.package_id, plan.base)
        {
            return Err(validation(
                "The private asset package plan changed before commit",
            ));
        }
        if tags.is_empty()
            || tags.len() > plan.bounds.len()
            || tags
                .iter()
                .zip(&plan.bounds)
                .any(|(tag, bound)| tag.payload.is_empty() || tag.payload.len() > *bound)
        {
            return Err(validation(
                "Materialized assets exceeded their declared bounds",
            ));
        }
        let final_count = plan
            .base
            .checked_add(tags.len())
            .ok_or_else(|| invalid("Asset count overflow"))?;
        let mut seen = std::collections::BTreeSet::new();
        for reference in &references {
            if !(plan.base..final_count).contains(&reference.new_tag_ordinal)
                || !seen.insert(reference.new_tag_ordinal)
                || matches!(reference.reference, crate::NewTagReference::Appended(index) if index >= final_count)
            {
                return Err(validation(
                    "Materialized asset reference is outside its package group",
                ));
            }
        }
        self.reservations.push(plan.reservation());
        if plan.package_index == self.packages.len() {
            self.packages.push(AssetPackage {
                id: plan.package_id,
                tags,
                references,
            });
        } else {
            let package = &mut self.packages[plan.package_index];
            package.tags.extend(tags);
            package.references.extend(references);
        }
        Ok(plan.package_index)
    }

    pub(crate) fn validate(&self) -> AuthoringResult<()> {
        for (index, package) in self.packages.iter().enumerate() {
            let budget = Budget::for_lengths(package.tags.iter().map(|tag| tag.payload.len()))?;
            if package.id as usize != PARHELION_ASSET_PACKAGE_ID as usize + index
                || budget.entries == 0
                || !budget.within_capacity()
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
        let mut assets = AssetPackages::default();
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
