//! Character appearance lives in the workspace document. Preview state contains no account draft.
use super::{SundialApp, account_workspace as account};
mod controls;

use crate::character::{Appearance, Catalog};
use crate::ui::model_preview::still;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

struct State {
    source: Option<(PathBuf, u64)>,
    pending: Option<mpsc::Receiver<Result<Catalog, String>>>,
    catalog: Option<Arc<Catalog>>,
    error: Option<String>,
    portrait: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            source: None,
            pending: None,
            catalog: None,
            error: None,
            portrait: true,
        }
    }
}

fn state(ctx: &egui::Context) -> Arc<Mutex<State>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<State>>>(egui::Id::new("character-appearance-page"))
            .clone()
    })
}

impl SundialApp {
    pub(super) fn draw_appearance_page(&mut self, ui: &mut egui::Ui) {
        self.draw_character_tabs(ui);
        ui.add_space(8.0);
        if self.character_count() == 0 {
            empty(
                ui,
                "No Character Selected",
                "Create a character in Sunrise to begin customizing its appearance.",
            );
            return;
        }
        let Some(native) = self
            .document
            .native_account()
            .filter(|d| d.supports_appearance())
        else {
            empty(
                ui,
                "Character Customization Is Unavailable",
                "This account needs Sunrise's SQLite schema v3 or later character appearance support. Older account formats remain available in Characters & Loadouts.",
            );
            return;
        };
        let packages = self.install_path.join("packages");
        let access = self.manifest.inspection_access();
        if access.is_suspended() {
            empty(
                ui,
                "Preview Paused",
                "Close Parhelion to resume reading character appearance assets.",
            );
            return;
        }
        let shared = state(ui.ctx());
        let Ok(mut state) = shared.lock() else { return };
        let source = (packages.clone(), access.generation());
        if state.source.as_ref() != Some(&source) {
            *state = State {
                source: Some(source),
                ..State::default()
            };
        }
        if let Some(receiver) = &state.pending {
            match receiver.try_recv() {
                Ok(Ok(catalog)) => {
                    state.catalog = Some(Arc::new(catalog));
                    state.pending = None;
                }
                Ok(Err(error)) => {
                    state.error = Some(error);
                    state.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.error =
                        Some("The appearance reader stopped before returning its result.".into());
                    state.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(error) = state.error.clone() {
            empty(ui, "Appearance Assets Could Not Be Loaded", &error);
            if ui.button("Retry").clicked() {
                state.error = None;
            }
            return;
        }
        let Some(catalog) = state.catalog.clone() else {
            if state.pending.is_none() {
                let (tx, rx) = mpsc::channel();
                state.pending = Some(rx);
                let repaint = ui.ctx().clone();
                let packages = packages.clone();
                let access = access.clone();
                let read = crate::model_preview::PackageRead::start();
                std::thread::spawn(move || {
                    let _read = read;
                    let result = access.read(|| Catalog::load(&packages));
                    let _ = tx.send(result);
                    repaint.request_repaint();
                });
            }
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading character models and color palettes…");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(50));
            return;
        };
        let index = self.selected_character;
        let metadata = match account::character_metadata(&self.document, index) {
            Ok(metadata) => metadata,
            Err(error) => {
                empty(ui, "Character Unavailable", &error);
                return;
            }
        };
        let saved = match native.appearance(index) {
            Ok(saved) => saved,
            Err(error) => {
                empty(ui, "Appearance Unavailable", &error);
                return;
            }
        };
        let mut appearance = match saved.clone().map_or_else(
            || catalog.default_appearance(metadata.race, metadata.gender),
            Ok,
        ) {
            Ok(value) => value,
            Err(error) => {
                empty(ui, "Appearance Unavailable", &error);
                return;
            }
        };
        let original = appearance.clone();
        let mut changed = false;
        let preview_id = egui::Id::new("character-appearance-preview");
        let wide = ui.available_width() >= 820.0;
        let height = ui.available_height().max(420.0);
        let preview_width = if wide {
            (ui.available_width() - 354.0).max(320.0)
        } else {
            ui.available_width()
        };
        let armor = self.appearance_armor(index, metadata.class_type);
        let toolbar_height = ui.spacing().interact_size.y.max(28.0);
        let mut portrait = state.portrait;
        let mut preview = |ui: &mut egui::Ui, value: &Appearance| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(10)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width((preview_width - 24.0).max(0.0));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().interact_size.y = toolbar_height;
                        still::info(ui, preview_id);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(!armor.is_empty(), |ui| {
                                ui.selectable_value(&mut portrait, false, "Full Body");
                            });
                            ui.selectable_value(&mut portrait, true, "Portrait");
                        });
                    });
                    ui.add_space(8.0);
                    let request = crate::model_preview::character::Character {
                        appearance: value.clone(),
                        armor: armor.clone(),
                        portrait,
                    };
                    still::show_character(
                        ui,
                        preview_id,
                        &packages,
                        request,
                        egui::vec2(
                            ui.available_width(),
                            if wide {
                                (height - 40.0 - toolbar_height - ui.spacing().interact_size.y)
                                    .max(320.0)
                            } else {
                                360.0
                            },
                        ),
                        access.clone(),
                    );
                    ui.add_space(8.0);
                    still::zoom_controls(ui, preview_id);
                });
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(preview_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| preview(ui, &appearance),
                );
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("appearance-controls")
                            .auto_shrink([false, true])
                            .max_height(height)
                            .show(ui, |ui| {
                                ui.set_width(306.0);
                                changed = controls::show(
                                    ui,
                                    &catalog,
                                    &self.manifest,
                                    &mut appearance,
                                    saved.is_some(),
                                );
                            });
                    },
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("appearance-page")
                .show(ui, |ui| {
                    preview(ui, &appearance);
                    ui.add_space(16.0);
                    changed = controls::show(
                        ui,
                        &catalog,
                        &self.manifest,
                        &mut appearance,
                        saved.is_some(),
                    );
                });
        }
        state.portrait = portrait;
        drop(state);
        if changed && (saved.is_none() || appearance != original) {
            if let Err(error) = catalog.validate(&appearance) {
                self.set_status(error, true);
                return;
            }
            match self
                .document
                .native_account_mut()
                .expect("checked native account")
                .set_appearance(index, appearance)
            {
                Ok(true) => self.report_edit(format!("Updated Character {} appearance", index + 1)),
                Ok(false) => {}
                Err(error) => self.set_status(error, true),
            }
        }
    }

    fn appearance_armor(
        &self,
        index: usize,
        class: u8,
    ) -> Vec<crate::ui::model_preview::Appearance> {
        account::equipped_item_snapshots(&self.document, index)
            .unwrap_or_default()
            .iter()
            .filter(|item| matches!(item.slot, "gauntlets" | "chest" | "legs" | "class_item"))
            .filter_map(|item| {
                let hash = item.definition_hash?;
                let plugs = item.plugs.display_value();
                super::item_editor::appearance::saved_for_class(
                    &self.manifest,
                    hash,
                    plugs.as_ref(),
                    Some(class),
                )
            })
            .collect()
    }
}

fn empty(ui: &mut egui::Ui, title: &str, detail: &str) {
    egui::Frame::group(ui.style())
        .corner_radius(8)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(6.0);
            ui.label(detail);
        });
}
