//! A private artwork draft. Only Apply changes the recipe.
mod controls;
mod crop;
mod preview;
#[cfg(test)]
mod tests;

use super::{
    Artwork,
    composition::{Background, Composition, Fit},
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::JoinHandle,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Badge,
    Watermark,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Badge => "Edit Badge Artwork",
            Self::Watermark => "Edit Release Watermark",
        }
    }
    fn background(self) -> Background {
        match self {
            Self::Badge => Background::Sunrise,
            Self::Watermark => Background::Transparent,
        }
    }
    pub(crate) fn default_artwork(self) -> Artwork {
        let bytes: &[u8] = match self {
            Self::Badge => include_bytes!("../../../../assets/parhelion/sunrise-badge-source.png"),
            Self::Watermark => include_bytes!(
                "../../../../assets/parhelion/watermark/sunrise-watermark-2-45x45.png"
            ),
        };
        Artwork::from_png(bytes).expect("bundled artwork is valid")
    }
}

pub(crate) enum Action {
    Apply(Artwork),
    Cancel,
}
#[derive(Clone, Copy, Default, PartialEq)]
enum Tab {
    #[default]
    Placement,
    Crop,
    Background,
}
enum ContextPreview {
    Badge(Vec<u8>),
    Watermark(Box<crate::icon_edit::WatermarkPreview>),
}

pub(crate) struct Editor {
    pub(crate) kind: Kind,
    source: Artwork,
    composition: Composition,
    tab: Tab,
    crop: crop::Selection,
    importing: Option<Receiver<Result<Option<Artwork>, String>>>,
    context_job: Option<Receiver<Result<ContextPreview, String>>>,
    context_worker: Option<JoinHandle<()>>,
    context_requested: bool,
    context: Option<ContextPreview>,
    context_error: Option<String>,
    source_texture: Option<egui::TextureHandle>,
    preview_texture: Option<egui::TextureHandle>,
    rendered: Option<Artwork>,
    error: Option<String>,
    completed: bool,
    pan_offset: Option<[f32; 2]>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        // Closing or replacing the editor must release its package readers before installation.
        if let Some(worker) = self.context_worker.take() {
            let _ = worker.join();
        }
    }
}

impl Editor {
    pub(crate) fn new(kind: Kind, current: Option<Artwork>) -> Self {
        let source = current.unwrap_or_else(|| kind.default_artwork());
        let mut composition = source
            .composition()
            .cloned()
            .unwrap_or_else(|| Composition {
                background: kind.background(),
                ..Default::default()
            });
        if kind == Kind::Watermark {
            composition.background = Background::Transparent;
        }
        Self {
            kind,
            source,
            composition,
            tab: Tab::default(),
            crop: crop::Selection::default(),
            importing: None,
            context_job: None,
            context_worker: None,
            context_requested: false,
            context: None,
            context_error: None,
            source_texture: None,
            preview_texture: None,
            rendered: None,
            error: None,
            completed: true,
            pan_offset: None,
        }
    }

