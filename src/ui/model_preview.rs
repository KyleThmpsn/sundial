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
    appearance::{Appearance, DyeTextureOverride},
    local::{LocalAppearance, LocalScene, stock_default_dyes},
};
pub mod chooser;
mod details;
mod fps;
pub mod icon;
mod image_job;
mod options;
pub use options::{Options, set_options};
mod rotation;
pub mod still;
mod window;
pub use crate::model_preview::PackageRead;
pub use crate::model_preview::timing;
pub use window::show;
pub use window::{pause_source, stop_reads};

/// Waits for background package reads to end, the model previews' and any other counted by
/// [`PackageRead`], up to `limit`, for an operation about to replace or remove packages. Pause the
/// previews first so they start no new read. Returns whether every read ended.
pub fn wait_for_package_reads(limit: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    while model_preview::package_reads_running() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    true
}

/// Appearance before socket plugs are composed, allowing a candidate to replace one socket.
#[derive(Clone, Debug)]
pub struct Loadout {
    pub arrangement: u16,
    pub dyes: [Vec<(i8, u16)>; 3],
    pub plugs: Vec<Option<u32>>,
}

/// The loading block: a spinner line, the bar line and the Cancel button with their spacing.
const LOADING_BLOCK_HEIGHT: f32 = 84.0;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Object(u32),
    Weapon(Appearance, Option<u64>),
    Local(LocalAppearance, Option<u32>),
}
type Selection = (PathBuf, Target);

fn clip_bytes(selection: &Selection, tag: u32) -> Result<Vec<u8>, String> {
    match &selection.1 {
        Target::Local(appearance, _) => appearance.clip_bytes(&selection.0, tag),
        _ => model_preview::assets::clip_bytes(&selection.0, tag),
    }
}

/// Reads a clip's bytes under `read`, the count an install waits on, and the Inspector's
/// `access` where there is one. Both are let go once the bytes are in hand.
fn read_clip(
    read: PackageRead,
    access: Option<&crate::catalog::PackageInspectionAccess>,
    selection: &Selection,
    tag: u32,
) -> Result<Vec<u8>, String> {
    let bytes = match access {
        Some(access) => access.read(|| clip_bytes(selection, tag)),
        None => clip_bytes(selection, tag),
    };
    drop(read);
    bytes
}

/// Playback rate, defaulting to real time so the struct can still derive `Default`.
#[derive(Clone, Copy, PartialEq)]
struct Speed(f32);
impl Default for Speed {
    fn default() -> Self {
        Self(1.0)
    }
}
type Pending = (Selection, mpsc::Receiver<Result<Model, String>>);
type DecodedAudio = Result<(tempfile::NamedTempFile, std::time::Duration), String>;

/// The shared viewer, grouped by how long each part lives. Each group has one reset, the only
/// place its fields are cleared. Fields directly on the viewer last for the app session: the GL
/// resources, the view the reader chose, the read in flight and a save being written.
#[derive(Default)]
struct Preview {
    /// The native window and the request that drives it, replaced by [`Preview::close`].
    window: Window,
    /// The selection on show and what was read and drawn for it, replaced by
    /// [`Preview::show_selection`].
    shown: Shown,
    /// One sound clip's decode and playback, ended by [`Audio::reset`].
    audio: Audio,
    /// Uploads, linked programs and exports on the app's one GL context. Never reset: it
    /// replaces a model's GL objects when the next model is drawn.
    gpu: model_preview::gpu::Shared,
    /// Kept across selections of the same model, and restored by [`Preview::reset_view`].
    camera: Camera,
    auto_rotate: bool,
    /// The orbit's last step, cleared whenever the orbit holds so it never jumps.
    rotation_tick: Option<f64>,
    /// Lighting and background, restored by Reset Lighting. Its Particle Study belongs to the
    /// selection, so [`Preview::show_selection`] turns it off.
    scene: Scene,
    /// The style drawn this frame: the chosen one, or Textured for an object without a mesh.
    style: render::Style,
    /// The style the reader picked, kept while an object without a mesh is shown.
    chosen_style: render::Style,
    speed: Speed,
    /// The Assets and Effects browser's filter.
    asset_query: String,
    /// The read in flight. It outlives its selection: a newer read waits for it, so two never
    /// overlap, and a closed viewer still drains it.
    pending: Option<Pending>,
    /// A save still being written. Baking a model's materials takes seconds, so it runs off
    /// the paint thread and reports to [`Shown::status`].
    saving: Option<mpsc::Receiver<String>>,
}

/// The native window, from open to close.
#[derive(Default)]
struct Window {
    open: bool,
    /// The launcher whose requests the viewer follows.
    owner: Option<egui::Id>,
    /// The viewport that opened the viewer, for its options and package pauses.
    source_viewport: Option<egui::ViewportId>,
    focus_requested: bool,
    paused: bool,
    request: Option<window::Request>,
    source: Source,
    details: Details,
}

