mod picker;
mod validation;

use crate::{
    catalog::Catalog,
    investment::titles::{Title, Unlock},
};
use eframe::egui;
use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
};

#[derive(Default)]
pub(super) struct Titles {
    source: Option<(PathBuf, u64)>,
    job: Option<JoinHandle<Result<Vec<Title>, String>>>,
    pub result: Option<Result<Vec<Title>, String>>,
}

pub(super) struct Selection {
    pub index: u16,
    pub unlock: Option<Unlock>,
}

impl Titles {
    pub fn draw(
        &self,
        ui: &mut egui::Ui,
        catalog: &Catalog,
        current: u64,
        character: usize,
    ) -> Option<Selection> {
        let choices = self.result.as_ref().and_then(|result| result.as_ref().ok());
        let selected_title = choices.and_then(|choices| {
            choices
                .iter()
                .find(|choice| u64::from(choice.index) == current)
        });
        let selected = if current == u64::from(u16::MAX) {
            "None".to_owned()
        } else {
            selected_title.map_or_else(|| format!("Title {current}"), |choice| choice.name.clone())
        };
        if let Some(title) = selected_title {
            picker::thumbnail(ui, catalog, title, 20.0);
        }
        let mut selection = None;
        egui::ComboBox::from_id_salt(("equipped-title", character))
            .selected_text(selected)
            .width(ui.available_width().clamp(100.0, 150.0))
            .truncate()
            .show_ui(ui, |ui| {
                ui.set_min_width(240.0);
                if picker::row(ui, catalog, None, current == u64::from(u16::MAX)).clicked() {
                    selection = Some(Selection {
                        index: u16::MAX,
                        unlock: None,
                    });
                    ui.close_menu();
                }
                if let Some(choices) = choices {
                    for choice in choices {
                        let response = ui
                            .add_enabled_ui(choice.unlock.is_ok(), |ui| {
                                picker::row(
                                    ui,
                                    catalog,
                                    Some(choice),
                                    u64::from(choice.index) == current,
                                )
                            })
                            .inner;
                        match &choice.unlock {
                            Ok(unlock) if response.clicked() => {
                                selection = Some(Selection {
                                    index: choice.index,
                                    unlock: Some(*unlock),
                                });
                                ui.close_menu();
                            }
                            Err(reason) => {
                                response.on_disabled_hover_text(reason);
                            }
                            _ => {}
                        }
                    }
                }
            })
            .response
            .on_hover_text("Selecting a title unlocks it for your account. Click Save to apply.");
        selection
    }

    pub fn refresh(&mut self, catalog: &Catalog, ctx: &egui::Context) {
        let access = catalog.inspection_access();
        let source = (catalog.install_path().join("packages"), access.generation());
        if self.source.as_ref() != Some(&source) && self.job.is_none() {
            self.source = Some(source.clone());
            self.result = None;
        }
        if self.job.as_ref().is_some_and(|job| job.is_finished()) {
            self.result = Some(
                self.job
                    .take()
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| Err("Title lookup stopped unexpectedly".into())),
            );
            if self.source.as_ref() != Some(&source) {
                self.source = Some(source.clone());
                self.result = None;
            }
        }
        if self.result.is_none() && self.job.is_none() && !access.is_suspended() {
            let ctx = ctx.clone();
            self.job = Some(thread::spawn(move || {
                let result = access.read(|| crate::investment::titles::load(&source.0));
                ctx.request_repaint();
                result
            }));
        }
    }
}

impl Drop for Titles {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.join();
        }
    }
}
