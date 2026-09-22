//! One selection-following preview shared by the catalog and object picker.
use crate::model_preview::{
    self, Model,
    render::{self, Camera},
};
use eframe::egui;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

pub use crate::model_preview::weapon::Appearance;
pub mod chooser;
mod window;
pub use window::pause_source;
pub use window::show;

/// Appearance before socket plugs are composed, allowing a candidate to replace one socket.
#[derive(Clone, Debug)]
pub struct Loadout {
    pub arrangement: u16,
    pub dyes: [Vec<(i8, u16)>; 3],
    pub plugs: Vec<Option<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Object(u32),
    Weapon(Appearance, Option<u64>),
}
type Selection = (PathBuf, Target);
type Pending = (Selection, mpsc::Receiver<Result<Model, String>>);

#[derive(Default)]
struct Preview {
    open: bool,
    owner: Option<egui::Id>,
    selection: Option<Selection>,
    pending: Option<Pending>,
    model: Option<Arc<Model>>,
    gpu: model_preview::gpu::Shared,
    error: Option<String>,
    camera: Camera,
    texture: Option<egui::TextureHandle>,
    rendered: Option<(Camera, [usize; 2], render::Style)>,
    style: render::Style,
    playing: bool,
    seconds: f32,
    last_tick: Option<std::time::Instant>,
    rendered_seconds: Option<f32>,
    preserve_camera: bool,
    request: Option<window::Request>,
    focus_requested: bool,
    source_viewport: Option<egui::ViewportId>,
    paused: bool,
}

/// Opens a preview and follows subsequent selections while the window remains open.
pub fn selection(ui: &mut egui::Ui, packages: Option<&Path>, tag: u32, name: &str) {
    let id = window::source_id(ui, "object");
    window::launcher(
        ui,
        id,
        packages.map(|path| window::Request::new(path, Target::Object(tag), name, None, false)),
    );
}

/// Opens the shared native preview for this viewport's authored appearance.
pub fn weapon(ui: &mut egui::Ui, packages: &Path, appearance: Appearance, name: &str) {
    let id = weapon_source(ui.ctx());
    weapon_preview(ui, packages, appearance, name, id, None);
}

/// Inspector reads participate in package suspension and invalidate after package changes.
pub(crate) fn inspected_weapon(
    ui: &mut egui::Ui,
    packages: &Path,
    appearance: Appearance,
    name: &str,
    access: Arc<crate::catalog::PackageInspectionAccess>,
) {
    // One model per inspector viewport/layer, including when navigation changes the item hash.
    let id = inspector_source(ui);
    weapon_preview(ui, packages, appearance, name, id, Some(access));
}

fn inspector_source(ui: &egui::Ui) -> egui::Id {
    egui::Id::new((
        "inspector-model-preview",
        ui.ctx().viewport_id(),
        ui.layer_id().id,
    ))
}

pub(crate) fn inspected_unavailable(ui: &egui::Ui) {
    window::clear_source(ui.ctx(), inspector_source(ui));
}

fn weapon_preview(
    ui: &mut egui::Ui,
    packages: &Path,
    appearance: Appearance,
    name: &str,
    id: egui::Id,
    access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
) {
    let generation = access.as_ref().map(|access| access.generation());
    window::launcher(
        ui,
        id,
        Some(window::Request::new(
            packages,
            Target::Weapon(appearance, generation),
            name,
            access,
            false,
        )),
    );
}

fn weapon_source(ctx: &egui::Context) -> egui::Id {
    egui::Id::new(("weapon-appearance-preview", ctx.viewport_id()))
}

/// Keeps an opened authored preview current while the user edits another tab.
pub fn follow_weapon(ctx: &egui::Context, packages: &Path, appearance: Appearance, name: &str) {
    window::follow(
        ctx,
        weapon_source(ctx),
        window::Request::new(
            packages,
            Target::Weapon(appearance, None),
            name,
            None,
            false,
        ),
    );
}

/// Whether this viewport currently owns the shared authored preview.
pub fn weapon_is_open(ctx: &egui::Context) -> bool {
    window::owned_by(ctx, weapon_source(ctx))
}

impl Preview {
    #[cfg(test)]
    fn sync(&mut self, ctx: &egui::Context, selection: Selection) {
        self.sync_with_access(ctx, selection, None);
    }