/// The request's own selection and the resources opened from it, which Back returns through.
/// A request for another selection replaces it.
#[derive(Default)]
struct Source {
    selection: Option<Selection>,
    navigation: Vec<(u32, String)>,
}

/// The Details window beside the viewer, replaced when it closes.
#[derive(Default)]
struct Details {
    open: bool,
    focus_requested: bool,
    /// The model the Details window last saw, so a new one repaints it.
    shown: Option<usize>,
}

/// One selection's state. Nothing here survives a change of selection or a close.
#[derive(Default)]
struct Shown {
    selection: Option<Selection>,
    /// A clip the viewer picked instead of the object's default animation.
    clip: Option<u32>,
    model: Option<Arc<Model>>,
    error: Option<String>,
    /// Shared with the reader thread, for its progress and to stop it when nobody is waiting.
    /// Stopped only by [`Shown::stop_load`].
    load: Option<model_preview::Load>,
    load_started: Option<std::time::Instant>,
    load_time: Option<std::time::Duration>,
    /// What was drawn of the loaded model, dropped with it by [`Shown::unload`].
    frames: Frames,
    /// The FPS overlay, counted across every model this selection reads.
    fps: fps::Counter,
    software_frames: u64,
    /// The entry the Assets and Effects browser shows.
    asset_entry: Option<details::Entry>,
    /// Images the browser uploaded, by tag.
    asset_textures: BTreeMap<u32, egui::TextureHandle>,
    /// Result of the last save, shown until another save starts or the selection changes.
    status: Option<String>,
}

/// The loaded model's playback clock and the software image drawn of it.
#[derive(Default)]
struct Frames {
    playing: bool,
    seconds: f32,
    last_tick: Option<std::time::Instant>,
    texture: Option<egui::TextureHandle>,
    software: image_job::Job,
    /// What the software image shows.
    rendered: Option<(Camera, Scene, [usize; 2], render::Style)>,
    rendered_seconds: Option<f32>,
}

impl Shown {
    /// Stops this selection's read. Its receiver stays in [`Preview::pending`] to drain.
    fn stop_load(&mut self) {
        if let Some(load) = self.load.take() {
            load.stop();
        }
    }

    /// Drops the model and what was drawn of it, so the same selection is read again, as when
    /// another clip is picked.
    fn unload(&mut self) {
        self.model = None;
        self.frames = Frames::default();
    }
}

/// One sound clip being decoded or played.
#[derive(Default)]
struct Audio {
    pending: Option<mpsc::Receiver<DecodedAudio>>,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// The decoded file, retained while the system player reads it.
    playback: Option<tempfile::NamedTempFile>,
    ends: Option<std::time::Instant>,
    tag: Option<u32>,
    status: Option<String>,
}

