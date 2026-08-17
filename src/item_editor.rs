use std::hash::Hash;

use eframe::egui;

const POWER_PER_LEVEL: i64 = 10;
const MINIMUM_POWERED_ITEM_POWER: i64 = 750;
const MAXIMUM_POWERED_ITEM_POWER: i64 = 1060;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativePlugDefault {
    Plug(u64),
    Empty,
}

impl NativePlugDefault {
    pub(crate) const fn value(self) -> Option<u64> {
        match self {
            Self::Plug(hash) => Some(hash),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemEditorAction {
    SetDefinition {
        hash: u64,
    },
    ClearDefinition,
    SetLevel {
        level: i64,
    },
    SetQuantity {
        quantity: i64,
    },
    SetPlug {
        socket_index: usize,
        hash: Option<u64>,
    },
}

pub(crate) enum DefinitionSummary<'a> {
    Empty,
    Known { name: &'a str, hash: &'a str },
    Unknown { hash: &'a str },
}

pub(crate) struct ItemHeader<'a> {
    pub label: Option<&'a str>,
    pub definition: DefinitionSummary<'a>,
    pub valid: bool,
    pub invalid_message: &'a str,
}

pub(crate) fn draw_item_header(ui: &mut egui::Ui, header: ItemHeader<'_>) {
    draw_item_header_contents(ui, header);
}

pub(crate) fn draw_item_header_with_trailing(
    ui: &mut egui::Ui,
    header: ItemHeader<'_>,
    trailing: impl FnOnce(&mut egui::Ui),
) {
    let row_height = ui.spacing().interact_size.y;
    let trailing_width = 64.0;
    let spacing = ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        let header_width = (ui.available_width() - trailing_width - spacing).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(header_width, row_height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| draw_item_header_contents(ui, header),
        );
        ui.allocate_ui_with_layout(
            egui::vec2(trailing_width, row_height),
            egui::Layout::right_to_left(egui::Align::Center),
            trailing,
        );
    });
}

fn draw_item_header_contents(ui: &mut egui::Ui, header: ItemHeader<'_>) {
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let monospace_font = egui::TextStyle::Monospace.resolve(ui.style());
    let text_color = ui.visuals().text_color();
    let strong_color = ui.visuals().strong_text_color();
    let weak_color = ui.visuals().weak_text_color();
    let error_color = ui.visuals().error_fg_color;
    let item_spacing = ui.spacing().item_spacing.x;
    let mut job = egui::text::LayoutJob {
        break_on_newline: false,
        ..Default::default()
    };
    let mut full_text = String::new();

    let body = egui::TextFormat {
        font_id: body_font.clone(),
        color: text_color,
        ..Default::default()
    };
    let strong = egui::TextFormat {
        font_id: body_font.clone(),
        color: strong_color,
        ..Default::default()
    };
    let weak = egui::TextFormat {
        font_id: body_font,
        color: weak_color,
        ..Default::default()
    };
    let monospace_weak = egui::TextFormat {
        font_id: monospace_font,
        color: weak_color,
        ..Default::default()
    };
    let error = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        color: error_color,
        ..Default::default()
    };
    let definition_spacing = if header.label.is_some() {
        item_spacing + 6.0
    } else {
        0.0
    };

    if let Some(label) = header.label {
        append_header_text(&mut job, &mut full_text, label, 0.0, strong);
    }
    match header.definition {
        DefinitionSummary::Empty => {
            append_header_text(&mut job, &mut full_text, "Empty", definition_spacing, weak);
        }
        DefinitionSummary::Known { name, hash } => {
            append_header_text(&mut job, &mut full_text, name, definition_spacing, body);
            append_header_text(&mut job, &mut full_text, hash, item_spacing, monospace_weak);
        }
        DefinitionSummary::Unknown { hash } => {
            append_header_text(
                &mut job,
                &mut full_text,
                &format!("Unknown item {hash}"),
                definition_spacing,
                error.clone(),
            );
        }
    }
    if !header.valid {
        append_header_text(
            &mut job,
            &mut full_text,
            header.invalid_message,
            item_spacing,
            error,
        );
    }

    let row_height = ui.spacing().interact_size.y;
    let width = ui.available_width().max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, row_height),
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
        |ui| ui.add(egui::Label::new(job).truncate().halign(egui::Align::LEFT)),
    )
    .inner
    .on_hover_text(full_text);
}

