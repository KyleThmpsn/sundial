//! One native viewer, independent of the tab that last supplied its selection.
use super::*;

#[derive(Clone)]
pub(super) struct Request {
    selection: Selection,
    name: String,
    access: Option<Arc<crate::catalog::PackageInspectionAccess>>,
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
    let response = ui
        .add_enabled(enabled, egui::Button::new("Model Preview…"))
        .on_hover_text("Open the selected model in a separate window.")
        .on_disabled_hover_text("Choose a game installation to preview models.");
    let shared = shared(ui.ctx());
    let Ok(mut preview) = shared.lock() else {
        return response;
    };
    if response.clicked() {
        preview.open = true;
        preview.owner = Some(owner);
        preview.source_viewport = Some(ui.ctx().viewport_id());
        preview.paused = false;
        preview.focus_requested = true;
        ui.ctx().request_repaint_of(viewport_id());
    }
    if preview.open && preview.owner == Some(owner) {
        update_request(ui.ctx(), &mut preview, request);
    }
    response
}

/// Temporarily disables a source viewport's model reads during package operations.
pub fn pause_source(ctx: &egui::Context, paused: bool) {
    let shared = shared(ctx);
    if let Ok(mut preview) = shared.lock()
        && preview.source_viewport == Some(ctx.viewport_id())
        && preview.paused != paused
    {
        preview.paused = paused;
        ctx.request_repaint_of(viewport_id());
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
        preview.model = None;
        preview.texture = None;
        preview.selection = None;
        ctx.request_repaint_of(viewport_id());
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("model-preview-window")
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
    let title = {
        let Ok(mut preview) = shared.lock() else {
            return;
        };
        if !preview.open {
            preview.discard_closed_result(ctx);
            return;
        }
        preview.request.as_ref().map_or_else(
            || "Model Preview".into(),
            |request| format!("Model Preview: {}", request.name),
        )
    };
    ctx.show_viewport_deferred(
        viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([720.0, 560.0])
            .with_min_inner_size([360.0, 320.0])
            .with_resizable(true),
        move |child, class| {
            let Ok(mut preview) = shared.lock() else {
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
                    .default_size([720.0, 560.0])
                    .min_size([320.0, 280.0])
                    .show(child, |ui| preview.draw_request(ui));
            } else {
                egui::CentralPanel::default().show(child, |ui| preview.draw_request(ui));
            }
            #[cfg(test)]
            tests::capture_screenshot(child);
            let close = !open
                || child.input(|i| i.viewport().close_requested())
                || child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if close {
                preview.close();
                child.request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}

impl Preview {
    fn draw_request(&mut self, ui: &mut egui::Ui) {
        let Some(mut request) = self.request.clone() else {
            ui.label("No model is selected.");
            return;
        };
        if self.paused
            || !request.enabled
            || request.access.as_ref().is_some_and(|a| a.is_suspended())
        {
            ui.heading(&request.name);
            ui.label("Model preview is paused while package access is unavailable.");
            self.last_tick = None;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        if let Target::Weapon(_, generation) = &mut request.selection.1 {
            *generation = request.access.as_ref().map(|a| a.generation());
        }
        self.preserve_camera = request.preserve_camera;
        self.sync_with_access(ui.ctx(), request.selection, request.access);
        self.draw(ui, &request.name);
    }

    fn close(&mut self) {
        self.open = false;
        self.request = None;
        self.owner = None;
        self.model = None;
        self.texture = None;
        self.selection = None;
        self.error = None;
        self.rendered = None;
        self.last_tick = None;
        self.playing = false;
        // Keep an in-flight reader until it finishes, preventing overlapping reads on reopen.
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
