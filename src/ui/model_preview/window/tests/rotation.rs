//! Window controls, elapsed camera motion and real software uploads from installed art.
use super::*;

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn view_option_rotates_without_starving_software_or_overriding_manual_controls() {
    let packages = crate::test_support::preview_packages();
    let output = crate::test_support::artifact_dir("probes");
    std::fs::create_dir_all(&output).unwrap();
    model_preview::gpu::set_available(false);
    let appearance = Appearance {
        arrangement: 930,
        dyes: vec![],
        dye_textures: vec![],
    };
    let model = Arc::new(model_preview::appearance::load(&packages, &appearance).unwrap());
    assert!(model.triangles.len() > 1_000);
    let mut check = Check {
        ctx: egui::Context::default(),
        output,
        time: 0.0,
        uploads: 0,
        image: None,
        canvas_size: None,
        minimized: false,
    };
    let request = Request::new(
        &packages,
        Target::Weapon(appearance, None),
        "Auto Rotate Check",
        None,
        false,
    );
    let initial = Camera {
        pitch: 0.4,
        zoom: 1.3,
        pan: [0.03, -0.04],
        ..Camera::default()
    };
    *shared(&check.ctx).lock().unwrap() = Preview {
        open: true,
        selection: Some(request.selection.clone()),
        request: Some(request),
        model: Some(model),
        camera: initial,
        source_viewport: Some(egui::ViewportId::ROOT),
        ..Preview::default()
    };
    check.wait_image();
    let baseline = check.image.clone().unwrap();
    check.save_image("rotation-off.png");
    check.time = 1.0;
    check.frame(vec![]);
    assert!(check.camera() == initial, "the option starts off");

    check.toggle();
    let before_uploads = check.uploads;
    check.advance_until_uploads(before_uploads + 2);
    let orbit = check.camera();
    assert!((orbit.yaw - initial.yaw).abs() > 0.02);
    assert_eq!(orbit.pitch, initial.pitch);
    assert_eq!(orbit.zoom, initial.zoom);
    assert_eq!(orbit.pan, initial.pan);
    assert_eq!(shared(&check.ctx).lock().unwrap().seconds, 0.0);
    let changed = baseline
        .pixels
        .iter()
        .zip(&check.image.as_ref().unwrap().pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 100, "orbit must change the displayed model");
    check.save_image("rotation-on.png");
    let orbit_uploads = check.uploads - before_uploads;
    check.manual_drag();

    check.pause_and_restore();

    check.click_label("Reset View");
    assert!(check.camera() == Camera::default());
    check.time += 0.05;
    check.frame(vec![]);
    assert!(check.camera().yaw > Camera::default().yaw);
    check.toggle();
    let stopped = check.camera();
    check.time += 100.0;
    check.frame(vec![]);
    assert!(check.camera() == stopped);
    check.wait_image();
    check.save_image("rotation-stopped.png");
    check.display_controls();
    std::fs::write(check.output.join("rotation-receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "configured_native_appearance": 930, "triangles": shared(&check.ctx).lock().unwrap().model.as_ref().unwrap().triangles.len(),
            "initial_yaw": initial.yaw, "orbit_yaw": orbit.yaw, "changed_pixels": changed,
            "software_uploads_during_orbit": orbit_uploads,
            "manual_drag_paused_orbit": true, "package_resume_has_no_jump": true,
            "minimized_window_resume_has_no_jump": true,
            "reset_view_restores_camera": true, "disabled_camera_stays_fixed": true,
            "material_playback_seconds": 0.0,
            "display_controls_change_pixels_and_restore": true,
        })).unwrap()).unwrap();
}

struct Check {
    ctx: egui::Context,
    output: PathBuf,
    time: f64,
    uploads: usize,
    image: Option<Arc<egui::ColorImage>>,
    canvas_size: Option<[usize; 2]>,
    minimized: bool,
}

impl Check {
    fn display_controls(&mut self) {
        let processed = self.image.clone().unwrap();
        for label in ["Bloom", "Filmic Output"] {
            self.click_label("View");
            self.click_label(label);
            self.click(egui::pos2(2.0, 2.0));
        }
        self.wait_image();
        let raw = self.image.clone().unwrap();
        assert!(
            processed
                .pixels
                .iter()
                .zip(&raw.pixels)
                .filter(|(a, b)| a != b)
                .count()
                > 100,
            "display controls must change the rendered preview"
        );
        self.save_image("output-disabled.png");
        for label in ["Bloom", "Filmic Output"] {
            self.click_label("View");
            self.click_label(label);
            self.click(egui::pos2(2.0, 2.0));
        }
        self.wait_image();
        self.save_image("output-restored.png");
        let restored = self.image.as_ref().unwrap();
        assert!(
            processed.size == restored.size,
            "control comparison changed canvas size"
        );
        assert!(
            processed.pixels == restored.pixels,
            "restoring display options must restore the preview"
        );
    }

