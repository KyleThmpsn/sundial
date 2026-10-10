//! A small preview drawn in place, for a page that shows unbuilt edits on a model as they are
//! made. The model is read in the background once per appearance, and a change to its surface
//! colors draws it again without reading it again.
use super::*;
mod camera;
mod playback;
pub use camera::zoom_controls;
pub(super) use playback::control as playback_control;

/// What a still shows: an item's appearance, or a whole object by its entity graph tag, such as a
/// vehicle a Sparrow summons.
#[derive(Clone, PartialEq)]
enum Subject {
    Appearance(Appearance),
    Object(u32),
    Local(LocalAppearance),
}

impl Subject {
    /// Whether `other` is the same model, which keeps the angle it was turned to.
    fn same_model(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Appearance(own), Self::Appearance(other)) => {
                own.arrangement == other.arrangement
            }
            (Self::Object(own), Self::Object(other)) => own == other,
            (Self::Local(own), Self::Local(other)) => own == other,
            _ => false,
        }
    }
}

type Wanted = (
    PathBuf,
    Subject,
    Option<Arc<BTreeMap<usize, crate::dyes::DyeSource>>>,
);

#[derive(Default)]
struct Still {
    /// The appearance the page asks for.
    wanted: Option<Wanted>,
    /// The appearance the model shows. An older one stays up while a newer one loads.
    shown: Option<Wanted>,
    model: Option<Arc<Model>>,
    /// The box around what the shown model draws, kept so each frame can fit it.
    bounds: Option<([f32; 3], [f32; 3])>,
    /// The read in flight. A new read waits for it, so two never overlap.
    pending: Option<(Wanted, mpsc::Receiver<Result<Model, String>>)>,
    load: Option<model_preview::Load>,
    error: Option<(Wanted, String)>,
    camera: Camera,
    /// The angle the model is fitted at. Unset fits at the default angle, so turning the model
    /// never changes its size. A placing view fits at its own angle, so a long weapon seen from
    /// the side fills the frame without leaving it.
    fit_angle: Option<(f32, f32)>,
    gpu: model_preview::gpu::Shared,
    texture: Option<egui::TextureHandle>,
    /// What the software image shows, so it is drawn again only when that changes.
    rendered: Option<(Camera, [usize; 2], Vec<SurfaceOverride>)>,
    /// Shared playback time for hardware and software shader animation.
    seconds: f32,
    /// A manual choice lasts for this model. A changed default clears the choice.
    playing: Option<bool>,
    autoplay: Option<bool>,
    last_tick: Option<std::time::Instant>,
    rendered_seconds: Option<f32>,
    software: image_job::Job,
    fps: fps::Counter,
    software_frames: u64,
    /// What stands in for the model until the first one is read, such as the item's icon.
    placeholder: Option<egui::TextureHandle>,
}

/// The camera a model is fitted with: the default one, turned to `angle` when one is set.
fn fit_camera(angle: Option<(f32, f32)>) -> Camera {
    match angle {
        Some((yaw, pitch)) => Camera {
            yaw,
            pitch,
            ..Camera::default()
        },
        None => Camera::default(),
    }
}

fn paused_id(ctx: &egui::Context) -> egui::Id {
    egui::Id::new(("model-preview-still-paused", ctx.viewport_id()))
}

/// Holds this viewport's stills while packages are unavailable.
pub(super) fn pause(ctx: &egui::Context, paused: bool) {
    // Read before the data lock is taken, since reading the viewport takes the context's lock.
    let id = paused_id(ctx);
    ctx.data_mut(|data| data.insert_temp(id, paused));
}

/// How much of a still's frame the model's outline fills. A still is small, so it sits closer
/// than the viewer's default framing.
const STILL_FILL: f32 = 0.92;

/// Sets the image `id`'s still shows until its first model is read, such as the item's icon, so
/// a page opens on the item rather than on an empty frame. `None` shows the frame alone.
pub fn placeholder(ctx: &egui::Context, id: egui::Id, image: Option<egui::TextureHandle>) {
    let state = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Still>>>(id)
            .clone()
    });
    if let Ok(mut still) = state.lock() {
        still.placeholder = image;
    }
}

