//! Shared investment-value table for weapon stats and equipped perk bonuses.
use super::left_cell;

pub(crate) struct Table {
    pub id: f32,
    pub name: f32,
    pub value: f32,
    pub preview: f32,
    pub action: f32,
}

impl Table {
    pub fn new(ui: &egui::Ui, internal: bool, preview: bool) -> Self {
        let id = if internal { 34.0 } else { 0.0 };
        let value = 68.0;
        let preview = if preview { 86.0 } else { 0.0 };
        let action = 22.0;
        let gaps = if internal { 4.0 } else { 3.0 } - if preview > 0.0 { 0.0 } else { 1.0 };
        Self {
            id,
            name: (ui.available_width() - id - value - preview - action - gaps * 8.0).max(120.0),
            value,
            preview,
            action,
        }
    }

    pub fn show(
        &self,
        ui: &mut egui::Ui,
        salt: impl std::hash::Hash,
        value_label: &str,
        value_hint: &str,
        rows: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Grid::new(salt).striped(true)
            .num_columns(3 + usize::from(self.id > 0.0) + usize::from(self.preview > 0.0))
            .min_col_width(0.0).spacing([8.0, 5.0]).show(ui, |ui| {
                if self.id > 0.0 { self.heading(ui, self.id, "ID").on_hover_text("Investment stat definition index"); }
                self.heading(ui, self.name, "Stat");
                self.heading(ui, self.value, value_label).on_hover_text(value_hint);
                if self.preview > 0.0 {
                    self.heading(ui, self.preview, "Preview").on_hover_text("Preview from the active decoded stat-display scaling. Final client formatting may differ");
                }
                ui.allocate_space(egui::vec2(self.action, ui.spacing().interact_size.y));
                ui.end_row();
                rows(ui);
            });
    }

    fn heading(&self, ui: &mut egui::Ui, width: f32, text: &str) -> egui::Response {
        left_cell(
            ui,
            width,
            egui::Label::new(egui::RichText::new(text).strong()).halign(egui::Align::LEFT),
        )
    }

    pub fn value(
        &self,
        ui: &mut egui::Ui,
        value: &mut i32,
        range: Option<(i32, i32)>,
        enabled: bool,
    ) -> egui::Response {
        let input = egui::DragValue::new(value)
            .speed(1.0)
            .clamp_existing_to_range(false);
        let input = match range {
            Some((min, max)) => input.range(min..=max),
            None => input,
        };
        ui.add_enabled_ui(enabled, |ui| left_cell(ui, self.value, input))
            .inner
    }
}

pub(crate) fn action(ui: &mut egui::Ui, width: f32, label: &str, hint: &str) -> egui::Response {
    crate::app::style::named_control(
        ui.add_sized(
            [width, ui.spacing().interact_size.y],
            egui::Button::new(label).frame(false),
        ),
        hint,
    )
    .on_hover_text(hint)
}