fn append_header_text(
    job: &mut egui::text::LayoutJob,
    full_text: &mut String,
    text: &str,
    leading_space: f32,
    format: egui::TextFormat,
) {
    if !full_text.is_empty() {
        full_text.push_str("  ");
    }
    full_text.push_str(text);
    job.append(text, leading_space, format);
}

#[derive(Clone, Debug)]
pub(crate) struct DefinitionChoice {
    pub hash: u64,
    pub label: String,
    /// Optional browse grouping. Callers keep equal groups adjacent.
    pub group: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClearDefinitionChoice {
    pub label: String,
    pub tooltip: String,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DefinitionPickerChoices {
    pub definitions: Vec<DefinitionChoice>,
    pub clear: Option<ClearDefinitionChoice>,
    pub empty_message: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PickerHeight {
    pub min: f32,
    pub max: f32,
}

pub(crate) fn draw_definition_picker(
    ui: &mut egui::Ui,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    choices_for_query: impl FnOnce(&str) -> DefinitionPickerChoices,
) -> Option<ItemEditorAction> {
    draw_definition_picker_with_open_request(ui, scope, query, height, false, choices_for_query)
}

pub(crate) fn draw_definition_picker_with_open_request(
    ui: &mut egui::Ui,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    open_requested: bool,
    choices_for_query: impl FnOnce(&str) -> DefinitionPickerChoices,
) -> Option<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let picker_response = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Click to browse, or type an item name or hex hash…")
                .desired_width(ui.available_width()),
        );
        let popup_id = ui.make_persistent_id("definition-browser");
        if open_requested {
            picker_response.request_focus();
        }
        if open_requested || picker_response.clicked() || picker_response.changed() {
            ui.memory_mut(|memory| memory.open_popup(popup_id));
        }
        if !ui.memory(|memory| memory.is_popup_open(popup_id)) {
            return None;
        }

        let choices = choices_for_query(query);
        let group_row_count = choices
            .definitions
            .iter()
            .enumerate()
            .filter(|(index, definition)| {
                definition.group.is_some()
                    && (*index == 0 || definition.group != choices.definitions[*index - 1].group)
            })
            .count();
        let row_count = choices.definitions.len() + group_row_count;
        let row_height = ui.spacing().interact_size.y;
        let picker_height = picker_list_height(row_count, row_height, height.min, height.max);
        let popup_direction = popup_direction(ui.ctx().screen_rect(), picker_response.rect);
        let mut action = None;
        egui::popup::popup_above_or_below_widget(
            ui,
            popup_id,
            &picker_response,
            popup_direction,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(picker_response.rect.width());
                if choices.definitions.is_empty() && choices.clear.is_none() {
                    ui.label(egui::RichText::new(&choices.empty_message).weak());
                    return;
                }
                if let Some(clear) = &choices.clear {
                    if ui
                        .selectable_label(clear.selected, &clear.label)
                        .on_hover_text(&clear.tooltip)
                        .clicked()
                    {
                        action = Some(ItemEditorAction::ClearDefinition);
                        ui.memory_mut(egui::Memory::close_popup);
                    }
                    ui.separator();
                }

                let rows = definition_picker_rows(&choices.definitions);
                egui::ScrollArea::vertical()
                    .min_scrolled_height(picker_height)
                    .max_height(picker_height)
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, rows.len(), |ui, visible_rows| {
                        for row_index in visible_rows {
                            match rows[row_index] {
                                DefinitionPickerRow::Group(group) => {
                                    ui.add_sized(
                                        [ui.available_width(), row_height],
                                        egui::Label::new(egui::RichText::new(group).strong()),
                                    );
                                }
                                DefinitionPickerRow::Definition(definition) => {
                                    let clicked = ui
                                        .add(
                                            egui::Button::new(&definition.label)
                                                .frame(false)
                                                .truncate()
                                                .min_size(egui::vec2(
                                                    ui.available_width(),
                                                    row_height,
                                                )),
                                        )
                                        .clicked();
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
            },
        );
        action
    })
    .inner
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NumericItemFields {
    pub level: Option<i64>,
    pub quantity: Option<i64>,
    pub quantity_max: Option<i64>,
}

pub(crate) fn draw_level_and_quantity(
    ui: &mut egui::Ui,
    scope: impl Hash,
    fields: NumericItemFields,
) -> Vec<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let mut actions = Vec::new();
        if let Some(level) = fields.level {
            let mut power = displayed_item_power(level);
            ui.label("Power");
            let response = ui.add(
                egui::DragValue::new(&mut power)
                    .speed(POWER_PER_LEVEL as f64)
                    .range(item_power_input_range())
                    .clamp_existing_to_range(false),
            );
            if response.changed()
                && let Some(authored_level) = authored_item_level(power)
                && authored_level != level
            {
                actions.push(ItemEditorAction::SetLevel {
                    level: authored_level,
                });
            }
            response.on_hover_text(
                "Sunrise stores one-tenth of the in-game power. Values snap down to a multiple of 10.",
            );
        }
        if fields.level.is_some() && fields.quantity.is_some() {
            ui.add_space(8.0);
        }
        if let Some(mut quantity) = fields.quantity {
            let quantity_max = fields
                .quantity_max
                .unwrap_or_else(|| i64::from(i32::MAX))
                .max(1);
            ui.label("Quantity");
            if ui
                .add(egui::DragValue::new(&mut quantity).range(1..=quantity_max))
                .changed()
            {
                actions.push(ItemEditorAction::SetQuantity { quantity });
            }
        }
        actions
    })
    .inner
}

