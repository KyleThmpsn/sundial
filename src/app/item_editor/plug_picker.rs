use super::*;

pub(crate) fn plug_choices_for_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    mode: PlugSelectionMode,
) -> (Vec<PlugChoice>, bool) {
    plug_choices_for_socket_type(catalog, item, socket_index, None, mode)
}

pub(crate) fn plug_choices_for_socket_type(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    socket_type_override: Option<u16>,
    mode: PlugSelectionMode,
) -> (Vec<PlugChoice>, bool) {
    let show_types = matches!(
        mode,
        PlugSelectionMode::GearType | PlugSelectionMode::GearKind | PlugSelectionMode::AnyPlug
    );
    let allowed = crate::investment::plug_selection::candidates_for_socket_type(
        catalog,
        item,
        socket_index,
        socket_type_override,
        mode,
    );
    let choices = allowed
        .iter()
        .copied()
        .map(|hash| PlugChoice {
            hash,
            label: catalog.plug_label(hash, false),
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
        preview: super::appearance::loadout(catalog, item.hash),
        preview_guarded: true,
        socket_index,
        socket_label: socket.map_or_else(
            || format!("Socket {}", socket_index + 1),
            |socket| socket.display_label(socket_index),
        ),
        current_hash,
        custom_current: false,
        current_label,
        native_default,
        native_default_label: match native_default {
            Some(NativePlugDefault::Plug(hash)) => Some(catalog.plug_label(hash, false)),
            _ => None,
        },
        choices,
        show_types,
        mode,
        scope_labels: scope_labels(item, socket.map_or("", |socket| socket.label.as_str())),
    }
}

/// Every selection scope labeled in the words of the socket and item being picked for.
pub(crate) fn scope_labels(item: &ItemDef, socket_label: &str) -> Vec<(PlugSelectionMode, String)> {
    PlugSelectionMode::ALL
        .iter()
        .map(|mode| (*mode, mode.contextual_label(item, socket_label)))
        .collect()
}

/// The Plugs Offered dropdown in a plug browser's controls, its scopes in the words of the socket
/// and item. It names itself on hover, as the workbench's filters do. Returns the scope picked
/// from its list, which can be the current one.
///
/// egui keeps one popup open at a time, so a combo box here would close the browser around it.
/// The list floats over the browser in an area of its own instead, open while it was drawn in the
/// pass before. A pick can land past the browser's edge, so a browser that closes on outside
/// clicks reopens on one.
pub(super) fn draw_plug_scope_selector(
    ui: &mut egui::Ui,
    snapshot: &PlugPickerSnapshot,
) -> Option<PlugSelectionMode> {
    if snapshot.scope_labels.is_empty() {
        return None;
    }
    let id = ui.make_persistent_id("plug-scope");
    let pass = ui.ctx().cumulative_pass_nr();
    let mut open = ui
        .data(|data| data.get_temp::<u64>(id))
        .is_some_and(|drawn| drawn + 1 >= pass);
    let current = snapshot
        .scope_labels
        .iter()
        .find(|(mode, _)| *mode == snapshot.mode)
        .map_or_else(
            || snapshot.mode.label().to_owned(),
            |(_, label)| label.clone(),
        );
    let mut picked = None;
    let mut button = draw_scope_button(ui, &current, &snapshot.scope_labels, open);
    if !open {
        button = button.on_hover_text(format!(
            "Plugs Offered: {current}\n{}",
            snapshot.mode.hint()
        ));
    }
    if button.clicked() {
        open = !open;
    }
    if open {
        // As wide as the button, as a combo box's list is.
        let frame = egui::Frame::popup(ui.style());
        let width = (button.rect.width() - frame.total_margin().sum().x).max(0.0);
        let list = egui::Area::new(id.with("list"))
            .order(egui::Order::Tooltip)
            .fixed_pos(button.rect.left_bottom())
            .show(ui.ctx(), |ui| {
                frame.show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_min_width(width);
                        for (mode, label) in &snapshot.scope_labels {
                            if ui
                                .selectable_label(*mode == snapshot.mode, label)
                                .on_hover_text(mode.hint())
                                .clicked()
                            {
                                picked = Some(*mode);
                            }
                        }
                    });
                });
            })
            .response;
        let clicked_elsewhere = ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !list.rect.contains(pos) && !button.rect.contains(pos))
        });
        // Escape shuts the list alone, so the browser around it never sees the key.
        let escaped =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if picked.is_some() || clicked_elsewhere || escaped {
            open = false;
        }
    }
    ui.data_mut(|data| {
        if open {
            data.insert_temp(id, pass);
        } else {
            data.remove::<u64>(id);
        }
    });
    picked
}

