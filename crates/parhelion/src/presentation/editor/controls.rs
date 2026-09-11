use super::*;

impl Editor {
    pub(super) fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.importing.is_none(), egui::Button::new("Import Image…"))
                .clicked()
            {
                self.import(ui.ctx());
            }
            if self.importing.is_some() {
                ui.spinner();
            }
        });
        ui.weak("PNG or JPEG · source and edits are saved in the recipe.");
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Placement, "Size and Position");
            ui.selectable_value(&mut self.tab, Tab::Crop, "Crop");
            if self.kind == Kind::Badge {
                ui.selectable_value(&mut self.tab, Tab::Background, "Background");
            }
        });
        ui.separator();
        match self.tab {
            Tab::Placement => self.placement(ui),
            Tab::Crop => crop::controls(ui, &mut self.composition.crop, self.source.pixels()),
            Tab::Background => background_controls(ui, &mut self.composition.background),
        }
    }

    fn placement(&mut self, ui: &mut egui::Ui) {
        ui.strong("Image Sizing");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.composition.fit, Fit::Contain, "Fit")
                .on_hover_text("Show the entire crop without stretching it.");
            ui.selectable_value(&mut self.composition.fit, Fit::Cover, "Fill")
                .on_hover_text("Fill the available area, cropping the edges when needed.");
        });
        ui.add(
            egui::Slider::new(&mut self.composition.scale, 10..=400)
                .text("Scale")
                .suffix("%"),
        );
        ui.add(
            egui::Slider::new(&mut self.composition.offset[0], -100..=100)
                .text("Horizontal")
                .suffix("%"),
        );
        ui.add(
            egui::Slider::new(&mut self.composition.offset[1], -100..=100)
                .text("Vertical")
                .suffix("%"),
        );
        if ui.button("Center Image").clicked() {
            self.composition.offset = [0, 0];
        }
        ui.add_space(10.0);
        ui.strong("Orientation");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Rotate Left").clicked() {
                self.composition.rotation = (self.composition.rotation + 3) % 4;
            }
            if ui.button("Rotate Right").clicked() {
                self.composition.rotation = (self.composition.rotation + 1) % 4;
            }
        });
        ui.checkbox(&mut self.composition.flip_horizontal, "Flip Horizontally");
        ui.checkbox(&mut self.composition.flip_vertical, "Flip Vertically");
        ui.add_space(10.0);
        if ui.button("Reset Size and Position").clicked() {
            self.composition = Composition {
                crop: self.composition.crop,
                background: self.composition.background.clone(),
                ..Default::default()
            };
        }
    }

    fn import(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.importing = Some(rx);
        self.error = None;
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("artwork-import".into())
            .spawn(move || {
                let result = rfd::FileDialog::new()
                    .set_title("Import Artwork")
                    .add_filter("PNG / JPEG Images", &["png", "jpg", "jpeg"])
                    .pick_file()
                    .map(|path| Artwork::from_path(&path))
                    .transpose();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        if let Err(error) = spawned {
            self.importing = None;
            self.error = Some(format!("Could not import image: {error}"));
        }
    }
}

fn background_controls(ui: &mut egui::Ui, background: &mut Background) {
    ui.strong("Card Background");
    let mut kind = match background {
        Background::Solid { .. } => 1,
        Background::Gradient { .. } => 2,
        _ => 0,
    };
    let old = kind;
    egui::ComboBox::from_id_salt("artwork-background")
        .selected_text(["Sunrise", "Solid Color", "Gradient"][kind])
        .show_ui(ui, |ui| {
            for (i, label) in ["Sunrise", "Solid Color", "Gradient"]
                .into_iter()
                .enumerate()
            {
                ui.selectable_value(&mut kind, i, label);
            }
        });
    if old != kind {
        *background = match kind {
            1 => Background::Solid {
                color: [20, 30, 38],
            },
            2 => Background::Gradient {
                start: [12, 27, 35],
                end: [51, 102, 111],
                angle: 90,
            },
            _ => Background::Sunrise,
        };
    }
    match background {
        Background::Solid { color } => {
            color_control(ui, "Color", color);
        }
        Background::Gradient { start, end, angle } => {
            color_control(ui, "Start Color", start);
            color_control(ui, "End Color", end);
            ui.add(
                egui::Slider::new(angle, 0..=360)
                    .text("Direction")
                    .suffix("°"),
            );
            if ui.button("Swap Colors").clicked() {
                std::mem::swap(start, end);
            }
        }
        _ => {
            ui.weak("The standard purple card background.");
        }
    }
    ui.add_space(6.0);
    ui.weak("The background fills the whole badge, including space around the image.");
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(color);
    });
}
