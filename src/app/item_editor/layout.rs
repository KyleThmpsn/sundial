use super::*;

pub(crate) fn draw_responsive_item_cards<T>(
    ui: &mut egui::Ui,
    items: &[T],
    minimum_card_width: f32,
    maximum_card_width: f32,
    mut draw: impl FnMut(&mut egui::Ui, &T),
) {
    let Some((column_count, grid_width)) = responsive_item_card_layout(
        ui.available_width(),
        ui.spacing().item_spacing.x,
        items.len(),
        minimum_card_width,
        maximum_card_width,
    ) else {
        return;
    };
    ui.scope(|ui| {
        ui.set_width(grid_width);
        ui.columns(column_count, |columns| {
            let mut counts = vec![0_usize; column_count];
            for (index, item) in items.iter().enumerate() {
                let column = index % column_count;
                if counts[column] != 0 {
                    columns[column].add_space(3.0);
                }
                draw(&mut columns[column], item);
                counts[column] += 1;
            }
        });
    });
}

pub(super) fn responsive_item_card_layout(
    available_width: f32,
    spacing: f32,
    item_count: usize,
    minimum_card_width: f32,
    maximum_card_width: f32,
) -> Option<(usize, f32)> {
    if item_count == 0 {
        return None;
    }

    let available_width = available_width.max(0.0);
    let minimum_card_width = minimum_card_width.max(1.0);
    let maximum_card_width = maximum_card_width.max(minimum_card_width);
    let column_count =
        (((available_width + spacing) / (minimum_card_width + spacing)).floor() as usize).max(1);
    let total_spacing = spacing * column_count.saturating_sub(1) as f32;
    let card_width =
        ((available_width - total_spacing) / column_count as f32).clamp(0.0, maximum_card_width);
    Some((
        column_count,
        card_width * column_count as f32 + total_spacing,
    ))
}

pub(super) fn picker_list_height(
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

pub(super) fn spaced_picker_list_height(
    row_count: usize,
    row_height: f32,
    row_spacing: f32,
    min_height: f32,
    max_height: f32,
) -> f32 {
    if row_count == 0 {
        return 0.0;
    }

    let row_spacing = row_spacing.max(0.0);
    let content_height =
        row_count as f32 * row_height + row_count.saturating_sub(1) as f32 * row_spacing;
    if content_height < min_height {
        return content_height.max(row_height);
    }
    if content_height <= max_height {
        return content_height;
    }

    let row_stride = row_height + row_spacing;
    let visible_rows =
        (((max_height + row_spacing) / row_stride).floor() as usize).clamp(1, row_count);
    visible_rows as f32 * row_height + visible_rows.saturating_sub(1) as f32 * row_spacing
}

pub(super) fn popup_direction(screen: egui::Rect, anchor: egui::Rect) -> egui::AboveOrBelow {
    let room_above = (anchor.top() - screen.top()).max(0.0);
    let room_below = (screen.bottom() - anchor.bottom()).max(0.0);
    if room_below >= room_above {
        egui::AboveOrBelow::Below
    } else {
        egui::AboveOrBelow::Above
    }
}