/// The dropdown button's width: its longest scope, the arrow and the padding around them.
fn scope_button_width(ui: &egui::Ui, labels: &[(PlugSelectionMode, String)]) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let widest = ui.fonts_mut(|fonts| {
        labels
            .iter()
            .map(|(_, label)| {
                fonts
                    .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
    });
    let spacing = ui.spacing();
    (widest + spacing.icon_spacing + spacing.icon_width + spacing.button_padding.x * 2.0).ceil()
}

/// The dropdown's button, drawn as egui draws a combo box: the value, then the arrow at the right.
/// It is as wide as the longest scope, so it keeps its width as the scope changes.
fn draw_scope_button(
    ui: &mut egui::Ui,
    current: &str,
    labels: &[(PlugSelectionMode, String)],
    open: bool,
) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let icon = egui::Vec2::splat(ui.spacing().icon_width);
    let gap = ui.spacing().icon_spacing;
    let width = scope_button_width(ui, labels).min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo {
        current_text_value: Some(current.to_owned()),
        ..egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), "Plugs Offered")
    });
    if ui.is_rect_visible(rect) {
        let visuals = if open {
            &ui.visuals().widgets.open
        } else {
            ui.style().interact(&response)
        };
        ui.painter().rect(
            rect.expand(visuals.expansion),
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink2(padding);
        let arrow = egui::Rect::from_center_size(
            egui::Align2::RIGHT_CENTER
                .align_size_within_rect(icon, inner)
                .center(),
            egui::vec2(icon.x * 0.7, icon.y * 0.45),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![arrow.left_top(), arrow.right_top(), arrow.center_bottom()],
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        ));
        let text = egui::WidgetText::from(current).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            inner.width() - gap - icon.x,
            egui::TextStyle::Button,
        );
        let at = egui::Align2::LEFT_CENTER
            .align_size_within_rect(text.size(), inner)
            .min;
        ui.painter().galley(at, text, visuals.text_color());
    }
    response
}

pub(crate) const SOCKET_PICKER_RESET_WIDTH: f32 = 48.0;

pub(crate) fn socket_picker_reset_width(ui: &egui::Ui) -> f32 {
    measured_button_width(ui, "Reset", SOCKET_PICKER_RESET_WIDTH)
}

pub(crate) fn measured_button_width(ui: &egui::Ui, label: &str, minimum: f32) -> f32 {
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::TextStyle::Button.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    (text.size().x + ui.spacing().button_padding.x * 2.0)
        .ceil()
        .max(minimum)
}

pub(crate) fn socket_picker_label_width(available_width: f32) -> f32 {
    (available_width * 0.28).clamp(76.0, 136.0)
}

pub(crate) fn draw_socket_picker_label(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
) -> egui::Response {
    let row_height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(width, row_height),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let mut socket_font = egui::TextStyle::Body.resolve(ui.style());
            socket_font.size = (socket_font.size - 1.0).max(1.0);
            ui.add(egui::Label::new(egui::RichText::new(label).font(socket_font)).truncate())
        },
    )
    .inner
}

pub(crate) fn draw_socket_picker_reset(
    ui: &mut egui::Ui,
    enabled: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    let row_height = ui.spacing().interact_size.y;
    let reset = ui.add_enabled(
        enabled,
        egui::Button::new("Reset").min_size(egui::vec2(socket_picker_reset_width(ui), row_height)),
    );
    if enabled {
        reset.on_hover_text(tooltip)
    } else {
        reset.on_disabled_hover_text(tooltip)
    }
}

