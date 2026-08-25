use super::*;

pub(crate) fn draw_definition_picker_with_open_request(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    trigger: (Option<&egui::Response>, bool),
    choices_for_query: impl FnOnce(&str) -> DefinitionPickerChoices,
) -> Option<ItemEditorAction> {
    draw_definition_picker_with_open_request_and_footer(
        ui,
        catalog,
        scope,
        query,
        height,
        trigger,
        (choices_for_query, |_| None::<()>),
    )
    .0
}

pub(crate) fn draw_definition_picker_with_open_request_and_footer<T>(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    trigger: (Option<&egui::Response>, bool),
    contents: (
        impl FnOnce(&str) -> DefinitionPickerChoices,
        impl FnOnce(&mut egui::Ui) -> Option<T>,
    ),
) -> (Option<ItemEditorAction>, Option<T>) {
    let (choices_for_query, draw_footer) = contents;
    ui.push_id(scope, |ui| {
        let (anchor, open_requested) = trigger;
        let picker_response = anchor.cloned().unwrap_or_else(|| {
            ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                egui::Button::new("Choose an item…"),
            )
        });
        let popup_id = ui.make_persistent_id("definition-browser");
        let just_opened = ui.is_enabled() && (open_requested || picker_response.clicked());
        if just_opened {
            ui.memory_mut(|memory| memory.open_popup(popup_id));
        }
        if !ui.memory(|memory| memory.is_popup_open(popup_id)) {
            return (None, None);
        }

        let row_height = ui.spacing().interact_size.y.max(44.0);
        let popup_direction = popup_direction(ui.ctx().screen_rect(), picker_response.rect);
        let mut action = None;
        let mut footer_action = None;
        egui::popup::popup_above_or_below_widget(
            ui,
            popup_id,
            &picker_response,
            popup_direction,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(picker_response.rect.width().max(360.0));
                let search_response = ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text("Search item name, description, type, or hex hash…")
                        .desired_width(ui.available_width()),
                );
                if just_opened {
                    search_response.request_focus();
                }
                let choices = choices_for_query(query);
                if let Some(hash) = choices.random_item_builder_hash {
                    if ui
                        .button("Open item in Random Item Builder")
                        .on_hover_text(
                            "Open this item and its current plugs in Random Item Builder",
                        )
                        .clicked()
                    {
                        action = Some(ItemEditorAction::OpenInRandomItemBuilder { hash });
                        ui.memory_mut(egui::Memory::close_popup);
                    }
                }
                ui.separator();
                let has_picker_choices = !choices.definitions.is_empty()
                    || !choices.existing_inventory.is_empty()
                    || choices.clear.is_some();
                if !has_picker_choices {
                    ui.label(egui::RichText::new(&choices.empty_message).weak());
                } else if let Some(clear) = &choices.clear {
                    if ui
                        .selectable_label(clear.selected, &clear.label)
                        .on_hover_text(&clear.tooltip)
                        .clicked()
                    {
                        action = Some(ItemEditorAction::ClearDefinition);
                        ui.memory_mut(egui::Memory::close_popup);
                    }
                }

                let rows = definition_picker_rows(&choices.definitions);
                let scroll_row_count = rows.len()
                    + choices.existing_inventory.len()
                    + usize::from(!choices.existing_inventory.is_empty());
                if scroll_row_count > 0 {
                    if choices.clear.is_some() {
                        ui.separator();
                    }
                    let picker_height = spaced_picker_list_height(
                        scroll_row_count,
                        row_height,
                        ui.spacing().item_spacing.y,
                        height.min,
                        height.max,
                    );
                    egui::ScrollArea::vertical()
                        .min_scrolled_height(picker_height)
                        .max_height(picker_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if !choices.existing_inventory.is_empty() {
                                ui.label(egui::RichText::new("Existing inventory item").strong());
                                for existing in &choices.existing_inventory {
                                    let label = format!(
                                        "{}  ({})",
                                        existing.name,
                                        format_hash_hex(existing.hash)
                                    );
                                    let response = draw_catalog_picker_row(
                                        ui,
                                        catalog,
                                        CatalogPickerRow {
                                            hash: existing.hash,
                                            primary: &label,
                                            primary_max_rows: 1,
                                            secondary: (!existing.type_name.trim().is_empty())
                                                .then_some(existing.type_name.as_str()),
                                            icon_size: 36.0,
                                            row_height,
                                            selected: false,
                                        },
                                    );
                                    let response =
                                        catalog_item_tooltip(response, catalog, existing.hash);
                                    if response.clicked() {
                                        action = Some(ItemEditorAction::EquipInventoryItem {
                                            item_index: existing.item_index,
                                        });
                                        ui.memory_mut(egui::Memory::close_popup);
                                    }
                                }
                                ui.separator();
                            }

                            for row in rows {
                                match row {
                                    DefinitionPickerRow::Group(group) => {
                                        ui.add_sized(
                                            [ui.available_width(), row_height],
                                            egui::Label::new(egui::RichText::new(group).strong())
                                                .halign(egui::Align::LEFT),
                                        );
                                    }
                                    DefinitionPickerRow::Definition(definition) => {
                                        let label = format!(
                                            "{}  ({})",
                                            definition.name,
                                            format_hash_hex(definition.hash)
                                        );
                                        let response = draw_catalog_picker_row(
                                            ui,
                                            catalog,
                                            CatalogPickerRow {
                                                hash: definition.hash,
                                                primary: &label,
                                                primary_max_rows: 1,
                                                secondary: (!definition
                                                    .type_name
                                                    .trim()
                                                    .is_empty())
                                                .then_some(definition.type_name.as_str()),
                                                icon_size: 36.0,
                                                row_height,
                                                selected: false,
                                            },
                                        );
                                        let response = catalog_item_tooltip(
                                            response,
                                            catalog,
                                            definition.hash,
                                        );
                                        let clicked = response.clicked();
                                        if clicked {
                                            action = Some(ItemEditorAction::SetDefinition {
                                                hash: definition.hash,
                                            });
                                            ui.memory_mut(egui::Memory::close_popup);
                                        }
                                    }
                                }
                            }
                        });
                }

                if let Some(selected) = draw_footer(ui) {
                    footer_action = Some(selected);
                    ui.memory_mut(egui::Memory::close_popup);
                }
            },
        );
        (action, footer_action)
    })
    .inner
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefinitionPickerRow<'a> {
    Group(&'a str),
    Definition(&'a DefinitionChoice),
}

fn definition_picker_rows(definitions: &[DefinitionChoice]) -> Vec<DefinitionPickerRow<'_>> {
    let mut first_group = None;
    let mut has_multiple_groups = false;
    for group in definitions
        .iter()
        .filter_map(|definition| definition.group.as_deref())
    {
        match first_group {
            Some(first_group) if first_group != group => {
                has_multiple_groups = true;
                break;
            }
            Some(_) => {}
            None => first_group = Some(group),
        }
    }

    let mut rows = Vec::with_capacity(definitions.len());
    let mut displayed_group = None::<&str>;
    for definition in definitions {
        if definition.group.as_deref() != displayed_group {
            if has_multiple_groups && let Some(group) = definition.group.as_deref() {
                rows.push(DefinitionPickerRow::Group(group));
            }
            displayed_group = definition.group.as_deref();
        }
        rows.push(DefinitionPickerRow::Definition(definition));
    }
    rows
}
