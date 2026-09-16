//! The workbench uses Sundial's icon and description rows in an independent popup.
mod browser;
pub(in crate::app::custom_perks) use browser::{
    BrowserList, browser, browser_with_toolbar, search, show_all,
};

/// Give a control the name a screen reader announces before its value.
///
/// A combo box built from an id rather than a visible label publishes an empty accessible
/// name, so a screen reader reads "All Sources" with no word for what that selects. Sighted
/// users take that word from the surrounding layout; this supplies the same word to anyone
/// who cannot see it. It changes nothing on screen.
/// Name the combo box built from `salt` in this `ui`. Call it after the combo draws: egui
/// writes an empty name for an unlabelled combo, and the last writer of the frame wins.
/// A wrong salt produces no name, which the accessibility tests fail on.
pub(in crate::app::custom_perks) fn name_combo(
    ui: &egui::Ui,
    salt: impl std::hash::Hash,
    name: &str,
) {
    // A combo box hashes its salt into an `Id` first, so hashing the bare salt here would
    // address a different node and silently name nothing.
    let id = ui.make_persistent_id(egui::Id::new(salt));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_label(name.to_owned());
    });
}

/// Give a widget the accessible name its visible label carries. The label sits beside the
/// widget in the layout without being linked to it, so a screen reader would otherwise
/// announce the widget with no name at all.
pub(in crate::app::custom_perks) fn name_response(
    ui: &egui::Ui,
    response: &egui::Response,
    name: &str,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name.to_owned());
    });
}

pub(in crate::app::custom_perks) fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}

pub(in crate::app::custom_perks) fn popup<T>(
    ui: &mut egui::Ui,
    scope: impl std::hash::Hash,
    label: &str,
    query: &mut String,
    contents: impl FnMut(&mut egui::Ui, &str, bool, f32) -> Option<T>,
) -> Option<T> {
    popup_with_width(ui, scope, label, query, 660.0, contents)
}

pub(in crate::app::custom_perks) fn popup_with_width<T>(
    ui: &mut egui::Ui,
    scope: impl std::hash::Hash,
    label: &str,
    query: &mut String,
    width: f32,
    mut contents: impl FnMut(&mut egui::Ui, &str, bool, f32) -> Option<T>,
) -> Option<T> {
    let id = ui.make_persistent_id(scope);
    let query_id = id.with("query");
    let mut local_query = ui.data_mut(|data| data.get_temp::<String>(query_id).unwrap_or_default());
    let anchor = ui.add(egui::Button::new(label).truncate());
    if anchor.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    let screen = ui.ctx().screen_rect();
    let below = screen.bottom() - anchor.rect.bottom();
    let above = anchor.rect.top() - screen.top();
    let direction = if below >= above {
        egui::AboveOrBelow::Below
    } else {
        egui::AboveOrBelow::Above
    };
    let height = (below.max(above) - 105.0).clamp(90.0, 390.0);
    let style = ui.style().clone();
    let mut picked = None;
    egui::popup::popup_above_or_below_widget(
        ui,
        id,
        &anchor,
        direction,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_style(style);
            ui.set_width(width.min((screen.width() - 40.0).max(300.0)));
            // TextEdit requests scrolling when its cursor moves. Consume that request
            // inside the popup so it cannot reach the workbench's enclosing scroll area.
            egui::ScrollArea::neither()
                .id_salt("popup-focus-boundary")
                .show(ui, |ui| {
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut local_query)
                            .hint_text("Search Names and Descriptions")
                            .desired_width(f32::INFINITY),
                    );
                    crate::app::style::named_control(search.clone(), "Search Choices");
                    if anchor.clicked() {
                        search.request_focus();
                    }
                    ui.separator();
                    picked = contents(
                        ui,
                        &local_query.trim().to_lowercase(),
                        search.changed() || anchor.clicked(),
                        height,
                    );
                });
        },
    );
    *query = local_query.clone();
    ui.data_mut(|data| data.insert_temp(query_id, local_query));
    if picked.is_some() {
        ui.memory_mut(egui::Memory::close_popup);
    }
    picked
}

pub(in crate::app::custom_perks) fn results<T>(
    ui: &mut egui::Ui,
    scope: impl std::hash::Hash,
    count: usize,
    height: f32,
    reset: bool,
    row_height: f32,
    mut row: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
) -> Option<T> {
    ui.label(format!("{count} Results"));
    if count == 0 {
        ui.label("No matching choices. Try fewer words or clear the filters.");
        return None;
    }
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(scope)
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false]);
    if reset {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    let mut picked = None;
    scroll.show_rows(ui, row_height, count, |ui, rows| {
        for index in rows {
            if let Some(value) = row(ui, index) {
                picked = Some(value);
            }
        }
    });
    picked
}
