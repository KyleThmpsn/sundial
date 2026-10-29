//! Place symbolic graphs across packages while keeping native enrollment pairs together.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tiger_pkg::TagHash;

struct Part {
    package: usize,
    id: u16,
    base: usize,
    entries: Vec<usize>,
}

pub(crate) struct GraphPlan {
    parts: Vec<Part>,
    tags: Vec<TagHash>,
    bounds: Vec<usize>,
    before: Vec<(u16, usize, usize)>,
    owner: Option<sundial::package_authoring::mod_content::Importer>,
    last: Option<(usize, u16, Budget)>,
    count: usize,
    total: usize,
}

impl GraphPlan {
    pub(super) fn new(assets: &AssetPackages) -> AuthoringResult<Self> {
        let before = state(assets)?;
        let managed = crate::package_profile::host().is_standalone();
        let owned: Vec<_> = before
            .iter()
            .enumerate()
            .filter(|(_, (id, _, _))| {
                !managed || crate::mod_family::build::owner(*id) == assets.owner
            })
            .collect();
        let count = owned.len();
        let last = owned
            .last()
            .map(|&(index, &(id, entries, blocks))| (index, id, Budget { entries, blocks }));
        Ok(Self {
            parts: Vec::new(),
            tags: Vec::new(),
            bounds: Vec::new(),
            total: before.len(),
            before,
            owner: assets.owner,
            last,
            count,
        })
    }

    fn place(&mut self, entries: Vec<usize>) -> AuthoringResult<()> {
        let added = Budget::for_lengths(entries.iter().map(|&index| self.bounds[index]))?;
        if !added.within_capacity() {
            return Err(invalid(
                "An asset or its required enrollment group exceeds native package capacity",
            ));
        }
        if self.last.is_none_or(|(_, _, budget)| !budget.fits(added)) {
            let id = match self
                .owner
                .filter(|_| crate::package_profile::host().is_standalone())
            {
                Some(owner) => {
                    crate::mod_family::build::allocate(owner, self.count).map_err(invalid)?
                }
                None => window()
                    .nth(self.count)
                    .ok_or_else(|| invalid("The authored asset package id range is exhausted"))?,
            };
            self.last = Some((
                self.total,
                id,
                Budget {
                    entries: 0,
                    blocks: 0,
                },
            ));
            self.count += 1;
            self.total += 1;
        }
        let (package, id, budget) = self.last.as_mut().expect("asset package selected");
        if self.parts.last().is_none_or(|part| part.id != *id) {
            self.parts.push(Part {
                package: *package,
                id: *id,
                base: budget.entries,
                entries: Vec::new(),
            });
        }
        let part = self.parts.last_mut().expect("asset part selected");
        for index in entries {
            self.tags[index] = crate::appended_tags::AppendedTagAllocator::new(*id, 0)
                .assigned_tag(budget.entries, "Asset graph", "resource")?;
            part.entries.push(index);
            budget.entries += 1;
        }
        budget.blocks += added.blocks;
        Ok(())
    }

    #[cfg(feature = "d2-model-importer")]
    pub(super) fn reserve(&mut self, index: usize, size: usize) -> AuthoringResult<()> {
        if index != self.bounds.len() {
            return if self.bounds.get(index).is_some_and(|bound| size <= *bound) {
                Ok(())
            } else {
                Err(invalid(
                    "Streaming assets must reserve their payload before assigning an address",
                ))
            };
        }
        self.bounds.push(size);
        self.tags.push(TagHash(u32::MAX));
        self.place(vec![index])?;
        // Runtime records can allocate between asset writes. Protect already assigned IDs.
        let id = self.tags[index].pkg_id();
        if let Some(owner) = self.owner {
            crate::mod_family::build::use_id(owner, id);
        } else {
            crate::mod_family::build::native(id);
        }
        Ok(())
    }

    pub(super) fn ordinal(&self, tag: TagHash) -> AuthoringResult<usize> {
        self.tags
            .iter()
            .position(|assigned| *assigned == tag)
            .ok_or_else(|| invalid("Asset tag is outside this stream"))
    }

    pub(crate) fn tag(&self, index: usize) -> AuthoringResult<TagHash> {
        self.tags
            .get(index)
            .copied()
            .ok_or_else(|| invalid("Asset declaration is outside its plan"))
    }
}

