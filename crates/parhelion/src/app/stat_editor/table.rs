//! Investment-value table for an equipped perk's stat bonuses.
use super::left_cell;

pub(crate) struct Table {
    pub name: f32,
    pub value: f32,
    pub action: f32,
}

impl Table {
    pub fn new(ui: &egui::Ui) -> Self {
        let value = 68.0;
        let action = 22.0;
        Self {
            name: (ui.available_width() - value - action - 16.0).max(120.0),
            value,
            action,
        }
    }

    pub fn show(
        &self,
        ui: &mut egui::Ui,
        salt: impl std::hash::Hash + std::fmt::Debug,
        value_label: &str,
        value_hint: &str,
        rows: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Grid::new(salt)
            .striped(true)
            .num_columns(3)
            .min_col_width(0.0)
            .spacing([8.0, 5.0])
            .show(ui, |ui| {
                self.heading(ui, self.name, "Stat");
                self.heading(ui, self.value, value_label)
                    .on_hover_text(value_hint);
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
