use super::*;

#[derive(Clone, Copy)]
struct DefinitionPickerBehavior {
    height: PickerHeight,
    supports_nested_popups: bool,
}

pub(crate) fn draw_definition_picker_with_open_request(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    trigger: (Option<&egui::Response>, bool),
    choices_for_query: impl FnOnce(&str) -> DefinitionPickerChoices,
) -> Option<ItemEditorAction> {
    draw_definition_picker_with_open_request_and_controls(
        ui,
        catalog,
        scope,
        query,
        DefinitionPickerBehavior {
            height,
            supports_nested_popups: false,
        },
        trigger,
        (|_, query| (choices_for_query(query), false), |_| None::<()>),
    )
    .0
}

pub(crate) fn draw_definition_picker_with_open_request_and_item_filter(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    trigger: (Option<&egui::Response>, bool),
    choices_for_query: impl FnOnce(
        &mut egui::Ui,
        &str,
        &mut ItemFilter,
    ) -> (DefinitionPickerChoices, bool),
) -> Option<ItemEditorAction> {
    draw_definition_picker_with_open_request_and_controls(
        ui,
        catalog,
        scope,
        query,
        DefinitionPickerBehavior {
            height,
            supports_nested_popups: true,
        },
        trigger,
        (
            |ui, query| {
                let filter_id = ui.make_persistent_id("item-filter");
                let mut filter = ui
                    .data_mut(|data| data.get_temp::<ItemFilter>(filter_id))
                    .unwrap_or_default();
                let choices = choices_for_query(ui, query, &mut filter);
                ui.data_mut(|data| data.insert_temp(filter_id, filter));
                choices
            },
            |_| None::<()>,
        ),
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
    draw_definition_picker_with_open_request_and_controls(
        ui,
        catalog,
        scope,
        query,
        DefinitionPickerBehavior {
            height,
            supports_nested_popups: false,
        },
        trigger,
        (|_, query| (choices_for_query(query), false), draw_footer),
    )
}

fn draw_definition_picker_with_open_request_and_controls<T>(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    behavior: DefinitionPickerBehavior,
    trigger: (Option<&egui::Response>, bool),
    contents: (
        impl FnOnce(&mut egui::Ui, &str) -> (DefinitionPickerChoices, bool),
        impl FnOnce(&mut egui::Ui) -> Option<T>,
    ),
) -> (Option<ItemEditorAction>, Option<T>) {
    let (choices_for_query, draw_footer) = contents;
    let DefinitionPickerBehavior {
        height,
        supports_nested_popups,
    } = behavior;
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
            if supports_nested_popups {
                set_independent_popup_open(ui, popup_id, true);
            } else {
                ui.memory_mut(|memory| memory.open_popup(popup_id));
            }
        }
        let is_open = if supports_nested_popups {
            independent_popup_is_open(ui, popup_id)
        } else {
            ui.memory(|memory| memory.is_popup_open(popup_id))
        };
        if !is_open {
            return (None, None);
        }

        let row_height = ui.spacing().interact_size.y.max(44.0);
        let popup_direction = popup_direction(ui.ctx().screen_rect(), picker_response.rect);
        let mut action = None;
        let mut footer_action = None;
        let mut nested_popup_interacted = false;
        let picker_style = ui.style().clone();
        let draw_popup = |ui: &mut egui::Ui| {
            ui.set_style(picker_style);
            ui.set_min_width(picker_response.rect.width().max(360.0));
            let search_response = ui.add(
                egui::TextEdit::singleline(query)
                    .hint_text("Search item name, description, type, or hex hash…")
                    .desired_width(ui.available_width()),
            );
            ui.ctx()
                .accesskit_node_builder(search_response.id, |node| node.set_label("Search items"));
            if just_opened {
                search_response.request_focus();
            }
            let (choices, interacted) = choices_for_query(ui, query);
            nested_popup_interacted = interacted;
            if let Some(hash) = choices.random_item_builder_hash {
                if ui
                    .button("Open item in Random Item Builder")
                    .on_hover_text("Open this item and its current plugs in Random Item Builder")
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
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(ui.available_width(), row_height),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(egui::RichText::new(group).strong());
                                        },
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
                                            secondary: (!definition.type_name.trim().is_empty())
                                                .then_some(definition.type_name.as_str()),
                                            icon_size: 36.0,
                                            row_height,
                                            selected: false,
                                        },
                                    );
                                    let response =
                                        catalog_item_tooltip(response, catalog, definition.hash);
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
        };
        if supports_nested_popups {
            let nested_popup_was_open = ui.memory(|memory| memory.any_popup_open());
            let popup_response = show_independent_picker_popup(
                ui,
                popup_id,
                &picker_response,
                popup_direction,
                draw_popup,
            );
            let clicked_outside =
                picker_response.clicked_elsewhere() && popup_response.clicked_elsewhere();
            let escape_closes_parent =
                ui.input(|input| input.key_pressed(egui::Key::Escape)) && !nested_popup_was_open;
            if (clicked_outside && !(nested_popup_was_open && nested_popup_interacted))
                || escape_closes_parent
                || action.is_some()
                || footer_action.is_some()
            {
                set_independent_popup_open(ui, popup_id, false);
            }
        } else {
            egui::popup::popup_above_or_below_widget(
                ui,
                popup_id,
                &picker_response,
                popup_direction,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                draw_popup,
            );
        }
        (action, footer_action)
    })
    .inner
}

fn independent_popup_open_id(popup_id: egui::Id) -> egui::Id {
    popup_id.with("independent-open")
}

fn independent_popup_is_open(ui: &egui::Ui, popup_id: egui::Id) -> bool {
    ui.data(|data| {
        data.get_temp::<bool>(independent_popup_open_id(popup_id))
            .unwrap_or(false)
    })
}

fn set_independent_popup_open(ui: &egui::Ui, popup_id: egui::Id, open: bool) {
    ui.data_mut(|data| data.insert_temp(independent_popup_open_id(popup_id), open));
}

fn show_independent_picker_popup<R>(
    parent_ui: &egui::Ui,
    popup_id: egui::Id,
    widget_response: &egui::Response,
    direction: egui::AboveOrBelow,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let (mut position, pivot) = match direction {
        egui::AboveOrBelow::Above => (widget_response.rect.left_top(), egui::Align2::LEFT_BOTTOM),
        egui::AboveOrBelow::Below => (widget_response.rect.left_bottom(), egui::Align2::LEFT_TOP),
    };
    if let Some(to_global) = parent_ui
        .ctx()
        .layer_transform_to_global(parent_ui.layer_id())
    {
        position = to_global * position;
    }

    let frame = egui::Frame::popup(parent_ui.style());
    let inner_width = (widget_response.rect.width() - frame.total_margin().sum().x).max(0.0);
    egui::Area::new(popup_id)
        .kind(egui::UiKind::Popup)
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .default_width(inner_width)
        .pivot(pivot)
        .show(parent_ui.ctx(), |ui| {
            frame
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_min_width(inner_width);
                        add_contents(ui)
                    })
                    .inner
                })
                .inner
        })
        .response
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