pub(crate) fn displayed_item_power(authored_level: i64) -> i64 {
    if authored_level <= 0 {
        0
    } else {
        authored_level
            .saturating_mul(POWER_PER_LEVEL)
            .max(MINIMUM_POWERED_ITEM_POWER)
    }
}

fn item_power_input_range() -> std::ops::RangeInclusive<i64> {
    0..=MAXIMUM_POWERED_ITEM_POWER
}

pub(crate) fn authored_item_level(display_power: i64) -> Option<i64> {
    if display_power < 0 {
        None
    } else if display_power == 0 {
        Some(0)
    } else {
        Some(display_power.max(MINIMUM_POWERED_ITEM_POWER) / POWER_PER_LEVEL)
    }
}

#[derive(Clone, Copy)]
enum DefinitionPickerRow<'a> {
    Group(&'a str),
    Definition(&'a DefinitionChoice),
}

fn definition_picker_rows(definitions: &[DefinitionChoice]) -> Vec<DefinitionPickerRow<'_>> {
    let mut rows = Vec::with_capacity(definitions.len());
    let mut displayed_group = None::<&str>;
    for definition in definitions {
        if definition.group.as_deref() != displayed_group {
            if let Some(group) = definition.group.as_deref() {
                rows.push(DefinitionPickerRow::Group(group));
            }
            displayed_group = definition.group.as_deref();
        }
        rows.push(DefinitionPickerRow::Definition(definition));
    }
    rows
}

