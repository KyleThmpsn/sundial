//! Section rendering produces an edit request without changing account data.
mod card;
use super::*;

impl SundialApp {
    pub(super) fn draw_profile_items_section(&mut self, ui: &mut egui::Ui) -> Option<Edit> {
        let SharedModel {
            count: profile_item_count,
            editable,
            capacity,
            account_ready,
            bucket_usage,
            groups,
        } = match self.shared_items_model() {
            Ok(model) => model,
            Err(error) => {
                draw_section_error(ui, &error.to_string());
                return None;
            }
        };

        ui.horizontal_wrapped(|ui| {
            ui.strong("Shared Items");
            let count = capacity.map_or_else(
                || format!("{profile_item_count} items"),
                |capacity| format!("{profile_item_count} / {capacity}"),
            );
            ui.label(egui::RichText::new(count).weak());
        });
        ui.label("Stackable profile-scoped definitions only.");

        if !editable {
            ui.label(
                egui::RichText::new(
                    "Profile-item editing is unavailable for the active account source.",
                )
                .weak(),
            );
        } else if capacity.is_some_and(|capacity| profile_item_count >= capacity) {
            ui.label(
                egui::RichText::new("The profile-item collection is full for this account source.")
                    .weak(),
            );
        } else if !account_ready {
            ui.label(
                egui::RichText::new(
                    "Add controls require an account collection in the active source. Existing rows remain visible.",
                )
                .weak(),
            );
        } else if bucket_usage.unresolved_count > 0 {
            draw_unresolved_bucket_warning(ui);
        }
        ui.add_space(4.0);

        if groups.is_empty() {
            ui.label(egui::RichText::new("No profile inventory buckets are available.").weak());
            return None;
        }

        let mut pending = None;
        for SharedBucket {
            group,
            title,
            can_add,
            add_tooltip,
        } in groups
        {
            let picker_key = format!(
                "profile-items:add:{}:{}",
                scope_id(group.key.scope),
                group.key.native_id
            );
            let repaint_context = ui.ctx().clone();
            let mut toggle_header = false;
            let mut open_picker = false;
            let mut picker_anchor = None;
            let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id((
                    "profile-items-bucket",
                    scope_id(group.key.scope),
                    group.key.native_id,
                )),
                true,
            )
            .show_header(ui, |ui| {
                toggle_header = ui
                    .add(
                        egui::Label::new(bucket_header_text(ui, &title))
                            .sense(egui::Sense::click()),
                    )
                    .clicked();
                if group.addable {
                    let response = ui.add_enabled(can_add, egui::Button::new("+").small());
                    let response = if can_add {
                        response.on_hover_text(&add_tooltip)
                    } else {
                        response.on_disabled_hover_text(&add_tooltip)
                    };
                    open_picker = response.clicked();
                    picker_anchor = Some(response);
                }
            });
            if toggle_header {
                header.toggle();
            }
            if open_picker {
                header.set_open(true);
                self.open_bucket_picker(&picker_key, "profile-items:add:");
                repaint_context.request_repaint();
            }
            header.body(|ui| {
                draw_bucket_details(ui, &group, &bucket_usage, InventoryScope::Profile);
                if self.searches.contains_key(&picker_key) {
                    let action = ui
                        .add_enabled_ui(can_add, |ui| {
                            let manifest = &self.manifest;
                            let request_open = take_bucket_picker_open_request(
                                &mut self.searches,
                                &picker_key,
                                ui.input(|input| input.pointer.any_click()),
                            );
                            let query = self.searches.entry(picker_key.clone()).or_default();
                            item_editor::draw_definition_picker_with_open_request(
                                ui,
                                manifest,
                                (
                                    "profile-items-add-definition",
                                    scope_id(group.key.scope),
                                    group.key.native_id,
                                ),
                                query,
                                picker_height(),
                                (picker_anchor.as_ref(), request_open),
                                |query| DefinitionPickerChoices {
                                    definitions: profile_bucket_definition_choices(
                                        manifest.profile_item_candidates(query).filter(
                                            |definition| {
                                                definition.metadata.scope == group.key.scope
                                                    && definition.metadata.native_bucket_id
                                                        == group.key.native_id
                                                    && u32::try_from(definition.hash).is_ok()
                                            },
                                        ),
                                    ),
                                    existing_inventory: Vec::new(),
                                    clear: None,
                                    random_item_builder_hash: None,
                                    empty_message: "No safe definitions in this bucket match"
                                        .to_owned(),
                                },
                            )
                        })
                        .inner;
                    if pending.is_none()
                        && let Some(ItemEditorAction::SetDefinition { hash }) = action
                    {
                        pending = Some(Edit::AddItem {
                            hash,
                            picker_key: picker_key.clone(),
                        });
                    }
                }
                let (minimum_card_width, maximum_card_width) =
                    self.preferences.item_card_width.dimensions();
                item_editor::draw_responsive_item_cards(
                    ui,
                    &group.items,
                    minimum_card_width,
                    maximum_card_width,
                    |ui, snapshot| {
                        let action =
                            self.draw_profile_item_card(ui, snapshot, editable, &bucket_usage);
                        if pending.is_none()
                            && let Some(action) = action
                        {
                            pending = Some(action);
                        }
                    },
                );
            });
        }
        pending
    }
}
