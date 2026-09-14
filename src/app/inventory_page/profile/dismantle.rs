//! Section rendering produces an edit request without changing account data.
mod card;
use super::*;

impl SundialApp {
    pub(super) fn draw_dismantle_reward_section(&mut self, ui: &mut egui::Ui) -> Option<Edit> {
        let DismantleModel {
            rewards,
            editable,
            capacity,
            account_ready,
            filtered,
            combined_gear_class,
        } = match self.dismantle_model() {
            Ok(model) => model,
            Err(error) => {
                draw_section_error(ui, &error.to_string());
                return None;
            }
        };
        let mut pending = None;
        let has_room = capacity.is_some_and(|capacity| rewards.len() < capacity);
        let picker_key = "dismantle-rewards:add".to_owned();
        let mut picker_anchor = None;
        let mut open_picker = false;

        ui.horizontal_wrapped(|ui| {
            ui.strong("Dismantle Rewards");
            let count = capacity.map_or_else(
                || format!("{} policies", rewards.len()),
                |capacity| format!("{} / {capacity}", rewards.len()),
            );
            ui.label(egui::RichText::new(count).weak());
            let can_add = editable && account_ready && has_room;
            let response = ui.add_enabled(can_add, egui::Button::new("+").small());
            let response = if can_add {
                response.on_hover_text("Add a dismantle payout policy")
            } else {
                response.on_disabled_hover_text(if !editable {
                    "Dismantle-policy editing is unavailable for the active account source"
                } else if !account_ready {
                    "The active account source has no account collection to edit"
                } else {
                    "The dismantle-policy array is full"
                })
            };
            if response.clicked() {
                self.searches.entry(picker_key.clone()).or_default();
                open_picker = true;
            }
            picker_anchor = Some(response);
        });
        ui.label(
            "Materials credited when Sunrise dismantles weapons or armor. Matching policies are added together.",
        );
        if filtered {
            ui.label(
                egui::RichText::new(
                    "Leave a filter on Any to match every rarity, gear class, or masterwork state.",
                )
                .weak(),
            );
        }
        ui.add_space(4.0);

        if self.searches.contains_key(&picker_key) {
            let action = ui
                .add_enabled_ui(editable && account_ready && has_room, |ui| {
                    let manifest = &self.manifest;
                    let query = self.searches.entry(picker_key.clone()).or_default();
                    item_editor::draw_definition_picker_with_open_request(
                        ui,
                        manifest,
                        "dismantle-reward-add-definition",
                        query,
                        picker_height(),
                        (picker_anchor.as_ref(), open_picker),
                        |query| DefinitionPickerChoices {
                            definitions: without_definition_groups(profile_definition_choices(
                                manifest
                                    .profile_item_candidates(query)
                                    .filter(|definition| u32::try_from(definition.hash).is_ok()),
                            )),
                            existing_inventory: Vec::new(),
                            clear: None,
                            random_item_builder_hash: None,
                            empty_message: "No profile material definitions match".to_owned(),
                        },
                    )
                })
                .inner;
            if let Some(ItemEditorAction::SetDefinition { hash }) = action {
                pending = Some(Edit::AddReward { hash, picker_key });
            }
        }

        if rewards.is_empty() {
            ui.label(egui::RichText::new("No dismantle payout policies.").weak());
            return pending;
        }

        let (minimum_card_width, maximum_card_width) =
            self.preferences.item_card_width.dimensions();
        item_editor::draw_responsive_item_cards(
            ui,
            &rewards,
            minimum_card_width,
            maximum_card_width,
            |ui, reward| {
                if pending.is_none()
                    && let Some(action) = self.draw_dismantle_reward_card(
                        ui,
                        reward,
                        editable,
                        filtered,
                        combined_gear_class,
                    )
                {
                    pending = Some(Edit::Reward {
                        location: reward.location,
                        action,
                    });
                }
            },
        );
        pending
    }
}
