//! A persistent inspector gives choices enough space and keeps browsing reversible.
#[cfg(test)]
mod tests;

/// The Include Unidentified switch of one picker. Its state is keyed on the picker, not on
/// the `Ui` it is drawn in, since a wrapped toolbar draws it in a child `Ui` whose id can
/// change from frame to frame.
pub(crate) fn show_all(ui: &mut egui::Ui, scope: impl std::hash::Hash) -> (bool, bool) {
    let id = egui::Id::new(("perk-picker-show-all", scope));
    let mut value = ui.data(|state| state.get_temp::<bool>(id).unwrap_or(false));
    let changed = ui
        .checkbox(&mut value, "Show All")
        .on_hover_text("Include unnamed assets.")
        .changed();
    ui.data_mut(|state| state.insert_temp(id, value));
    (value, changed)
}

pub(crate) fn browser<T>(
    ui: &mut egui::Ui,
    scope: impl std::hash::Hash,
    label: &str,
    title: &str,
    query: &mut String,
    mut contents: impl FnMut(&mut egui::Ui, &str, bool, f32) -> Option<T>,
) -> Option<T> {
    browser_with_toolbar(ui, scope, label, title, query, |ui, query, opened, _| {
        let changed = ui
            .horizontal(|ui| search(ui, query, opened, ui.available_width() - 64.0))
            .inner;
        ui.separator();
        let height = (ui.available_height() - 90.0).max(110.0);
        contents(ui, &query.trim().to_lowercase(), opened || changed, height)
    })
}

pub(crate) fn search(ui: &mut egui::Ui, query: &mut String, opened: bool, width: f32) -> bool {
    sundial::ui::catalog::search(ui, query, opened, width, "Search Effects or Perks")
}

/// The room the Clear button after a search box takes, with the spacing before it.
pub(crate) const CLEAR_WIDTH: f32 = 56.0;

pub(crate) fn browser_with_toolbar<T>(
    ui: &mut egui::Ui,
    scope: impl std::hash::Hash,
    label: &str,
    title: &str,
    query: &mut String,
    mut contents: impl FnMut(&mut egui::Ui, &mut String, bool, f32) -> Option<T>,
) -> Option<T> {
    let id = ui.make_persistent_id(scope);
    let clicked = ui.add(egui::Button::new(label).truncate()).clicked();
    let mut open = clicked || ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let mut picked = None;
    if open {
        let screen = ui.ctx().screen_rect();
        let width = (screen.width() - 48.0).clamp(280.0, 880.0);
        let height = (screen.height() - 96.0).clamp(240.0, 640.0);
        // A picker opens on its whole listing, so the item in use is there to be selected. The
        // search a reader left behind last time is not what they are looking for now.
        let mut local_query = if clicked {
            String::new()
        } else {
            ui.data(|data| data.get_temp::<String>(id.with("query")))
                .unwrap_or_default()
        };
        egui::Window::new(title)
            .id(id.with("window"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_pos(screen.center() - egui::vec2(width, height) * 0.5)
            .default_size(egui::vec2(width, height))
            .min_width(width.min(620.0))
            .max_width(width)
            .min_height(height.min(360.0))
            .max_height(height)
            .show(ui.ctx(), |ui| {
                crate::app::style::workbench_style(ui);
                let available = ui.available_height().max(110.0);
                picked = contents(ui, &mut local_query, clicked, available);
            });
        *query = local_query.clone();
        ui.data_mut(|data| data.insert_temp(id.with("query"), local_query));
        // Escape closes an open dropdown first, and the window only when none is open.
        let escaped = ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
            && !ui.ctx().memory(egui::Memory::any_popup_open);
        if escaped || picked.is_some() {
            open = false;
        }
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    picked
}

pub(crate) use sundial::ui::catalog::BrowserList;
