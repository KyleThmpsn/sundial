//! The importer window applies a complete private perk and its weapon dependencies.
use super::*;
use crate::app::socket_editor;
use crate::perk::import::{self, Request};

const SOURCE_PLUG: u32 = 2_077_819_806;
type Choice = (u16, u16, u32, u16);

#[derive(Default)]
pub(super) struct Picker {
    open: bool,
    selected: Option<Choice>,
    notice: String,
    failed: bool,
}

pub(super) struct Prepared {
    packages: PathBuf,
    result: Result<import::Prepared, String>,
}

impl PackageAuthoringApp {
    pub(super) fn draw_modern_perk_button(&mut self, ui: &mut egui::Ui, has_source: bool) {
        if ui
            .add_enabled(has_source, egui::Button::new("Import Modern Perk…"))
            .clicked()
        {
            self.importer.perks.open = true;
            self.importer.perks.selected = None;
            self.importer.perks.notice.clear();
        }
    }

    fn modern_perk_choices(&self) -> Vec<(Choice, String)> {
        let (Some(donor), Some(catalog)) = (self.current_donor(), self.catalog.as_ref()) else {
            return Vec::new();
        };
        if !self.recipe.kind.is_weapon() || donor.summary.type_name != "Sword" {
            return Vec::new();
        }
        let mut choices = Vec::new();
        for (socket, column) in donor.sockets.iter().enumerate().skip(4) {
            let inherited = socket_editor::inherited_socket_choices(
                column.native_default,
                &column.ordered_embedded_choices,
                column.max_authored_choices,
            );
            let Ok(plugs) = socket_editor::recipe_socket_choices(&self.recipe, socket, &inherited)
            else {
                continue;
            };
            for (choice, plug) in plugs.into_iter().enumerate() {
                if self
                    .recipe
                    .overrides
                    .socket_plug_variants
                    .iter()
                    .any(|variant| {
                        usize::from(variant.socket_index) == socket
                            && usize::from(variant.choice_index) == choice
                    })
                {
                    continue;
                }
                for perk in catalog.item_sandbox_perk_indices(plug) {
                    choices.push((
                        (socket as u16, choice as u16, plug, perk),
                        format!(
                            "{} · {} · Choice {}",
                            column.label,
                            catalog.item_display_name(plug).unwrap_or("Unnamed Perk"),
                            choice + 1
                        ),
                    ));
                }
            }
        }
        choices
    }

    pub(super) fn draw_modern_perk_window(&mut self, ctx: &egui::Context) {
        if !self.importer.perks.open {
            return;
        }
        let choices = self.modern_perk_choices();
        if !choices
            .iter()
            .any(|(choice, _)| Some(*choice) == self.importer.perks.selected)
        {
            self.importer.perks.selected = choices.first().map(|(choice, _)| *choice);
        }
        let idle = self.importer_idle();
        let mut open = true;
        let mut start = false;
        egui::Window::new("Import Modern Perk")
            .open(&mut open)
            .default_width(470.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Eager Edge");
                ui.label(format!("Weapon: {}", self.recipe.name));
                ui.label("Imports the modern package controller with its private lunge and sword profile dependencies.");
                ui.label("This is an experimental candidate. Swing consumption and the source status record still need verification.");
                ui.add_space(8.0);
                if choices.is_empty() {
                    ui.label("Choose a sword recipe with an available native trait choice.");
                } else {
                    let selected = choices.iter()
                        .find(|(choice, _)| Some(*choice) == self.importer.perks.selected)
                        .map(|(_, label)| label.as_str()).unwrap_or("Choose a Socket");
                    ui.add_enabled_ui(idle, |ui| {
                        egui::ComboBox::from_id_salt("modern-perk-destination")
                            .width(430.0)
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (choice, label) in &choices {
                                    ui.selectable_value(&mut self.importer.perks.selected, Some(*choice), label);
                                }
                            });
                    });
                }
                if !self.importer.perks.notice.is_empty() {
                    if self.importer.perks.failed {
                        ui.colored_label(ui.visuals().error_fg_color, &self.importer.perks.notice);
                    } else {
                        ui.label(&self.importer.perks.notice);
                    }
                }
                if self.importer.busy() {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Preparing import…"); });
                }
                start = ui.add_enabled(
                    idle && self.importer.settings.modern_packages.is_some()
                        && self.importer.perks.selected.is_some(),
                    egui::Button::new("Import into Draft"),
                ).clicked();
            });
        self.importer.perks.open = open;
        if start {
            self.start_perk_import(ctx);
        }
    }

    fn start_perk_import(&mut self, ctx: &egui::Context) {
        let (Some(modern), Some((socket, choice, plug, perk))) = (
            self.importer.settings.modern_packages.clone(),
            self.importer.perks.selected,
        ) else {
            return;
        };
        let output = data_root().and_then(|root| {
            service::reserve_assets(&root.join("modern-perks"), "perk")
                .map_err(|error| error.to_string())
        });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                self.importer.perks.notice = error;
                self.importer.perks.failed = true;
                return;
            }
        };
        let recipe = self.recipe.clone();
        let packages = self.packages.clone();
        let profile_key = sundial::package_authoring::fnv1_name_hash(&format!(
            "{}.modern-perk.{SOURCE_PLUG:08X}.{socket}.{choice}",
            recipe.namespace,
        ));
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.import_started = Some(Instant::now());
        self.importer.perks.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = import::prepare(&Request {
                modern_packages: &modern,
                native_packages: &packages,
                output: &output,
                recipe: &recipe,
                plug_hash: SOURCE_PLUG,
                socket_index: socket,
                choice_index: choice,
                source_plug_hash: plug,
                source_perk_index: perk,
                profile_key,
                name: "Eager Edge",
            });
            let _ = sender.send(Event::PerkPrepared(Box::new(Prepared { packages, result })));
            ctx.request_repaint();
        });
    }

    pub(super) fn finish_imported_perk(&mut self, prepared: Prepared) {
        let result = if self.packages != prepared.packages {
            Err("The native package folder changed during import. Prepare again.".into())
        } else {
            prepared
                .result
                .and_then(|candidate| candidate.apply(&mut self.recipe))
        };
        match result {
            Ok(()) => {
                self.recipe_dirty = true;
                self.invalidate_results();
                self.importer.perks.notice =
                    "Eager Edge candidate applied. Save and build the draft to stage its packages."
                        .into();
                self.importer.perks.failed = false;
            }
            Err(error) => {
                self.importer.perks.notice = error;
                self.importer.perks.failed = true;
            }
        }
    }
}
