//! One native viewer, independent of the tab that last supplied its selection.
use super::*;

#[derive(Clone)]
pub(super) struct Request {
    selection: Selection,
    name: String,
    pub(super) access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
    preserve_camera: bool,
    enabled: bool,
}

impl Request {
    pub(super) fn new(
        packages: &Path,
        target: Target,
        name: &str,
        access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
        preserve_camera: bool,
    ) -> Self {
        Self {
            selection: (packages.to_owned(), target),
            name: name.into(),
            access,
            preserve_camera,
            enabled: true,
        }
    }
}

fn shared(ctx: &egui::Context) -> Arc<Mutex<Preview>> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Preview>>>(egui::Id::new("shared-model-preview"))
            .clone()
    })
}

pub(super) fn source_id(ui: &egui::Ui, kind: &str) -> egui::Id {
    egui::Id::new((
        "model-preview-source",
        kind,
        ui.ctx().viewport_id(),
        ui.layer_id().id,
    ))
}

pub(super) fn owned_by(ctx: &egui::Context, owner: egui::Id) -> bool {
    shared(ctx)
        .lock()
        .is_ok_and(|preview| preview.open && preview.owner == Some(owner))
}

pub(super) fn launcher(
    ui: &mut egui::Ui,
    owner: egui::Id,
    mut request: Option<Request>,
) -> egui::Response {
    let enabled = ui.is_enabled() && request.is_some();
    if let Some(request) = &mut request {
        request.enabled = enabled;
    }
    let active = owned_by(ui.ctx(), owner);
    let label = format!("{}  Model Preview", egui_phosphor::regular::CUBE);
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new(label)
                .selected(active)
                .min_size(egui::vec2(132.0, 24.0)),
        )
        .on_hover_text("Open in a separate window")
        .on_disabled_hover_text("No preview for this selection");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Model Preview")
    });
    let shared = shared(ui.ctx());
    let Ok(mut preview) = shared.lock() else {
        return response;
    };
    if response.clicked() {
        open(ui.ctx(), &mut preview, owner);
    }
    if preview.open && preview.owner == Some(owner) {
        update_request(ui.ctx(), &mut preview, request);
    }
    response
}

/// Side of the corner launcher, and its inset from the corner.
const CORNER_SIZE: f32 = 24.0;
const CORNER_INSET: f32 = 6.0;

/// A small icon in the top-right corner of a preview at `over` that opens the viewer on
/// `request`. It shows while the pointer is on the preview, and selected while the viewer is
/// open on `owner`.
pub(super) fn corner_launcher(
    ui: &mut egui::Ui,
    owner: egui::Id,
    over: egui::Rect,
    request: Request,
) -> Option<egui::Response> {
    let active = owned_by(ui.ctx(), owner);
    let response = (active || ui.rect_contains_pointer(over)).then(|| {
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                over.right() - CORNER_INSET - CORNER_SIZE,
                over.top() + CORNER_INSET,
            ),
            egui::Vec2::splat(CORNER_SIZE),
        );
        let response = ui.interact(rect, owner.with("corner-launcher"), egui::Sense::click());
        let visuals = ui.style().interact_selectable(&response, active);
        ui.painter().rect(
            rect,
            3.0,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::ARROW_SQUARE_OUT,
            crate::ui::icon_font(ui, 14.0),
            visuals.text_color(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open in Window")
        });
        response.on_hover_text("Open in a separate window")
    });
    let shared = shared(ui.ctx());
    let Ok(mut preview) = shared.lock() else {
        return response;
    };
    if response.as_ref().is_some_and(egui::Response::clicked) {
        open(ui.ctx(), &mut preview, owner);
    }
    if preview.open && preview.owner == Some(owner) {
        update_request(ui.ctx(), &mut preview, Some(request));
    }
    response
}

/// Open and focus a one-shot selection that remains visible after its source menu closes.
pub(super) fn open_request(ctx: &egui::Context, owner: egui::Id, request: Request) {
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock() {
        open(ctx, &mut preview, owner);
        update_request(ctx, &mut preview, Some(request));
    }
}

