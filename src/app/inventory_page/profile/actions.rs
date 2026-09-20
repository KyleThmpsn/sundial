//! Apply one requested profile edit after rendering, then update history and picker state.
use super::*;

pub(super) enum Edit {
    AddItem {
        hash: u64,
        picker_key: String,
    },
    Item {
        location: ProfileItemLocation,
        action: ProfileItemAction,
    },
    Seen {
        position: usize,
        seen: bool,
    },
    AddReward {
        hash: u64,
        picker_key: String,
    },
    Reward {
        location: DismantleRewardLocation,
        action: DismantleRewardAction,
    },
}

impl SundialApp {
    pub(super) fn apply_profile_edit(&mut self, edit: Edit) {
        match self.apply_profile_action(edit) {
            Ok(label) => self.report_edit(label),
            Err(error) => self.set_status(error, true),
        }
    }

    fn apply_profile_action(&mut self, edit: Edit) -> Result<&'static str, String> {
        match edit {
            Edit::AddItem { hash, picker_key } => {
                crate::app::account_validation::apply_with_bucket_limits(
                    &mut self.document,
                    &self.manifest,
                    |document| {
                        account::add_profile_item(document, definition_hash(hash)?, 1)
                            .map_err(|error| error.to_string())
                    },
                )?;
                self.searches.remove(&picker_key);
                Ok("Added a shared profile item")
            }
            Edit::Item { location, action } => {
                let removed = matches!(action, ProfileItemAction::Remove);
                crate::app::account_validation::apply_with_bucket_limits(
                    &mut self.document,
                    &self.manifest,
                    |document| {
                        account::apply_profile_item_action(document, location, action)
                            .map_err(|error| error.to_string())
                    },
                )?;
                if removed {
                    self.searches.retain(|key, _| {
                        key.starts_with("profile-items:add:") || !key.starts_with("profile-items:")
                    });
                }
                Ok(if removed {
                    "Removed a shared profile item"
                } else {
                    "Updated a shared profile item"
                })
            }
            Edit::Seen { position, seen } => {
                self.document
                    .native_account_mut()
                    .ok_or("Native account is unavailable")?
                    .set_profile_item_seen(position, seen)
                    .map_err(|error| error.to_string())?;
                Ok("Updated shared item seen state")
            }
            Edit::AddReward { hash, picker_key } => {
                account::add_dismantle_reward(&mut self.document, definition_hash(hash)?)
                    .map_err(|error| error.to_string())?;
                self.searches.remove(&picker_key);
                Ok("Added a dismantle reward policy")
            }
            Edit::Reward { location, action } => {
                let removed = matches!(action, DismantleRewardAction::Remove);
                account::apply_dismantle_reward_action(&mut self.document, location, action)
                    .map_err(|error| error.to_string())?;
                if removed {
                    self.searches
                        .retain(|key, _| !key.starts_with("dismantle-rewards:edit:"));
                }
                Ok(if removed {
                    "Removed a dismantle reward policy"
                } else {
                    "Updated a dismantle reward policy"
                })
            }
        }
    }
}

fn definition_hash(hash: u64) -> Result<u32, String> {
    u32::try_from(hash).map_err(|_| "The selected definition hash does not fit in 32 bits".into())
}
