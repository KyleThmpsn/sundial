//! Socket role and choice controls; recipe writes are deferred to commands.
use super::super::{
    LogEntry, PlugChoicePickerButton, authoring_socket_label_width, draw_socket_role_label,
    named_control, socket_choice_columns,
};
use super::{RowChoices, RowCommand, SocketRowContext};

pub(super) fn draw_disabled(
    ui: &mut egui::Ui,
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
) -> Option<RowCommand> {
    let catalog = context.catalog;
    let donor = context.donor;
    let socket_index = context.socket_index;
    let mut selected_type = choices.socket_type_override;
    let mut activate = false;
    let mut options_command = None;
    ui.horizontal(|ui| {
        let row_height = ui.spacing().interact_size.y;
        let spacing = ui.spacing().item_spacing.x;
        let available_width = ui.available_width();
        let label_width = authoring_socket_label_width(available_width);
        let value_width = (available_width - label_width - spacing).max(110.0);
        draw_socket_role_label(ui, catalog, donor, socket_index, context.is_added, &mut selected_type, label_width);
        ui.allocate_ui_with_layout(
            egui::vec2(value_width, row_height),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_align(egui::Align::Min),
            |ui| {
                ui.weak(if context.is_added { "Choose a socket role" } else { "Disabled in gameplay donor" }).on_hover_text(
                    "The gameplay donor uses the native 0xFFFF disabled sentinel with no default, embedded members, or compatible plug set.",
                );
                if context.show_experimental_options && !context.is_added {
                    activate = ui.small_button("Activate Socket…").clicked();
                }
                if context.is_added {
                    options_command = draw_options(ui, socket_index, choices.is_overridden, true, context.can_remove_added, context.private_perk_socket);
                }
            },
        );
    });
    if selected_type != choices.socket_type_override {
        Some(RowCommand::ChangeRole(selected_type))
    } else if activate {
        Some(RowCommand::Activate)
    } else {
        options_command
    }
}

pub(super) fn draw_active(
    ui: &mut egui::Ui,
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
) -> Option<RowCommand> {
    let catalog = context.catalog;
    let donor = context.donor;
    let socket_index = context.socket_index;
    let socket = &donor.sockets[socket_index];
    let plug_selection_mode = context.plug_selection_mode;
    let RowChoices {
        socket_type_override,
        current_len,
        is_overridden,
        page_start,
        page_end,
        can_add,
        ..
    } = *choices;
    let current_page = &choices.current_page;
    let mut selected_type = socket_type_override;
    let mut selection = None;
    let mut options_command = None;
    ui.horizontal_top(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let available_width = ui.available_width();
        let label_width = 168.0;
        let button_count = page_end - page_start;
        let options_width = sundial::investment::authoring_button_width(ui, "…");
        let add_label = if current_len == 0 {
            "+ Set Plug"
        } else {
            "+ Add Choice"
        };
        let add_width = sundial::investment::authoring_button_width(ui, "+ Add Choice").ceil();
        // Actions have fixed columns; only choices wrap, keeping every row aligned.
        let choice_area_width =
            (available_width - label_width - options_width - add_width - spacing * 3.0).max(96.0);
        let columns = socket_choice_columns(choice_area_width, button_count);
        let button_width = ((choice_area_width - spacing * columns.saturating_sub(1) as f32)
            / columns as f32)
            .max(96.0)
            .floor() as u16;
        draw_socket_role_label(
            ui,
            catalog,
            donor,
            socket_index,
            context.is_added,
            &mut selected_type,
            label_width,
        );
        ui.allocate_ui_with_layout(
            egui::vec2(choice_area_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(choice_area_width);
                ui.horizontal_wrapped(|ui| {
                    for (choice_offset, hash) in current_page.iter().copied().enumerate() {
                        let choice_index = page_start + choice_offset;
                        if let Some(command) =
                            draw_choice(ui, context, choices, choice_index, hash, button_width)
                        {
                            selection = Some(command);
                        }
                    }
                });
                draw_paging(ui, context.page, choices);
            },
        );
        if can_add {
            let choice_index = current_len;
            match catalog.draw_supported_plug_choice_picker(
                ui,
                donor.summary.hash,
                socket.index,
                socket_type_override,
                choice_index,
                None,
                context.queries.entry(choice_index).or_default(),
                PlugChoicePickerButton {
                    text: add_label,
                    icon_hash: None,
                    tooltip: None,
                    width: add_width as u16,
                },
                plug_selection_mode,
            ) {
                Ok(Some(chosen)) => {
                    selection = Some(RowCommand::EditChoice {
                        index: choice_index,
                        hash: chosen.hash,
                    })
                }
                Ok(None) => {}
                Err(error) => context.log.push(LogEntry::error(error)),
            }
        } else {
            ui.allocate_space(egui::vec2(add_width, ui.spacing().interact_size.y));
        }
        options_command = draw_options(
            ui,
            socket.index,
            is_overridden,
            context.is_added,
            context.can_remove_added,
            context.private_perk_socket,
        );
    });
    if selected_type != socket_type_override {
        Some(RowCommand::ChangeRole(selected_type))
    } else if options_command.is_some() {
        options_command
    } else {
        selection
    }
}