fn state(assets: &AssetPackages) -> AuthoringResult<Vec<(u16, usize, usize)>> {
    assets
        .packages
        .iter()
        .map(|package| {
            let budget = Budget::for_lengths(package.tags.iter().map(|tag| tag.payload.len()))?;
            Ok((package.id, budget.entries, budget.blocks))
        })
        .collect()
}

fn root(parents: &[usize], mut index: usize) -> usize {
    while parents[index] != index {
        index = parents[index];
    }
    index
}

impl AssetPackages {
    /// `together` contains declarations whose native enrollment requires one package.
    /// Their declaration order need not be adjacent. Payload linking uses `GraphPlan::tag`.
    pub(crate) fn plan_graph(
        &self,
        bounds: Vec<usize>,
        together: &[(usize, usize)],
    ) -> AuthoringResult<GraphPlan> {
        if bounds.is_empty() {
            return Err(invalid("An asset graph cannot be empty"));
        }
        let mut plan = GraphPlan::new(self)?;
        let mut parents: Vec<_> = (0..bounds.len()).collect();
        for &(left, right) in together {
            if left >= parents.len() || right >= parents.len() || left == right {
                return Err(invalid(
                    "Asset enrollment has an invalid owner or companion",
                ));
            }
            let left = root(&parents, left);
            let right = root(&parents, right);
            parents[left.max(right)] = left.min(right);
        }
        let mut units = BTreeMap::<usize, Vec<usize>>::new();
        for index in 0..bounds.len() {
            units.entry(root(&parents, index)).or_default().push(index);
        }
        plan.tags = vec![TagHash(u32::MAX); bounds.len()];
        plan.bounds = bounds;
        for entries in units.into_values() {
            plan.place(entries)?;
        }
        Ok(plan)
    }

    /// Check all payloads, directory references and target packages before mutating any package.
    pub(crate) fn commit_graph(
        &mut self,
        plan: GraphPlan,
        tags: Vec<NewTagSpec>,
        references: Vec<NewTagReferenceOverride>,
    ) -> AuthoringResult<()> {
        if self.owner != plan.owner || state(self)? != plan.before {
            return Err(validation(
                "The asset graph allocation changed before commit",
            ));
        }
        if tags.len() != plan.bounds.len()
            || tags
                .iter()
                .zip(&plan.bounds)
                .any(|(tag, bound)| tag.payload.is_empty() || tag.payload.len() > *bound)
        {
            return Err(validation(
                "Materialized assets exceeded their declared bounds",
            ));
        }
        let mut owners = BTreeSet::new();
        let mut by_package = BTreeMap::<u16, Vec<NewTagReferenceOverride>>::new();
        for reference in references {
            let owner = plan.tag(reference.new_tag_ordinal)?;
            if !owners.insert(owner) {
                return Err(validation("An asset has duplicate directory references"));
            }
            let target = match reference.reference {
                crate::NewTagReference::Appended(index) => {
                    crate::NewTagReference::Tag(plan.tag(index)?)
                }
                target => target,
            };
            let target = match target {
                crate::NewTagReference::Tag(tag) if tag.pkg_id() == owner.pkg_id() => {
                    crate::NewTagReference::Appended(usize::from(tag.entry_index()))
                }
                target => target,
            };
            by_package
                .entry(owner.pkg_id())
                .or_default()
                .push(NewTagReferenceOverride {
                    new_tag_ordinal: usize::from(owner.entry_index()),
                    reference: target,
                });
        }
        if plan.parts.is_empty() {
            return Err(invalid("An asset graph cannot be empty"));
        }
        let mut tags: Vec<_> = tags.into_iter().map(Some).collect();
        for part in plan.parts {
            let bounds = part
                .entries
                .iter()
                .map(|&index| plan.bounds[index])
                .collect();
            let added = part
                .entries
                .into_iter()
                .map(|index| tags[index].take().expect("unique graph placement"));
            self.register(part.id);
            if part.package == self.packages.len() {
                self.packages.push(AssetPackage {
                    id: part.id,
                    tags: Vec::new(),
                    references: Vec::new(),
                });
            }
            let package = &mut self.packages[part.package];
            package.tags.extend(added);
            package
                .references
                .extend(by_package.remove(&part.id).unwrap_or_default());
            self.reservations.push(Reservation {
                package_index: part.package,
                base: part.base,
                bounds,
            });
        }
        Ok(())
    }
}