impl Audio {
    /// Cancels the decode, stops the clip and clears its message.
    fn reset(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
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
        *self = Self::default();
    }
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

/// Opens the authored viewer on pinned assets before they have been built or installed.
pub fn local_weapon(ui: &mut egui::Ui, packages: &Path, appearance: LocalAppearance, name: &str) {
    window::launcher(
        ui,
        weapon_source(ui.ctx()),
        Some(window::Request::new(
            packages,
            Target::Local(appearance, None),
            name,
            None,
            false,
        )),
    );
}

/// Open an account item's saved appearance directly, retaining package inspection guards.
pub(crate) fn open_inspected_weapon(
    ctx: &egui::Context,
    packages: &Path,
    appearance: Appearance,
    name: &str,
    access: Arc<crate::catalog::PackageInspectionAccess>,
) {
    let generation = access.generation();
    window::open_request(
        ctx,
        egui::Id::new(("item-menu-model-preview", ctx.viewport_id())),
        window::Request::new(
            packages,
            Target::Weapon(appearance, Some(generation)),
            name,
            Some(access),
            false,
        ),
    );
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

/// Keeps the authored viewer on the imported source when another editor tab is active.
pub fn follow_local_weapon(
    ctx: &egui::Context,
    packages: &Path,
    appearance: LocalAppearance,
    name: &str,
) {
    window::follow(
        ctx,
        weapon_source(ctx),
        window::Request::new(packages, Target::Local(appearance, None), name, None, false),
    );
}

/// Whether this viewport currently owns the shared authored preview.
pub fn weapon_is_open(ctx: &egui::Context) -> bool {
    window::owned_by(ctx, weapon_source(ctx))
}

/// An icon in the top-right corner of the preview `id` at `over` that opens `appearance` in the
/// shared viewer, which has the full tools. It shows while the pointer is on the preview. While
/// the viewer stays open it follows the preview's appearance.
pub fn pop_out(
    ui: &mut egui::Ui,
    id: egui::Id,
    over: egui::Rect,
    packages: &Path,
    (appearance, name): (Appearance, &str),
) -> Option<egui::Response> {
    pop_out_with_access(ui, (id, id), over, packages, (appearance, name), None)
}

/// An Inspector corner launcher retains package access after its source view closes. The still
/// `id` is the window's, while `owner` names the item, so a viewer opened on one item stays on
/// it as the Inspector moves to another.
pub(crate) fn inspected_pop_out(
    ui: &mut egui::Ui,
    (id, owner): (egui::Id, egui::Id),
    over: egui::Rect,
    packages: &Path,
    subject: (Appearance, &str),
    access: Arc<crate::catalog::PackageInspectionAccess>,
) -> Option<egui::Response> {
    pop_out_with_access(ui, (id, owner), over, packages, subject, Some(access))
}

fn pop_out_with_access(
    ui: &mut egui::Ui,
    (id, owner): (egui::Id, egui::Id),
    over: egui::Rect,
    packages: &Path,
    (appearance, name): (Appearance, &str),
    access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
) -> Option<egui::Response> {
    still::playback_control(ui, id, over);
    let owner = pop_out_source(ui.ctx(), owner);
    let generation = access.as_ref().map(|access| access.generation());
    window::corner_launcher(
        ui,
        owner,
        over,
        window::Request::new(
            packages,
            Target::Weapon(appearance, generation),
            name,
            access,
            false,
        ),
    )
}

/// The corner icon [`pop_out`] draws, for a preview of the object `tag` drawn with
/// [`still::show_object`].
pub fn pop_out_object(
    ui: &mut egui::Ui,
    id: egui::Id,
    over: egui::Rect,
    packages: &Path,
    (tag, name): (u32, &str),
) -> Option<egui::Response> {
    still::playback_control(ui, id, over);
    let owner = pop_out_source(ui.ctx(), id);
    window::corner_launcher(
        ui,
        owner,
        over,
        window::Request::new(packages, Target::Object(tag), name, None, false),
    )
}

/// Open an unbuilt appearance in the shared viewer, retaining its private asset reader.
pub fn pop_out_local(
    ui: &mut egui::Ui,
    id: egui::Id,
    over: egui::Rect,
    packages: &Path,
    (appearance, name): (LocalAppearance, &str),
) -> Option<egui::Response> {
    still::playback_control(ui, id, over);
    window::corner_launcher(
        ui,
        pop_out_source(ui.ctx(), id),
        over,
        window::Request::new(packages, Target::Local(appearance, None), name, None, false),
    )
}

/// Updates an inline preview's separate viewer while the editor is on another tab.
pub fn follow_pop_out(
    ctx: &egui::Context,
    id: egui::Id,
    packages: &Path,
    appearance: Appearance,
    name: &str,
) {
    window::follow(
        ctx,
        pop_out_source(ctx, id),
        window::Request::new(
            packages,
            Target::Weapon(appearance, None),
            name,
            None,
            false,
        ),
    );
}

/// The imported counterpart of [`follow_pop_out`], retaining the source reader.
pub fn follow_pop_out_local(
    ctx: &egui::Context,
    id: egui::Id,
    packages: &Path,
    appearance: LocalAppearance,
    name: &str,
) {
    window::follow(
        ctx,
        pop_out_source(ctx, id),
        window::Request::new(packages, Target::Local(appearance, None), name, None, false),
    );
}

/// Whether the shared viewer is open on the preview `id` through its `pop_out`.
pub fn popped_out(ctx: &egui::Context, id: egui::Id) -> bool {
    window::owned_by(ctx, pop_out_source(ctx, id))
}

fn pop_out_source(ctx: &egui::Context, id: egui::Id) -> egui::Id {
    egui::Id::new(("model-preview-pop-out", id, ctx.viewport_id()))
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
    /// Puts `selection` on show, or nothing when the window closes. The previous selection's
    /// read is stopped, its sound silenced and its model, images, clock, clip, browser entry
    /// and save message dropped. The view the reader chose stays.
    fn show_selection(&mut self, selection: Option<Selection>) {
        // Nobody is waiting on the previous object any more.
        self.shown.stop_load();
        self.shown = Shown {
            selection,
            ..Shown::default()
        };
        self.audio.reset();
        self.scene.particle_study = false;
        self.hold_clocks();
    }

    /// Returns the camera to its starting view, with the orbit starting again from there.
    fn reset_view(&mut self) {
        self.camera = Camera::default();
        self.rotation_tick = None;
    }

    /// Holds the playback clock and the orbit while the canvas is not drawn, so neither jumps
    /// when it is drawn again.
    fn hold_clocks(&mut self) {
        self.shown.frames.last_tick = None;
        self.rotation_tick = None;
    }

    fn sync_with_access(
        &mut self,
        ctx: &egui::Context,
        selection: Selection,
        access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
        preserve_camera: bool,
    ) {
        if self.shown.selection.as_ref() != Some(&selection) {
            let keep_camera = preserve_camera
                || self.shown.selection.as_ref().is_some_and(|old| {
                    old.0 == selection.0
                        && matches!((&old.1, &selection.1),
                    (Target::Weapon(a, _), Target::Weapon(b, _)) if a.arrangement == b.arrangement)
                });
            self.show_selection(Some(selection.clone()));
            if !keep_camera {
                self.reset_view();
            }
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
                    let shown = &mut self.shown;
                    shown.load_time = shown.load_started.take().map(|start| start.elapsed());
                    match result {
                        Ok(model) => {
                            shown.frames.playing = model.has_animation()
                                || model.has_cloth()
                                || model.has_shader_animation();
                            shown.frames.last_tick = None;
                            shown.model = Some(Arc::new(model));
                        }
                        // A load the viewer already abandoned is not an error worth showing.
                        Err(error) if error == model_preview::CANCELLED => {}
                        Err(error) => shown.error = Some(error),
                    }
                }
                // The read has ended, so there is nothing left to stop.
                self.shown.load = None;
                self.pending = None;
            }
        }
        if self.pending.is_none() && self.shown.model.is_none() && self.shown.error.is_none() {
            let (sender, receiver) = mpsc::channel();
            self.pending = Some((selection.clone(), receiver));
            let repaint = ctx.clone();
            let repaint_viewport = ctx.viewport_id();
            let progress = model_preview::Load::default();
            let clip = self.shown.clip;
            self.shown.load = Some(progress.clone());
            self.shown.load_started = Some(std::time::Instant::now());
            let read = model_preview::PackageRead::start();
            std::thread::spawn(move || {
                let _read = read;
                let label = format!("viewer load {:?}", selection.1);
                let load = || match selection.1 {
                    Target::Object(tag) => {
                        model_preview::load_reported(&selection.0, tag, &progress, clip)
                    }
                    Target::Weapon(appearance, _) => model_preview::appearance::load_clip_reported(
                        &selection.0,
                        &appearance,
                        &progress,
                        clip,
                    ),
                    Target::Local(appearance, object) => {
                        appearance.load(&selection.0, &progress, object, clip)
                    }
                };
                let result = model_preview::timing::measure(
                    || label,
                    "worker",
                    "geometry and other",
                    std::time::Duration::ZERO,
                    || match access {
                        Some(access) => access.read(load),
                        None => load(),
                    },
                );
                let _ = sender.send(result);
                repaint.request_repaint_of(repaint_viewport);
            });
        }
        if self.pending.is_some() {
            // Fast enough that the stage text and bar read as live progress, not a freeze.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// The wait, showing what the reader is doing and offering a way out of it: a small block
    /// in the middle of the space the model will take, not a strip across the top.
    fn draw_loading(&mut self, ui: &mut egui::Ui) {
        let stage = self.shown.load.as_ref().map(model_preview::Load::stage);
        let stage = stage.unwrap_or_default();
        let message = if stage.message.is_empty() {
            "Loading"
        } else {
            stage.message.as_str()
        };
        let cancel = self.draw_centered(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(message);
            });
            // The bar keeps the block's width, with the count beside it in the weak colour.
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(width, 12.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if stage.total > 0 {
                        ui.weak(
                            egui::RichText::new(format!("{} of {}", stage.done, stage.total))
                                .small(),
                        );
                    }
                    let fraction = if stage.total > 0 {
                        stage.done as f32 / stage.total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(ui.available_width())
                            .desired_height(4.0),
                    );
                },
            );
            ui.small_button("Cancel").clicked()
        });
        if cancel {
            self.shown.stop_load();
            self.shown.error = Some("Loading cancelled.".into());
        }
    }

    /// Capture the requested frame on the preview GPU, with software export for fallback views.
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
            self.shown.frames.seconds,
            self.style,
        );
        let captured = (model_preview::gpu::available() && self.gpu.fallback(&model).is_none())
            .then(|| {
                self.gpu.image(
                    ctx.clone(),
                    model_preview::gpu::Frame {
                        model: model.clone(),
                        camera,
                        scene,
                        seconds,
                        style,
                        pose: None,
                        animate: true,
                        dyes: None,
                    },
                    [1600, 1200],
                )
            });
        self.write_in_background(ctx, "Image saved", path, move || {
            let image = match captured {
                Some(captured) => captured.wait()?,
                None => render::styled_image(&model, camera, scene, [1600, 1200], seconds, style),
            };
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
        let (model, seconds) = (model.clone(), self.shown.frames.seconds);
        let mut baker = (model_preview::gpu::available() && self.gpu.fallback(&model).is_none())
            .then(|| self.gpu.baker(ctx.clone(), model.clone()));
        let message = if model.triangle_effects.iter().any(Option::is_some) {
            "Model saved. Transparent shader effects are omitted from GLB. Save an image to retain them."
        } else {
            "Model saved"
        };
        self.write_in_background(ctx, message, path, move || match baker.as_mut() {
            Some(baker) => model_preview::export::glb_using(&model, seconds, Some(baker)),
            None => model_preview::export::glb(&model, seconds),
        });
    }

    fn save_audio(&mut self, ctx: &egui::Context, tag: u32, decoded: bool) {
        let Some(selection) = self.shown.selection.clone() else {
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
            .window
            .request
            .as_ref()
            .and_then(|request| request.access.clone());
        // Counted from before the save starts, so an install waits for the clip to be read.
        let read = model_preview::PackageRead::start();
        self.write_in_background(ctx, "Audio saved", path, move || {
            let bytes = read_clip(read, access.as_deref(), &selection, tag)?;
            if decoded {
                model_preview::assets::decoded_wave(&bytes)
            } else {
                Ok(bytes)
            }
        });
    }

    fn play_audio(&mut self, ctx: &egui::Context, tag: u32) {
        let Some(selection) = self.shown.selection.clone() else {
            return;
        };
        self.audio.reset();
        let access = self
            .window
            .request
            .as_ref()
            .and_then(|request| request.access.clone());
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.audio = Audio {
            pending: Some(receiver),
            cancel: Some(Arc::clone(&cancel)),
            tag: Some(tag),
            status: Some("Decoding audio".into()),
            ..Audio::default()
        };
        let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
        let read = model_preview::PackageRead::start();
        std::thread::spawn(move || {
            let decode = || {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Audio decoding canceled".to_owned());
                }
                // Only the clip's bytes need the packages, so a long decode never holds up an
                // install or Parhelion's suspension of package access.
                let bytes = read_clip(read, access.as_deref(), &selection, tag)?;
                let wave = model_preview::assets::decoded_wave_cancelable(&bytes, &cancel)?;
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
            let _ = sender.send(decode());
            repaint.request_repaint_of(viewport);
        });
    }

    fn poll_audio(&mut self, ctx: &egui::Context) {
        let audio = &mut self.audio;
        if let Some(receiver) = &audio.pending {
            match receiver.try_recv() {
                Ok(Ok((file, duration))) => {
                    #[cfg(windows)]
                    {
                        match play_wave(file.path()) {
                            Ok(()) => {
                                audio.playback = Some(file);
                                audio.ends = Some(std::time::Instant::now() + duration);
                                audio.status = Some("Playing audio".into());
                            }
                            Err(error) => audio.status = Some(error),
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        drop(file);
                        let _ = duration;
                        audio.status = Some("Audio playback is unavailable here".into());
                    }
                    audio.pending = None;
                }
                Ok(Err(error)) => {
                    audio.status = Some(error);
                    audio.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    audio.status = Some("Audio decoding stopped unexpectedly".into());
                    audio.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if let Some(end) = audio.ends {
            if std::time::Instant::now() >= end {
                audio.reset();
            } else {
                ctx.request_repaint_after(end.saturating_duration_since(std::time::Instant::now()));
            }
        }
        // A finished decode has nothing left to cancel.
        if audio.pending.is_none() {
            audio.cancel = None;
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
        // No outcome yet for the save just started.
        self.shown.status = None;
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
    fn poll_saving(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.saving else {
            return;
        };
        match receiver.try_recv() {
            Ok(message) => {
                self.shown.status = Some(message);
                self.saving = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.shown.status = Some("The save stopped before it finished.".into());
                self.saving = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, name: &str) {
        if model_preview::gpu::available() {
            self.gpu.service(ui);
        }
        ui.horizontal(|ui| {
            if !self.window.source.navigation.is_empty() && ui.button("Back").clicked() {
                self.window.source.navigation.pop();
            }
            let kind = self.shown.model.as_deref().map(preview_kind);
            // The kind goes in first at the right, so a long truncated name cannot push it out.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(kind) = kind {
                    ui.weak(kind);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(name).heading()).truncate());
                });
            });
        });
        ui.add_space(4.0);
        if let Some(error) = self.shown.error.clone() {
            self.hold_clocks();
            self.draw_centered(ui, |ui| {
                ui.add(egui::Label::new(error).wrap());
                ui.add_space(4.0);
                ui.small_button("Retry").clicked()
            })
            .then(|| self.shown.error = None);
            return;
        }
        let Some(model) = self.shown.model.clone() else {
            self.hold_clocks();
            self.draw_loading(ui);
            return;
        };
        if model.triangles.is_empty() && model.particle_sources.is_empty() {
            self.hold_clocks();
            self.draw_assets(ui, &model);
            return;
        }
        self.style = if model.triangles.is_empty() {
            render::Style::Textured
        } else {
            self.chosen_style
        };
        self.poll_saving(ui.ctx());
        self.poll_audio(ui.ctx());
        self.draw_toolbar(ui, &model);
        // The transport sits under the canvas, so the canvas keeps its top edge whatever it
        // shows.
        egui::Panel::bottom(ui.id().with("model-preview-bottom"))
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 6,
                bottom: 2,
            }))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| self.draw_playback(ui, &model));
        self.draw_viewport(ui, &model);
    }

    /// One row: the shading and the view reset at the left, the menus at the right.
    fn draw_toolbar(&mut self, ui: &mut egui::Ui, model: &Arc<Model>) {
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = 22.0;
            if !model.triangles.is_empty() && (model.has_surface_mesh() || model.particle_geometry)
            {
                // The three shadings side by side, in the theme's faint frame.
                egui::Frame::NONE
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .corner_radius(3)
                    .inner_margin(egui::Margin::same(2))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
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
                self.reset_view();
            }
            // The menus sit at the right, Details outermost.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let details = ui
                    .add(egui::Button::new("Details").selected(self.window.details.open))
                    .on_hover_text("Open in a separate window");
                if details.clicked() {
                    let window = &mut self.window.details;
                    window.open = !window.open;
                    window.focus_requested = window.open;
                    // The host frame shows the window, so it has to run.
                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                }
                ui.add_enabled_ui(self.saving.is_none(), |ui| {
                    ui.menu_button("Save", |ui| {
                        if ui.button("Image…").clicked() {
                            ui.close();
                            self.save_image(ui.ctx(), model);
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
                                ui.close();
                                self.save_model(ui.ctx(), model);
                            }
                        }
                    });
                });
                crate::ui::sticky_menu_button(ui, "View", |ui| {
                    ui.set_max_width(260.0);
                    self.draw_rotation_option(ui);
                    if !model.particle_sources.is_empty() || model.has_particle_material_study() {
                        if ui.checkbox(&mut self.scene.particle_study, "Particle Study").changed() {
                            self.shown.frames.playing = self.scene.particle_study
                                || model.has_animation()
                                || model.has_cloth()
                                || model.has_shader_animation();
                            self.shown.frames.last_tick = None;
                        }
                        if model.particle_simulation().is_some() {
                            ui.small("Stored particle programs drive count, placement, age and motion in a fixed studio scene. Live attachments and engine scheduling are unavailable.");
                        } else {
                            ui.small("Approximate material and sprite study. This system needs additional inputs or simulation rules for playback.");
                            for particle in &model.assets.particles {
                                if let Some(reason) = &particle.simulation_notice {
                                    ui.small(reason);
                                }
                            }
                        }
                        ui.separator();
                    }
                    // The picker sits in the menu itself: a color button's own popup lies
                    // outside the menu, so its first click closed the menu.
                    ui.label("Background");
                    let [r, g, b] = self.scene.background;
                    let mut background = egui::Color32::from_rgb(r, g, b);
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut background,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        self.scene.background = [background.r(), background.g(), background.b()];
                    }
                    ui.separator();
                    ui.checkbox(&mut self.scene.bloom, "Bloom");
                    ui.checkbox(&mut self.scene.filmic, "Filmic Output");
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
                            particle_study: self.scene.particle_study,
                            ..Scene::default()
                        };
                    }
                });
            });
        });
    }

    /// A small block in the middle of the space the model takes, for a state with no model:
    /// returns what its contents return.
    fn draw_centered<T>(
        &mut self,
        ui: &mut egui::Ui,
        contents: impl FnOnce(&mut egui::Ui) -> T,
    ) -> T {
        let space = ui.available_rect_before_wrap();
        let width = (space.width() - 48.0).clamp(160.0, 360.0);
        let block = egui::Align2::CENTER_CENTER
            .align_size_within_rect(egui::vec2(width, LOADING_BLOCK_HEIGHT), space);
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(block)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        ui.spacing_mut().item_spacing.y = 8.0;
        contents(&mut ui)
    }

    /// Transport controls and the timeline for animated, particle and shader-driven objects.
    fn draw_playback(&mut self, ui: &mut egui::Ui, model: &Model) {
        // An object with clips but no idle clip has no animation until one is picked, so the
        // picker shows for it too.
        if model.has_animation()
            || model.has_cloth()
            || !model.clips.is_empty()
            || model.has_shader_animation()
            || self.scene.particle_study
        {
            let duration = model.animation_duration().unwrap_or_else(|| {
                if !self.scene.particle_study || model.particle_sources.is_empty() {
                    if self.scene.particle_study
                        && let Some(simulation) = model.particle_simulation()
                    {
                        simulation.duration
                    } else if self.scene.particle_study && model.has_particle_material_study() {
                        model
                            .assets
                            .particles
                            .iter()
                            .filter_map(|particle| particle.program.as_ref()?.lifetime_default())
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
            });
            let now = std::time::Instant::now();
            // Playback holds while the viewer is in the background, as the orbit does.
            let frames = &mut self.shown.frames;
            let moving = frames.playing && rotation::animating(ui.ctx());
            if moving {
                if let Some(last) = frames.last_tick {
                    frames.seconds += now.duration_since(last).as_secs_f32() * self.speed.0;
                    if !model.has_shader_animation() && model.rigs.len() <= 1 {
                        frames.seconds = if duration > 0.0 {
                            frames.seconds.rem_euclid(duration)
                        } else {
                            0.0
                        };
                    }
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
            }
            frames.last_tick = moving.then_some(now);
            let timeline_end = if model.has_shader_animation() || model.rigs.len() > 1 {
                frames.seconds.max(60.0)
            } else {
                duration
            };
            // One row: transport at the left, the clip, speed and name at the right, the
            // timeline across what is left.
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 22.0;
                if ui
                    .button(if self.shown.frames.playing {
                        "Pause"
                    } else {
                        "Play"
                    })
                    .clicked()
                {
                    self.shown.frames.playing = !self.shown.frames.playing;
                }
                if ui.button("Restart").clicked() {
                    self.shown.frames.seconds = 0.0;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if model.has_shader_animation() {
                        ui.label("Shader Animation")
                            .on_hover_text("Native material timing, UV motion and color changes.");
                    } else if let Some(animation) = model.active_clip() {
                        // Several clips are named by the combo above.
                        if model.clips.len() <= 1 {
                            let name = model
                                .clips
                                .iter()
                                .find(|clip| clip.tag == animation.tag)
                                .map_or("Animation", |clip| clip.name.as_str());
                            ui.label(name).on_hover_text(format!(
                                "Native clip 0x{:08X} · {} frames at {} preview FPS",
                                animation.tag, animation.frames, animation.fps
                            ));
                        }
                    } else if self.scene.particle_study && model.particle_simulation().is_some() {
                        ui.label("Particle Simulation");
                    } else if model.has_cloth() {
                        ui.label("Cloth Simulation").on_hover_text(
                            "Cloth motion in the studio with authored body colliders.",
                        );
                    } else if self.scene.particle_study && !model.particle_sources.is_empty() {
                        ui.label("Particle Study");
                    } else if self.scene.particle_study && model.has_particle_material_study() {
                        ui.label("Particle Material Study");
                    }
                    if model.clips.len() > 1 || (!model.has_animation() && !model.clips.is_empty())
                    {
                        let playing = self.shown.clip.or(model.active_clip().map(|a| a.tag));
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
                                        self.shown.clip = Some(clip.tag);
                                        self.shown.unload();
                                    }
                                }
                            });
                    }
                    ui.add(
                        egui::DragValue::new(&mut self.speed.0)
                            .range(0.1..=4.0)
                            .speed(0.02)
                            .prefix("x")
                            .fixed_decimals(2),
                    )
                    .on_hover_text("Playback speed");
                    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(60.0);
                    let response = ui.add(
                        egui::Slider::new(&mut self.shown.frames.seconds, 0.0..=timeline_end)
                            // Rounding the running clock must not look like a user edit.
                            .clamping(egui::SliderClamping::Edits)
                            .suffix(" s")
                            .fixed_decimals(2),
                    );
                    if response.dragged() || response.changed() {
                        self.shown.frames.playing = false;
                    }
                });
            });
        }
    }

    /// The model view: drag and scroll move the camera, then the GPU or software path paints.
    fn draw_viewport(&mut self, ui: &mut egui::Ui, model: &Arc<Model>) {
        let available = ui.available_size_before_wrap();
        let size = egui::vec2(available.x.max(32.0), available.y.max(32.0));
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
        let orbit = self.advance_rotation(ui.ctx(), &response);
        let gpu = model_preview::gpu::available() && self.gpu.fallback(model).is_none();
        // Until the GPU draws the model, while its programs link and its images upload, the
        // software image stands in. It is painted first, so the GPU's first frame covers it.
        if !(gpu && self.gpu.drawn(model)) {
            self.draw_software(ui, rect, model, size, orbit);
        }
        if gpu {
            self.gpu.paint(
                ui,
                rect,
                model_preview::gpu::Frame {
                    model: model.clone(),
                    camera: self.camera,
                    scene: self.scene,
                    style: self.style,
                    seconds: self.shown.frames.seconds,
                    pose: None,
                    animate: true,
                    dyes: None,
                },
            );
            if !self.gpu.drawn(model) && self.shown.frames.texture.is_none() {
                let spinner = egui::Rect::from_min_size(
                    rect.left_bottom() + egui::vec2(10.0, -26.0),
                    egui::vec2(16.0, 16.0),
                );
                egui::Spinner::new().paint_at(ui, spinner);
                ui.painter().text(
                    spinner.right_center() + egui::vec2(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Loading Model",
                    egui::FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }
        self.draw_fps(ui, rect);
        self.draw_overlays(ui, rect, model);
    }

    /// Paints the software image of `model`, asking for a new one when the view changed.
    fn draw_software(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        model: &Arc<Model>,
        size: egui::Vec2,
        orbit: bool,
    ) {
        let pixels = render_size(size, ui.ctx().pixels_per_point());
        // Limit software rendering to 30 FPS even when the surrounding app repaints faster.
        let frames = &mut self.shown.frames;
        let seconds = (frames.seconds * 30.0).floor() / 30.0;
        let request = image_job::Key {
            model: Arc::as_ptr(model) as usize,
            camera: self.camera,
            scene: self.scene,
            size: pixels,
            seconds,
            style: self.style,
            overrides: Vec::new(),
            orbit,
        };
        if let Some((rendered, image)) = frames.software.update(ui.ctx(), model.clone(), request) {
            self.shown.software_frames = self.shown.software_frames.saturating_add(1);
            if let Some(texture) = &mut frames.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                frames.texture = Some(ui.ctx().load_texture(
                    "object-model-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            frames.rendered = Some((
                rendered.camera,
                rendered.scene,
                rendered.size,
                rendered.style,
            ));
            frames.rendered_seconds = Some(rendered.seconds);
        }
        if let Some(texture) = &frames.texture {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn draw_fps(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let source = self
            .window
            .source_viewport
            .unwrap_or_else(|| ui.ctx().viewport_id());
        let enabled = options::get(ui.ctx(), source).show_fps;
        let frames = if enabled {
            self.gpu
                .completed_frames()
                .saturating_add(self.shown.software_frames)
        } else {
            0
        };
        self.shown.fps.draw(ui, rect, enabled, frames);
    }

    /// Short notes over the canvas, bottom left: a save or sound in progress, what the drawing
    /// leaves out, and how to move the view. They take no room from the model.
    fn draw_overlays(&self, ui: &egui::Ui, rect: egui::Rect, model: &Model) {
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(11.5);
        let color = ui.visuals().weak_text_color();
        let mut lines: Vec<String> = Vec::new();
        if let Some(model) = &self.shown.model
            && let Some(reason) = self.gpu.fallback(model)
        {
            lines.push(reason);
        }
        if self.saving.is_some() {
            lines.push("Saving…".into());
        }
        lines.extend(self.shown.status.iter().chain(&self.audio.status).cloned());
        if self.scene.particle_study {
            lines.push(
                if model.particle_simulation().is_some() {
                    "Particle simulation · fixed studio scene"
                } else if model.particle_sources.is_empty() {
                    "Material study · spawn and motion not shown"
                } else {
                    "Sprite study · synthetic placement and motion"
                }
                .into(),
            );
        } else if model.particle_geometry && !model.has_object_mesh() {
            lines.push("Draw mesh only · emitted particles not shown".into());
        } else if model.particle_geometry {
            lines.push("Particle meshes available in Solid and Wireframe".into());
        } else if model.triangles.is_empty() && !model.particle_sources.is_empty() {
            lines.push("Particle playback unavailable · View > Particle Study".into());
        }
        lines.push("Drag to rotate · Shift-drag to pan · Scroll to zoom".into());
        let mut bottom = rect.bottom() - 8.0;
        for line in lines.iter().rev() {
            let galley = painter.layout_no_wrap(line.clone(), font.clone(), color);
            let at = egui::pos2(rect.left() + 10.0, bottom - galley.size().y);
            let backing =
                egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
            painter.rect_filled(backing, 3.0, egui::Color32::from_black_alpha(110));
            painter.galley(at, galley, color);
            bottom = backing.top() - 4.0;
        }
    }

    fn browse(&mut self, tag: u32, name: Option<&str>) {
        let navigation = &mut self.window.source.navigation;
        if navigation
            .last()
            .is_some_and(|(current, _)| *current == tag)
        {
            return;
        }
        navigation.push((
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
        let handle = self.shown.asset_textures.entry(tag).or_insert_with(|| {
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
        let handle = self.shown.asset_textures.entry(tag).or_insert_with(|| {
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
                match program.coverage() {
                    Ok(coverage) => {
                        ui.label(format!("Expression Instructions: {} of {} supported",
                            coverage.available, coverage.instructions));
                        if !coverage.unsupported.is_empty() {
                            ui.label(format!("Unsupported Instructions: {}", coverage.unsupported.iter()
                                .map(|opcode| format!("0x{opcode:02X}")).collect::<Vec<_>>().join(", ")));
                        }
                        if coverage.runtime_inputs {
                            ui.weak("This program requires engine inputs or controller state.");
                        }
                        ui.weak("Expression support does not include native spawn, motion or shader playback.");
                    }
                    Err(error) => { ui.label(format!("Expression coverage unavailable: {error}")); }
                }
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

fn render_size(size: egui::Vec2, pixels_per_point: f32) -> [usize; 2] {
    // Match display density during playback and inspection. Background jobs coalesce
    // frames, while the fixed budget bounds software work on large displays.
    let density = pixels_per_point.min(1536.0 / size.x.max(size.y).max(1.0));
    [
        (size.x * density).round().max(1.0) as usize,
        (size.y * density).round().max(1.0) as usize,
    ]
}

#[cfg(test)]
mod tests;
