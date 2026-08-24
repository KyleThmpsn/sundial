use super::state::*;
use super::*;

pub(super) fn table_link(
    ui: &mut egui::Ui,
    width: f32,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(
                egui::Button::new(text)
                    .frame(false)
                    .truncate()
                    .min_size(egui::vec2(width, TABLE_CELL_HEIGHT)),
            )
        },
    )
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn table_drag_value(ui: &mut egui::Ui, width: f32, value: &mut i32) -> egui::Response {
    table_drag_value_ranged(ui, width, value, i32::MIN..=i32::MAX)
}

pub(super) fn table_drag_value_ranged(
    ui: &mut egui::Ui,
    width: f32,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(egui::DragValue::new(value).speed(1.0).range(range))
        },
    )
    .inner
}

pub(super) fn draw_definition_index_cell(
    ui: &mut egui::Ui,
    width: f32,
    definition_index: usize,
    definition: Option<&UnlockDefinition>,
    metadata_selection: MetadataSelection,
    state: &mut UiState,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            let label = egui::RichText::new(format!("#{definition_index}")).monospace();
            if let Some(definition) = definition {
                let response = ui
                    .add(
                        egui::Button::new(label)
                            .frame(false)
                            .truncate()
                            .min_size(egui::vec2(width, TABLE_CELL_HEIGHT)),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(definition_metadata_tooltip(definition));
                if response.clicked() {
                    state.metadata_inspector.open(metadata_selection);
                }
            } else {
                ui.add(egui::Label::new(label.weak()).truncate());
            }
        },
    );
}

pub(super) fn draw_definition_hash_hex_cell(
    ui: &mut egui::Ui,
    width: f32,
    definition: Option<&UnlockDefinition>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if let Some(definition) = definition {
                draw_hash_link(ui, definition.hash, definition_hash_hex_text(definition));
            } else {
                ui.label(egui::RichText::new("-").weak());
            }
        },
    );
}

pub(super) fn draw_remove_cell(
    ui: &mut egui::Ui,
    width: f32,
    accessible_label: &str,
) -> egui::Response {
    let mut layout = egui::Layout::left_to_right(egui::Align::Center);
    layout.main_align = egui::Align::Center;
    let cell = ui.allocate_ui_with_layout(egui::vec2(width, TABLE_CELL_HEIGHT), layout, |ui| {
        super::super::components::draw_trash_button(ui, true, accessible_label)
    });
    cell.inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(accessible_label)
}

pub(super) fn metadata_click(
    response: egui::Response,
    accessible_label: &'static str,
) -> egui::Response {
    let response = response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible_label)
    });
    response
}
