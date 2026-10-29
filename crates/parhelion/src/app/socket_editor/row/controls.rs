//! Socket role and choice controls; recipe writes are deferred to commands.
use super::super::{
    LogEntry, PlugChoicePickerButton, draw_socket_role_label, named_control, socket_choice_columns,
};
use super::{RowChoices, RowCommand, SocketRowContext, SocketView};
use sundial::investment::PlugChoicePickerOptions;

/// Width of the socket role column, shared by every row so the choices line up.
const ROLE_WIDTH: f32 = 168.0;

/// The socket's role, which the author can change.
fn draw_role(
    ui: &mut egui::Ui,
    context: &SocketRowContext<'_>,
    role: &mut Option<u16>,
    width: f32,
) {
    draw_socket_role_label(
        ui,
        context.catalog,
        context.donor,
        context.socket_index,
        context.is_added,
        role,
        width,
    );
}

pub(super) fn draw_disabled(
    ui: &mut egui::Ui,
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
) -> Option<RowCommand> {
    if !context.is_added
        && choices.socket_type_override == Some(u16::MAX)
        && context.donor.sockets[context.socket_index].socket_type != u16::MAX
        && choices.current_len == 0
    {
        let mut restore = false;
        ui.horizontal(|ui| {
            ui.weak(format!("Socket {} Removed", context.socket_index + 1));
            restore = ui.button("Restore Socket").clicked();
        });
        return restore.then_some(RowCommand::Reset);
    }
    let mut selected_type = choices.socket_type_override;
    let mut activate = false;
    let mut options_command = None;
    ui.horizontal(|ui| {
        let row_height = ui.spacing().interact_size.y;
        let spacing = ui.spacing().item_spacing.x;
        let available_width = ui.available_width();
        let label_width = ROLE_WIDTH;
        let value_width = (available_width - label_width - spacing).max(110.0);
        draw_role(ui, context, &mut selected_type, label_width);
        ui.allocate_ui_with_layout(
            egui::vec2(value_width, row_height),
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
            |ui| {
                ui.weak(if context.is_added {
                    "Choose a socket role"
                } else {
                    "Disabled in base weapon"
                });
                if context.show_experimental_options && !context.is_added {
                    activate = ui.small_button("Activate Socket…").clicked();
                }
                if context.is_added {
                    options_command = draw_options(ui, context, choices);
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
    let RowChoices {
        socket_type_override,
        current_len,
        view,
        page_start,
        page_end,
        can_add,
        ..
    } = *choices;
    let randomized = view == SocketView::Randomized;
    let current_page = &choices.current_page;
    let mut selected_type = socket_type_override;
    let mut selection = None;
    let mut options_command = None;
    let mut custom_choice = None;
    // A gear perk is built on the plug it replaces, so on gear a custom perk never adds a choice.
    // A custom perk added here joins the end of the column, as a randomized choice of a lane that
    // rolls and a reusable one of a lane that does not, so it is offered in that list's view.
    let offer_custom_perk = context.recipe.kind.is_weapon() && randomized == choices.rolls();
    let shows_base_roll = randomized
        && current_len > 0
        && page_end == current_len
        && choices
            .stock_roll
            .as_ref()
            .is_some_and(|roll| !roll.randomized.is_empty());
    ui.horizontal_top(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let available_width = ui.available_width();
        let label_width = ROLE_WIDTH;
        let button_count = page_end - page_start;
        let options_width =
            sundial::investment::authoring_button_width(ui, crate::app::style::MORE);
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
        draw_role(ui, context, &mut selected_type, label_width);
        ui.allocate_ui_with_layout(
            egui::vec2(choice_area_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(choice_area_width);
                if !current_page.is_empty() {
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
                }
                if shows_base_roll
                    && let Some(command) = draw_base_roll(ui, context, choices, choice_area_width)
                {
                    selection = Some(command);
                }
                if randomized && current_len == 0 {
                    ui.weak("Set a reusable plug first.");
                }
                // A build that replaces a random socket's roll with its reusable plugs needs a
                // default for it. Keep that warning in this socket's choice area, leaving it
                // available as a drop target.
                if !randomized
                    && current_len == 0
                    && !choices.is_overridden
                    && choices.stock_roll.as_ref().is_some_and(|roll| !roll.kept)
                    && !egui::DragAndDrop::has_payload_of_type::<ChoiceDrag>(ui.ctx())
                {
                    let message = "Rolls at random with no default. Set a plug to build.";
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message).color(ui.visuals().warn_fg_color),
                            )
                            .truncate(),
                        );
                    });
                }
                if current_page.is_empty()
                    && can_add
                    && let Some(command) = draw_empty_choice_drop(
                        ui,
                        donor.summary.hash,
                        socket.index,
                        choice_area_width,
                    )
                {
                    selection = Some(command);
                }
                draw_paging(ui, context.page, choices);
            },
        );
        if can_add {
            let choice_index = current_len;
            // Quiet like the row's role and menu, so a row leads with its perks. It takes its
            // frame on hover and focus.
            let picked = ui
                .scope(|ui| {
                    crate::app::style::quiet(ui);
                    catalog.draw_supported_plug_choice_picker(
                        ui,
                        context.queries.entry(choice_index).or_default(),
                        PlugChoicePickerOptions {
                            preview: context.preview,
                            donor_hash: donor.summary.hash,
                            socket_index: socket.index,
                            socket_type_override,
                            choice_index,
                            current_hash: None,
                            mode: &mut *context.plug_selection_mode,
                            button: PlugChoicePickerButton {
                                text: add_label,
                                icon_hash: None,
                                icon_override: None,
                                tooltip: None,
                                width: add_width as u16,
                            },
                        },
                        |ui| {
                            let clicked = offer_custom_perk && draw_custom_perk_action(ui);
                            if clicked {
                                custom_choice = Some(choice_index);
                            }
                            clicked
                        },
                    )
                })
                .inner;
            match picked {
                Ok(Some(chosen)) => {
                    selection = Some(RowCommand::EditChoice {
                        index: choice_index,
                        hash: chosen.hash,
                    });
                }
                Ok(None) => {}
                Err(error) => context.log.push(LogEntry::error(error)),
            }
        } else {
            ui.allocate_space(egui::vec2(add_width, ui.spacing().interact_size.y));
        }
        options_command = draw_options(ui, context, choices);
    });
    if let Some(choice) = custom_choice {
        *context.perk_request = Some(crate::app::custom_perks::workbench::Request::SelectChoice {
            socket: socket_index,
            choice,
        });
    }
    if selected_type != socket_type_override {
        Some(RowCommand::ChangeRole(selected_type))
    } else if options_command.is_some() {
        options_command
    } else {
        selection
    }
}

