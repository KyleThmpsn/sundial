//! One selection-following preview shared by the catalog and object picker.
use crate::model_preview::{
    self, Model,
    render::{self, Camera, Scene},
};
use eframe::egui;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

pub use crate::model_preview::{
    SurfaceOverride,
    weapon::{Appearance, DyeTextureOverride},
};
pub mod chooser;
pub mod still;
mod window;
pub use window::show;
pub use window::{pause_source, stop_reads};

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

/// Playback rate, defaulting to real time so the struct can still derive `Default`.
#[derive(Clone, Copy, PartialEq)]
struct Speed(f32);
impl Default for Speed {
    fn default() -> Self {
        Self(1.0)
    }
}
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
    scene: Scene,
    texture: Option<egui::TextureHandle>,
    asset_textures: BTreeMap<u32, egui::TextureHandle>,
    rendered: Option<(Camera, Scene, [usize; 2], render::Style)>,
    /// The style drawn this frame: the chosen one, or Textured for an object without a mesh.
    style: render::Style,
    /// The style the reader picked, kept while an object without a mesh is shown.
    chosen_style: render::Style,
    playing: bool,
    seconds: f32,
    last_tick: Option<std::time::Instant>,
    rendered_seconds: Option<f32>,
    preserve_camera: bool,
    request: Option<window::Request>,
    source_selection: Option<Selection>,
    navigation: Vec<(u32, String)>,
    particle_page: usize,
    effect_page: usize,
    sound_page: usize,
    child_page: usize,
    component_page: usize,
    reference_page: usize,
    focus_requested: bool,
    source_viewport: Option<egui::ViewportId>,
    paused: bool,
    /// Shared with the reader thread, for its progress and to stop it when nobody is waiting.
    load: Option<model_preview::Load>,
    load_started: Option<std::time::Instant>,
    load_time: Option<std::time::Duration>,
    speed: Speed,
    /// A clip the viewer picked instead of the object's default animation.
    clip: Option<u32>,
    /// Result of the last save, shown until the next one.
    status: Option<String>,
    /// A save still being written. Baking a model's materials takes seconds, so it runs off
    /// the paint thread and reports here.
    saving: Option<mpsc::Receiver<String>>,
    audio_pending:
        Option<mpsc::Receiver<Result<(tempfile::NamedTempFile, std::time::Duration), String>>>,
    playback: Option<tempfile::NamedTempFile>,
    audio_ends: Option<std::time::Instant>,
    audio_tag: Option<u32>,
    audio_status: Option<String>,
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

