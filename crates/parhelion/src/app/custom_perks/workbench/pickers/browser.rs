//! A persistent inspector gives choices enough space and keeps browsing reversible.
#[cfg(test)]
mod tests;

pub(in crate::app::custom_perks) fn show_all(ui: &mut egui::Ui) -> (bool, bool) {
    let id = egui::Id::new("perk-picker-show-all");
    let mut value = ui.data(|state| state.get_temp::<bool>(id).unwrap_or(false));
    let changed = ui
        .checkbox(&mut value, "Show All")
        .on_hover_text("Include unnamed effects and debug assets.")
        .changed();
    ui.data_mut(|state| state.insert_temp(id, value));
    (value, changed)
}

pub(in crate::app::custom_perks) fn browser<T>(
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

pub(in crate::app::custom_perks) fn search(
    ui: &mut egui::Ui,
    query: &mut String,
    opened: bool,
    width: f32,
) -> bool {
    sundial::ui::catalog::search(ui, query, opened, width, "Search Effects or Perks")
}

pub(in crate::app::custom_perks) fn browser_with_toolbar<T>(
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
        let width = (screen.width() - 48.0).clamp(280.0, 1100.0);
        let height = (screen.height() - 96.0).clamp(240.0, 760.0);
        let mut local_query = ui
            .data(|data| data.get_temp::<String>(id.with("query")))
            .unwrap_or_default();
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
                // Keep the user-sized window stable through loading and empty searches.
                ui.set_min_height(available);
                picked = contents(ui, &mut local_query, clicked, available);
            });
        *query = local_query.clone();
        ui.data_mut(|data| data.insert_temp(id.with("query"), local_query));
        if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) || picked.is_some() {
            open = false;
        }
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    picked
}

pub(in crate::app::custom_perks) use sundial::ui::catalog::BrowserList;
