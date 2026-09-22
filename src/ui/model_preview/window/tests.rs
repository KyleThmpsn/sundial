use super::*;

pub(super) fn capture_screenshot(ctx: &egui::Context) {
    for event in ctx.input(|i| i.events.clone()) {
        if let egui::Event::Screenshot { image, .. } = event {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("model-preview-test-image"), image));
        }
    }
}

fn request(tag: u32) -> Request {
    Request::new(
        Path::new("packages"),
        Target::Object(tag),
        "Selected Model",
        None,
        false,
    )
}

#[test]
fn launcher_is_compact_lazy_and_only_its_owner_follows_selection() {
    let ctx = egui::Context::default();
    let owner = egui::Id::new("source");
    let draw = |events| {
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = launcher(ui, owner, Some(request(1))).rect;
                });
            },
        );
        rect
    };
    let rect = draw(vec![]);
    assert!(rect.height() < 40.0);
    assert!(!shared(&ctx).lock().unwrap().open);
    assert!(shared(&ctx).lock().unwrap().pending.is_none());
    let click = |pressed| egui::Event::PointerButton {
        pos: rect.center(),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    draw(vec![egui::Event::PointerMoved(rect.center()), click(true)]);
    draw(vec![click(false)]);
    assert!(owned_by(&ctx, owner));
    follow(&ctx, egui::Id::new("other"), request(2));
    assert_eq!(
        shared(&ctx)
            .lock()
            .unwrap()
            .request
            .as_ref()
            .unwrap()
            .selection
            .1,
        Target::Object(1)
    );
    follow(&ctx, owner, request(3));
    assert_eq!(
        shared(&ctx)
            .lock()
            .unwrap()
            .request
            .as_ref()
            .unwrap()
            .selection
            .1,
        Target::Object(3)
    );
    assert!(
        shared(&ctx).lock().unwrap().pending.is_none(),
        "the button never starts a reader"
    );
}

#[test]
fn viewer_survives_the_source_tab_and_escape_closes_without_reopening() {
    let ctx = egui::Context::default();
    let state = shared(&ctx);
    *state.lock().unwrap() = Preview {
        open: true,
        request: Some(request(1)),
        selection: Some(request(1).selection),
        model: Some(Arc::new(Model::default())),
        ..Default::default()
    };
    // No source UI is drawn. The host alone keeps the viewer alive.
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), show);
    }
    assert!(state.lock().unwrap().texture.is_some());
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        show,
    );
    let preview = state.lock().unwrap();
    assert!(!preview.open);
    assert!(preview.texture.is_none() && preview.model.is_none() && preview.request.is_none());
    drop(preview);
    let _ = ctx.run(egui::RawInput::default(), show);
    assert!(!state.lock().unwrap().open);
}

#[test]
fn closing_keeps_the_in_flight_reader_serialized_and_discards_its_result() {
    let (sender, receiver) = mpsc::channel();
    let mut preview = Preview {
        open: true,
        pending: Some((request(1).selection, receiver)),
        ..Default::default()
    };
    preview.close();
    preview.discard_closed_result(&egui::Context::default());
    assert!(preview.pending.is_some());
    sender.send(Ok(Model::default())).unwrap();
    preview.discard_closed_result(&egui::Context::default());
    assert!(preview.pending.is_none() && preview.model.is_none());
}

#[test]
fn paused_viewer_starts_no_reads_and_short_layouts_keep_the_canvas_inside() {
    let ctx = egui::Context::default();
    let mut preview = Preview {
        paused: true,
        request: Some(request(1)),
        ..Default::default()
    };
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| preview.draw_request(ui));
    });
    assert!(preview.pending.is_none());
    preview.paused = false;
    preview.selection = Some(request(1).selection);
    preview.model = Some(Arc::new(Model::default()));
    for size in [egui::vec2(360.0, 320.0), egui::vec2(720.0, 560.0)] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bottom = ui.max_rect().bottom();
                    preview.draw_request(ui);
                    assert!(ui.min_rect().bottom() <= bottom + 1.0);
                });
            },
        );
    }
}

#[cfg(windows)]
mod desktop;
