//! A small preview drawn in place, for a page that shows unbuilt edits on a model as they are
//! made. The model is read in the background once per appearance, and a change to its surface
//! colors draws it again without reading it again.
use super::*;

type Wanted = (PathBuf, Appearance);

#[derive(Default)]
struct Still {
    /// The appearance the page asks for.
    wanted: Option<Wanted>,
    /// The appearance the model shows. An older one stays up while a newer one loads.
    shown: Option<Wanted>,
    model: Option<Arc<Model>>,
    /// The read in flight. A new read waits for it, so two never overlap.
    pending: Option<(Wanted, mpsc::Receiver<Result<Model, String>>)>,
    load: Option<model_preview::Load>,
    error: Option<(Wanted, String)>,
    camera: Camera,
    gpu: model_preview::gpu::Shared,
    texture: Option<egui::TextureHandle>,
    /// What the software image shows, so it is drawn again only when that changes.
    rendered: Option<(Camera, [usize; 2], Vec<SurfaceOverride>)>,
    /// When the shown model arrived, the clock for its dye animations.
    arrived: Option<std::time::Instant>,
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

/// Draws `appearance` in a `size` rectangle, with `overrides` in place of its dyes' colors.
/// Dragging turns the model and a double click faces it forward again.
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    packages: &Path,
    appearance: Option<Appearance>,
    overrides: &[SurfaceOverride],
    size: egui::Vec2,
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
    still.wanted = appearance.map(|appearance| (packages.to_owned(), appearance));
    still.receive();
    if paused {
        if let Some(load) = still.load.take() {
            load.stop();
        }
    } else {
        still.request(ui.ctx());
    }
    if response.dragged() {
        let delta = response.drag_delta();
        still.camera.yaw += delta.x * 0.01;
        still.camera.pitch =
            (still.camera.pitch + delta.y * 0.01).clamp(-render::MAX_PITCH, render::MAX_PITCH);
    }
    if response.double_clicked() {
        still.camera = Camera::default();
    }
    still.draw(ui, rect, overrides, paused);
    response.on_hover_cursor(egui::CursorIcon::Grab)
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
        match result {
            Ok(model) => {
                // Another item faces forward. The same item keeps the angle it was turned to.
                if self
                    .shown
                    .as_ref()
                    .is_none_or(|shown| shown.1.arrangement != source.1.arrangement)
                {
                    self.camera = Camera::default();
                }
                self.model = Some(Arc::new(model));
                self.shown = Some(source);
                self.error = None;
                self.rendered = None;
                self.arrived = Some(std::time::Instant::now());
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
        let (packages, appearance) = wanted.clone();
        let (repaint, viewport) = (ctx.clone(), ctx.viewport_id());
        std::thread::spawn(move || {
            let result = model_preview::weapon::load_reported(&packages, &appearance, &load);
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
        if let Some(model) = self.model.clone() {
            self.draw_model(ui, &painter, rect, &model, overrides);
        }
        // An older model stays up while the one the page asks for loads.
        if self.shown != self.wanted && !paused {
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

    fn draw_model(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        model: &Arc<Model>,
        overrides: &[SurfaceOverride],
    ) {
        model.set_surface_overrides(overrides);
        if model_preview::gpu::available() {
            let seconds = if model.has_shader_animation() {
                ui.ctx().request_repaint();
                self.arrived.map_or(0.0, |at| at.elapsed().as_secs_f32())
            } else {
                0.0
            };
            self.gpu.paint(
                ui,
                rect,
                model_preview::gpu::Frame {
                    model: model.clone(),
                    camera: self.camera,
                    scene: Scene::default(),
                    style: render::Style::Textured,
                    seconds,
                    positions: None,
                },
            );
            return;
        }
        // The software fallback draws one still frame, again only when the view changes.
        let pixels = render_size(rect.size(), false);
        let key = (self.camera, pixels, overrides.to_vec());
        if self.rendered.as_ref() != Some(&key) {
            let image = render::styled_image(
                model,
                self.camera,
                Scene::default(),
                pixels,
                0.0,
                render::Style::Textured,
            );
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
        }
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}
