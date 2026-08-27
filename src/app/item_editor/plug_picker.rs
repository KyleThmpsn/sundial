use super::*;

pub(crate) fn plug_choices_for_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    mode: PlugSelectionMode,
) -> (Vec<PlugChoice>, bool) {
    let show_types = matches!(
        mode,
        PlugSelectionMode::GearType | PlugSelectionMode::AnyPlug
    );
    let Some(socket) = item.sockets.get(socket_index) else {
        return (Vec::new(), show_types);
    };
    let allowed = match mode {
        PlugSelectionMode::Supported => catalog.socket_options(socket).to_vec(),
        PlugSelectionMode::SocketAndGearType => catalog
            .socket_and_gear_type_options(item, socket_index)
            .to_vec(),
        PlugSelectionMode::MatchingSocketType => {
            catalog.socket_type_options(socket.socket_type).to_vec()
        }
        PlugSelectionMode::GearType => catalog.gear_type_options(item, socket_index),
        PlugSelectionMode::AnyPlug => catalog.all_plug_options().to_vec(),
    };
    let choices = allowed
        .into_iter()
        .map(|hash| PlugChoice {
            hash,
            label: catalog.plug_label(hash, true),
            type_name: if show_types {
                catalog
                    .plug_type_name(hash)
                    .unwrap_or("Unknown type")
                    .to_owned()
            } else {
                String::new()
            },
        })
        .collect();
    (choices, show_types)
}

pub(crate) fn plug_picker_snapshot(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current_hash: Option<u64>,
    current_label: String,
    native_default: Option<NativePlugDefault>,
    mode: PlugSelectionMode,
) -> PlugPickerSnapshot {
    let socket = item.sockets.get(socket_index);
    let (choices, show_types) = plug_choices_for_socket(catalog, item, socket_index, mode);
    PlugPickerSnapshot {
        socket_index,
        socket_label: socket.map_or_else(
            || format!("Socket {}", socket_index + 1),
            |socket| socket.display_label(socket_index),
        ),
        current_hash,
        current_label,
        native_default,
        native_default_label: match native_default {
            Some(NativePlugDefault::Plug(hash)) => Some(catalog.plug_label(hash, true)),
            _ => None,
        },
        choices,
        show_types,
    }
}

