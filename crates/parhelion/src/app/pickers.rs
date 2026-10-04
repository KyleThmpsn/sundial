//! The workbench's pickers open in a window of their own, Sundial's rows beside a detail pane.
mod browser;
pub(crate) use browser::{
    BrowserList, CLEAR_WIDTH, browser, browser_with_toolbar, search, show_all,
};

/// Whether a list of `rows` is long enough to take a filter. Twelve or fewer read at a glance.
pub(crate) const fn wants_filter(rows: usize) -> bool {
    rows > 12
}

/// Give a control the name a screen reader announces before its value.
///
/// A combo box built from an id rather than a visible label publishes an empty accessible
/// name, so a screen reader reads "All Sources" with no word for what that selects. Sighted
/// users take that word from the surrounding layout; this supplies the same word to anyone
/// who cannot see it. It changes nothing on screen.
/// Name the combo box built from `salt` in this `ui`. Call it after the combo draws: egui
/// writes an empty name for an unlabelled combo, and the last writer of the frame wins.
/// A wrong salt produces no name, which the accessibility tests fail on.
pub(crate) fn name_combo(ui: &egui::Ui, salt: impl std::hash::Hash, name: &str) {
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
pub(crate) fn name_response(ui: &egui::Ui, response: &egui::Response, name: &str) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name.to_owned());
    });
}

pub(crate) fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}