pub(crate) fn draw_plug_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
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
        let mut requested_mode = None::<PlugSelectionMode>;
        ui.horizontal(|ui| {
            let row_height = ui.spacing().interact_size.y;
            let spacing = ui.spacing().item_spacing.x;
            let available_width = ui.available_width();
            let socket_label_width = socket_picker_label_width(available_width);
            let plug_width = (available_width
                - socket_label_width
                - socket_picker_reset_width(ui)
                - spacing * 2.0)
                .max(110.0);
            let screen = ui.ctx().content_rect();
            let popup_width = (plug_width + 140.0)
                .clamp(440.0, 680.0)
                .min((screen.width() - 24.0).max(320.0));

            draw_socket_picker_label(ui, &snapshot.socket_label, socket_label_width);
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
            let visual = super::appearance::supported(catalog, snapshot);
            if visual {
                match super::appearance::browser(
                    ui,
                    catalog,
                    popup_id.with("preview"),
                    button.clicked(),
                    query,
                    snapshot,
                    |_| false,
                ) {
                    Some(ItemEditorAction::SetPlug { hash, .. }) => selection = Some(hash),
                    Some(ItemEditorAction::SetPlugSelectionMode { mode }) => {
                        requested_mode = Some(mode);
                    }
                    _ => {}
                }
            }
            if !visual && button.clicked() {
                egui::Popup::toggle_id(ui, popup_id);
            }
            let popup_direction = popup_direction(screen, button.rect);
            let picker_style = ui.style().clone();
            crate::ui::dropdown(&button, popup_id)
                .align(popup_direction)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_style(picker_style);
                    ui.set_min_width(popup_width);
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
                        true,
                        (&mut selection, &mut requested_mode),
                        |_| false,
                    );
                });
            if selection.is_some() {
                egui::Popup::close_all(ui);
            }
            // A scope picked where its list reaches past the browser reads as a click outside it.
            if !visual && requested_mode.is_some() {
                egui::Popup::open_id(ui, popup_id);
            }

            let reset_enabled = snapshot
                .native_default
                .is_some_and(|default| snapshot.current_hash != default.value());
            let reset_tooltip = match snapshot.native_default {
                Some(NativePlugDefault::Plug(hash)) => format!(
                    "Restore this socket's default: {}",
                    snapshot
                        .native_default_label
                        .as_deref()
                        .map_or_else(|| format_hash_hex(hash), str::to_owned)
                ),
                Some(NativePlugDefault::Empty) => "Restore this socket's default: None".to_owned(),
                None => "This socket has no default".to_owned(),
            };
            let reset = draw_socket_picker_reset(ui, reset_enabled, reset_tooltip);
            if reset.clicked() {
                selection = snapshot.native_default.map(NativePlugDefault::value);
                egui::Popup::close_all(ui);
            }
        });
        // A scope change keeps the browser open; the caller rebuilds the snapshot with it.
        requested_mode
            .map(|mode| ItemEditorAction::SetPlugSelectionMode { mode })
            .or_else(|| {
                selection.map(|hash| ItemEditorAction::SetPlug {
                    socket_index: snapshot.socket_index,
                    hash,
                })
            })
    })
    .inner
}

pub(crate) fn draw_plug_icon_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
    anchor: &egui::Response,
) -> Option<ItemEditorAction> {
    icon_picker(
        ui,
        catalog,
        scope,
        query,
        snapshot,
        height,
        anchor,
        true,
        |_| false,
    )
}

/// `leading_action` draws at the right of the popup's controls. Return true from it when its
/// action should close the popup. The authoring bridge draws this picker outside Sundial's
/// inspector, so its rows offer no Inspect Definition.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_plug_icon_picker_with_action(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
    anchor: &egui::Response,
    leading_action: impl FnOnce(&mut egui::Ui) -> bool,
) -> Option<ItemEditorAction> {
    icon_picker(
        ui,
        catalog,
        scope,
        query,
        snapshot,
        height,
        anchor,
        false,
        leading_action,
    )
}

#[allow(clippy::too_many_arguments)]
fn icon_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
    anchor: &egui::Response,
    inspect: bool,
    header: impl FnOnce(&mut egui::Ui) -> bool,
) -> Option<ItemEditorAction> {
    let searchable = snapshot.choices.len() > 12;
    if !searchable {
        query.clear();
    }
    // Namespace the popup directly instead of opening a child `Ui`. A child
    // scope participates in the surrounding layout even when the popup is
    // closed, which adds a second item gap beside compact icon buttons.
    let popup_id = ui.make_persistent_id(scope).with("plug-browser");
    if super::appearance::supported(catalog, snapshot) {
        return super::appearance::browser(
            ui,
            catalog,
            popup_id.with("preview"),
            ui.is_enabled() && anchor.clicked(),
            query,
            snapshot,
            header,
        );
    }
    if ui.is_enabled() && anchor.clicked() {
        egui::Popup::toggle_id(ui, popup_id);
    }
    let screen = ui.ctx().content_rect();
    let popup_width = 520.0_f32.min((screen.width() - 24.0).max(320.0));
    let row_height = ui.spacing().interact_size.y;
    let mut selection = None::<Option<u64>>;
    let mut requested_mode = None::<PlugSelectionMode>;
    let mut header_clicked = false;
    let picker_style = ui.style().clone();
    crate::ui::dropdown(anchor, popup_id)
        .align(popup_direction(screen, anchor.rect))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_style(picker_style);
            ui.set_min_width(popup_width);
            header_clicked = draw_plug_browser_contents(
                ui,
                catalog,
                query,
                snapshot,
                height,
                row_height,
                popup_width,
                searchable,
                true,
                inspect,
                (&mut selection, &mut requested_mode),
                header,
            );
        });
    if selection.is_some() || header_clicked {
        egui::Popup::close_all(ui);
    }
    // A scope picked where its list reaches past the browser reads as a click outside it. The
    // browser stays open, and the caller gathers the plugs again with the scope.
    if requested_mode.is_some() {
        egui::Popup::open_id(ui, popup_id);
    }
    requested_mode
        .map(|mode| ItemEditorAction::SetPlugSelectionMode { mode })
        .or_else(|| {
            selection.map(|hash| ItemEditorAction::SetPlug {
                socket_index: snapshot.socket_index,
                hash,
            })
        })
}

