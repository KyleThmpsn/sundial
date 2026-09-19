use super::*;

#[test]
fn finishing_or_closing_artwork_preview_waits_for_its_package_reader() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    for poll in [false, true] {
        let mut editor = Editor::new(Kind::Watermark, None);
        let released = Arc::new(AtomicBool::new(false));
        let worker_released = Arc::clone(&released);
        let (result_sender, receiver) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel();
        editor.context_job = Some(receiver);
        editor.context_worker = Some(std::thread::spawn(move || {
            result_sender
                .send(Err("Synthetic preview error".into()))
                .unwrap();
            ready_sender.send(()).unwrap();
            wait_for_release.recv().unwrap();
            worker_released.store(true, Ordering::SeqCst);
        }));
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let (entered_sender, entered) = mpsc::channel();
        let (finished_sender, finished) = mpsc::channel();
        let closer = std::thread::spawn(move || {
            entered_sender.send(()).unwrap();
            if poll {
                editor.poll();
                assert!(editor.context_worker.is_none());
                assert!(editor.context_job.is_none());
                assert_eq!(
                    editor.context_error.as_deref(),
                    Some("Synthetic preview error")
                );
            }
            drop(editor);
            finished_sender
                .send(released.load(Ordering::SeqCst))
                .unwrap();
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = finished.recv_timeout(Duration::from_millis(50)).ok();
        release.send(()).unwrap();
        let reader_released =
            early.unwrap_or_else(|| finished.recv_timeout(Duration::from_secs(5)).unwrap());
        closer.join().unwrap();
        assert!(early.is_none(), "The editor detached its package reader");
        assert!(reader_released);
    }
}

#[test]
fn artwork_preview_tracks_spawned_workers_and_reports_disconnection() {
    let mut editor = Editor::new(Kind::Watermark, None);
    editor.load_context(&egui::Context::default(), Path::new(""), None);
    assert!(editor.context_worker.is_some());
    while !editor.context_worker.as_ref().unwrap().is_finished() {
        std::thread::yield_now();
    }
    editor.poll();
    assert!(editor.context_worker.is_none());
    assert_eq!(
        editor.context_error.as_deref(),
        Some("Select a weapon to preview its icon.")
    );

    let (sender, receiver) = mpsc::channel();
    editor.context_job = Some(receiver);
    editor.context_worker = Some(std::thread::spawn(move || drop(sender)));
    while !editor.context_worker.as_ref().unwrap().is_finished() {
        std::thread::yield_now();
    }
    editor.poll();
    assert!(editor.context_worker.is_none());
    assert!(editor.context_job.is_none());
    assert!(
        editor
            .context_error
            .as_ref()
            .unwrap()
            .contains("stopped unexpectedly")
    );
}

fn source() -> Artwork {
    Artwork::from_source(image::RgbaImage::from_fn(240, 135, |x, y| {
        image::Rgba([
            30 + (x / 2) as u8,
            70 + (y / 2) as u8,
            120,
            if x > 25 && x < 215 { 255 } else { 0 },
        ])
    }))
    .unwrap()
}

fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Action>) {
    let mut action = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            action = editor.show(ctx);
        },
    );
    (output, action)
}

fn text_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    fn find(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|s| find(&s.shape, label))
        .unwrap_or_else(|| panic!("Missing {label}"))
}

#[test]
fn artwork_dialog_keeps_actions_visible_at_small_and_large_sizes() {
    for size in [
        egui::vec2(360.0, 640.0),
        egui::vec2(760.0, 680.0),
        egui::vec2(1200.0, 860.0),
    ] {
        for kind in [Kind::Badge, Kind::Watermark] {
            for tab in [Tab::Placement, Tab::Crop, Tab::Background] {
                if kind == Kind::Watermark && tab == Tab::Background {
                    continue;
                }
                let mut editor = Editor::new(kind, Some(source()));
                editor.tab = tab;
                let before = editor.current().unwrap();
                let ctx = egui::Context::default();
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    (output, _) = frame(&ctx, &mut editor, size, vec![]);
                }
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for label in [kind.title(), "Cancel", "Apply Artwork"] {
                    assert!(
                        screen.contains_rect(text_rect(&output, label)),
                        "{label} overflows {size:?} for {kind:?}: {:?}",
                        text_rect(&output, label)
                    );
                }
                assert_eq!(
                    editor.current().unwrap(),
                    before,
                    "Drawing must not change an artwork draft"
                );
            }
        }
    }
}

#[test]
fn cancel_and_failed_import_keep_the_original_while_apply_returns_the_edit() {
    let original = source();
    let size = egui::vec2(1000.0, 800.0);
    for apply in [false, true] {
        let mut editor = Editor::new(Kind::Badge, Some(original.clone()));
        editor.composition.scale = 170;
        editor.composition.crop = [1000, 1000, 8000, 8000];
        let edited = editor.current().unwrap();
        let (sender, receiver) = mpsc::channel();
        editor.importing = Some(receiver);
        sender.send(Err("Could not decode image".into())).unwrap();
        editor.poll();
        assert_eq!(editor.current().unwrap(), edited);
        editor.error = None;
        let ctx = egui::Context::default();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            (output, _) = frame(&ctx, &mut editor, size, vec![]);
        }
        let pos = text_rect(&output, if apply { "Apply Artwork" } else { "Cancel" }).center();
        let click = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let _ = frame(&ctx, &mut editor, size, click(true));
        let (_, action) = frame(&ctx, &mut editor, size, click(false));
        match (apply, action) {
            (true, Some(Action::Apply(saved))) => assert_eq!(saved, edited),
            (false, Some(Action::Cancel)) => {}
            _ => panic!("The footer did not return the requested action"),
        }
        assert_eq!(original.composition().unwrap().scale, 100);
    }
}