/// Draws `appearance` in a `size` rectangle, with `overrides` in place of its dyes' colors,
/// inside a hairline border. Dragging turns the model, Shift-drag pans and scrolling zooms at the
/// pointer. A double click restores the starting view.
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    appearance: Option<Appearance>,
    overrides: &[SurfaceOverride],
    size: egui::Vec2,
) -> egui::Response {
    show_sources(ui, id, packages, appearance, overrides, size, None)
}

/// Draw local shader materials on ordinary native preview geometry before the recipe is built.
pub fn show_sources(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    appearance: Option<Appearance>,
    overrides: &[SurfaceOverride],
    size: egui::Vec2,
    sources: Option<Arc<BTreeMap<usize, crate::dyes::DyeSource>>>,
) -> egui::Response {
    show_subject(
        ui,
        id,
        packages,
        appearance.map(Subject::Appearance),
        overrides,
        size,
        sources,
    )
}

/// Draws the object `tag`, an entity graph such as a vehicle, in a `size` rectangle like [`show`]
/// draws an appearance.
pub fn show_object(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    tag: Option<u32>,
    size: egui::Vec2,
) -> egui::Response {
    show_subject(ui, id, packages, tag.map(Subject::Object), &[], size, None)
}

/// Draw an unbuilt imported appearance through the native model and material renderer.
pub fn show_local(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    appearance: LocalAppearance,
    size: egui::Vec2,
) -> egui::Response {
    show_subject(
        ui,
        id,
        packages,
        Some(Subject::Local(appearance)),
        &[],
        size,
        None,
    )
}

fn show_subject(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    subject: Option<Subject>,
    overrides: &[SurfaceOverride],
    size: egui::Vec2,
    sources: Option<Arc<BTreeMap<usize, crate::dyes::DyeSource>>>,
) -> egui::Response {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Still>>>(id)
            .clone()
    });
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let Ok(mut still) = state.lock() else {
        return response;
    };
    let paused_id = paused_id(ui.ctx());
    let paused = !ui.is_enabled()
        || ui
            .ctx()
            .data(|data| data.get_temp::<bool>(paused_id))
            .unwrap_or(false);
    still.wanted = subject.map(|subject| (packages.to_owned(), subject, sources));
    still.receive();
    request_preview(&mut still, ui.ctx(), paused);
    still.interact(ui, &response);
    still.draw(ui, rect, overrides, paused);
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Model Preview"));
    let response = if let Some(model) = &still.model {
        if model.notices.is_empty() {
            response
        } else {
            response.on_hover_text(model.notices.join("\n"))
        }
    } else {
        response
    };
    response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text(
            "Drag to rotate · Shift-drag to pan · Scroll to zoom · Double-click to reset",
        )
}