/// Opens the viewer for `owner`, in front.
fn open(ctx: &egui::Context, preview: &mut Preview, owner: egui::Id) {
    preview.open = true;
    preview.owner = Some(owner);
    preview.source_viewport = Some(ctx.viewport_id());
    preview.paused = false;
    preview.focus_requested = true;
    ctx.request_repaint_of(viewport_id());
}

/// Temporarily disables a source viewport's model reads during package operations.
pub fn pause_source(ctx: &egui::Context, paused: bool) {
    super::still::pause(ctx, paused);
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock()
        && preview.source_viewport == Some(ctx.viewport_id())
        && preview.paused != paused
    {
        preview.paused = paused;
        ctx.request_repaint_of(viewport_id());
    }
}

/// Stops the current model read so suspending package access does not wait for it.
pub fn stop_reads(ctx: &egui::Context) {
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock()
        && let Some(load) = preview.load.take()
    {
        load.stop();
    }
}

pub(super) fn follow(ctx: &egui::Context, owner: egui::Id, request: Request) {
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock()
        && preview.open
        && preview.owner == Some(owner)
    {
        update_request(ctx, &mut preview, Some(request));
    }
}

pub(super) fn clear_source(ctx: &egui::Context, owner: egui::Id) {
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock()
        && preview.owner == Some(owner)
    {
        preview.request = None;
        preview.source_selection = None;
        preview.navigation.clear();
        // The pending receiver still drains, so stopping here only ends an unwanted read sooner.
        if let Some(load) = preview.load.take() {
            load.stop();
        }
        preview.load_started = None;
        preview.load_time = None;
        preview.status = None;
        preview.asset_entry = None;
        preview.model = None;
        preview.texture = None;
        preview.asset_textures.clear();
        preview.stop_audio();
        preview.audio_pending = None;
        preview.audio_status = None;
        preview.selection = None;
        ctx.request_repaint_of(viewport_id());
    }
}

pub(super) fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("model-preview-window")
}

fn details_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("model-preview-details-window")
}

fn update_request(ctx: &egui::Context, preview: &mut Preview, request: Option<Request>) {
    let changed = match (&preview.request, &request) {
        (Some(old), Some(new)) => {
            old.selection != new.selection
                || old.name != new.name
                || old.enabled != new.enabled
                || old.preserve_camera != new.preserve_camera
        }
        (None, None) => false,
        _ => true,
    };
    preview.request = request;
    if changed {
        ctx.request_repaint_of(viewport_id());
    }
}

