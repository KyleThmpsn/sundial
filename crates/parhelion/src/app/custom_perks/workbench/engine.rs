//! Engine kinds, installed objects and native references in one catalog.
use super::*;
use sundial::investment::PerkSources;

use sundial::ui::catalog::{content, kinds};
mod components;
mod scripts;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
enum View {
    Kinds,
    Assets,
    Perks,
    Paths,
    References,
    Markers,
    #[default]
    Scripts,
    Components,
}

#[derive(Default)]
pub(super) struct EngineCatalog {
    pub open: bool,
    pub(super) retry_requested: bool,
    view: View,
    asset_query: String,
    content: content::Browser,
    /// The last native-map export: where it was written, or why it was not.
    export_status: Option<Result<std::path::PathBuf, String>>,
    pub(super) scan_details: bool,
    pub(super) kinds: kinds::Kinds,
    pub(super) copy_requested: Option<u16>,
    pub(super) markers: super::markers::Markers,
    scripts: scripts::Browser,
    components: components::Browser,
}

impl EngineCatalog {
    /// Whether the marker index is worth reading. It opens every object in the game, so it is
    /// only started once the view asking for it is on screen.
    pub(super) fn wants_markers(&self) -> bool {
        self.open && self.view == View::Markers
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        choices: &[WeaponSandboxPerkChoice],
        sources: &PerkSources,
        browser: assets::Browser<'_>,
        experimental: bool,
    ) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Engine Catalog")
            .id(egui::Id::new("engine-catalog"))
            .open(&mut open)
            .collapsible(false)
            .default_size(egui::vec2(980.0, 640.0))
            .min_width(560.0)
            .min_height(360.0)
            .max_width((ctx.screen_rect().width() - 40.0).max(560.0))
            .max_height((ctx.screen_rect().height() - 64.0).max(360.0))
            .show(ctx, |ui| {
                crate::app::style::perk_workbench_style(ui);
                if !experimental {
                    self.view = View::Kinds;
                }
                let previous = self.view;
                // Most used first. Markers read every object in the game, so they come last.
                ui.horizontal_wrapped(|ui| {
                    if experimental {
                        if ui
                            .selectable_label(
                                matches!(
                                    self.view,
                                    View::Scripts
                                        | View::Components
                                        | View::Paths
                                        | View::References
                                ),
                                "Native Resources",
                            )
                            .clicked()
                        {
                            self.view = View::Scripts;
                        }
                        ui.selectable_value(&mut self.view, View::Assets, "Objects and Effects");
                        ui.selectable_value(&mut self.view, View::Perks, "Perk Effects");
                    }
                    ui.selectable_value(&mut self.view, View::Kinds, "Behavior Kinds");
                    if experimental {
                        ui.selectable_value(&mut self.view, View::Markers, "Markers");
                        crate::app::style::more_menu(ui, "Catalog", |ui| {
                            if ui.button("Scan Details…").clicked() {
                                self.scan_details = true;
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    browser.discovery.data.is_some(),
                                    egui::Button::new("Export Native Map…"),
                                )
                                .clicked()
                            {
                                self.export_map(browser.discovery);
                                ui.close_menu();
                            }
                        });
                    }
                });
                if matches!(
                    self.view,
                    View::Scripts | View::Components | View::Paths | View::References
                ) {
                    ui.horizontal_wrapped(|ui| {
                        for (view, label) in [
                            (View::Scripts, "Object Behaviors"),
                            (View::Components, "Component Types"),
                            (View::Paths, "Paths"),
                            (View::References, "Links"),
                        ] {
                            ui.selectable_value(&mut self.view, view, label);
                        }
                    });
                }
                if browser.discovery.error.is_some()
                    && !browser.discovery.busy()
                    && ui.button("Retry Scan").clicked()
                {
                    self.retry_requested = true;
                }
                match &self.export_status {
                    Some(Ok(path)) => {
                        ui.weak(format!("Exported to {}", path.display()))
                            .on_hover_text(path.display().to_string());
                    }
                    Some(Err(error)) => {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    None => {}
                }
                if !matches!(self.view, View::Assets | View::Markers)
                    && let Some(error) = &browser.discovery.error
                {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if browser.discovery.busy() {
                    if let Some((current, total)) = browser.discovery.progress {
                        ui.weak(format!("Reading {current} of {total}"));
                    } else {
                        ui.weak("Reading…");
                    }
                }
                ui.separator();
                ui.push_id(self.view, |ui| match self.view {
                    View::Kinds => {
                        if let Some(effect) =
                            self.draw(ui, choices, sources, browser.discovery, experimental)
                            && experimental
                        {
                            self.content.open_effect(usize::from(effect));
                            self.view = View::Perks;
                        }
                    }
                    View::Markers => {
                        // A marker's object is often one the asset browser also knows, so
                        // "Find in Objects and Effects" hands its name straight to that view.
                        if let Some(query) = self.markers.draw(ui, browser.discovery) {
                            self.asset_query = query;
                            self.view = View::Assets;
                        }
                    }
                    View::Assets => {
                        browser.draw(
                            ui,
                            assets::AssetScope::Any,
                            &mut self.asset_query,
                            previous != self.view,
                            None,
                            None,
                        );
                    }
                    View::Scripts => {
                        if let Some(data) = &browser.discovery.data {
                            if let Some(tag) = self.scripts.draw(ui, data) {
                                self.content.open_resource(tag);
                                self.view = View::References;
                            }
                        } else {
                            Self::loading(ui, browser.discovery);
                        }
                    }
                    View::Components => {
                        if let Some(data) = &browser.discovery.data {
                            if let Some(tag) = self.components.draw(ui, data) {
                                self.content.open_resource(tag);
                                self.view = View::References;
                            }
                        } else {
                            Self::loading(ui, browser.discovery);
                        }
                    }
                    _ => {
                        if let Some(content::Jump::Kind(family, kind)) = self.draw_content(
                            ui,
                            choices,
                            browser.discovery,
                            sources,
                            browser.catalog,
                        ) {
                            self.kinds.open(family, kind);
                            self.view = View::Kinds;
                        }
                    }
                });
            });
        self.open = open;
        if self.scan_details {
            egui::Window::new("Scan Details")
                .open(&mut self.scan_details)
                .default_width(520.0)
                .show(ctx, |ui| {
                    crate::app::style::perk_workbench_style(ui);
                    if browser.discovery.busy() {
                        ui.spinner();
                        ui.label("Scanning resources…");
                    }
                    if let Some((current, total)) = browser.discovery.progress {
                        ui.label(format!("Scan Progress: {current} of {total}"));
                    }
                    if let Some(error) = &browser.discovery.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    if let Some(data) = &browser.discovery.data {
                        sundial::ui::catalog::scan::show(ui, data);
                    } else {
                        ui.label("No catalog snapshot is available.");
                    }
                });
        }
    }

    fn export_map(&mut self, discovery: &discovery::Discovery) {
        let Some(data) = &discovery.data else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("native-perk-map.json")
            .save_file()
        else {
            return;
        };
        let value = serde_json::json!({
            "tft": &*data.names,
            "effects": &*data.effects,
            "perks": &*data.perks,
            "perk_assets": &data.perk_assets,
        });
        // Writing a file with no word either way reads as nothing having happened, and the
        // reader chose the location, so naming it back is what confirms the export ran.
        self.export_status = Some(
            serde_json::to_vec_pretty(&value)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    sundial::package_authoring::replace_authoring_file(&path, &bytes)
                        .map_err(|error| error.to_string())
                })
                .map(|()| path),
        );
    }

    fn loading(ui: &mut egui::Ui, discovery: &discovery::Discovery) {
        if discovery.busy() {
            ui.spinner();
            ui.label("Reading game content…");
            if let Some((current, total)) = discovery.progress {
                ui.small(format!("{current} of {total} resources"));
            }
        } else {
            ui.label("No game content.");
        }
    }

    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        sources: &PerkSources,
        discovery: &discovery::Discovery,
        experimental: bool,
    ) -> Option<u16> {
        let source = if let Some(data) = &discovery.data {
            kinds::Source::Ready(&data.perks)
        } else if discovery.busy() {
            kinds::Source::Loading
        } else {
            kinds::Source::Unavailable
        };
        self.kinds.reference_links = experimental;
        let mut copy = None;
        let mut inspect = None;
        let mut actions =
            |ui: &mut egui::Ui, selected: Option<u16>, location: kinds::UseLocation| {
                crate::app::style::perk_workbench_style(ui);
                let available = selected
                    .is_some_and(|index| choices.iter().any(|choice| choice.perk_index == index));
                if ui
                    .add_enabled(available, egui::Button::new("Copy as New Perk"))
                    .on_disabled_hover_text("Select an available effect.")
                    .clicked()
                {
                    copy = selected;
                    if matches!(location, kinds::UseLocation::Menu) {
                        ui.close_menu();
                    }
                }
                let inspect_label = match location {
                    kinds::UseLocation::Menu => "Inspect References…",
                    kinds::UseLocation::Footer => "Inspect…",
                };
                if experimental
                    && ui
                        .add_enabled(selected.is_some(), egui::Button::new(inspect_label))
                        .clicked()
                {
                    inspect = selected.map(usize::from);
                    if matches!(location, kinds::UseLocation::Menu) {
                        ui.close_menu();
                    }
                }
            };
        let open = self
            .kinds
            .draw(ui, sources, source, &mut Some(&mut actions));
        if let Some(index) = copy {
            self.copy_requested = Some(index);
        }
        if let Some(index) = inspect {
            crate::app::runtime_dependencies::request(ui.ctx(), Some(index));
        }
        open
    }

    fn draw_content(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        discovery: &discovery::Discovery,
        sources: &PerkSources,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<content::Jump> {
        if let Some(data) = &discovery.data {
            let view = match self.view {
                View::Perks => content::View::Perks,
                View::Paths => content::View::Paths,
                View::References => content::View::References,
                _ => return None,
            };
            return self.content.draw(
                ui,
                view,
                choices,
                data,
                discovery.packages(),
                sources,
                catalog,
            );
        } else {
            Self::loading(ui, discovery);
        }
        None
    }
}