    fn sync_with_access(
        &mut self,
        ctx: &egui::Context,
        selection: Selection,
        access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
    ) {
        if self.selection.as_ref() != Some(&selection) {
            let keep_camera = self.preserve_camera
                || self.selection.as_ref().is_some_and(|old| {
                    old.0 == selection.0
                        && matches!((&old.1, &selection.1),
                    (Target::Weapon(a, _), Target::Weapon(b, _)) if a.arrangement == b.arrangement)
                });
            self.selection = Some(selection.clone());
            self.model = None;
            self.error = None;
            self.texture = None;
            self.rendered = None;
            if !keep_camera {
                self.camera = Camera::default();
            }
            self.playing = false;
            self.seconds = 0.0;
            self.last_tick = None;
            self.rendered_seconds = None;
        }
        if let Some((source, receiver)) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "The model reader stopped before returning a result.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                if *source == selection {
                    match result {
                        Ok(model) => {
                            self.playing =
                                model.animation.is_some() || model.has_shader_animation();
                            self.last_tick = None;
                            self.model = Some(Arc::new(model));
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                self.pending = None;
            }
        }
        if self.pending.is_none() && self.model.is_none() && self.error.is_none() {
            let (sender, receiver) = mpsc::channel();
            self.pending = Some((selection.clone(), receiver));
            let repaint = ctx.clone();
            let repaint_viewport = ctx.viewport_id();
            std::thread::spawn(move || {
                let load = || match selection.1 {
                    Target::Object(tag) => model_preview::load(&selection.0, tag),
                    Target::Weapon(appearance, _) => {
                        model_preview::weapon::load(&selection.0, &appearance)
                    }
                };
                let result = match access {
                    Some(access) => access.read(load),
                    None => load(),
                };
                let _ = sender.send(result);
                repaint.request_repaint_of(repaint_viewport);
            });
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, name: &str) {
        ui.add(egui::Label::new(egui::RichText::new(name).heading()).truncate())
            .on_hover_text(name);
        ui.add_space(4.0);
        if let Some(error) = &self.error {
            ui.label(error);
            if ui.button("Retry").clicked() {
                self.error = None;
            }
            return;
        }
        let Some(model) = &self.model else {
            ui.spinner();
            ui.label("Loading Model…");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("preview-style")
                .selected_text(self.style.label())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for style in [
                        render::Style::Textured,
                        render::Style::Solid,
                        render::Style::Wireframe,
                    ] {
                        ui.selectable_value(&mut self.style, style, style.label());
                    }
                });
            if ui.button("Reset View").clicked() {
                self.camera = Camera::default();
            }
            ui.menu_button("Details", |ui| {
                ui.set_max_width(320.0);
                ui.label(format!("{} triangles", model.triangles.len()));
                ui.label("Approximate lighting. Runtime physics are not shown.");
                if let Some(notice) = &model.animation_notice {
                    ui.label(notice);
                }
                for notice in &model.notices {
                    ui.label(notice);
                }
            });
        });
        ui.add_space(4.0);
        if model.animation.is_some() || model.has_shader_animation() {
            let duration = model.animation.as_ref().map_or(60.0, |a| a.duration());
            let now = std::time::Instant::now();
            if self.playing {
                if let Some(last) = self.last_tick {
                    self.seconds += now.duration_since(last).as_secs_f32();
                    if !model.has_shader_animation() {
                        self.seconds = self.seconds.rem_euclid(duration);
                    }
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
            }
            self.last_tick = Some(now);
            ui.horizontal(|ui| {
                if ui
                    .button(if self.playing { "Pause" } else { "Play" })
                    .clicked()
                {
                    self.playing = !self.playing;
                }
                if ui.button("Restart").clicked() {
                    self.seconds = 0.0;
                }
                if model.has_shader_animation() {
                    ui.label("Shader Animation")
                        .on_hover_text("Native material timing, UV motion and color changes.");
                } else if let Some(animation) = &model.animation {
                    ui.label("Idle").on_hover_text(format!(
                        "Native clip 0x{:08X} - {} frames at {} FPS",
                        animation.tag, animation.frames, animation.fps
                    ));
                }
            });
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(80.0);
                let timeline_end = if model.has_shader_animation() {
                    self.seconds.max(60.0)
                } else {
                    duration
                };
                let response = ui.add(
                    egui::Slider::new(&mut self.seconds, 0.0..=timeline_end)
                        // Rounding the running clock must not look like a user edit.
                        .clamping(egui::SliderClamping::Edits)
                        .suffix(" s")
                        .fixed_decimals(2),
                );
                if response.dragged() || response.changed() {
                    self.playing = false;
                }
            });
        }
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(32.0), (available.y - 24.0).max(32.0));
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            self.camera.yaw += delta.x * 0.01;
            self.camera.pitch = (self.camera.pitch + delta.y * 0.01).clamp(-1.5, 1.5);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.camera.zoom = (self.camera.zoom * (scroll * 0.002).exp()).clamp(0.25, 5.0);
        }
        if model_preview::gpu::available() {
            let positions = model
                .animation
                .as_ref()
                .map(|a| Arc::new(a.vertices(model, self.seconds.rem_euclid(a.duration()))));
            self.gpu.paint(
                ui,
                rect,
                model_preview::gpu::Frame {
                    model: model.clone(),
                    camera: self.camera,
                    style: self.style,
                    seconds: self.seconds,
                    positions,
                },
            );
            ui.label("Drag to rotate · Scroll to zoom");
            return;
        }
        let pixels = render_size(size, self.playing);
        // Limit software rendering to 30 FPS even when the surrounding app repaints faster.
        let seconds = (self.seconds * 30.0).floor() / 30.0;
        if self.rendered != Some((self.camera, pixels, self.style))
            || self.rendered_seconds != Some(seconds)
        {
            let image = render::styled_image(model, self.camera, pixels, seconds, self.style);
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "object-model-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.rendered = Some((self.camera, pixels, self.style));
            self.rendered_seconds = Some(seconds);
        }
        if let Some(texture) = &self.texture {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        ui.label("Drag to rotate · Scroll to zoom");
    }
}

fn render_size(size: egui::Vec2, playing: bool) -> [usize; 2] {
    // Keep CPU-rendered motion responsive, restoring inspection detail on pause.
    let resolution = if playing { 320.0 } else { 640.0 };
    let density = (resolution / size.x.max(size.y)).min(1.0);
    [(size.x * density) as usize, (size.y * density) as usize]
}

#[cfg(test)]
mod tests;