    fn pause_and_restore(&mut self) {
        pause_source(&self.ctx, true);
        let paused = self.camera();
        self.time += 100.0;
        self.frame(vec![]);
        assert!(self.camera() == paused);
        pause_source(&self.ctx, false);
        self.time += 100.0;
        self.frame(vec![]);
        assert!(
            self.camera() == paused,
            "resume must not consume the pause interval"
        );
        self.minimized = true;
        self.time += 100.0;
        self.frame(vec![]);
        assert!(self.camera() == paused);
        self.minimized = false;
        self.time += 100.0;
        self.frame(vec![]);
        assert!(
            self.camera() == paused,
            "restoring the window must not jump"
        );
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 960.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .minimized = Some(self.minimized);
        let output = self.ctx.run_ui(input, |ui| show(ui));
        crate::test_support::capture::record(&output);
        let texture = shared(&self.ctx)
            .lock()
            .unwrap()
            .texture
            .as_ref()
            .map(egui::TextureHandle::id);
        if let Some((_, delta)) = output
            .textures_delta
            .set
            .iter()
            .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
            .find(|(id, delta)| Some(**id) == texture && delta.pos.is_none())
        {
            let egui::ImageData::Color(image) = &delta.image;
            self.image = Some(image.clone());
            self.uploads += 1;
        }
        self.canvas_size = texture
            .and_then(|texture| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| image_bounds(&shape.shape, texture))
            })
            .map(|rect| {
                crate::ui::model_preview::render_size(rect.size(), self.ctx.pixels_per_point())
            });
        output
    }

    fn camera(&self) -> Camera {
        shared(&self.ctx).lock().unwrap().camera
    }

    fn click_label(&mut self, label: &str) {
        let output = self.frame(vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| label_position(&shape.shape, label))
            .unwrap_or_else(|| panic!("missing preview action {label}"));
        self.click(position);
    }

    fn click(&mut self, position: egui::Pos2) {
        for pressed in [true, false] {
            self.frame(crate::test_support::primary_press(position, pressed));
        }
    }

    fn toggle(&mut self) {
        self.click_label("View");
        let menu = self.frame(vec![]);
        crate::test_support::capture::write(&self.ctx, &menu, "preview-auto-rotate-menu");
        self.click_label("Auto Rotate");
        self.click(egui::pos2(2.0, 2.0));
    }

    fn wait_image(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !self.has_current_image() {
            assert!(
                std::time::Instant::now() < deadline,
                "software preview stopped producing frames"
            );
            self.frame(vec![]);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn has_current_image(&self) -> bool {
        let state = shared(&self.ctx);
        let preview = state.lock().unwrap();
        self.image.is_some()
            && preview.rendered.is_some_and(|(camera, scene, size, _)| {
                camera == preview.camera && scene == preview.scene && Some(size) == self.canvas_size
            })
    }

    fn advance_until_uploads(&mut self, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.uploads < count {
            assert!(
                std::time::Instant::now() < deadline,
                "moving camera starved software rendering"
            );
            self.time += 0.05;
            self.frame(vec![]);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn manual_drag(&mut self) {
        let output = self.frame(vec![]);
        let texture = shared(&self.ctx)
            .lock()
            .unwrap()
            .texture
            .as_ref()
            .unwrap()
            .id();
        let canvas = output
            .shapes
            .iter()
            .find_map(|shape| image_bounds(&shape.shape, texture))
            .unwrap();
        let pointer = canvas.center();
        self.frame(crate::test_support::primary_press(pointer, true));
        let held = self.camera();
        self.time += 5.0;
        self.frame(vec![]);
        assert!(
            self.camera() == held,
            "a held pointer pauses orbit before dragging"
        );
        let moved = pointer + egui::vec2(30.0, 8.0);
        self.frame(vec![egui::Event::PointerMoved(moved)]);
        let dragged = self.camera();
        assert!((dragged.yaw - held.yaw - 0.30).abs() < 0.0001);
        assert!((dragged.pitch - held.pitch - 0.08).abs() < 0.0001);
        assert_eq!(dragged.zoom, held.zoom);
        assert_eq!(dragged.pan, held.pan);
        self.frame(crate::test_support::primary_press(moved, false));
        assert!(self.camera() == dragged);
        self.time += 0.05;
        self.frame(vec![]);
        assert!(
            self.camera().yaw > dragged.yaw,
            "orbit resumes from the dragged angle"
        );
    }

    fn save_image(&self, name: &str) {
        let image = self.image.as_ref().unwrap();
        let pixels: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect();
        std::fs::write(
            self.output.join(name),
            model_preview::export::png(&pixels, image.width(), image.height()).unwrap(),
        )
        .unwrap();
    }
}

fn label_position(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
    match shape {
        egui::Shape::Text(text) if text.galley.text() == label => {
            Some(text.pos + text.galley.size() * 0.5)
        }
        egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| label_position(shape, label)),
        _ => None,
    }
}

fn image_bounds(shape: &egui::Shape, texture: egui::TextureId) -> Option<egui::Rect> {
    match shape {
        egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
        egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| image_bounds(shape, texture)),
        _ => None,
    }
}