fn draw_choice(
    ui: &mut egui::Ui,
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
    choice_index: usize,
    hash: u32,
    button_width: u16,
) -> Option<RowCommand> {
    let catalog = context.catalog;
    let recipe = &*context.recipe;
    let donor = context.donor;
    let socket = &donor.sockets[context.socket_index];
    let socket_type_override = choices.socket_type_override;
    let plug_selection_mode = context.plug_selection_mode;
    let mut selection = None;
    let variant = recipe
        .overrides
        .socket_plug_variants
        .iter()
        .find(|variant| {
            usize::from(variant.socket_index) == socket.index
                && usize::from(variant.choice_index) == choice_index
                && variant.source_plug_hash.parse_u32().ok() == Some(hash)
        });
    let button_label = variant
        .and_then(|variant| variant.name.clone())
        .unwrap_or_else(|| catalog.plug_label(hash, false));
    let removable = choice_index > 0;
    let tooltip = variant.map(|variant| {
        catalog.private_plug_tooltip(
            hash,
            variant
                .classification_donor_hash
                .as_ref()
                .and_then(|hash| hash.parse_u32().ok()),
            variant.name.as_deref(),
            variant.description.as_deref(),
        )
    });
    ui.allocate_ui_with_layout(
        egui::vec2(f32::from(button_width), ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
        |ui| {
            // One continuous choice tile; removal belongs inside its edge.
            ui.painter().rect_filled(
                ui.max_rect(),
                ui.visuals().widgets.inactive.corner_radius,
                ui.visuals().widgets.inactive.weak_bg_fill,
            );
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.set_min_width(f32::from(button_width));
            let remove_width = sundial::investment::authoring_button_width(ui, "×").ceil();
            let picker_width = if removable {
                button_width.saturating_sub(remove_width as u16)
            } else {
                button_width
            };
            match catalog.draw_supported_plug_choice_picker(
                ui,
                donor.summary.hash,
                socket.index,
                socket_type_override,
                choice_index,
                Some(hash),
                context.queries.entry(choice_index).or_default(),
                PlugChoicePickerButton {
                    tooltip: tooltip.as_deref(),
                    text: &button_label,
                    icon_hash: Some(hash),
                    width: picker_width,
                },
                plug_selection_mode,
            ) {
                Ok(Some(chosen)) => {
                    selection = Some(RowCommand::EditChoice {
                        index: choice_index,
                        hash: chosen.hash,
                    });
                }
                Ok(None) => {}
                Err(error) => context.log.push(LogEntry::error(error)),
            }
            if removable
                && named_control(
                    ui.add_sized(
                        [remove_width, ui.spacing().interact_size.y],
                        egui::Button::new("×").frame(false),
                    ),
                    format!("Remove additional choice: {button_label}"),
                )
                .on_hover_text("Remove this additional choice")
                .clicked()
            {
                selection = Some(RowCommand::EditChoice {
                    index: choice_index,
                    hash: None,
                });
            }
        },
    );
    selection
}

fn draw_paging(ui: &mut egui::Ui, page: &mut usize, choices: &RowChoices) {
    let RowChoices {
        page_count,
        page_start,
        page_end,
        current_len,
        ..
    } = *choices;
    if page_count > 1 {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(*page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                *page -= 1;
            }
            ui.weak(format!(
                "Choices {}–{} of {}",
                page_start + 1,
                page_end,
                current_len
            ));
            if ui
                .add_enabled(*page + 1 < page_count, egui::Button::new("Next"))
                .clicked()
            {
                *page += 1;
            }
        });
    }
}

fn draw_options(
    ui: &mut egui::Ui,
    socket_index: usize,
    is_overridden: bool,
    is_added: bool,
    can_remove_added: bool,
    private_perk_socket: &mut Option<usize>,
) -> Option<RowCommand> {
    let mut command = None;
    ui.push_id(("socket-options", socket_index), |ui| {
    let response = ui.menu_button("…", |ui| {
        if ui.button("Custom Perks…")
            .on_hover_text("Custom perk editing is planned for a future release. Reuse a saved custom perk in this socket.")
            .clicked() {
            *private_perk_socket = Some(socket_index);
            ui.close_menu();
        }
        ui.separator();
        if is_added {
            if ui.add_enabled(can_remove_added, egui::Button::new("Remove Added Socket"))
                .on_hover_text(if can_remove_added { "Remove this socket and its custom perk assignments" } else { "Remove later added sockets first to preserve socket order" })
                .clicked() {
                command = Some(RowCommand::RemoveAdded);
                ui.close_menu();
            }
        } else if ui.add_enabled(is_overridden, egui::Button::new("Reset Choices & Role"))
            .on_hover_text("Restore the base weapon's choices and role. Custom overrides on retained choices remain.")
            .clicked() {
            command = Some(RowCommand::Reset);
            ui.close_menu();
        }
    }).response.on_hover_text(if is_added {
        "Socket options: reuse custom perks or remove this added socket"
    } else {
        "Socket options: reuse custom perks or reset this socket"
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Socket Options"));
});
    command
}
