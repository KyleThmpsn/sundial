use eframe::egui;

pub(crate) struct PlugSection {
    state: egui::collapsing_header::CollapsingState,
    title: String,
    header: Option<egui::Response>,
}

impl PlugSection {
    pub(crate) fn new(
        ui: &egui::Ui,
        id: impl std::hash::Hash,
        count: usize,
        defaults: bool,
    ) -> Self {
        Self {
            state: egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(id),
                false,
            ),
            title: if defaults {
                format!("Plugs ({count}, default plugs)")
            } else {
                format!("Plugs ({count})")
            },
            header: None,
        }
    }

    pub(crate) fn draw_header(&mut self, ui: &mut egui::Ui) {
        self.header = Some(
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                self.state
                    .show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
                if ui
                    .add(
                        egui::Label::new(&self.title)
                            .truncate()
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(&self.title)
                    .clicked()
                {
                    self.state.toggle(ui);
                }
            })
            .response,
        );
    }

    pub(crate) fn show_body(mut self, ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
        if self.header.is_none() {
            self.draw_header(ui);
        }
        self.state
            .show_body_indented(self.header.as_ref().expect("plug header"), ui, body);
    }
}