/// What the preview shows, for the label beside its name.
fn preview_kind(model: &Model) -> &'static str {
    if model.has_object_mesh() {
        if model.particle_sources.is_empty()
            && model.assets.particles.is_empty()
            && model.assets.sounds.is_empty()
            && model.assets.effect_nodes.is_empty()
            && model.assets.lights.is_empty()
        {
            "3D Model"
        } else {
            "Model and Effects"
        }
    } else if model.light_geometry && model.assets.particles.is_empty() {
        "Light Preview"
    } else if !model.assets.sounds.is_empty()
        && model.assets.particles.is_empty()
        && model.assets.effect_nodes.is_empty()
    {
        "Sound Preview"
    } else {
        "Effect Preview"
    }
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
            // Nobody is waiting on the previous object any more.
            if let Some(load) = self.load.take() {
                load.stop();
            }
            self.load_started = None;
            self.load_time = None;
            self.status = None;
            self.stop_audio();
            self.audio_pending = None;
            self.audio_status = None;
            self.clip = None;
            self.particle_page = 0;
            self.effect_page = 0;
            self.sound_page = 0;
            self.child_page = 0;
            self.component_page = 0;
            self.reference_page = 0;
            self.selection = Some(selection.clone());
            self.model = None;
            self.error = None;
            self.texture = None;
            self.asset_textures.clear();
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
                    self.load_time = self.load_started.take().map(|start| start.elapsed());
                    match result {
                        Ok(model) => {
                            if model.has_particle_material_study() {
                                self.seconds = model
                                    .assets
                                    .particles
                                    .iter()
                                    .filter_map(|particle| {
                                        particle.program.as_ref()?.lifetime_default()
                                    })
                                    .next()
                                    .unwrap_or(0.85)
                                    * 0.2;
                            }
                            self.playing = model.animation.is_some()
                                || model.has_shader_animation()
                                || !model.particle_sources.is_empty()
                                || model.has_particle_material_study();
                            self.last_tick = None;
                            self.model = Some(Arc::new(model));
                        }
                        // A load the viewer already abandoned is not an error worth showing.
                        Err(error) if error == model_preview::CANCELLED => {}
                        Err(error) => self.error = Some(error),
                    }
                }
                self.load = None;
                self.pending = None;
            }
        }
        if self.pending.is_none() && self.model.is_none() && self.error.is_none() {
            let (sender, receiver) = mpsc::channel();
            self.pending = Some((selection.clone(), receiver));
            let repaint = ctx.clone();
            let repaint_viewport = ctx.viewport_id();
            let progress = model_preview::Load::default();
            let clip = self.clip;
            self.load = Some(progress.clone());
            self.load_started = Some(std::time::Instant::now());
            std::thread::spawn(move || {
                let load = || match selection.1 {
                    Target::Object(tag) => {
                        model_preview::load_reported(&selection.0, tag, &progress, clip)
                    }
                    Target::Weapon(appearance, _) => {
                        model_preview::weapon::load_reported(&selection.0, &appearance, &progress)
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
            // Fast enough that the stage text and bar read as live progress, not a freeze.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// The wait, showing what the reader is doing and offering a way out of it.
    fn draw_loading(&mut self, ui: &mut egui::Ui) {
        let stage = self.load.as_ref().map(model_preview::Load::stage);
        let stage = stage.unwrap_or_default();
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(if stage.message.is_empty() {
                "Loading"
            } else {
                stage.message.as_str()
            });
        });
        ui.add_space(6.0);
        let bar = if stage.total > 0 {
            egui::ProgressBar::new(stage.done as f32 / stage.total as f32)
                .text(format!("{} / {}", stage.done, stage.total))
        } else {
            // Nothing countable yet, so the bar only shows that work is still moving.
            egui::ProgressBar::new(0.0).animate(true)
        };
        ui.add(bar.desired_height(8.0));
        ui.add_space(6.0);
        if ui.button("Cancel").clicked() {
            if let Some(load) = self.load.take() {
                load.stop();
            }
            self.error = Some("Loading cancelled.".into());
        }
    }

    /// Exports render through the rasterizer, so a saved image matches a headless run.
    fn save_image(&mut self, ctx: &egui::Context, model: &Arc<Model>) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("model-preview.png")
            .add_filter("PNG Image", &["png"])
            .save_file()
        else {
            return;
        };
        let (model, camera, scene, seconds, style) = (
            model.clone(),
            self.camera,
            self.scene,
            self.seconds,
            self.style,
        );
        self.write_in_background(ctx, "Image saved", path, move || {
            let image = render::styled_image(&model, camera, scene, [1600, 1200], seconds, style);
            let pixels: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(egui::Color32::to_array)
                .collect();
            model_preview::export::png(&pixels, image.width(), image.height())
        });
    }

    fn save_model(&mut self, ctx: &egui::Context, model: &Arc<Model>) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("model.glb")
            .add_filter("glTF Binary", &["glb"])
            .save_file()
        else {
            return;
        };
        let (model, seconds) = (model.clone(), self.seconds);
        self.write_in_background(ctx, "Model saved", path, move || {
            model_preview::export::glb(&model, seconds)
        });
    }

    fn save_audio(&mut self, ctx: &egui::Context, tag: u32, decoded: bool) {
        let Some(packages) = self.selection.as_ref().map(|selection| selection.0.clone()) else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!(
                "audio-{tag:08X}.{}",
                if decoded { "wav" } else { "wem" }
            ))
            .add_filter(
                if decoded { "WAV Audio" } else { "Wwise Audio" },
                if decoded { &["wav"][..] } else { &["wem"][..] },
            )
            .save_file()
        else {
            return;
        };
        let access = self
            .request
            .as_ref()
            .and_then(|request| request.access.clone());
        self.write_in_background(ctx, "Audio saved", path, move || {
            let read = || {
                let bytes = model_preview::assets::clip_bytes(&packages, tag)?;
                if decoded {
                    model_preview::assets::decoded_wave(&bytes)
                } else {
                    Ok(bytes)
                }
            };
            match access {
                Some(access) => access.read(read),
                None => read(),
            }
        });
    }

    fn play_audio(&mut self, ctx: &egui::Context, tag: u32) {
        let Some(packages) = self.selection.as_ref().map(|selection| selection.0.clone()) else {
            return;
        };
        self.stop_audio();
        let access = self
            .request
            .as_ref()
            .and_then(|request| request.access.clone());
        let (sender, receiver) = mpsc::channel();
        self.audio_pending = Some(receiver);
        self.audio_tag = Some(tag);
        self.audio_status = Some("Decoding audio".into());
        let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
        std::thread::spawn(move || {
            let decode = || {
                let bytes = model_preview::assets::clip_bytes(&packages, tag)?;
                let wave = model_preview::assets::decoded_wave(&bytes)?;
                let duration = model_preview::assets::wave_duration(&wave)
                    .ok_or("Decoded audio has no valid duration")?;
                let mut file = tempfile::Builder::new()
                    .prefix("sundial-preview-")
                    .suffix(".wav")
                    .tempfile()
                    .map_err(|error| error.to_string())?;
                use std::io::Write;
                file.write_all(&wave).map_err(|error| error.to_string())?;
                file.flush().map_err(|error| error.to_string())?;
                Ok((file, duration))
            };
            let result = match access {
                Some(access) => access.read(decode),
                None => decode(),
            };
            let _ = sender.send(result);
            repaint.request_repaint_of(viewport);
        });
    }

    fn stop_audio(&mut self) {
        #[cfg(windows)]
        if self.playback.is_some() {
            // SAFETY: A null sound name stops the process's asynchronous PlaySound clip.
            unsafe {
                windows_sys::Win32::Media::Audio::PlaySoundW(
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                );
            }
        }
        self.playback = None;
        self.audio_ends = None;
        self.audio_tag = None;
    }

    fn draw_audio_status(&mut self, ui: &mut egui::Ui) {
        if let Some(receiver) = &self.audio_pending {
            match receiver.try_recv() {
                Ok(Ok((file, duration))) => {
                    #[cfg(windows)]
                    {
                        match play_wave(file.path()) {
                            Ok(()) => {
                                self.playback = Some(file);
                                self.audio_ends = Some(std::time::Instant::now() + duration);
                                self.audio_status = Some("Playing audio".into());
                            }
                            Err(error) => self.audio_status = Some(error),
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        drop(file);
                        let _ = duration;
                        self.audio_status = Some("Audio playback is unavailable here".into());
                    }
                    self.audio_pending = None;
                }
                Ok(Err(error)) => {
                    self.audio_status = Some(error);
                    self.audio_pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.audio_status = Some("Audio decoding stopped unexpectedly".into());
                    self.audio_pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if let Some(end) = self.audio_ends {
            if std::time::Instant::now() >= end {
                self.stop_audio();
                self.audio_status = None;
            } else {
                ui.ctx().request_repaint_after(
                    end.saturating_duration_since(std::time::Instant::now()),
                );
            }
        }
        if let Some(status) = &self.audio_status {
            ui.label(status);
        }
    }

    /// Encodes and writes a save on its own thread and reports the outcome to `saving`.
    fn write_in_background(
        &mut self,
        ctx: &egui::Context,
        done: &'static str,
        path: PathBuf,
        encode: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.saving = Some(receiver);
        self.status = None;
        let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
        std::thread::spawn(move || {
            let outcome = encode().and_then(|bytes| {
                crate::storage::replace_file(&path, &bytes).map_err(|error| error.to_string())
            });
            let _ = sender.send(match outcome {
                Ok(()) => done.to_owned(),
                Err(error) => error,
            });
            repaint.request_repaint_of(viewport);
        });
    }

    /// Picks up a finished save, or shows that one is still running.
    fn draw_saving(&mut self, ui: &mut egui::Ui) {
        let Some(receiver) = &self.saving else {
            return;
        };
        match receiver.try_recv() {
            Ok(message) => {
                self.status = Some(message);
                self.saving = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = Some("The save stopped before it finished.".into());
                self.saving = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Saving");
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, name: &str) {
        ui.horizontal(|ui| {
            if !self.navigation.is_empty() && ui.button("Back").clicked() {
                self.navigation.pop();
            }
            let kind = self.model.as_deref().map(preview_kind);
            // The kind goes in first at the right, so a long truncated name cannot push it out.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(kind) = kind {
                    ui.weak(kind);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(name).heading()).truncate())
                        .on_hover_text(name);
                });
            });
        });
        ui.add_space(4.0);
        if let Some(error) = &self.error {
            ui.label(error);
            if ui.button("Retry").clicked() {
                self.error = None;
            }
            return;
        }
        let Some(model) = self.model.clone() else {
            self.draw_loading(ui);
            return;
        };
        if model.triangles.is_empty() && model.particle_sources.is_empty() {
            self.draw_assets(ui, &model);
            return;
        }
        self.style = if model.triangles.is_empty() {
            render::Style::Textured
        } else {
            self.chosen_style
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size.y = 24.0;
            if !model.triangles.is_empty() && (model.has_surface_mesh() || model.particle_geometry)
            {
                egui::ComboBox::from_id_salt("preview-style")
                    .selected_text(self.chosen_style.label())
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for style in [
                            render::Style::Textured,
                            render::Style::Solid,
                            render::Style::Wireframe,
                        ] {
                            ui.selectable_value(&mut self.chosen_style, style, style.label());
                        }
                    });
                self.style = self.chosen_style;
            }
            if ui.button("Reset View").clicked() {
                self.camera = Camera::default();
            }
            ui.menu_button("View", |ui| {
                ui.set_max_width(260.0);
                ui.horizontal(|ui| {
                    ui.label("Background");
                    ui.color_edit_button_srgb(&mut self.scene.background);
                });
                ui.add(egui::Slider::new(&mut self.scene.exposure, 0.2..=3.0).text("Exposure"));
                ui.add(egui::Slider::new(&mut self.scene.key, 0.0..=1.5).text("Key"));
                ui.add(egui::Slider::new(&mut self.scene.fill, 0.0..=1.0).text("Fill"));
                let (mut yaw, mut pitch) = light_angles(self.scene.light);
                let turned = ui.add(
                    egui::Slider::new(&mut yaw, -std::f32::consts::PI..=std::f32::consts::PI)
                        .text("Light Yaw"),
                );
                let tipped = ui.add(
                    egui::Slider::new(&mut pitch, -render::MAX_PITCH..=render::MAX_PITCH)
                        .text("Light Pitch"),
                );
                // Only a real edit rewrites the vector, so an untouched rig stays exact.
                if turned.changed() || tipped.changed() {
                    self.scene.light = light_vector(yaw, pitch);
                }
                if ui.button("Reset Lighting").clicked() {
                    self.scene = Scene {
                        background: self.scene.background,
                        ..Scene::default()
                    };
                }
            });
            ui.add_enabled_ui(self.saving.is_none(), |ui| {
                ui.menu_button("Save", |ui| {
                    if ui.button("Image…").clicked() {
                        ui.close_menu();
                        self.save_image(ui.ctx(), &model);
                    }
                    if !model.triangles.is_empty() {
                        let model_label = if model.light_geometry && !model.has_surface_mesh() {
                            "Light Volume…"
                        } else if model.particle_geometry {
                            "Particle Mesh…"
                        } else {
                            "3D Model…"
                        };
                        if ui.button(model_label).clicked() {
                            ui.close_menu();
                            self.save_model(ui.ctx(), &model);
                        }
                    }
                });
            });
            ui.menu_button("Details", |ui| {
                ui.set_max_width(320.0);
                ui.label(format!("{} triangles", model.triangles.len()));
                ui.label(format!("{} vertices", model.vertices.len()));
                ui.label(format!("{} meshes", model.tags.len()));
                ui.label(format!("{} textures", model.textures.len()));
                if let Some(elapsed) = self.load_time {
                    ui.label(format!("Loaded in {:.2} s", elapsed.as_secs_f32()));
                }
                if model.particle_geometry {
                    if model.has_particle_material_study() {
                        ui.label("One static instance evaluated");
                    } else if model.particle_sources.is_empty() {
                        ui.label("Spawn, motion and timing not shown");
                    } else {
                        ui.label("Packaged sprites · native timing not mapped");
                    }
                } else if !model.particle_sources.is_empty() {
                    ui.label("Packaged sprites at the point emitter · native timing not mapped");
                }
                if model.light_geometry {
                    if model.has_surface_mesh() {
                        ui.label("Linked light volumes listed below");
                    } else {
                        ui.label("Outline only · illumination not simulated");
                    }
                }
                if !model.particle_geometry
                    && !model.light_geometry
                    && model.particle_sources.is_empty()
                {
                    ui.label("Approximate lighting · no runtime physics");
                }
                if let Some(notice) = &model.animation_notice {
                    ui.label(notice);
                }
                for notice in &model.notices {
                    ui.label(notice);
                }
            });
        });
        self.draw_saving(ui);
        self.draw_audio_status(ui);
        if let Some(status) = &self.status {
            ui.label(status);
        }
        if model.particle_geometry && model.particle_sources.is_empty() {
            ui.weak(if model.has_particle_material_study() {
                "Material study · spawn and motion not shown"
            } else {
                "Draw mesh only · emitted particles not shown"
            });
        } else if !model.particle_sources.is_empty() {
            ui.weak("Sprite study · spawn and motion not shown");
        }
        if !model.assets.is_empty() {
            let heading = asset_heading(&model.assets);
            egui::CollapsingHeader::new(heading)
                .default_open(model.triangles.is_empty() && model.particle_sources.is_empty())
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| self.draw_asset_rows(ui, &model));
                });
        }
        ui.add_space(4.0);
        self.draw_playback(ui, &model);
        self.draw_viewport(ui, &model);
    }

    /// Transport controls and the timeline for animated, particle and shader-driven objects.
    fn draw_playback(&mut self, ui: &mut egui::Ui, model: &Model) {
        // An object with clips but no idle clip has no animation until one is picked, so the
        // picker shows for it too.
        if model.animation.is_some()
            || !model.clips.is_empty()
            || model.has_shader_animation()
            || !model.particle_sources.is_empty()
            || model.has_particle_material_study()
        {
            let duration = model.animation.as_ref().map_or_else(
                || {
                    if model.particle_sources.is_empty() {
                        if model.has_particle_material_study() {
                            model
                                .assets
                                .particles
                                .iter()
                                .filter_map(|particle| {
                                    particle.program.as_ref()?.lifetime_default()
                                })
                                .fold(0.05, f32::max)
                        } else {
                            60.0
                        }
                    } else {
                        model
                            .particle_sources
                            .iter()
                            .map(|source| source.period)
                            .fold(0.05, f32::max)
                    }
                },
                |a| a.duration(),
            );
            let now = std::time::Instant::now();
            if self.playing {
                if let Some(last) = self.last_tick {
                    self.seconds += now.duration_since(last).as_secs_f32() * self.speed.0;
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
                ui.label("Speed");
                ui.add(
                    egui::DragValue::new(&mut self.speed.0)
                        .range(0.1..=4.0)
                        .speed(0.02)
                        .prefix("x")
                        .fixed_decimals(2),
                )
                .on_hover_text("Playback speed");
                if model.clips.len() > 1 || (model.animation.is_none() && !model.clips.is_empty()) {
                    let playing = self.clip.or(model.animation.as_ref().map(|a| a.tag));
                    let selected = model
                        .clips
                        .iter()
                        .find(|clip| Some(clip.tag) == playing)
                        .map_or("Clip", |clip| clip.name.as_str());
                    egui::ComboBox::from_id_salt("preview-clip")
                        .selected_text(selected)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for clip in &model.clips {
                                if ui
                                    .selectable_label(Some(clip.tag) == playing, &clip.name)
                                    .clicked()
                                    && Some(clip.tag) != playing
                                {
                                    // Clips are read with the object, so pick one and reload.
                                    self.clip = Some(clip.tag);
                                    self.model = None;
                                    self.texture = None;
                                    self.rendered = None;
                                    self.seconds = 0.0;
                                }
                            }
                        });
                }
                if model.has_shader_animation() {
                    ui.label("Shader Animation")
                        .on_hover_text("Native material timing, UV motion and color changes.");
                } else if let Some(animation) = &model.animation {
                    // Several clips are named by the combo above.
                    if model.clips.len() <= 1 {
                        let name = model
                            .clips
                            .iter()
                            .find(|clip| clip.tag == animation.tag)
                            .map_or("Animation", |clip| clip.name.as_str());
                        ui.label(name).on_hover_text(format!(
                            "Native clip 0x{:08X} · {} frames at {} FPS",
                            animation.tag, animation.frames, animation.fps
                        ));
                    }
                } else if !model.particle_sources.is_empty() {
                    ui.label("Particle Study");
                } else if model.has_particle_material_study() {
                    ui.label("Particle Material Study");
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
    }

    /// The model view: drag and scroll move the camera, then the GPU or software path paints.
    fn draw_viewport(&mut self, ui: &mut egui::Ui, model: &Arc<Model>) {
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(32.0), (available.y - 24.0).max(32.0));
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            // Shift, the right button or the middle button slide the model instead of turning it.
            let panning = ui.input(|i| {
                i.modifiers.shift
                    || i.pointer.button_down(egui::PointerButton::Secondary)
                    || i.pointer.button_down(egui::PointerButton::Middle)
            });
            if panning {
                self.camera.pan[0] += delta.x / size.x.max(1.0);
                self.camera.pan[1] += delta.y / size.y.max(1.0);
            } else {
                self.camera.yaw += delta.x * 0.01;
                self.camera.pitch = (self.camera.pitch + delta.y * 0.01)
                    .clamp(-render::MAX_PITCH, render::MAX_PITCH);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.camera.zoom = (self.camera.zoom * (scroll * 0.002).exp()).clamp(0.25, 5.0);
        }
        if model_preview::gpu::available()
            && !(self.style == render::Style::Textured
                && (!model.particle_sources.is_empty() || model.has_particle_material_study()))
        {
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
                    scene: self.scene,
                    style: self.style,
                    seconds: self.seconds,
                    positions,
                },
            );
            ui.label("Drag to rotate · Shift-drag to pan · Scroll to zoom");
            return;
        }
        let pixels = render_size(size, self.playing);
        // Limit software rendering to 30 FPS even when the surrounding app repaints faster.
        let seconds = (self.seconds * 30.0).floor() / 30.0;
        if self.rendered != Some((self.camera, self.scene, pixels, self.style))
            || self.rendered_seconds != Some(seconds)
        {
            let image =
                render::styled_image(model, self.camera, self.scene, pixels, seconds, self.style);
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "object-model-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.rendered = Some((self.camera, self.scene, pixels, self.style));
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
        ui.label("Drag to rotate · Shift-drag to pan · Scroll to zoom");
    }

    fn draw_assets(&mut self, ui: &mut egui::Ui, model: &Model) {
        let assets = &model.assets;
        let counts = [
            (
                assets.particles.len(),
                "particle system",
                "particle systems",
            ),
            (assets.sounds.len(), "sound event", "sound events"),
            (assets.effect_nodes.len(), "effect node", "effect nodes"),
            (assets.lights.len(), "light", "lights"),
            (model.clips.len(), "animation clip", "animation clips"),
            (assets.children.len(), "child object", "child objects"),
            (assets.components.len(), "component", "components"),
        ];
        let summary: Vec<_> = counts
            .into_iter()
            .filter(|(count, _, _)| *count > 0)
            .map(|(count, singular, plural)| {
                format!("{count} {}", if count == 1 { singular } else { plural })
            })
            .collect();
        if !summary.is_empty() {
            ui.label(summary.join(" · "));
        }
        ui.collapsing("Source Details", |ui| {
            ui.label(format!("Resource 0x{:08X}", assets.source));
            ui.label(format!(
                "Class 0x{:08X} · type {} · {} bytes",
                assets.class, assets.file_type, assets.size
            ));
        });
        if model.triangles.is_empty() && model.particle_sources.is_empty() && assets.image.is_none()
        {
            if !assets.particles.is_empty() {
                ui.group(|ui| {
                    ui.heading("No Visual Output");
                });
            } else if assets.sounds.is_empty() && assets.lights.is_empty() && assets.image.is_none()
            {
                ui.group(|ui| {
                    ui.heading("No Visual Output");
                });
            }
        }
        ui.add_space(8.0);
        self.draw_saving(ui);
        self.draw_audio_status(ui);
        if let Some(status) = &self.status {
            ui.label(status);
        }
        for notice in &model.notices {
            ui.label(notice);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.draw_asset_rows(ui, model);
        });
    }

    fn draw_asset_rows(&mut self, ui: &mut egui::Ui, model: &Model) {
        let assets = &model.assets;
        if assets.is_empty() && model.clips.is_empty() {
            ui.label("No Previewable Components");
        }
        if !model.clips.is_empty() && model.triangles.is_empty() {
            ui.collapsing("Animation Clips", |ui| {
                for clip in &model.clips {
                    if ui
                        .button(format!("{} · 0x{:08X}", clip.name, clip.tag))
                        .clicked()
                    {
                        self.browse(clip.tag, Some(&clip.name));
                    }
                }
            });
        }
        if let Some(texture) = &assets.image {
            self.draw_asset_texture(ui, texture.tag, texture.size, &texture.rgba, "Texture");
        }
        if !assets.effect_nodes.is_empty() {
            egui::CollapsingHeader::new("Effect Sequence")
                .default_open(true)
                .show(ui, |ui| {
                    let page = asset_page(
                        ui,
                        "Effect Nodes",
                        assets.effect_nodes.len(),
                        &mut self.effect_page,
                    );
                    for node in &assets.effect_nodes[page] {
                        if node.index == 0
                            && assets.source != node.source
                            && ui
                                .button(format!("Effect Component 0x{:08X}", node.source))
                                .clicked()
                        {
                            self.browse(node.source, None);
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "{}. {} · class 0x{:08X}",
                                node.index + 1,
                                node.kind(),
                                node.class
                            ));
                            if let Some(timing) = node.timing {
                                ui.label(format!(
                                    "Start {:.4} s · Duration {:.4} s",
                                    timing.start, timing.duration
                                ));
                            }
                            if let Some(tag) = node.target {
                                if ui.button(format!("Open 0x{tag:08X}")).clicked() {
                                    self.browse(tag, None);
                                }
                            }
                        });
                    }
                });
        }
        let particles = asset_page(
            ui,
            "Particle Systems",
            assets.particles.len(),
            &mut self.particle_page,
        );
        for particle in &assets.particles[particles] {
            ui.group(|ui| {
                ui.heading("Particle System");
                ui.label(
                    particle
                        .name
                        .as_deref()
                        .unwrap_or(&format!("0x{:08X}", particle.tag)),
                );
                if assets.source != particle.tag && ui.button("Open Particle System").clicked() {
                    self.browse(particle.tag, particle.name.as_deref());
                }
                if let Some(definition) = particle.definition {
                    if ui
                        .button(format!("Particle Definition 0x{definition:08X}"))
                        .clicked()
                    {
                        self.browse(definition, None);
                    }
                }
                draw_particle_program(ui, particle);
                if !particle.compute_passes.is_empty() {
                    ui.collapsing("Native Compute Passes", |ui| {
                        for pass in &particle.compute_passes {
                            ui.label(format!(
                                "{}: shader 0x{:08X}, material 0x{:08X}, {} bytes",
                                pass.phase, pass.shader, pass.material, pass.size
                            ));
                        }
                    });
                }
                self.draw_particle_links(ui, particle);
                if !particle.material_textures.is_empty() {
                    ui.collapsing("Material Texture Slots", |ui| {
                        for (slot, texture) in &particle.material_textures {
                            self.draw_asset_texture(
                                ui,
                                texture.tag,
                                texture.size,
                                &texture.rgba,
                                &format!("Shader Slot {slot} · 0x{:08X}", texture.tag),
                            );
                        }
                    });
                }
                if !particle.material_samplers.is_empty() {
                    ui.collapsing("Pixel Samplers", |ui| {
                        for (index, sampler) in particle.material_samplers.iter().enumerate() {
                            ui.label(format!(
                                "Sampler {}: U {:?}, V {:?}",
                                index + 1,
                                sampler.u,
                                sampler.v
                            ));
                        }
                    });
                }
                if particle.material_slot_omissions > 0 {
                    ui.label(format!(
                        "{} material texture slots not shown",
                        particle.material_slot_omissions
                    ));
                }
                if let Some(gradient) = &particle.gradient {
                    self.draw_asset_gradient(ui, gradient.tag, gradient.size, &gradient.rgba);
                }
                if let Some(notice) = &particle.notice {
                    ui.label(notice);
                } else if particle.definition.is_none() {
                }
            });
        }
        let sounds = asset_page(
            ui,
            "Sound Events",
            assets.sounds.len(),
            &mut self.sound_page,
        );
        for sound in &assets.sounds[sounds] {
            ui.group(|ui| {
                ui.heading("Sound");
                ui.label(
                    sound
                        .name
                        .as_deref()
                        .unwrap_or(&format!("0x{:08X}", sound.tag)),
                );
                if assets.source != sound.tag && ui.button("Open Sound").clicked() {
                    self.browse(sound.tag, sound.name.as_deref());
                }
                for clip in &sound.clips {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} · {} · {} bytes",
                            clip.name
                                .as_deref()
                                .unwrap_or(&format!("0x{:08X}", clip.tag)),
                            clip.format(),
                            clip.size
                        ));
                        if self.audio_tag == Some(clip.tag) && self.playback.is_some() {
                            if ui.button("Stop").clicked() {
                                self.stop_audio();
                                self.audio_status = None;
                            }
                        } else if ui
                            .add_enabled(
                                cfg!(windows) && self.audio_pending.is_none(),
                                egui::Button::new("Play"),
                            )
                            .clicked()
                        {
                            self.play_audio(ui.ctx(), clip.tag);
                        }
                        if ui
                            .add_enabled(self.saving.is_none(), egui::Button::new("Save WAV…"))
                            .clicked()
                        {
                            self.save_audio(ui.ctx(), clip.tag, true);
                        }
                        if ui
                            .add_enabled(self.saving.is_none(), egui::Button::new("Save Source…"))
                            .clicked()
                        {
                            self.save_audio(ui.ctx(), clip.tag, false);
                        }
                    });
                }
                if let Some(notice) = &sound.notice {
                    ui.label(notice);
                } else if sound.clips.is_empty() {
                }
            });
        }
        for light in &assets.lights {
            ui.group(|ui| {
                ui.heading("Light");
                ui.label(format!("Range: {:.2}", light.radius));
                ui.label(format!(
                    "Volume Offset: {:.2}, {:.2}, {:.2}",
                    light.volume_offset[0], light.volume_offset[1], light.volume_offset[2]
                ));
                if light.half_fov > 0.0 {
                    ui.label(format!(
                        "Half Field of View: {:.1}°",
                        light.half_fov.to_degrees()
                    ));
                }
                if assets.source != light.tag
                    && ui
                        .button(format!("Open Light 0x{:08X}", light.tag))
                        .clicked()
                {
                    self.browse(light.tag, None);
                }
            });
        }
        if !assets.children.is_empty() {
            ui.collapsing("Child Objects", |ui| {
                let page = asset_page(
                    ui,
                    "Child Objects",
                    assets.children.len(),
                    &mut self.child_page,
                );
                for &tag in &assets.children[page] {
                    if ui.button(format!("0x{tag:08X}")).clicked() {
                        self.browse(tag, None);
                    }
                }
            });
        }
        if !assets.components.is_empty() {
            ui.collapsing("Components", |ui| {
                let page = asset_page(
                    ui,
                    "Components",
                    assets.components.len(),
                    &mut self.component_page,
                );
                for component in &assets.components[page] {
                    let header = component
                        .header
                        .map_or("Unknown".into(), |class| format!("0x{class:08X}"));
                    let data = component
                        .data
                        .map_or("Unknown".into(), |class| format!("0x{class:08X}"));
                    if ui
                        .button(format!("0x{:08X} · {header} / {data}", component.tag))
                        .clicked()
                    {
                        self.browse(component.tag, None);
                    }
                }
            });
        }
        if !assets.references.is_empty() {
            ui.collapsing("Resource Handles", |ui| {
                let page = asset_page(
                    ui,
                    "Resource Handles",
                    assets.references.len(),
                    &mut self.reference_page,
                );
                for reference in &assets.references[page] {
                    if ui
                        .button(format!(
                            "{} · class 0x{:08X} · type {}",
                            reference
                                .name
                                .as_deref()
                                .unwrap_or(&format!("0x{:08X}", reference.tag)),
                            reference.class,
                            reference.file_type
                        ))
                        .clicked()
                    {
                        self.browse(reference.tag, reference.name.as_deref());
                    }
                }
            });
        }
    }

    fn browse(&mut self, tag: u32, name: Option<&str>) {
        if self
            .navigation
            .last()
            .is_some_and(|(current, _)| *current == tag)
        {
            return;
        }
        self.navigation.push((
            tag,
            name.map(str::to_owned)
                .unwrap_or_else(|| format!("0x{tag:08X}")),
        ));
    }

    fn draw_asset_texture(
        &mut self,
        ui: &mut egui::Ui,
        tag: u32,
        size: [usize; 2],
        rgba: &[u8],
        label: &str,
    ) {
        let handle = self.asset_textures.entry(tag).or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba);
            ui.ctx().load_texture(
                format!("asset-{tag:08X}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        let scale = (280.0 / size[0].max(size[1]) as f32).min(1.0);
        ui.image((
            handle.id(),
            egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale),
        ));
        ui.label(label);
    }

    fn draw_asset_gradient(&mut self, ui: &mut egui::Ui, tag: u32, size: [usize; 2], rgba: &[u8]) {
        let handle = self.asset_textures.entry(tag).or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba);
            ui.ctx().load_texture(
                format!("asset-{tag:08X}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        ui.label("Particle Color Ramp");
        ui.image((handle.id(), egui::vec2(280.0, 24.0)));
    }

    /// Buttons that open the emitter, particle mesh and material of one particle system.
    fn draw_particle_links(
        &mut self,
        ui: &mut egui::Ui,
        particle: &model_preview::assets::Particle,
    ) {
        if let Some(emitter) = particle.emitter {
            if ui.button(format!("Emitter 0x{emitter:08X}")).clicked() {
                self.browse(emitter, None);
            }
        }
        if let Some(model) = particle.emitter_model {
            if ui.button(format!("Particle Mesh 0x{model:08X}")).clicked() {
                self.browse(model, None);
            }
        }
        if let Some(material) = particle.material {
            if ui.button(format!("Material 0x{material:08X}")).clicked() {
                self.browse(material, None);
            }
        }
    }
}

