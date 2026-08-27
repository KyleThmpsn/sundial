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
        ui.label(egui::RichText::new(title).strong().size(17.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_placement_tracks_available_width() {
        assert_eq!(
            WorkspacePlacement::for_width(SIDE_WORKSPACE_BREAKPOINT - 1.0),
            WorkspacePlacement::Bottom
        );
        assert_eq!(
            WorkspacePlacement::for_width(SIDE_WORKSPACE_BREAKPOINT),
            WorkspacePlacement::Side
        );
    }

    #[test]
    fn side_workspace_always_preserves_primary_content_width() {
        for available_width in [SIDE_WORKSPACE_BREAKPOINT, 1_056.0, 1_500.0] {
            let maximum_width = (available_width - PRIMARY_WORKSPACE_MIN_WIDTH)
                .clamp(SIDE_WORKSPACE_MIN_WIDTH, SIDE_WORKSPACE_MAX_WIDTH);
            assert!(available_width - maximum_width >= PRIMARY_WORKSPACE_MIN_WIDTH);
        }
    }

    #[test]
    fn compact_widths_do_not_use_a_side_workspace() {
        assert!(!uses_side_workspace(SIDE_WORKSPACE_BREAKPOINT - 1.0));
        assert!(uses_side_workspace(SIDE_WORKSPACE_BREAKPOINT));
    }
}
