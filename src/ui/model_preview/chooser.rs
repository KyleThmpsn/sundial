//! Reversible appearance browsing. Only the explicit use action returns a selection.
use super::*;
#[cfg(test)]
mod tests;

pub fn show<T>(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    opened: bool,
    contents: impl FnOnce(&mut egui::Ui, bool) -> Option<T>,
) -> Option<T> {
    if !ui.is_enabled() {
        ui.data_mut(|data| data.insert_temp(id, false));
        return None;
    }
    let mut open = opened || ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let mut picked = None;
    if open {
        let screen = ui.ctx().screen_rect();
        let size = egui::vec2(
            (screen.width() - 32.0).min(640.0),
            (screen.height() - 48.0).min(680.0),
        );
        egui::Window::new(title)
            .id(id.with("window"))
            .open(&mut open)
            .collapsible(false)
            .default_size(size)
            .max_size(size)
            .show(ui.ctx(), |ui| picked = contents(ui, opened));
        if picked.is_some() || ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
            open = false;
        }
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    picked
}

/// A stable viewer for a chooser, preserving the camera across different model candidates.
pub fn preview(ui: &mut egui::Ui, packages: &Path, appearance: Appearance, name: &str) {
    preview_with_access(ui, packages, appearance, name, None);
}

pub(crate) fn preview_with_access(
    ui: &mut egui::Ui,
    packages: &Path,
    appearance: Appearance,
    name: &str,
    access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
) {
    let id = egui::Id::new((
        "appearance-chooser-model",
        ui.ctx().viewport_id(),
        ui.layer_id().id,
    ));
    let generation = access.as_ref().map(|access| access.generation());
    window::launcher(
        ui,
        id,
        Some(window::Request::new(
            packages,
            Target::Weapon(appearance, generation),
            name,
            access,
            true,
        )),
    );
}
