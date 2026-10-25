use super::*;

pub(super) fn capture_screenshot(ctx: &egui::Context) {
    for event in ctx.input(|i| i.events.clone()) {
        if let egui::Event::Screenshot { image, .. } = event {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("model-preview-test-image"), image));
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
mod desktop;

mod inspection;
mod rotation;