/// Right-click Inspect Definition on one plug row.
fn draw_inspect_menu(response: &egui::Response, hash: u64) {
    crate::app::inspector::definition_context_menu(response, "Inspect Definition", hash);
}

/// Draws the browser's controls, the plug the socket holds, then the list. Fills in the plug
/// picked and the scope asked for, and returns whether the header's action asked to close the
/// popup.
///
/// The socket's choice leads with the selection fill. Where no Reset sits beside the socket
/// (`show_native_reset`), its default follows, so a reset is one click near the top. No plug is
/// the list's first row while nothing is searched for.
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
    inspect: bool,
    (selection, scope): (&mut Option<Option<u64>>, &mut Option<PlugSelectionMode>),
    header: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    ui.set_min_width(popup_width);
    let header_clicked = draw_browser_controls(
        ui,
        query,
        snapshot,
        (searchable, popup_width),
        scope,
        header,
    );
    let rows = ChoiceRows::new(catalog, snapshot, inspect, row_height.max(40.0));
    let mut pick = |picked: Option<Option<u64>>| {
        if picked.is_some() {
            *selection = picked;
        }
    };
    if snapshot.custom_current {
        rows.draw_custom(ui, &snapshot.current_label);
    } else {
        pick(rows.draw(ui, snapshot.current_hash, true));
    }
    let default = snapshot
        .native_default
        .map(NativePlugDefault::value)
        .filter(|default| show_native_reset && snapshot.current_hash != *default);
    if let Some(default) = default {
        pick(rows.draw(ui, default, false));
    }
    ui.separator();

    let pinned = |choice: Option<u64>| {
        (!snapshot.custom_current && choice == snapshot.current_hash) || Some(choice) == default
    };
    let search_query = CatalogSearchQuery::new(query);
    let mut plugs = snapshot
        .choices
        .iter()
        .filter(|choice| {
            !pinned(Some(choice.hash))
                && search_query.matches(catalog, choice.hash, &[&choice.label, &choice.type_name])
        })
        .collect::<Vec<_>>();
    plugs.sort_by_cached_key(|choice| {
        (
            std::cmp::Reverse(search_query.name_match_count(&choice.label)),
            choice.label.to_lowercase(),
            choice.hash,
        )
    });
    let mut entries = Vec::with_capacity(plugs.len() + 1);
    if search_query.is_empty() && !pinned(None) {
        entries.push(None);
    }
    entries.extend(plugs.iter().map(|choice| Some(choice.hash)));
    if !entries.is_empty() {
        let list_height = picker_list_height(entries.len(), rows.height, height.min, height.max);
        egui::ScrollArea::vertical()
            .min_scrolled_height(list_height)
            .max_height(list_height)
            .auto_shrink([false, false])
            .show_rows(ui, rows.height, entries.len(), |ui, range| {
                for index in range {
                    pick(rows.draw(ui, entries[index], false));
                }
            });
    }
    if plugs.is_empty() {
        ui.weak(if search_query.is_empty() {
            "No plugs available"
        } else {
            "No matching plugs"
        });
    }
    header_clicked
}

/// The narrowest a browser's search shares its line with the other controls.
const SEARCH_MIN_WIDTH: f32 = 160.0;

