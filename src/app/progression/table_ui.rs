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
                    .min_size(egui::vec2(0.0, TABLE_CELL_HEIGHT)),
            )
        },
    )
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn table_drag_value(
    ui: &mut egui::Ui,
    width: f32,
    value: &mut i32,
    enabled: bool,
) -> egui::Response {
    table_drag_value_ranged(ui, width, value, i32::MIN..=i32::MAX, enabled)
}

pub(super) fn table_drag_value_ranged(
    ui: &mut egui::Ui,
    width: f32,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
    enabled: bool,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_enabled(enabled, egui::DragValue::new(value).speed(1.0).range(range))
        },
    )
    .inner
}

pub(super) fn draw_remove_cell(
    ui: &mut egui::Ui,
    width: f32,
    accessible_label: &str,
    enabled: bool,
) -> egui::Response {
    let mut layout = egui::Layout::left_to_right(egui::Align::Center);
    layout.main_align = egui::Align::Center;
    let cell = ui.allocate_ui_with_layout(egui::vec2(width, TABLE_CELL_HEIGHT), layout, |ui| {
        super::super::components::draw_trash_button(ui, enabled, accessible_label)
    });
    cell.inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(accessible_label)
}

pub(super) fn sortable_table_header(
    ui: &mut egui::Ui,
    id: &'static str,
    columns: &[(f32, &str)],
    default: TableSort,
    state: &mut UiState,
) -> TableSort {
    let mut sort = state.table_sorts.get(id).copied().unwrap_or(default);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (column, (width, label)) in columns.iter().enumerate() {
            if label.is_empty() {
                ui.allocate_space(egui::vec2(*width, TABLE_CELL_HEIGHT));
                continue;
            }
            let marker = if sort.column == column {
                if sort.descending {
                    Some(Glyph::ChevronDown)
                } else {
                    Some(Glyph::ChevronUp)
                }
            } else {
                None
            };
            let response = sortable_header_cell(ui, *width, label, marker).on_hover_text("Sort");
            if response.clicked() {
                if sort.column == column {
                    sort.descending = !sort.descending;
                } else {
                    sort = TableSort::ascending(column);
                }
            }
        }
    });
    state.table_sorts.insert(id, sort);
    sort
}
