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
        let _ = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    rect = launcher(ui, owner, Some(request(1))).rect;
                });
            },
        );
        rect
    };
    let rect = draw(vec![]);
    assert!(
        rect.height() <= 26.0,
        "launcher was {} pixels high",
        rect.height()
    );
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

#[cfg(any(windows, target_os = "linux"))]
mod desktop;

mod inspection;
mod rotation;