/// A browser's controls: the search when the list is long, Plugs Offered, then the caller's own
/// action. They are made in the order they read, so Tab reaches the search first. The search
/// leaves the action the width it took in the pass before, and a pass where that width changes is
/// redrawn before it shows. Where the search would be narrower than [`SEARCH_MIN_WIDTH`], it takes
/// the line above the others. Returns whether the action asked to close the browser.
fn draw_browser_controls(
    ui: &mut egui::Ui,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    (searchable, width): (bool, f32),
    scope: &mut Option<PlugSelectionMode>,
    header: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    // Fixed ids, so the search keeps its focus and the dropdown its state on either layout.
    let id = ui.make_persistent_id("plug-browser-controls");
    let action_width = ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(0.0);
    let gaps = ui.spacing().item_spacing.x * 2.0;
    let room = width - scope_button_width(ui, &snapshot.scope_labels) - action_width - gaps;
    let one_line = room >= SEARCH_MIN_WIDTH;
    if searchable && !one_line {
        draw_browser_search(ui, id.with("search"), query, width);
    }
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            if searchable && one_line {
                draw_browser_search(ui, id.with("search"), query, room);
            }
            *scope = draw_plug_scope_selector(ui, snapshot);
            let action = ui.scope(header);
            let drawn = action.response.rect.width().max(0.0);
            if (drawn - action_width).abs() > 0.5 {
                ui.data_mut(|data| data.insert_temp(id, drawn));
                ui.ctx().request_discard("plug browser action width");
            }
            action.inner
        })
        .inner
    })
    .inner
}

/// The search field. Hashes match too.
fn draw_browser_search(ui: &mut egui::Ui, id: egui::Id, query: &mut String, width: f32) {
    let search = ui.add(
        egui::TextEdit::singleline(query)
            .id(id)
            .hint_text("Search by name or hash")
            .desired_width(width),
    );
    ui.ctx()
        .accesskit_node_builder(search.id, |node| node.set_label("Search plugs"));
}

/// How a browser draws a choice: one row height for every row, the plug names more than one
/// offered plug shares, and whether a row offers Inspect Definition.
struct ChoiceRows<'a> {
    catalog: &'a Catalog,
    snapshot: &'a PlugPickerSnapshot,
    inspect: bool,
    height: f32,
    repeated: std::collections::BTreeSet<&'a str>,
}

impl<'a> ChoiceRows<'a> {
    fn new(
        catalog: &'a Catalog,
        snapshot: &'a PlugPickerSnapshot,
        inspect: bool,
        height: f32,
    ) -> Self {
        let mut named = std::collections::BTreeSet::new();
        let repeated = snapshot
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .filter(|name| !named.insert(*name))
            .collect();
        Self {
            catalog,
            snapshot,
            inspect,
            height,
            repeated,
        }
    }

    /// Draws `choice`, which is no plug when `None`, and returns it once clicked. Its description
    /// starts with Default for the socket's default, and with its hash where another offered
    /// plug has its name.
    fn draw(&self, ui: &mut egui::Ui, choice: Option<u64>, selected: bool) -> Option<Option<u64>> {
        let default = self.snapshot.native_default.map(NativePlugDefault::value) == Some(choice);
        let Some(hash) = choice else {
            let response = draw_picker_row(
                ui,
                None,
                CatalogPickerRow {
                    hash: 0,
                    primary: "None",
                    primary_max_rows: 1,
                    secondary: default.then_some("Default"),
                    icon_size: 28.0,
                    row_height: self.height,
                    selected,
                },
            );
            return response.clicked().then_some(None);
        };
        let offered = self
            .snapshot
            .choices
            .iter()
            .find(|offered| offered.hash == hash);
        let name = offered.map_or_else(
            || self.catalog.plug_label(hash, false),
            |offered| offered.label.clone(),
        );
        let type_name = offered
            .filter(|_| self.snapshot.show_types)
            .map_or("", |offered| offered.type_name.as_str());
        let description = self
            .catalog
            .description(hash)
            .map(single_line_text)
            .unwrap_or_default();
        let mut secondary = picker_secondary_text(type_name, &description);
        if self.repeated.contains(name.as_str()) {
            secondary = picker_secondary_text(&format_hash_hex(hash), &secondary);
        }
        if default {
            secondary = picker_secondary_text("Default", &secondary);
        }
        let response = draw_catalog_picker_row(
            ui,
            self.catalog,
            CatalogPickerRow {
                hash,
                primary: &name,
                primary_max_rows: 1,
                secondary: (!secondary.is_empty()).then_some(secondary.as_str()),
                icon_size: 28.0,
                row_height: self.height,
                selected,
            },
        );
        let response = catalog_item_tooltip(response, self.catalog, hash);
        if self.inspect {
            draw_inspect_menu(&response, hash);
        }
        response.clicked().then_some(Some(hash))
    }

    /// The custom perk the socket holds, which has no catalog row of its own.
    fn draw_custom(&self, ui: &mut egui::Ui, name: &str) {
        draw_picker_row(
            ui,
            None,
            CatalogPickerRow {
                hash: 0,
                primary: name,
                primary_max_rows: 1,
                secondary: None,
                icon_size: 28.0,
                row_height: self.height,
                selected: true,
            },
        );
    }
}