impl Still {
    fn receive(&mut self) {
        let Some((source, receiver)) = &self.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The model reader stopped before returning a result.".into())
            }
        };
        let source = source.clone();
        self.pending = None;
        self.load = None;
        if self.wanted.as_ref() != Some(&source) {
            return;
        }
        match result {
            Ok(model) => {
                // Another item faces forward. The same item keeps the angle it was turned to.
                if self
                    .shown
                    .as_ref()
                    .is_none_or(|shown| shown.0 != source.0 || !shown.1.same_model(&source.1))
                {
                    self.camera = Camera::default();
                    self.playing = None;
                }
                self.bounds = Some(render::drawn_bounds(&model, render::Style::Textured));
                self.model = Some(Arc::new(model));
                self.shown = Some(source);
                self.error = None;
                self.rendered = None;
                self.seconds = 0.0;
                self.last_tick = None;
                self.rendered_seconds = None;
                self.software = image_job::Job::default();
                self.fps = fps::Counter::default();
                self.software_frames = 0;
            }
            Err(error) if error == model_preview::CANCELLED => {}
            Err(error) => self.error = Some((source, error)),
        }
    }

    fn request(&mut self, ctx: &egui::Context) {
        let Some(wanted) = &self.wanted else {
            return;
        };
        if let Some((source, _)) = &self.pending {
            // The page moved on, so the read in flight is not worth finishing.
            if source != wanted
                && let Some(load) = self.load.take()
            {
                load.stop();
            }
            return;
        }
        if self.shown.as_ref() == Some(wanted)
            || self
                .error
                .as_ref()
                .is_some_and(|(failed, _)| failed == wanted)
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let load = model_preview::Load::default();
        self.pending = Some((wanted.clone(), receiver));
        self.load = Some(load.clone());
        let (packages, subject, sources) = wanted.clone();
        let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
        let read = model_preview::PackageRead::start();
        std::thread::spawn(move || {
            let _read = read;
            let model = match &subject {
                Subject::Appearance(appearance) => {
                    model_preview::appearance::load_reported(&packages, appearance, &load)
                }
                Subject::Object(tag) => model_preview::load_reported(&packages, *tag, &load, None),
                Subject::Local(appearance) => appearance.load(&packages, &load, None, None),
            };
            let result = model.and_then(|mut model| {
                if let Some(sources) = sources {
                    crate::dyes::source::apply(&packages, &sources, &mut model, &load)?;
                }
                Ok(model)
            });
            let _ = sender.send(result);
            repaint.request_repaint_of(viewport);
        });
    }

    fn draw(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        overrides: &[SurfaceOverride],
        paused: bool,
    ) {
        let painter = ui.painter_at(rect);
        let [red, green, blue] = Scene::default().background;
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(red, green, blue));
        let failed = self
            .error
            .as_ref()
            .filter(|(failed, _)| Some(failed) == self.wanted.as_ref())
            .map(|(_, error)| error.clone());
        let message = if let Some(error) = failed {
            Some(error)
        } else if self.wanted.is_none() {
            Some("No Preview".to_owned())
        } else if self.model.is_none() && paused {
            Some("Paused".to_owned())
        } else {
            None
        };
        if let Some(message) = message {
            let galley = painter.layout(
                message,
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
                rect.width() - 24.0,
            );
            let at = rect.center() - galley.size() / 2.0;
            painter.galley(at, galley, ui.visuals().weak_text_color());
            return;
        }
        let drawn = self
            .model
            .clone()
            .is_some_and(|model| self.draw_model(ui, &painter, rect, &model, overrides, paused));
        if drawn {
            let enabled = options::get(ui.ctx(), ui.ctx().viewport_id()).show_fps;
            self.fps.draw(
                ui,
                rect,
                enabled,
                if enabled {
                    self.gpu
                        .completed_frames()
                        .saturating_add(self.software_frames)
                } else {
                    0
                },
            );
        }
        // Until the first model is read, the item's icon, faded, stands in for it.
        if self.model.is_none()
            && let Some(image) = &self.placeholder
        {
            let side = (rect.width().min(rect.height()) * 0.4).min(96.0);
            painter.image(
                image.id(),
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(side)),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(110),
            );
        }
        // An older model stays up while the one the page asks for loads, and while the software
        // renderer draws the first image of a newly read one, which would otherwise pass for it.
        let rendering = self.model.is_some() && !drawn;
        if (self.shown != self.wanted || rendering) && !paused {
            let spinner = egui::Rect::from_min_size(
                rect.left_bottom() + egui::vec2(10.0, -26.0),
                egui::vec2(16.0, 16.0),
            );
            egui::Spinner::new().paint_at(ui, spinner);
            painter.text(
                spinner.right_center() + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Loading Model",
                egui::FontId::proportional(12.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    /// Paints `model`, and returns whether what it painted is that model. A software image of the
    /// model shown before stays up until the first one of this model is ready.
    fn draw_model(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        model: &Arc<Model>,
        overrides: &[SurfaceOverride],
        paused: bool,
    ) -> bool {
        model.set_surface_overrides(overrides);
        let animated = model.has_shader_animation() || model.has_animation() || model.has_cloth();
        let now = std::time::Instant::now();
        let moving = animated && !paused && self.playback_enabled(ui.ctx());
        if moving {
            if let Some(last) = self.last_tick {
                self.seconds += now.duration_since(last).as_secs_f32().min(0.1);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
        self.last_tick = moving.then_some(now);
        let seconds = if animated { self.seconds } else { 0.0 };
        // The software image's whole pixels, which the fit also uses so two previews of one size
        // frame a model alike however fractional their rectangles.
        let pixels = render_size(rect.size(), ui.ctx().pixels_per_point());
        let fit = self.bounds.map_or(1.0, |bounds| {
            render::fitted_zoom(
                model,
                bounds,
                fit_camera(self.fit_angle),
                pixels.map(|side| side as f32),
                STILL_FILL,
            )
        });
        let camera = Camera {
            zoom: self.camera.zoom * fit,
            ..self.camera
        };
        if model_preview::gpu::available() && self.gpu.fallback(model).is_none() {
            self.gpu.paint(
                ui,
                rect,
                model_preview::gpu::Frame {
                    model: model.clone(),
                    camera,
                    scene: Scene::default(),
                    style: render::Style::Textured,
                    seconds,
                    pose: None,
                    animate: true,
                    dyes: None,
                },
            );
            return true;
        }
        // Software previews also advance the material program while preserving cached static views.
        let key = (camera, pixels, overrides.to_vec());
        let request = image_job::Key {
            model: Arc::as_ptr(model) as usize,
            camera,
            scene: Scene::default(),
            size: pixels,
            seconds,
            style: render::Style::Textured,
            overrides: overrides.to_vec(),
            orbit: false,
        };
        if let Some((rendered, image)) = self.software.update(ui.ctx(), model.clone(), request) {
            self.software_frames = self.software_frames.saturating_add(1);
            match &mut self.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "model-preview-still",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.rendered = Some(key);
            self.rendered_seconds = Some(rendered.seconds);
        }
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        // Reading a model clears what was rendered, so an image of this one has been drawn.
        self.rendered.is_some()
    }
}

/// A fixed angle for placing points on a model. Each looks along one model axis, so a dragged
/// point moves along the other two and never drifts in depth.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum View {
    /// Turned freely by dragging. Points do not move here.
    Orbit,
    /// Across the weapon, muzzle to the right.
    #[default]
    Side,
    /// Down onto the weapon.
    Top,
    /// From behind the weapon, looking toward the muzzle.
    Rear,
}

impl View {
    /// The placing views first, since points move only in those.
    pub const ALL: [Self; 4] = [Self::Side, Self::Top, Self::Rear, Self::Orbit];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Orbit => "Orbit",
            Self::Side => "Side",
            Self::Top => "Top",
            Self::Rear => "Rear",
        }
    }

    /// The yaw and pitch a snapped view holds the camera at.
    fn angle(self) -> Option<(f32, f32)> {
        use std::f32::consts::FRAC_PI_2;
        match self {
            Self::Orbit => None,
            Self::Side => Some((0.0, 0.0)),
            Self::Top => Some((0.0, -FRAC_PI_2)),
            Self::Rear => Some((-FRAC_PI_2, 0.0)),
        }
    }

    /// The model-space directions that point right and up on screen in this view. The orbit
    /// answers with the side view's, so keys still move a point predictably there.
    #[must_use]
    pub fn axes(self) -> ([f32; 3], [f32; 3]) {
        let (yaw, pitch) = self.angle().unwrap_or((0.0, 0.0));
        render::screen_axes(Camera {
            yaw,
            pitch,
            ..Camera::default()
        })
    }
}

/// A point drawn over the model, such as a marker being placed.
#[derive(Clone, Copy, Debug)]
pub struct Pin<'a> {
    /// Where the point is, in the model's own space.
    pub position: [f32; 3],
    pub label: &'a str,
    pub selected: bool,
    /// Where the point sat before it was moved, drawn faintly with a line to where it is.
    pub origin: Option<[f32; 3]>,
}

