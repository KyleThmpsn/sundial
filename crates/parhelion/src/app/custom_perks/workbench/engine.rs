//! Engine kinds, installed objects and native references in one catalog.
use super::*;
use sundial::investment::PerkSources;

use sundial::ui::catalog::{content, kinds};

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
enum View {
    #[default]
    Kinds,
    Assets,
    Perks,
    Paths,
    References,
}

#[derive(Default)]
pub(super) struct EngineCatalog {
    pub open: bool,
    view: View,
    asset_query: String,
    content: content::Browser,
    export_error: Option<String>,
    pub(super) kinds: kinds::Kinds,
    pub(super) copy_requested: Option<u16>,
}

impl EngineCatalog {
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
                crate::app::style::workbench_style(ui);
                if !experimental {
                    self.view = View::Kinds;
                }
                let previous = self.view;
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.view, View::Kinds, "Kinds");
                    if experimental {
                        for (view, label) in [
                            (View::Assets, "Objects and Effects"),
                            (View::Perks, "Perk References"),
                            (View::Paths, "TFT Paths"),
                            (View::References, "TFT References"),
                        ] {
                            ui.selectable_value(&mut self.view, view, label);
                        }
                        crate::app::style::more_menu(ui, |ui| {
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
                if let Some(error) = &self.export_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                ui.push_id(self.view, |ui| match self.view {
                    View::Kinds => self.draw(ui, choices, sources, browser.discovery, experimental),
                    View::Assets => {
                        browser.draw(
                            ui,
                            assets::AssetScope::Any,
                            &mut self.asset_query,
                            previous != self.view,
                            None,
                        );
                    }
                    _ => self.draw_content(ui, choices, browser.discovery),
                });
            });
        self.open = open;
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
        self.export_error = serde_json::to_vec_pretty(&value)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                sundial::package_authoring::replace_authoring_file(&path, &bytes)
                    .map_err(|error| error.to_string())
            })
            .err();
    }

    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        sources: &PerkSources,
        discovery: &discovery::Discovery,
        experimental: bool,
    ) {
        let source = if let Some(data) = &discovery.data {
            kinds::Source::Ready(&data.perks)
        } else if discovery.busy() {
            kinds::Source::Loading
        } else {
            kinds::Source::Unavailable
        };
        let mut copy = None;
        let mut inspect = None;
        let mut actions =
            |ui: &mut egui::Ui, selected: Option<u16>, location: kinds::UseLocation| {
                crate::app::style::workbench_style(ui);
                let available = selected
                    .is_some_and(|index| choices.iter().any(|choice| choice.perk_index == index));
                if ui
                    .add_enabled(available, egui::Button::new("Copy as New Perk"))
                    .on_hover_text("Open a new draft containing only the selected internal effect.")
                    .on_disabled_hover_text("Select an effect that is available to copy.")
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
        self.kinds
            .draw(ui, sources, source, &mut Some(&mut actions));
        if let Some(index) = copy {
            self.copy_requested = Some(index);
        }
        if let Some(index) = inspect {
            crate::app::runtime_dependencies::request(ui.ctx(), Some(index));
        }
    }

    fn draw_content(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        discovery: &discovery::Discovery,
    ) {
        if let Some(data) = &discovery.data {
            let view = match self.view {
                View::Perks => content::View::Perks,
                View::Paths => content::View::Paths,
                View::References => content::View::References,
                _ => return,
            };
            self.content.draw(ui, view, choices, data);
        } else if discovery.busy() {
            ui.spinner();
            ui.label("Reading Native Content…");
            if let Some((current, total)) = discovery.progress {
                ui.small(format!("{current} of {total} resources"));
            }
        }
        if let Some(error) = &discovery.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}
