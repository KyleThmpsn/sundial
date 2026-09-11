use super::*;

impl Editor {
    pub(super) fn sync_textures(&mut self, ctx: &egui::Context) {
        if self.source_texture.is_none() {
            self.source_texture = Some(load_texture(ctx, "artwork-source", self.source.pixels()));
        }
        let Ok(current) = self.current() else {
            return;
        };
        if self.rendered.as_ref() == Some(&current) {
            return;
        }
        let image = match (&mut self.context, self.kind) {
            (Some(ContextPreview::Watermark(preview)), Kind::Watermark) => preview.render(&current),
            (context, Kind::Badge) => {
                let mask = match context {
                    Some(ContextPreview::Badge(mask)) => Some(mask.as_slice()),
                    _ => None,
                };
                crate::badge_icon::preview(Some(&current), mask)
                    .map(|image| color_image(&image))
                    .map_err(|e| e.to_string())
            }
            _ => crate::watermark::render_custom_corner_preview(&current)
                .map(|image| color_image(&image))
                .map_err(|e| e.to_string()),
        };
        match image {
            Ok(image) => {
                if let Some(texture) = &mut self.preview_texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.preview_texture = Some(ctx.load_texture(
                        "artwork-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            Err(error) => self.error = Some(error),
        }
        self.rendered = Some(current);
    }

    pub(super) fn draw_preview(&mut self, ui: &mut egui::Ui) {
        ui.strong(if self.kind == Kind::Badge {
            "Badge Preview"
        } else {
            "Weapon Icon Preview"
        });
        ui.add_space(6.0);
        if let Some(texture) = self.preview_texture.clone() {
            if self.kind == Kind::Badge {
                let width = ui.available_width().min(440.0);
                let size = egui::vec2(width, width * 268.0 / 440.0);
                let response = ui.add(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(size)
                        .sense(egui::Sense::drag()),
                );
                if self.completed {
                    ribbon(ui, response.rect);
                }
                self.pan(&response, size);
                ui.checkbox(&mut self.completed, "Preview Completed Badge");
            } else {
                ui.horizontal_top(|ui| {
                    let side = (ui.available_width() - 90.0).clamp(96.0, 220.0);
                    let response = ui.add(
                        egui::Image::new(&texture)
                            .fit_to_exact_size(egui::vec2(side, side))
                            .sense(egui::Sense::drag()),
                    );
                    self.pan(
                        &response,
                        egui::vec2(side * 27.0 / 96.0, side * 23.0 / 96.0),
                    );
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Image::new(&texture).fit_to_exact_size(egui::vec2(64.0, 64.0)),
                        );
                        ui.weak("Small Size");
                    });
                });
                ui.weak("Shown as a silhouette in game. Transparent areas stay clear.");
            }
        }
        if self.context_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Loading preview…");
            });
        }
        if let Some(error) = &self.context_error {
            ui.weak("Game preview unavailable").on_hover_text(error);
        }
        ui.weak("Drag the preview to move the artwork.");
        if self.tab == Tab::Crop {
            ui.add_space(10.0);
            ui.strong("Source Image");
            ui.weak("Drag a rectangle over the part you want to keep.");
            if let Some(texture) = &self.source_texture {
                self.crop.draw(ui, texture, &mut self.composition.crop);
            }
        }
    }

    fn pan(&mut self, response: &egui::Response, size: egui::Vec2) {
        if response.drag_started() {
            self.pan_offset = Some(self.composition.offset.map(f32::from));
        }
        if response.dragged() {
            let delta = response.drag_delta() / size * 100.0;
            let offset = self
                .pan_offset
                .get_or_insert(self.composition.offset.map(f32::from));
            for (index, value) in [delta.x, delta.y].into_iter().enumerate() {
                offset[index] = (offset[index] + value).clamp(-100.0, 100.0);
                self.composition.offset[index] = offset[index].round() as i16;
            }
        }
        if response.drag_stopped() {
            self.pan_offset = None;
        }
    }
}

fn ribbon(ui: &egui::Ui, rect: egui::Rect) {
    let edge = rect.width() * 0.245;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            rect.left_top(),
            rect.left_top() + egui::vec2(edge, 0.0),
            rect.left_top() + egui::vec2(0.0, edge),
        ],
        egui::Color32::from_rgb(218, 181, 78),
        egui::Stroke::NONE,
    ));
    let center = rect.left_top() + egui::vec2(edge * 0.32, edge * 0.30);
    let points = (0..16)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 16.0;
            let radius = edge * if i % 2 == 0 { 0.14 } else { 0.055 };
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect::<Vec<_>>();
    for i in 0..points.len() {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![center, points[i], points[(i + 1) % points.len()]],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
}