pub(crate) fn draw_plug_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
) -> Option<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let searchable = snapshot.choices.len() > 12;
        if !searchable {
            query.clear();
        }
        let mut selection = None::<Option<u64>>;
        ui.horizontal(|ui| {
            let row_height = ui.spacing().interact_size.y;
            let spacing = ui.spacing().item_spacing.x;
            let available_width = ui.available_width();
            let socket_label_width = (available_width * 0.28).clamp(76.0, 104.0);
            let reset_button_width = 48.0;
            let plug_width =
                (available_width - socket_label_width - reset_button_width - spacing * 2.0)
                    .max(110.0);
            let screen = ui.ctx().screen_rect();
            let popup_width = (plug_width + 140.0)
                .clamp(440.0, 680.0)
                .min((screen.width() - 24.0).max(320.0));

            ui.allocate_ui_with_layout(
                egui::vec2(socket_label_width, row_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let mut socket_font = egui::TextStyle::Body.resolve(ui.style());
                    socket_font.size = (socket_font.size - 1.0).max(1.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&snapshot.socket_label).font(socket_font),
                        )
                        .truncate(),
                    )
                },
            )
            .inner
            .on_hover_text(&snapshot.socket_label);
            let popup_id = ui.make_persistent_id("plug-browser");
            let button = ui
                .allocate_ui_with_layout(
                    egui::vec2(plug_width, row_height),
                    egui::Layout::left_to_right(egui::Align::Center)
                        .with_main_align(egui::Align::Min),
                    |ui| {
                        let button = snapshot.current_hash.map_or_else(
                            || egui::Button::new(&snapshot.current_label),
                            |hash| {
                                catalog_button(
                                    ui,
                                    catalog,
                                    hash,
                                    &snapshot.current_label,
                                    (row_height - 6.0).max(16.0),
                                )
                            },
                        );
                        ui.add(
                            button
                                .truncate()
                                .min_size(egui::vec2(plug_width, row_height)),
                        )
                    },
                )
                .inner;
            let button = if let Some(hash) = snapshot.current_hash {
                catalog_item_tooltip(button, catalog, hash)
            } else {
                button
            };
            if button.clicked() {
                ui.memory_mut(|memory| memory.toggle_popup(popup_id));
            }
            let popup_direction = popup_direction(screen, button.rect);
            egui::popup::popup_above_or_below_widget(
                ui,
                popup_id,
                &button,
                popup_direction,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    draw_plug_browser_contents(
                        ui,
                        catalog,
                        query,
                        snapshot,
                        height,
                        row_height,
                        popup_width,
                        searchable,
                        false,
                        &mut selection,
                    );
                },
            );
            if selection.is_some() {
                ui.memory_mut(egui::Memory::close_popup);
            }

            let reset_enabled = snapshot
                .native_default
                .is_some_and(|default| snapshot.current_hash != default.value());
            let reset = ui.add_enabled(
                reset_enabled,
                egui::Button::new("Reset").min_size(egui::vec2(reset_button_width, row_height)),
            );
            let reset_tooltip = match snapshot.native_default {
                Some(NativePlugDefault::Plug(hash)) => format!(
                    "Restore this socket's native default: {}",
                    snapshot
                        .native_default_label
                        .as_deref()
                        .map_or_else(|| format_hash_hex(hash), str::to_owned)
                ),
                Some(NativePlugDefault::Empty) => {
                    "Restore this socket's native default: None".to_owned()
                }
                None => "No native default is available for this socket".to_owned(),
            };
            let reset = if reset_enabled {
                reset.on_hover_text(reset_tooltip)
            } else {
                reset.on_disabled_hover_text(reset_tooltip)
            };
            if reset.clicked() {
                selection = snapshot.native_default.map(NativePlugDefault::value);
                ui.memory_mut(egui::Memory::close_popup);
            }
        });
        selection.map(|hash| ItemEditorAction::SetPlug {
            socket_index: snapshot.socket_index,
            hash,
        })
    })
    .inner
}

