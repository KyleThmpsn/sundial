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

#[test]
fn artwork_editor_visual_capture() {
    let Some(directory) = std::env::var_os("PARHELION_ARTWORK_CAPTURE_DIR").map(PathBuf::from)
    else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    let image = std::env::var_os("PARHELION_ARTWORK_CAPTURE_IMAGE")
        .map(PathBuf::from)
        .map(|path| Artwork::from_path(&path).unwrap())
        .unwrap_or_else(source);
    let watermark = std::env::var_os("PARHELION_ARTWORK_CAPTURE_WATERMARK")
        .map(PathBuf::from)
        .map(|path| Artwork::from_path(&path).unwrap())
        .unwrap_or_else(source);
    let packages = std::env::var_os("SUNDIAL_TEST_PACKAGES").map(PathBuf::from);
    for (name, kind, tab, width) in [
        ("badge-background", Kind::Badge, Tab::Background, 1100.0),
        ("badge-crop", Kind::Badge, Tab::Crop, 1100.0),
        ("badge-narrow", Kind::Badge, Tab::Placement, 380.0),
        ("watermark", Kind::Watermark, Tab::Placement, 1100.0),
    ] {
        let mut editor = Editor::new(
            kind,
            Some(if kind == Kind::Badge {
                image.clone()
            } else {
                watermark.clone()
            }),
        );
        editor.tab = tab;
        if kind == Kind::Badge {
            editor.composition.background = Background::Gradient {
                start: [5, 18, 24],
                end: [38, 82, 88],
                angle: 90,
            };
        }
        if let Some(packages) = &packages {
            editor.context = Some(match kind {
                Kind::Badge => {
                    let manager =
                        sundial::package_authoring::open_shadowkeep_package_manager(packages)
                            .unwrap();
                    ContextPreview::Badge(crate::badge_icon::preview_mask(&manager).unwrap())
                }
                Kind::Watermark => ContextPreview::Watermark(Box::new(
                    crate::icon_edit::WatermarkPreview::load(
                        packages,
                        tiger_pkg::TagHash(0x8132_5796),
                        crate::AuthoredWeaponRarity::Legendary,
                        crate::WeaponIconEdit::default(),
                    )
                    .unwrap(),
                )),
            });
        }
        editor.composition.scale = 90;
        let ctx = egui::Context::default();
        let size = egui::vec2(width, 800.0);
        let mut textures = std::collections::HashMap::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            (output, _) = frame(&ctx, &mut editor, size, vec![]);
            capture_textures(&mut textures, &output);
        }
        capture(&directory, name, &ctx, output, size, textures);
    }
}

fn capture_textures(
    textures: &mut std::collections::HashMap<egui::TextureId, egui::ColorImage>,
    output: &egui::FullOutput,
) {
    for (id, delta) in &output.textures_delta.set {
        let image = match &delta.image {
            egui::ImageData::Color(image) => (**image).clone(),
            egui::ImageData::Font(image) => egui::ColorImage {
                size: image.size,
                pixels: image.srgba_pixels(None).collect(),
            },
        };
        if let Some([x, y]) = delta.pos {
            let target = textures
                .get_mut(id)
                .expect("partial texture update follows a full upload");
            for row in 0..image.size[1] {
                let start = (y + row) * target.size[0] + x;
                target.pixels[start..start + image.size[0]]
                    .copy_from_slice(&image.pixels[row * image.size[0]..(row + 1) * image.size[0]]);
            }
        } else {
            textures.insert(*id, image);
        }
    }
}

fn capture(
    directory: &Path,
    name: &str,
    ctx: &egui::Context,
    output: egui::FullOutput,
    size: egui::Vec2,
    textures: std::collections::HashMap<egui::TextureId, egui::ColorImage>,
) {
    let texture_name = |id: egui::TextureId| match id {
        egui::TextureId::Managed(n) => format!("{name}-texture-{n}.png"),
        _ => panic!("unexpected native texture"),
    };
    for (id, image) in textures {
        let pixels = image
            .pixels
            .iter()
            .flat_map(|c| c.to_array())
            .collect::<Vec<_>>();
        image::save_buffer(
            directory.join(texture_name(id)),
            &pixels,
            image.size[0] as u32,
            image.size[1] as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    let meshes = ctx.tessellate(output.shapes, 1.0).into_iter().filter_map(|p| {
        let egui::epaint::Primitive::Mesh(mesh) = p.primitive else { return None; };
        Some(serde_json::json!({"texture": texture_name(mesh.texture_id), "clip": [p.clip_rect.min.x,p.clip_rect.min.y,p.clip_rect.max.x,p.clip_rect.max.y],
            "indices": mesh.indices, "vertices": mesh.vertices.iter().map(|v| serde_json::json!([v.pos.x,v.pos.y,v.uv.x,v.uv.y,v.color.to_array()])).collect::<Vec<_>>() }))
    }).collect::<Vec<_>>();
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!({"width": size.x,"height": size.y,"meshes": meshes}))
            .unwrap(),
    )
    .unwrap();
}
