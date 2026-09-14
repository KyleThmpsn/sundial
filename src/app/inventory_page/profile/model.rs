//! Read account capabilities and prepare the collections before drawing them.
use super::super::model::ItemBucket;
use super::*;
use crate::app::inventory::InventoryError;

pub(super) struct DismantleModel {
    pub rewards: Vec<DismantleRewardSnapshot>,
    pub editable: bool,
    pub capacity: Option<usize>,
    pub account_ready: bool,
    pub filtered: bool,
    pub combined_gear_class: bool,
}

pub(super) struct SharedModel {
    pub count: usize,
    pub editable: bool,
    pub capacity: Option<usize>,
    pub account_ready: bool,
    pub bucket_usage: BucketUsage,
    pub groups: Vec<SharedBucket>,
}

pub(super) struct SharedBucket {
    pub group: ItemBucket<ProfileItemSnapshot>,
    pub title: String,
    pub can_add: bool,
    pub add_tooltip: String,
}

impl SundialApp {
    pub(super) fn dismantle_model(&self) -> Result<DismantleModel, InventoryError> {
        Ok(DismantleModel {
            rewards: account::dismantle_rewards(&self.document)?.unwrap_or_default(),
            editable: account::dismantle_rewards_editable(&self.document),
            capacity: account::dismantle_reward_capacity(&self.document),
            account_ready: account::account_collection_ready(&self.document),
            filtered: account::filtered_dismantle_rewards(&self.document),
            combined_gear_class: account::supports_combined_dismantle_gear_class(&self.document),
        })
    }

    pub(super) fn shared_items_model(&self) -> Result<SharedModel, InventoryError> {
        let items = account::profile_items(&self.document)?.unwrap_or_default();
        let count = items.len();
        let bucket_usage = self.profile_bucket_usage(&items);
        let mut groups = self.group_items_by_bucket(
            items,
            |item| Some(u64::from(item.definition_hash)),
            InventoryScope::Profile,
        );
        let candidates = distinct_candidate_buckets(
            self.manifest
                .profile_item_candidates("")
                .map(|definition| *definition.metadata),
        );
        add_candidate_buckets(&mut groups, candidates, InventoryScope::Profile);
        let editable = account::profile_items_editable(&self.document);
        let capacity = account::profile_item_capacity(&self.document);
        let account_ready = account::account_collection_ready(&self.document);
        let array_has_room = capacity.is_some_and(|capacity| count < capacity);
        let groups = groups
            .into_iter()
            .map(|group| {
                let title = bucket_header_label(&group, &bucket_usage, InventoryScope::Profile);
                let blocker =
                    bucket_add_blocker(group.key, group.capacity, &bucket_usage, &group.label);
                let can_add = editable
                    && group.addable
                    && account_ready
                    && array_has_room
                    && bucket_usage.occupancy_complete
                    && blocker.is_none();
                let add_tooltip = bucket_add_tooltip(
                    can_add,
                    editable,
                    account_ready,
                    array_has_room,
                    bucket_usage.occupancy_complete,
                    blocker.as_deref(),
                    &group.label,
                );
                SharedBucket {
                    group,
                    title,
                    can_add,
                    add_tooltip,
                }
            })
            .collect();
        Ok(SharedModel {
            count,
            editable,
            capacity,
            account_ready,
            bucket_usage,
            groups,
        })
    }
}
