//! Profile inventory routing. Read models, rendering and edit commits live in child modules.
mod actions;
mod dismantle;
mod model;
mod shared;

use actions::Edit;
use model::{DismantleModel, SharedBucket, SharedModel};

use crate::app::account_workspace as account;

use eframe::egui;

use crate::{catalog::InventoryScope, hash::format_hash_hex};

use super::super::{
    SundialApp,
    inspector::DefinitionInspectionContext,
    inventory::{
        self, DismantleGearClass, DismantleRarity, DismantleRewardAction, DismantleRewardLocation,
        DismantleRewardSnapshot, ProfileItemAction, ProfileItemLocation, ProfileItemSnapshot,
    },
    item_editor::{
        self, DefinitionPickerChoices, DefinitionSummary, ItemEditorAction, ItemHeader,
        NumericItemFields,
    },
};
use super::{
    buckets::{
        add_candidate_buckets, bucket_add_blocker, bucket_add_tooltip, bucket_header_label,
        bucket_header_text, distinct_candidate_buckets, draw_bucket_details,
        profile_swap_candidate, scope_id,
    },
    definitions::{
        profile_bucket_definition_choices, profile_definition_choices, without_definition_groups,
    },
    interactions::take_bucket_picker_open_request,
    model::{BucketUsage, ProfileInventorySection},
    presentation::{
        InventoryPageKind, dismantle_class_label, dismantle_masterwork_label,
        dismantle_rarity_label, dismantle_rarity_summary, draw_schema_notice, draw_section_error,
        draw_unresolved_bucket_warning, picker_height,
    },
};

impl SundialApp {
    pub(in crate::app) fn draw_profile_inventory_page(&mut self, ui: &mut egui::Ui) {
        let mode = inventory::schema_mode(&self.document);
        let section_id = ui.make_persistent_id("profile-inventory-section");
        let mut section = ui.data_mut(|data| {
            data.get_temp::<ProfileInventorySection>(section_id)
                .unwrap_or_default()
        });
        let dismantle_rewards_available = account::dismantle_rewards_available(&self.document);
        if section == ProfileInventorySection::DismantleRewards && !dismantle_rewards_available {
            section = ProfileInventorySection::SharedItems;
        }
        if section == ProfileInventorySection::PendingRewards
            && self.document.native_account().is_none()
        {
            section = ProfileInventorySection::SharedItems;
        }

        ui.horizontal(|ui| {
            ui.heading("Profile Inventory");
            crate::ui_help::info(ui, "Items shared by the account and available to every character. Storage and limits follow the active Sunrise account source.");
        });
        if self.document.uses_json_account() {
            draw_schema_notice(ui, mode, InventoryPageKind::Profile);
        }
        ui.add_space(4.0);

        if dismantle_rewards_available {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::SharedItems,
                    "Shared Items",
                );
                ui.selectable_value(
                    &mut section,
                    ProfileInventorySection::DismantleRewards,
                    "Dismantle Rewards",
                );
                if self.document.native_account().is_some() {
                    ui.selectable_value(
                        &mut section,
                        ProfileInventorySection::PendingRewards,
                        "Pending Rewards",
                    );
                }
            });
            ui.add_space(4.0);
        }
        ui.data_mut(|data| data.insert_temp(section_id, section));

        let edit = egui::ScrollArea::vertical()
            .id_salt(("profile-inventory-page", section))
            .show(ui, |ui| match section {
                ProfileInventorySection::SharedItems => self.draw_profile_items_section(ui),
                ProfileInventorySection::DismantleRewards => self.draw_dismantle_reward_section(ui),
                ProfileInventorySection::PendingRewards => {
                    self.draw_pending_rewards(ui);
                    None
                }
            })
            .inner;
        if let Some(edit) = edit
            && ui.is_enabled()
        {
            self.apply_profile_edit(edit);
        }
    }
}