#[derive(Clone, Debug)]
pub(crate) struct PlugChoice {
    pub hash: u64,
    pub label: String,
    pub type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PlugPickerSnapshot {
    pub socket_index: usize,
    pub socket_label: String,
    pub current_hash: Option<u64>,
    pub current_label: String,
    pub native_default: Option<NativePlugDefault>,
    pub native_default_label: Option<String>,
    pub choices: Vec<PlugChoice>,
    pub show_types: bool,
}

pub(crate) fn draw_plug_picker(
    ui: &mut egui::Ui,
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
            const SOCKET_LABEL_WIDTH: f32 = 132.0;
            const RESET_BUTTON_WIDTH: f32 = 54.0;

            let row_height = ui.spacing().interact_size.y;
            let spacing = ui.spacing().item_spacing.x;
            let plug_width =
                (ui.available_width() - SOCKET_LABEL_WIDTH - RESET_BUTTON_WIDTH - spacing * 2.0)
                    .max(160.0);
            let screen = ui.ctx().screen_rect();
            let popup_width = (plug_width + 140.0)
                .clamp(440.0, 680.0)
                .min((screen.width() - 24.0).max(320.0));

            ui.allocate_ui_with_layout(
                egui::vec2(SOCKET_LABEL_WIDTH, row_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| ui.add(egui::Label::new(&snapshot.socket_label).truncate()),
            );
            let popup_id = ui.make_persistent_id("plug-browser");
            let button = ui
                .allocate_ui_with_layout(
                    egui::vec2(plug_width, row_height),
                    egui::Layout::left_to_right(egui::Align::Center)
                        .with_main_align(egui::Align::Min),
                    |ui| {
                        ui.add(
                            egui::Button::new(&snapshot.current_label)
                                .truncate()
                                .min_size(egui::vec2(plug_width, row_height)),
                        )
                    },
                )
                .inner;
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
                    ui.set_min_width(popup_width);
                    if searchable {
                        ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Search plug name or hex hash…")
                                .desired_width(popup_width - 20.0),
                        );
                        ui.separator();
                    }
                    if ui
                        .selectable_label(snapshot.current_hash.is_none(), "None")
                        .clicked()
                    {
                        selection = Some(None);
                    }
                    let current_is_choice = snapshot.current_hash.is_some_and(|current| {
                        snapshot.choices.iter().any(|choice| choice.hash == current)
                    });
                    if let Some(hash) = snapshot.current_hash
                        && !current_is_choice
                        && ui
                            .selectable_label(
                                true,
                                format!("{}  (custom/current)", snapshot.current_label),
                            )
                            .clicked()
                    {
                        selection = Some(Some(hash));
                    }
                    ui.separator();

                    let needle = query.trim().to_lowercase();
                    let visible = snapshot
                        .choices
                        .iter()
                        .filter(|choice| {
                            needle.is_empty()
                                || choice.label.to_lowercase().contains(&needle)
                                || snapshot.show_types
                                    && choice.type_name.to_lowercase().contains(&needle)
                        })
                        .collect::<Vec<_>>();
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
                        let picker_height =
                            picker_list_height(visible.len(), row_height, height.min, height.max);
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(picker_height)
                            .max_height(picker_height)
                            .auto_shrink([false, false])
                            .show_rows(ui, row_height, visible.len(), |ui, rows| {
                                for index in rows {
                                    let choice = visible[index];
                                    let option_width = ui.available_width();
                                    let type_name = if snapshot.show_types {
                                        choice.type_name.as_str()
                                    } else {
                                        ""
                                    };
                                    let clicked = ui
                                        .allocate_ui_with_layout(
                                            egui::vec2(option_width, row_height),
                                            egui::Layout::left_to_right(egui::Align::Center)
                                                .with_main_align(egui::Align::Min),
                                            |ui| {
                                                ui.add(
                                                    egui::Button::new(&choice.label)
                                                        .shortcut_text(
                                                            egui::RichText::new(type_name)
                                                                .text_style(egui::TextStyle::Button)
                                                                .weak(),
                                                        )
                                                        .selected(
                                                            snapshot.current_hash
                                                                == Some(choice.hash),
                                                        )
                                                        .frame(false)
                                                        .truncate()
                                                        .min_size(egui::vec2(
                                                            option_width,
                                                            row_height,
                                                        )),
                                                )
                                            },
                                        )
                                        .inner
                                        .clicked();
                                    if clicked {
                                        selection = Some(Some(choice.hash));
                                    }
                                }
                            });
                    }
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
                egui::Button::new("Reset").min_size(egui::vec2(RESET_BUTTON_WIDTH, row_height)),
            );
            let reset_tooltip = match snapshot.native_default {
                Some(NativePlugDefault::Plug(hash)) => format!(
                    "Restore this socket's native default: {}",
                    snapshot
                        .native_default_label
                        .as_deref()
                        .map_or_else(|| format!("0x{hash:08X}"), str::to_owned)
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

pub(crate) fn picker_list_height(
    row_count: usize,
    row_height: f32,
    min_height: f32,
    max_height: f32,
) -> f32 {
    let content_height = row_count as f32 * row_height;
    if content_height < min_height {
        content_height.max(row_height)
    } else {
        content_height.min(max_height)
    }
}

fn popup_direction(screen: egui::Rect, anchor: egui::Rect) -> egui::AboveOrBelow {
    let room_above = (anchor.top() - screen.top()).max(0.0);
    let room_below = (screen.bottom() - anchor.bottom()).max(0.0);
    if room_below >= room_above {
        egui::AboveOrBelow::Below
    } else {
        egui::AboveOrBelow::Above
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_item_header_keeps_trailing_action_inside_available_width() {
        egui::__run_test_ui(|ui| {
            ui.set_width(320.0);
            let expected_right = ui.max_rect().right();
            let mut trailing_rect = None;
            draw_item_header_with_trailing(
                ui,
                ItemHeader {
                    label: Some("Equipped · Class item"),
                    definition: DefinitionSummary::Known {
                        name: "An intentionally very long installed item definition name",
                        hash: "0x12345678",
                    },
                    valid: false,
                    invalid_message: "not valid for this character inventory",
                },
                |ui| trailing_rect = Some(ui.button("Remove").rect),
            );

            let trailing_rect = trailing_rect.expect("trailing action should be drawn");
            assert!(trailing_rect.right() <= expected_right + 0.5);
            assert!(ui.min_rect().right() <= expected_right + 0.5);
        });
    }

    #[test]
    fn authored_levels_display_as_in_game_power() {
        assert_eq!(displayed_item_power(0), 0);
        assert_eq!(displayed_item_power(1), 750);
        assert_eq!(displayed_item_power(74), 750);
        assert_eq!(displayed_item_power(75), 750);
        assert_eq!(displayed_item_power(106), 1_060);
        assert_eq!(
            displayed_item_power(i64::from(i32::MAX)),
            i64::from(i32::MAX) * 10
        );
    }

    #[test]
    fn entered_power_snaps_down_and_converts_to_authored_level() {
        assert_eq!(authored_item_level(-1), None);
        assert_eq!(authored_item_level(0), Some(0));
        assert_eq!(authored_item_level(1), Some(75));
        assert_eq!(authored_item_level(749), Some(75));
        assert_eq!(authored_item_level(750), Some(75));
        assert_eq!(authored_item_level(759), Some(75));
        assert_eq!(authored_item_level(760), Some(76));
        assert_eq!(authored_item_level(1_060), Some(106));
    }

    #[test]
    fn power_input_range_keeps_unpowered_items_editable() {
        let range = item_power_input_range();
        assert!(range.contains(&0));
        assert!(range.contains(&MINIMUM_POWERED_ITEM_POWER));
        assert!(range.contains(&MAXIMUM_POWERED_ITEM_POWER));
        assert!(!range.contains(&-1));
        assert!(!range.contains(&(MAXIMUM_POWERED_ITEM_POWER + 1)));
    }

    #[test]
    fn drawing_power_does_not_rewrite_existing_out_of_range_values() {
        egui::__run_test_ui(|ui| {
            let actions = draw_level_and_quantity(
                ui,
                "out-of-range-power",
                NumericItemFields {
                    level: Some(200),
                    quantity: None,
                    quantity_max: None,
                },
            );
            assert!(actions.is_empty());
        });
    }
}