/// Identifies the choice being dragged. The plug travels with it so a drop on another socket
/// can place it there, which a socket index alone could not express.
#[derive(Clone, Copy, Eq, PartialEq)]
struct ChoiceDrag {
    socket_index: usize,
    choice_index: usize,
    hash: u32,
}

/// Drag and drop identifiers must survive between frames, so they are built from the recipe
/// rather than from `Ui::id`, which is derived from how many widgets came before it.
fn choice_drag_id(donor_hash: u32, socket_index: usize, choice_index: usize) -> egui::Id {
    egui::Id::new(("socket-choice-drag", donor_hash, socket_index, choice_index))
}

fn choice_drop_id(donor_hash: u32, socket_index: usize, choice_index: usize) -> egui::Id {
    egui::Id::new(("socket-choice-drop", donor_hash, socket_index, choice_index))
}

/// A socket with no choices has no tile to drop onto, so it offers its whole choice area while a
/// drag is in flight. Without this a blank socket is the one place a perk cannot be dropped, even
/// though the drag grip invites it.
fn draw_empty_choice_drop(
    ui: &mut egui::Ui,
    donor_hash: u32,
    socket_index: usize,
    width: f32,
) -> Option<RowCommand> {
    if !egui::DragAndDrop::has_payload_of_type::<ChoiceDrag>(ui.ctx()) {
        return None;
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Sense::hover(),
    );
    let drop = ui.interact(
        rect,
        choice_drop_id(donor_hash, socket_index, 0),
        egui::Sense::hover(),
    );
    let visuals = if drop.dnd_hover_payload::<ChoiceDrag>().is_some() {
        ui.visuals().widgets.active
    } else {
        ui.visuals().widgets.inactive
    };
    ui.painter().rect_stroke(
        rect,
        visuals.corner_radius,
        visuals.fg_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop a perk here",
        egui::TextStyle::Body.resolve(ui.style()),
        visuals.fg_stroke.color,
    );
    // The socket is empty, so the dropped perk becomes its default. As with a drop onto another
    // socket's tile, the source keeps its own copy.
    drop.dnd_release_payload::<ChoiceDrag>()
        .map(|dragged| RowCommand::EditChoice {
            index: 0,
            hash: Some(dragged.hash),
        })
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
    // A choice leaves the socket, so a reusable choice of a lane that rolls leaves both lists.
    let remove_hint = if !choices.rolls() {
        "Remove this additional choice"
    } else if choice_index < choices.reusable {
        "Remove from Reusable and Randomized"
    } else {
        "Remove from Randomized"
    };
    let icon_override = crate::artwork_browser::preview::icon(
        ui,
        catalog,
        variant.and_then(|variant| variant.icon.as_ref()),
    );
    // An imported ornament takes the place of a stock one and shows its own render and rarity.
    #[cfg(feature = "d2-model-importer")]
    let ornament =
        crate::app::imported_view::ornament(recipe, ui.ctx(), (socket.index, choice_index, hash));
    #[cfg(feature = "d2-model-importer")]
    let (icon_override, rarity) = (
        icon_override.or(ornament.as_ref().map(|ornament| ornament.icon)),
        ornament.and_then(|ornament| ornament.rarity),
    );
    #[cfg(not(feature = "d2-model-importer"))]
    let rarity = None;
    let tooltip = variant.map(|variant| sundial::investment::PlugTooltip {
        classification_hash: variant
            .classification_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok()),
        name: variant.name.as_deref(),
        description: variant.description.as_deref(),
        rarity,
    });
    let tile = ui.allocate_ui_with_layout(
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
            let grip = egui_phosphor::regular::DOTS_SIX_VERTICAL;
            // The bare glyph with no button padding, so the grip sits against its perk and takes
            // as little width from the label as it can.
            let grip_width = (sundial::investment::authoring_button_width(ui, grip)
                - ui.spacing().button_padding.x * 2.0)
                .ceil()
                .max(1.0);
            let picker_width = button_width
                .saturating_sub(if removable { remove_width as u16 } else { 0 })
                .saturating_sub(grip_width as u16);
            {
                let drag_id = choice_drag_id(donor.summary.hash, socket.index, choice_index);
                ui.dnd_drag_source(
                    drag_id,
                    ChoiceDrag {
                        socket_index: socket.index,
                        choice_index,
                        hash,
                    },
                    |ui| {
                        ui.add_sized(
                            [grip_width, ui.spacing().interact_size.y],
                            egui::Label::new(grip).selectable(false),
                        );
                    },
                )
                .response
                .on_hover_text("Drag to reorder or move to another socket.");
                // The grip alone is too small to follow, so the perk trails the pointer instead.
                if ui.ctx().is_being_dragged(drag_id)
                    && let Some(pointer) = ui.ctx().pointer_interact_pos()
                {
                    egui::Area::new(drag_id.with("preview"))
                        .order(egui::Order::Tooltip)
                        .fixed_pos(pointer + egui::vec2(12.0, 8.0))
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label(button_label.clone());
                            });
                        });
                }
            }
            match catalog.draw_supported_plug_choice_picker(
                ui,
                context.queries.entry(choice_index).or_default(),
                PlugChoicePickerOptions {
                    preview: context.preview,
                    donor_hash: donor.summary.hash,
                    socket_index: socket.index,
                    socket_type_override,
                    choice_index,
                    current_hash: variant.is_none().then_some(hash),
                    mode: &mut *context.plug_selection_mode,
                    button: PlugChoicePickerButton {
                        tooltip,
                        text: &button_label,
                        icon_hash: Some(hash),
                        icon_override,
                        width: picker_width,
                    },
                },
                |ui| {
                    let clicked = draw_custom_perk_action(ui);
                    if clicked {
                        *context.perk_request =
                            Some(crate::app::custom_perks::workbench::Request::SelectChoice {
                                socket: socket.index,
                                choice: choice_index,
                            });
                    }
                    clicked
                },
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
                .on_hover_text(remove_hint)
                .clicked()
            {
                selection = Some(RowCommand::EditChoice {
                    index: choice_index,
                    hash: None,
                });
            }
        },
    );
    let drop = ui.interact(
        tile.response.rect,
        choice_drop_id(donor.summary.hash, socket.index, choice_index),
        egui::Sense::hover(),
    );
    let elsewhere = |dragged: &ChoiceDrag| {
        dragged.socket_index != socket.index || dragged.choice_index != choice_index
    };
    // Show where the perk would land while a drag is in flight.
    if drop
        .dnd_hover_payload::<ChoiceDrag>()
        .is_some_and(|dragged| elsewhere(&dragged))
    {
        ui.painter().rect_stroke(
            tile.response.rect,
            ui.visuals().widgets.active.corner_radius,
            ui.visuals().widgets.active.fg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    if let Some(dragged) = drop.dnd_release_payload::<ChoiceDrag>()
        && elsewhere(&dragged)
    {
        // Within a socket the order is what matters. Across sockets there is no shared order,
        // so the perk is placed in the socket it was dropped on and the source keeps its own.
        selection = Some(if dragged.socket_index == socket.index {
            RowCommand::MoveChoice {
                from: dragged.choice_index,
                to: choice_index,
            }
        } else {
            RowCommand::EditChoice {
                index: choice_index,
                hash: Some(dragged.hash),
            }
        });
    }
    choice_menu(ui, &tile.response, choice_index, choices).or(selection)
}

/// The base's randomized plugs past a weapon lane's choices, on a lane that does not roll from its
/// own choices. The build drops them. They form a group of their own under the weapon's perks:
/// a heading with Add to Randomized Plug Set, which makes them the lane's own randomized choices,
/// then the plugs, outlined rather than filled and laid out in their own columns.
fn draw_base_roll(
    ui: &mut egui::Ui,
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
    width: f32,
) -> Option<RowCommand> {
    let roll = choices.stock_roll.as_ref()?;
    ui.add_space(ui.spacing().item_spacing.y);
    // A plain row, not a wrapping one: a scoped button in a wrapping line never moves to the next
    // line, and its label wraps per glyph in what is left.
    let command = ui
        .horizontal(|ui| {
            ui.add(
                egui::Label::new(egui::RichText::new("Base Weapon Roll · Not Built").weak())
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
            ui.scope(|ui| {
                crate::app::style::quiet(ui);
                ui.add(
                    egui::Button::new("Add to Randomized Plug Set")
                        .wrap_mode(egui::TextWrapMode::Truncate),
                )
            })
            .inner
            .clicked()
            .then_some(RowCommand::RollFromBase)
        })
        .inner;
    let spacing = ui.spacing().item_spacing.x;
    let columns = socket_choice_columns(width, roll.randomized.len());
    let button_width = ((width - spacing * columns.saturating_sub(1) as f32) / columns as f32)
        .max(96.0)
        .floor() as u16;
    let socket_index = context.donor.sockets[context.socket_index].index;
    ui.horizontal_wrapped(|ui| {
        for (offset, &hash) in roll.randomized.iter().enumerate() {
            let label = context.catalog.plug_label(hash, false);
            ui.allocate_ui_with_layout(
                egui::vec2(f32::from(button_width), ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
                |ui| {
                    ui.painter().rect_stroke(
                        ui.max_rect(),
                        ui.visuals().widgets.inactive.corner_radius,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    ui.add_enabled_ui(false, |ui| {
                        // The picker's button would fill the tile and hide the outline.
                        crate::app::style::quiet(ui);
                        context.catalog.draw_supported_plug_choice_picker(
                            ui,
                            &mut String::new(),
                            PlugChoicePickerOptions {
                                preview: context.preview,
                                donor_hash: context.donor.summary.hash,
                                socket_index,
                                socket_type_override: choices.socket_type_override,
                                choice_index: choices.current_len + offset,
                                current_hash: Some(hash),
                                mode: &mut *context.plug_selection_mode,
                                button: PlugChoicePickerButton {
                                    text: &label,
                                    icon_hash: Some(hash),
                                    icon_override: None,
                                    tooltip: None,
                                    width: button_width,
                                },
                            },
                            |_| false,
                        )
                    })
                },
            );
        }
    });
    command
}

/// The plug browser's custom perk action, at the right of its controls.
fn draw_custom_perk_action(ui: &mut egui::Ui) -> bool {
    ui.button("Use Custom Perk…")
        .on_hover_text("Choose or create a custom perk")
        .clicked()
}

fn choice_menu(
    ui: &mut egui::Ui,
    response: &egui::Response,
    choice_index: usize,
    choices: &RowChoices,
) -> Option<RowCommand> {
    let mut selection = None;
    let removable = choice_index > 0;
    // Every choice rolls once a lane rolls, so these change only whether it is reusable too. The
    // default always stays reusable, and a choice left randomized alone starts a roll on a lane
    // that has none.
    let split = match choices.view {
        SocketView::Randomized => (choice_index >= choices.reusable).then_some((
            "Make Reusable",
            true,
            "Offer it in Reusable too. It keeps rolling.",
        )),
        SocketView::Reusable => (removable && choices.can_roll).then_some((
            "Remove from Reusable",
            false,
            if choices.rolls() {
                "It keeps rolling in Randomized."
            } else {
                "It stays in Randomized, so the socket rolls."
            },
        )),
    };
    // The picker button owns pointer clicks inside the tile. Use its bounds
    // to open the context menu over that child as well.
    let menu_id = response.id.with("socket-choice-context-menu");
    let open = if ui.rect_contains_pointer(response.rect)
        && ui.input(|input| input.pointer.secondary_clicked())
    {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    egui::Popup::context_menu(response)
        .id(menu_id)
        .open_memory(open)
        .show(|ui| {
            if ui.button("Edit as Custom Perk…").clicked() {
                selection = Some(RowCommand::EditPerk(choice_index));
                ui.close();
            }
            if removable {
                ui.separator();
            }
            if removable && ui.button("Make Default").clicked() {
                selection = Some(RowCommand::MakeDefault(choice_index));
                ui.close();
            }
            if let Some((label, reusable, hint)) = split
                && ui.button(label).on_hover_text(hint).clicked()
            {
                selection = Some(RowCommand::SetReusable {
                    index: choice_index,
                    reusable,
                });
                ui.close();
            }
            if removable && ui.button("Remove Choice").clicked() {
                selection = Some(RowCommand::EditChoice {
                    index: choice_index,
                    hash: None,
                });
                ui.close();
            }
            if !removable {
                ui.weak("Default Choice");
            }
        });
    selection
}

fn draw_paging(ui: &mut egui::Ui, page: &mut usize, choices: &RowChoices) {
    let RowChoices {
        page_count,
        page_start,
        page_end,
        shown_len,
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
                shown_len
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
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
) -> Option<RowCommand> {
    let socket_index = context.socket_index;
    let is_overridden = choices.is_overridden;
    let is_added = context.is_added;
    let can_remove_added = context.can_remove_added;
    let (reset, reset_hint, menu_hint) = if is_added {
        ("", "", "Remove this socket")
    } else {
        (
            "Reset Choices & Role",
            "Restore the base item's choices and role. Kept choices keep their overrides.",
            "Remove or reset this socket",
        )
    };
    let mut command = None;
    ui.push_id(("socket-options", socket_index), |ui| {
        crate::app::style::quiet(ui);
        let icon = crate::app::style::light_icon(ui, crate::app::style::MORE);
        let response = ui
            .menu_button(icon, |ui| {
                if is_added {
                    if ui
                        .add_enabled(can_remove_added, egui::Button::new("Remove Socket"))
                        .on_hover_text(if can_remove_added {
                            "Remove this socket and its custom perks"
                        } else {
                            "Remove later added sockets first"
                        })
                        .clicked()
                    {
                        command = Some(RowCommand::Remove);
                        ui.close();
                    }
                } else {
                    if ui
                        .button("Remove Socket")
                        .on_hover_text(
                            "Remove its choices and custom perks. Other sockets stay in place.",
                        )
                        .clicked()
                    {
                        command = Some(RowCommand::Remove);
                        ui.close();
                    }
                    if ui
                        .add_enabled(is_overridden, egui::Button::new(reset))
                        .on_hover_text(reset_hint)
                        .clicked()
                    {
                        command = Some(RowCommand::Reset);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(menu_hint);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Socket Options")
        });
    });
    command
}