/// Draw once per host frame, after the source windows have supplied their selections.
/// Keeping this outside the source tabs also keeps the native window alive between tabs.
pub fn show(ctx: &egui::Context) {
    let shared = shared(ctx);
    let (title, details_open) = {
        let Ok(mut preview) = shared.lock() else {
            return;
        };
        if !preview.open {
            preview.discard_closed_result(ctx);
            return;
        }
        let title = preview.request.as_ref().map_or_else(
            || "Model Preview".into(),
            |request| format!("Model Preview: {}", request.name),
        );
        (title, preview.details_open)
    };
    let viewer = shared.clone();
    ctx.show_viewport_deferred(
        viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(crate::ui::native_title(&title))
            .with_icon(crate::ui::window_icon())
            .with_inner_size([1040.0, 760.0])
            .with_min_inner_size([360.0, 320.0])
            .with_resizable(true),
        move |child, class| {
            let Ok(mut preview) = viewer.lock() else {
                return;
            };
            if std::mem::take(&mut preview.focus_requested) {
                child.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            let mut open = true;
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(&title)
                    .id(egui::Id::new("model-preview-fallback"))
                    .open(&mut open)
                    .collapsible(false)
                    .default_size([1040.0, 760.0])
                    .min_size([320.0, 280.0])
                    .show(child, |ui| preview.draw_request(ui));
            } else {
                egui::CentralPanel::default().show(child, |ui| preview.draw_request(ui));
            }
            #[cfg(test)]
            tests::capture_screenshot(child);
            if preview.details_open {
                let shown = preview
                    .model
                    .as_ref()
                    .map(|model| Arc::as_ptr(model) as usize);
                if preview.details_shown != shown {
                    preview.details_shown = shown;
                    child.request_repaint_of(details_viewport_id());
                }
            }
            let close = !open
                || child.input(|i| i.viewport().close_requested())
                || child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if close {
                preview.close();
                child.request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
    if details_open {
        show_details(ctx, shared);
    }
}

/// The Details window, on whatever the viewer shows. It closes with the viewer.
fn show_details(ctx: &egui::Context, shared: Arc<Mutex<Preview>>) {
    const TITLE: &str = "Details";
    ctx.show_viewport_deferred(
        details_viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_icon(crate::ui::window_icon())
            .with_inner_size([760.0, 640.0])
            .with_min_inner_size([360.0, 300.0])
            .with_resizable(true),
        move |child, class| {
            let Ok(mut preview) = shared.lock() else {
                return;
            };
            if std::mem::take(&mut preview.details_focus_requested) {
                child.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            // The heading follows the viewer, which can move between host frames.
            let name = preview
                .navigation
                .last()
                .map(|(_, name)| name.clone())
                .or_else(|| preview.request.as_ref().map(|request| request.name.clone()))
                .unwrap_or_default();
            let mut open = true;
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(TITLE)
                    .id(egui::Id::new("model-preview-details-fallback"))
                    .open(&mut open)
                    .collapsible(false)
                    .default_size([760.0, 640.0])
                    .min_size([320.0, 260.0])
                    .show(child, |ui| preview.draw_details_window(ui, &name));
            } else {
                egui::CentralPanel::default()
                    .show(child, |ui| preview.draw_details_window(ui, &name));
            }
            if preview.pending.is_some() {
                child.request_repaint_after(std::time::Duration::from_millis(100));
            }
            let close = !open
                || child.input(|i| i.viewport().close_requested())
                || child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if close {
                preview.details_open = false;
                preview.details_shown = None;
                child.request_repaint_of(viewport_id());
                child.request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}

impl Preview {
    fn draw_request(&mut self, ui: &mut egui::Ui) {
        let Some(mut request) = self.request.clone() else {
            ui.weak("No model selected");
            self.discard_closed_result(ui.ctx());
            return;
        };
        if self.paused
            || !request.enabled
            || request.access.as_ref().is_some_and(|a| a.is_suspended())
        {
            ui.heading(&request.name);
            ui.weak("Paused while packages are unavailable");
            self.last_tick = None;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        if let Target::Weapon(_, generation) = &mut request.selection.1 {
            *generation = request.access.as_ref().map(|a| a.generation());
        }
        let (selection, name) = self.resolve_request(&request);
        self.preserve_camera = request.preserve_camera;
        self.sync_with_access(ui.ctx(), selection, request.access);
        self.draw(ui, &name);
    }

    fn resolve_request(&mut self, request: &Request) -> (Selection, String) {
        if self.source_selection.as_ref() != Some(&request.selection) {
            self.source_selection = Some(request.selection.clone());
            self.navigation.clear();
        }
        if let Some((tag, name)) = self.navigation.last() {
            (
                (request.selection.0.clone(), Target::Object(*tag)),
                name.clone(),
            )
        } else {
            (request.selection.clone(), request.name.clone())
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.request = None;
        self.source_selection = None;
        self.navigation.clear();
        self.details_open = false;
        self.details_shown = None;
        self.asset_entry = None;
        self.owner = None;
        self.model = None;
        self.texture = None;
        self.asset_textures.clear();
        self.stop_audio();
        self.audio_pending = None;
        self.audio_status = None;
        self.selection = None;
        self.error = None;
        self.rendered = None;
        self.last_tick = None;
        self.playing = false;
        self.status = None;
        self.load_started = None;
        self.load_time = None;
        // Stop the reader rather than paying for an object nobody will look at. The pending
        // receiver is still drained on later frames so a reopen never overlaps two reads.
        if let Some(load) = self.load.take() {
            load.stop();
        }
    }

    fn discard_closed_result(&mut self, ctx: &egui::Context) {
        if let Some((_, receiver)) = &self.pending {
            if matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            } else {
                self.pending = None;
            }
        }
    }
}

#[cfg(test)]
mod tests;