    pub(crate) fn load_context(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        icon: Option<(
            tiger_pkg::TagHash,
            crate::AuthoredWeaponRarity,
            crate::WeaponIconEdit,
        )>,
    ) {
        if self.context_requested {
            return;
        }
        self.context_requested = true;
        let packages = PathBuf::from(packages);
        let kind = self.kind;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.context_job = Some(rx);
        let spawned = std::thread::Builder::new()
            .name("artwork-preview".into())
            .spawn(move || {
                let result = match kind {
                    Kind::Badge => {
                        sundial::package_authoring::open_shadowkeep_package_manager(&packages)
                            .and_then(|manager| {
                                crate::badge_icon::preview_mask(&manager).map_err(|e| e.to_string())
                            })
                            .map(ContextPreview::Badge)
                    }
                    Kind::Watermark => icon
                        .ok_or_else(|| "Select a weapon to preview its icon.".to_owned())
                        .and_then(|(container, rarity, edit)| {
                            crate::icon_edit::WatermarkPreview::load(
                                &packages, container, rarity, edit,
                            )
                        })
                        .map(|preview| ContextPreview::Watermark(Box::new(preview))),
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        match spawned {
            Ok(worker) => self.context_worker = Some(worker),
            Err(error) => {
                self.context_job = None;
                self.context_error = Some(format!("Could not load preview: {error}"));
            }
        }
    }

    fn poll(&mut self) {
        if let Some(result) = receive(&mut self.importing) {
            match result {
                Ok(Some(source)) => {
                    self.source = source;
                    self.composition = Composition {
                        background: self.composition.background.clone(),
                        ..Default::default()
                    };
                    self.source_texture = None;
                    self.rendered = None;
                    self.error = None;
                }
                Ok(None) => {}
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(result) = receive(&mut self.context_job) {
            let result = if self
                .context_worker
                .take()
                .is_some_and(|worker| worker.join().is_err())
            {
                Err("Artwork preview stopped unexpectedly.".into())
            } else {
                result
            };
            match result {
                Ok(context) => {
                    self.context = Some(context);
                    self.context_error = None;
                }
                Err(error) => self.context_error = Some(error),
            }
            self.rendered = None;
        }
    }

    fn current(&self) -> Result<Artwork, String> {
        self.source.with_composition(self.composition.clone())
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<Action> {
        self.poll();
        self.sync_textures(ctx);
        let size = ctx.screen_rect().size();
        let width = (size.x - 48.0).clamp(260.0, 880.0);
        let mut height = (size.y - 180.0).clamp(180.0, 570.0);
        if width >= 690.0 && self.tab != Tab::Crop {
            height = height.min(430.0);
        }
        let before = self.composition.clone();
        let modal =
            egui::Modal::new(egui::Id::new("presentation-artwork-editor")).show(ctx, |ui| {
                crate::app::workbench_style(ui);
                ui.set_width(width);
                ui.heading(self.kind.title());
                ui.add_space(4.0);
                ui.separator();
                if width >= 690.0 {
                    ui.horizontal_top(|ui| {
                        let left = (width * 0.48).floor();
                        ui.allocate_ui_with_layout(
                            egui::vec2(left, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(left);
                                egui::ScrollArea::vertical()
                                    .id_salt("artwork-preview-scroll")
                                    .max_height(height)
                                    .show(ui, |ui| self.draw_preview(ui));
                            },
                        );
                        let (divider, _) =
                            ui.allocate_exact_size(egui::vec2(1.0, height), egui::Sense::hover());
                        ui.painter().vline(
                            divider.center().x,
                            divider.y_range(),
                            ui.visuals().widgets.noninteractive.bg_stroke,
                        );
                        ui.vertical(|ui| {
                            ui.set_width((width - left - 20.0).max(250.0));
                            egui::ScrollArea::vertical()
                                .id_salt("artwork-controls-scroll")
                                .max_height(height)
                                .show(ui, |ui| self.draw_controls(ui));
                        });
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("artwork-editor-scroll")
                        .max_height(height)
                        .show(ui, |ui| {
                            self.draw_preview(ui);
                            ui.separator();
                            self.draw_controls(ui);
                        });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        return Some(Action::Cancel);
                    }
                    let ready = self.importing.is_none() && self.composition.validate().is_ok();
                    if ui
                        .add_enabled(ready, egui::Button::new("Apply Artwork"))
                        .clicked()
                    {
                        match self.current() {
                            Ok(artwork) => return Some(Action::Apply(artwork)),
                            Err(error) => self.error = Some(error),
                        }
                    }
                    None
                })
                .inner
            });
        if before != self.composition {
            ctx.request_repaint();
        }
        if self.importing.is_some() || self.context_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let close = modal.should_close();
        modal.inner.or_else(|| close.then_some(Action::Cancel))
    }
}

fn receive<T>(pending: &mut Option<Receiver<Result<T, String>>>) -> Option<Result<T, String>> {
    let receiver = pending.as_ref()?;
    let value = match receiver.try_recv() {
        Ok(value) => value,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => Err("Image operation stopped unexpectedly.".into()),
    };
    *pending = None;
    Some(value)
}

pub(super) fn color_image(image: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    )
}
fn load_texture(ctx: &egui::Context, name: &str, image: &image::RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(name, color_image(image), egui::TextureOptions::LINEAR)
}