/// What happened over the pins this frame.
pub struct Pinned {
    pub response: egui::Response,
    /// The pin double-clicked this frame, which the caller selects.
    pub pressed: Option<usize>,
    /// The selected pin being dragged and how far it moved this frame, in the model's units.
    pub moved: Option<(usize, [f32; 3])>,
}

/// Frames the model again in the preview `id`: no pan and the fitted zoom. The view's angle is
/// applied on its next draw.
pub fn frame(ctx: &egui::Context, id: egui::Id) {
    let state = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Still>>>(id)
            .clone()
    });
    if let Ok(mut still) = state.lock() {
        still.camera = Camera::default();
    }
}

/// How near the pointer has to come to a pin to grab it, in points.
const GRAB: f32 = 10.0;

/// Draws `appearance` like [`show`] with `pins` over it. A double click on a pin selects it, and
/// anywhere else frames the model again. In a snapped `view` the selected pin is dragged across
/// the view's plane and any other drag pans. The orbit turns as usual. Scrolling zooms at the
/// pointer.
#[allow(clippy::too_many_arguments)]
pub fn show_pins(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    appearance: Option<Appearance>,
    size: egui::Vec2,
    view: View,
    pins: &[Pin<'_>],
) -> Pinned {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Still>>>(id)
            .clone()
    });
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let mut pinned = Pinned {
        response: response.clone(),
        pressed: None,
        moved: None,
    };
    let Ok(mut still) = state.lock() else {
        return pinned;
    };
    let paused_id = paused_id(ui.ctx());
    let paused = !ui.is_enabled()
        || ui
            .ctx()
            .data(|data| data.get_temp::<bool>(paused_id))
            .unwrap_or(false);
    still.wanted =
        appearance.map(|appearance| (packages.to_owned(), Subject::Appearance(appearance), None));
    still.receive();
    request_preview(&mut still, ui.ctx(), paused);
    if let Some((yaw, pitch)) = view.angle() {
        still.camera.yaw = yaw;
        still.camera.pitch = pitch;
    }
    let frame = rect.size();
    still.fit_angle = view.angle();
    // The camera the model is drawn with, fitted as `draw_model` fits it.
    let fit_angle = still.fit_angle;
    let projection = still
        .model
        .clone()
        .zip(still.bounds)
        .map(|(model, bounds)| {
            let fit = render::fitted_zoom(
                &model,
                bounds,
                fit_camera(fit_angle),
                [frame.x, frame.y],
                STILL_FILL,
            );
            (model, bounds, fit)
        });
    let camera_for = |camera: Camera, fit: f32| Camera {
        zoom: camera.zoom * fit,
        ..camera
    };
    let screen = |camera: Camera, point: [f32; 3]| {
        projection.as_ref().map(|(model, bounds, fit)| {
            let [x, y, _] = render::project(
                model,
                *bounds,
                camera_for(camera, *fit),
                [frame.x, frame.y],
                point,
            );
            rect.min + egui::vec2(x, y)
        })
    };
    let nearest = |camera: Camera, at: egui::Pos2| {
        pins.iter()
            .enumerate()
            .filter_map(|(index, pin)| {
                let distance = screen(camera, pin.position)?.distance(at);
                (distance <= GRAB).then_some((index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    };
    let grabbed_id = id.with("grabbed-pin");
    // Where the button went down, not where the drag was recognised a few points later, or a
    // drag that starts on a small ring misses it and pans.
    if response.drag_started()
        && let Some(at) = ui
            .input(|input| input.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
    {
        // Only the selected pin moves, so a drag that starts near another one pans instead of
        // dragging it off by accident.
        let grabbed = (view != View::Orbit)
            .then(|| nearest(still.camera, at))
            .flatten()
            .filter(|index| pins[*index].selected);
        ui.data_mut(|data| data.insert_temp(grabbed_id, grabbed));
    }
    if response.dragged() {
        let delta = response.drag_delta();
        let grabbed = ui
            .data(|data| data.get_temp::<Option<usize>>(grabbed_id))
            .flatten();
        match (grabbed, &projection) {
            (Some(index), Some((model, bounds, fit))) => {
                let scale = render::screen_scale(
                    model,
                    *bounds,
                    camera_for(still.camera, *fit),
                    [frame.x, frame.y],
                );
                let (right, up) = render::screen_axes(still.camera);
                let across = delta.x / scale;
                let rise = -delta.y / scale;
                let moved = std::array::from_fn(|axis| right[axis] * across + up[axis] * rise);
                pinned.moved = Some((index, moved));
            }
            (Some(_), None) => {}
            (None, _) if view == View::Orbit => {
                still.camera.yaw += delta.x * 0.01;
                still.camera.pitch = (still.camera.pitch + delta.y * 0.01)
                    .clamp(-render::MAX_PITCH, render::MAX_PITCH);
            }
            (None, _) => {
                still.camera.pan[0] += delta.x / frame.x.max(1.0);
                still.camera.pan[1] += delta.y / frame.y.max(1.0);
            }
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.remove::<Option<usize>>(grabbed_id));
    }
    // Zoom at the pointer: the point under it stays under it.
    if response.hovered()
        && let Some(at) = response.hover_pos()
    {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 {
            // The page under the preview must not scroll as well.
            ui.ctx()
                .input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
            let factor = (scroll * 0.002).exp();
            let zoom = (still.camera.zoom * factor).clamp(0.5, 60.0);
            let applied = zoom / still.camera.zoom;
            for axis in 0..2 {
                let cursor = (at[axis] - rect.min[axis]) / frame[axis].max(1.0);
                let offset = cursor - 0.5 - still.camera.pan[axis];
                still.camera.pan[axis] += offset * (1.0 - applied);
            }
            still.camera.zoom = zoom;
        }
    }
    // A double click on a pin selects it. Anywhere else it frames the model again.
    let picked = response
        .double_clicked()
        .then(|| response.interact_pointer_pos())
        .flatten()
        .and_then(|at| nearest(still.camera, at));
    if picked.is_some() {
        pinned.pressed = picked;
    } else if response.double_clicked() {
        let (yaw, pitch) = view
            .angle()
            .unwrap_or((Camera::default().yaw, Camera::default().pitch));
        still.camera = Camera {
            yaw,
            pitch,
            ..Camera::default()
        };
    }
    still.draw(ui, rect, &[], paused);
    let camera = still.camera;
    drop(still);
    let hovered = response.hover_pos().and_then(|at| nearest(camera, at));
    draw_pins(ui, rect, pins, hovered, |point| screen(camera, point));
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Model Preview"));
    let over_selected = hovered.is_some_and(|index| pins[index].selected);
    pinned.response = response.on_hover_cursor(match (view, hovered) {
        (_, Some(_)) if !over_selected => egui::CursorIcon::PointingHand,
        (View::Orbit, _) => egui::CursorIcon::Grab,
        (_, Some(_)) => egui::CursorIcon::Move,
        (_, None) => egui::CursorIcon::AllScroll,
    });
    pinned
}

/// Pins as rings with a dark edge, so they read on a bright model and on the dark backdrop. The
/// selected one is filled and crossed by guide lines, the selected and hovered ones are labelled,
/// and a moved one keeps a faint ring where it started.
fn draw_pins(
    ui: &egui::Ui,
    rect: egui::Rect,
    pins: &[Pin<'_>],
    hovered: Option<usize>,
    screen: impl Fn([f32; 3]) -> Option<egui::Pos2>,
) {
    let painter = ui.painter_at(rect);
    let edge = egui::Stroke::new(3.5, egui::Color32::from_black_alpha(200));
    let light = egui::Color32::from_gray(235);
    let faint = egui::Color32::from_white_alpha(110);
    // An opaque grey reads the same over the model and the backdrop, where a translucent white
    // turned bright over the dark background.
    let guide = egui::Stroke::new(1.0, egui::Color32::from_gray(70));
    let font = egui::FontId::proportional(12.0);
    // The selection last, so it sits over every other pin.
    let order = pins
        .iter()
        .enumerate()
        .filter(|(_, pin)| !pin.selected)
        .chain(pins.iter().enumerate().filter(|(_, pin)| pin.selected));
    for (index, pin) in order {
        let Some(at) = screen(pin.position) else {
            continue;
        };
        if let Some(origin) = pin.origin.and_then(&screen)
            && origin.distance(at) > 1.0
        {
            painter.line_segment([origin, at], egui::Stroke::new(1.0, faint));
            painter.circle_stroke(origin, 5.0, egui::Stroke::new(1.0, faint));
        }
        if pin.selected {
            painter.hline(rect.x_range(), at.y, guide);
            painter.vline(at.x, rect.y_range(), guide);
            painter.circle_filled(at, 7.75, edge.color);
            painter.circle_filled(at, 6.0, light);
        } else {
            painter.circle_stroke(at, 5.5, edge);
            painter.circle_stroke(at, 5.5, egui::Stroke::new(1.75, light));
        }
        if pin.selected || hovered == Some(index) {
            let galley = painter.layout_no_wrap(pin.label.to_owned(), font.clone(), light);
            // Up and to the right, flipped to the left or below where that would leave the frame.
            let size = galley.size();
            let x = if at.x + 15.0 + size.x <= rect.right() - 2.0 {
                at.x + 10.0
            } else {
                at.x - 10.0 - size.x
            };
            let y = if at.y - size.y - 8.0 >= rect.top() + 2.0 {
                at.y - size.y - 6.0
            } else {
                at.y + 8.0
            };
            let label = egui::pos2(x, y);
            let backing =
                egui::Rect::from_min_size(label, galley.size()).expand2(egui::vec2(5.0, 2.0));
            painter.rect_filled(backing, 3.0, egui::Color32::from_black_alpha(170));
            painter.galley(label, galley, light);
        }
    }
}

fn request_preview(still: &mut Still, context: &egui::Context, paused: bool) {
    if paused {
        if let Some(load) = still.load.take() {
            load.stop();
        }
    } else {
        still.request(context);
    }
}

#[cfg(test)]
mod tests;
