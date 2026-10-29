//! Incremental clip and audio placement. Addresses are fixed before payload references bind.
use super::*;
#[cfg(feature = "d2-model-importer")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{collections::BTreeMap, sync::Mutex};
use tiger_pkg::TagHash;

#[cfg(feature = "d2-model-importer")]
static NEXT: AtomicUsize = AtomicUsize::new(0);
static PLANS: Mutex<BTreeMap<usize, graph::GraphPlan>> = Mutex::new(BTreeMap::new());

fn plans() -> std::sync::MutexGuard<'static, BTreeMap<usize, graph::GraphPlan>> {
    PLANS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn clear() {
    plans().clear();
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn reserve(id: usize, ordinal: usize, size: usize) -> AuthoringResult<()> {
    plans()
        .get_mut(&id)
        .ok_or_else(|| invalid("Asset stream has ended"))?
        .reserve(ordinal, size)
}

pub(crate) fn tag(id: usize, ordinal: usize) -> AuthoringResult<TagHash> {
    plans()
        .get(&id)
        .ok_or_else(|| invalid("Asset stream has ended"))?
        .tag(ordinal)
}

pub(crate) fn ordinal(id: usize, tag: TagHash) -> AuthoringResult<usize> {
    plans()
        .get(&id)
        .ok_or_else(|| invalid("Asset stream has ended"))?
        .ordinal(tag)
}

#[cfg(feature = "d2-model-importer")]
pub(crate) struct Stream {
    id: usize,
    pub allocator: crate::appended_tags::AppendedTagAllocator,
    pub tags: Vec<NewTagSpec>,
}

#[cfg(feature = "d2-model-importer")]
impl Stream {
    pub(crate) fn new(assets: &AssetPackages) -> AuthoringResult<Self> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        plans().insert(id, graph::GraphPlan::new(assets)?);
        Ok(Self {
            id,
            allocator: crate::appended_tags::AppendedTagAllocator::assets(id),
            tags: Vec::new(),
        })
    }

    pub(crate) fn target(
        &mut self,
    ) -> (
        crate::appended_tags::AppendedTagAllocator,
        &mut Vec<NewTagSpec>,
    ) {
        (self.allocator, &mut self.tags)
    }

    pub(crate) fn finish(mut self, assets: &mut AssetPackages) -> AuthoringResult<Vec<TagHash>> {
        let plan = plans()
            .remove(&self.id)
            .ok_or_else(|| invalid("Asset stream has ended"))?;
        if self.tags.is_empty() {
            return Ok(Vec::new());
        }
        let assigned = (0..self.tags.len())
            .map(|index| plan.tag(index))
            .collect::<AuthoringResult<Vec<_>>>()?;
        assets.commit_graph(plan, std::mem::take(&mut self.tags), Vec::new())?;
        Ok(assigned)
    }
}

#[cfg(feature = "d2-model-importer")]
impl Drop for Stream {
    fn drop(&mut self) {
        plans().remove(&self.id);
    }
}
