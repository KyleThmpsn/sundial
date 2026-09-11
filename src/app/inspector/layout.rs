use eframe::egui;

const SIDE_WORKSPACE_BREAKPOINT: f32 = 980.0;
const SIDE_WORKSPACE_DEFAULT_WIDTH: f32 = 540.0;
const SIDE_WORKSPACE_MIN_WIDTH: f32 = 420.0;
const SIDE_WORKSPACE_MAX_WIDTH: f32 = 760.0;
const PRIMARY_WORKSPACE_MIN_WIDTH: f32 = 500.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum WorkspacePlacement {
    Side,
    Bottom,
}

impl WorkspacePlacement {
    fn for_width(width: f32) -> Self {
        if width >= SIDE_WORKSPACE_BREAKPOINT {
            Self::Side
        } else {
            Self::Bottom
        }
    }
}

pub(in crate::app) fn uses_side_workspace(width: f32) -> bool {
    WorkspacePlacement::for_width(width) == WorkspacePlacement::Side
}

pub(in crate::app) fn workspace<R>(
    ui: &mut egui::Ui,
    side_id: &'static str,
    _bottom_id: &'static str,
    add_contents: impl FnOnce(&mut egui::Ui, WorkspacePlacement) -> R,
) -> R {
    let placement = WorkspacePlacement::for_width(ui.available_width());
    match placement {
        WorkspacePlacement::Side => {
            let maximum_width = (ui.available_width() - PRIMARY_WORKSPACE_MIN_WIDTH)
                .clamp(SIDE_WORKSPACE_MIN_WIDTH, SIDE_WORKSPACE_MAX_WIDTH);
            egui::SidePanel::right(side_id)
                .resizable(true)
                .default_width(SIDE_WORKSPACE_DEFAULT_WIDTH.min(maximum_width))
                .width_range(SIDE_WORKSPACE_MIN_WIDTH..=maximum_width)
                .frame(
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show_inside(ui, |ui| add_contents(ui, placement))
                .inner
        }
        WorkspacePlacement::Bottom => {
            let maximum_height = (ui.available_height() * 0.7).max(260.0);
            let preferred_height = (ui.available_height() * 0.48).clamp(280.0, 360.0);
            let height = maximum_height.min(preferred_height);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height((height - 16.0).max(0.0));
                            add_contents(ui, placement)
                        })
                        .inner
                },
            )
            .inner
        }
    }
}

pub(in crate::app) fn heading(ui: &mut egui::Ui, title: impl Into<String>) -> bool {
    let mut close = false;
    let title = title.into();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button("Close").clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&title).strong().size(17.0)).truncate(),
                )
                .on_hover_text(&title);
            });
        });
    });
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_heading_keeps_close_button_inside_panel() {
        for width in [260.0, 420.0, 540.0] {
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 300.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    heading(ui, "Unlock Value Definition #12345 · A very long objective name that should never push Close outside the inspector");
                });
            });
            let close = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Close" => Some(text),
                    _ => None,
                })
                .expect("Close must be drawn");
            assert!(close.pos.x >= 0.0);
            assert!(close.pos.x + close.galley.size().x < width);
            assert!(
                close.pos.y < 40.0,
                "Heading must not consume the panel height"
            );
        }
    }
}