/// The decoded particle program, or a note when a definition exists without one.
fn draw_particle_program(ui: &mut egui::Ui, particle: &model_preview::assets::Particle) {
    if let Some(program) = &particle.program {
        egui::CollapsingHeader::new("Particle Program")
            .id_salt((particle.tag, "program"))
            .show(ui, |ui| {
                ui.label(format!(
                    "{} expression bytes · {} constants · {} default vectors",
                    program.bytecode.len(),
                    program.constants.len(),
                    program.defaults.len()
                ));
                ui.label(format!("Section Sizes: {:?}", program.sections));
                if let Some(seconds) = program.lifetime_default() {
                    ui.label(format!(
                        "Default Lifetime: {seconds:.3} s · Compiled Ceiling: {:.3} s",
                        program.lifetime_ceiling
                    ));
                } else {
                    ui.label(format!(
                        "Compiled Lifetime Ceiling: {:.3} s",
                        program.lifetime_ceiling
                    ));
                }
                if let Some([first, second]) = program.state_routes() {
                    ui.label(format!(
                        "State Vector A: bank {} vector {} to bank {} vector {}",
                        first.0.bank,
                        first.0.scalar / 4,
                        first.1.bank,
                        first.1.scalar / 4
                    ));
                    ui.label(format!(
                        "State Vector B: bank {} vector {} to bank {} vector {}",
                        second.0.bank,
                        second.0.scalar / 4,
                        second.1.bank,
                        second.1.scalar / 4
                    ));
                    ui.label(format!(
                        "State Copy: {} bytes",
                        program.section(3).map_or(0, |code| code.len())
                    ));
                }
                let defaults = program
                    .routes
                    .iter()
                    .filter(|route| route.is_some_and(|route| route.bank == 6))
                    .count();
                ui.label(format!("{defaults} parameters use stored defaults"));
                egui::CollapsingHeader::new("Output Routes")
                    .id_salt((particle.tag, "routes"))
                    .show(ui, |ui| match program.register_writes() {
                        Ok(writes) => {
                            for (output, route) in program.routes.iter().enumerate() {
                                let Some(route) = route else { continue };
                                let value = program
                                    .default_for(output)
                                    .map(|value| format!(" = {value:.3}"))
                                    .unwrap_or_default();
                                let mut selectors = writes
                                    .iter()
                                    .filter(|write| {
                                        write.bank == route.bank && write.slot == route.scalar / 4
                                    })
                                    .map(|write| (write.section, write.selector))
                                    .collect::<Vec<_>>();
                                selectors.sort_unstable();
                                selectors.dedup();
                                let touched = if selectors.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        " · writes (section/selector): {}",
                                        selectors
                                            .iter()
                                            .map(|(section, selector)| format!(
                                                "{section}/{selector}"
                                            ))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )
                                };
                                ui.label(format!(
                                    "{output}: {}:{}{value}{touched}",
                                    route.bank, route.scalar
                                ));
                            }
                            ui.weak(
                                "Selectors 0 and 1 vectors · 2 and 3 halves · 4 to 7 scalar lanes",
                            );
                        }
                        Err(error) => {
                            ui.label(format!("Register write map unavailable: {error}"));
                        }
                    });
            });
    } else if particle.definition.is_some() {
        ui.weak("Particle program not decoded");
    }
}