pub(crate) fn draw_plug_icon_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
    anchor: &egui::Response,
) -> Option<ItemEditorAction> {
    let searchable = snapshot.choices.len() > 12;
    if !searchable {
        query.clear();
    }
    // Namespace the popup directly instead of opening a child `Ui`. A child
    // scope participates in the surrounding layout even when the popup is
    // closed, which adds a second item gap beside compact icon buttons.
    let popup_id = ui.make_persistent_id(scope).with("plug-browser");
    if ui.is_enabled() && anchor.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    let screen = ui.ctx().screen_rect();
    let popup_width = 520.0_f32.min((screen.width() - 24.0).max(320.0));
    let row_height = ui.spacing().interact_size.y;
    let mut selection = None::<Option<u64>>;
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        anchor,
        popup_direction(screen, anchor.rect),
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            draw_plug_browser_contents(
                ui,
                catalog,
                query,
                snapshot,
                height,
                row_height,
                popup_width,
                searchable,
                true,
                &mut selection,
            );
        },
    );
    if selection.is_some() {
        ui.memory_mut(egui::Memory::close_popup);
    }
    selection.map(|hash| ItemEditorAction::SetPlug {
        socket_index: snapshot.socket_index,
        hash,
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_plug_browser_contents(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
    row_height: f32,
    popup_width: f32,
    searchable: bool,
    show_native_reset: bool,
    selection: &mut Option<Option<u64>>,
) {
    ui.set_min_width(popup_width);
    if searchable {
        ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Search name, type, source, description, or hash…")
                .desired_width(popup_width - 20.0),
        );
        ui.separator();
    }
    if ui
        .selectable_label(snapshot.current_hash.is_none(), "None")
        .clicked()
    {
        *selection = Some(None);
    }
    if let Some(hash) = snapshot.current_hash {
        let current_choice = snapshot.choices.iter().find(|choice| choice.hash == hash);
        let current_label = current_choice.map_or_else(
            || format!("{}  (custom/current)", catalog.plug_label(hash, true)),
            |choice| format!("{}  (current)", choice.label),
        );
        let current_type = current_choice
            .filter(|_| snapshot.show_types)
            .map(|choice| choice.type_name.as_str())
            .unwrap_or_default();
        let current_description = catalog
            .description(hash)
            .map(single_line_text)
            .unwrap_or_default();
        let current_secondary = picker_secondary_text(current_type, &current_description);
        let current_row_height = row_height.max(52.0);
        let response = draw_catalog_picker_row(
            ui,
            catalog,
            CatalogPickerRow {
                hash,
                primary: &current_label,
                primary_max_rows: 2,
                secondary: (!current_secondary.is_empty()).then_some(current_secondary.as_str()),
                icon_size: 28.0,
                row_height: current_row_height,
                selected: true,
            },
        );
        if catalog_item_tooltip(response, catalog, hash).clicked() {
            *selection = Some(Some(hash));
        }
    }
    ui.separator();

    let search_query = CatalogSearchQuery::new(query);
    let mut visible = snapshot
        .choices
        .iter()
        .filter(|choice| {
            snapshot.current_hash != Some(choice.hash)
                && search_query.matches(catalog, choice.hash, &[&choice.label, &choice.type_name])
        })
        .collect::<Vec<_>>();
    visible.sort_by_cached_key(|choice| {
        (
            std::cmp::Reverse(search_query.name_match_count(&choice.label)),
            choice.label.to_lowercase(),
            choice.hash,
        )
    });
    if visible.is_empty() {
        ui.label(
            egui::RichText::new(if searchable {
                "No matching plugs found"
            } else {
                "No plugs available"
            })
            .weak(),
        );
    } else {
        let option_row_height = row_height.max(40.0);
        let picker_height =
            picker_list_height(visible.len(), option_row_height, height.min, height.max);
        egui::ScrollArea::vertical()
            .min_scrolled_height(picker_height)
            .max_height(picker_height)
            .auto_shrink([false, false])
            .show_rows(ui, option_row_height, visible.len(), |ui, rows| {
                for index in rows {
                    let choice = visible[index];
                    let type_name = if snapshot.show_types {
                        choice.type_name.as_str()
                    } else {
                        ""
                    };
                    let description = catalog
                        .description(choice.hash)
                        .map(single_line_text)
                        .unwrap_or_default();
                    let secondary = picker_secondary_text(type_name, &description);
                    let response = draw_catalog_picker_row(
                        ui,
                        catalog,
                        CatalogPickerRow {
                            hash: choice.hash,
                            primary: &choice.label,
                            primary_max_rows: 1,
                            secondary: (!secondary.is_empty()).then_some(secondary.as_str()),
                            icon_size: 28.0,
                            row_height: option_row_height,
                            selected: snapshot.current_hash == Some(choice.hash),
                        },
                    );
                    if catalog_item_tooltip(response, catalog, choice.hash).clicked() {
                        *selection = Some(Some(choice.hash));
                    }
                }
            });
    }

    let reset_enabled = show_native_reset
        && snapshot
            .native_default
            .is_some_and(|default| snapshot.current_hash != default.value());
    if reset_enabled {
        ui.separator();
        let label = match snapshot.native_default {
            Some(NativePlugDefault::Plug(hash)) => format!(
                "Reset to native default: {}",
                snapshot
                    .native_default_label
                    .as_deref()
                    .map_or_else(|| format_hash_hex(hash), str::to_owned)
            ),
            Some(NativePlugDefault::Empty) => "Reset to native default: None".to_owned(),
            None => String::new(),
        };
        if ui.button(label).clicked() {
            *selection = snapshot.native_default.map(NativePlugDefault::value);
        }
    }
}