fn asset_heading(assets: &model_preview::assets::Assets) -> String {
    let mut kinds = Vec::new();
    for (count, singular, plural) in [
        (
            assets.particles.len(),
            "Particle System",
            "Particle Systems",
        ),
        (assets.sounds.len(), "Sound Event", "Sound Events"),
        (assets.lights.len(), "Light", "Lights"),
        (assets.effect_nodes.len(), "Effect Node", "Effect Nodes"),
    ] {
        if count > 0 {
            kinds.push(format!(
                "{count} {}",
                if count == 1 { singular } else { plural }
            ));
        }
    }
    if kinds.is_empty() {
        "Assets and Effects".to_owned()
    } else {
        format!(
            "Assets and Effects ({})",
            kinds.into_iter().take(2).collect::<Vec<_>>().join(", ")
        )
    }
}

fn asset_page(
    ui: &mut egui::Ui,
    label: &str,
    total: usize,
    page: &mut usize,
) -> std::ops::Range<usize> {
    const SIZE: usize = 16;
    *page = (*page).min(total.saturating_sub(1) / SIZE);
    let start = *page * SIZE;
    let end = (start + SIZE).min(total);
    if total > SIZE {
        ui.horizontal(|ui| {
            ui.label(format!("{label} {}–{end} of {total}", start + 1));
            if ui
                .add_enabled(*page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                *page -= 1;
            }
            if ui
                .add_enabled(end < total, egui::Button::new("Next"))
                .clicked()
            {
                *page += 1;
            }
        });
    }
    let start = *page * SIZE;
    start..(start + SIZE).min(total)
}

#[cfg(windows)]
fn play_wave(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a terminated file path that remains valid during this call. The
    // temporary file is retained by Preview while asynchronous playback is active.
    let started = unsafe {
        PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_ASYNC | SND_FILENAME | SND_NODEFAULT,
        )
    };
    if started == 0 {
        return Err("The system audio player could not play this clip".into());
    }
    Ok(())
}

/// Spherical angles for the key light. Reading them back never disturbs the stored vector,
/// so an untouched rig keeps the exact approved direction.
fn light_angles(light: [f32; 3]) -> (f32, f32) {
    (light[0].atan2(-light[2]), light[1].clamp(-1.0, 1.0).asin())
}

fn light_vector(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [cp * sy, sp, -cp * cy]
}

fn render_size(size: egui::Vec2, playing: bool) -> [usize; 2] {
    // Keep CPU-rendered motion responsive, restoring inspection detail on pause.
    let resolution = if playing { 320.0 } else { 640.0 };
    let density = (resolution / size.x.max(size.y)).min(1.0);
    [(size.x * density) as usize, (size.y * density) as usize]
}

#[cfg(test)]
mod tests;
