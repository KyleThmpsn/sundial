//! Real native appearances, legacy draw comparison, retained images and GPU timing.
use super::*;
use sha2::{Digest, Sha256};

struct NativeCase {
    name: String,
    frame: Frame,
}

fn cases(packages: &std::path::Path, output: &std::path::Path) -> Vec<NativeCase> {
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let manager = crate::investment::discovery::open_packages(packages).unwrap();
    let mut cases = Vec::new();
    let mut inputs = Vec::new();
    for (hash, label) in [(0x23DB_942F, "age-old-bond"), (0x90D4_2801, "austringer")] {
        let loadout = catalog
            .preview_loadout(hash)
            .expect("Required native weapon");
        let appearance = catalog.preview_appearance(&loadout);
        let model =
            Arc::new(crate::model_preview::appearance::load(packages, &appearance).unwrap());
        assert!(!model.triangles.is_empty());
        let tags: Vec<_> = model
            .tags
            .iter()
            .map(|tag| {
                let bytes = manager.read_tag(tiger_pkg::TagHash(*tag)).unwrap();
                json!({"tag":format!("{tag:08X}"),"sha256":hex::encode(Sha256::digest(&bytes))})
            })
            .collect();
        inputs.push(json!({
            "item_hash":hash,"label":label,"arrangement":appearance.arrangement,
            "dyes":appearance.dyes,"model_tags":tags,"triangles":model.triangles.len(),
            "vertices":model.vertices.len(),"notices":model.notices,
            "textures":model.textures.iter().map(|texture|json!({"tag":texture.tag,"size":texture.size,
                "decoded_rgba_sha256":hex::encode(Sha256::digest(&texture.rgba))})).collect::<Vec<_>>()
        }));
        for (name, seconds, yaw, style) in [
            ("initial", 0.0, 0.0, Style::Textured),
            ("animated", 0.5, 0.0, Style::Textured),
            ("paused", 0.5, 0.0, Style::Textured),
            ("rotated", 0.5, 1.4, Style::Textured),
            ("solid", 0.5, 1.4, Style::Solid),
            ("rewound", 0.0, 0.0, Style::Textured),
        ] {
            cases.push(NativeCase {
                name: format!("{label}-{name}"),
                frame: Frame {
                    animate: false,
                    dyes: None,
                    model: model.clone(),
                    camera: Camera {
                        yaw,
                        ..Default::default()
                    },
                    scene: Scene::default(),
                    style,
                    seconds,
                    pose: None,
                },
            });
        }
    }
    fs::write(output.join("inputs.json"),serde_json::to_vec_pretty(&json!({
        "native_profile":"Shadowkeep","packages":packages,"appearances":inputs,
        "limits":"Model tag and decoded texture fingerprints identify the examined inputs. This is not a digest of every mounted package."
    })).unwrap()).unwrap();
    cases
}

#[test]
#[ignore = "Opens OpenGL, requires SUNDIAL_PREVIEW_PACKAGES and fresh SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn native_weapon_playback_preserves_images_with_batched_gpu_submission() {
    let packages = crate::test_support::preview_packages();
    let output =
        PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("fresh output"))
            .join("native-throughput");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("Use fresh verification output");
    let cases = cases(&packages, &output);
    let completed = Arc::new(AtomicBool::new(false));
    let done = completed.clone();
    eframe::run_native("Native GPU Preview Verification", eframe::NativeOptions {
        renderer:eframe::Renderer::Glow,
        viewport:egui::ViewportBuilder::default().with_inner_size([660.0,460.0]).with_active(false),
        event_loop_builder:Some(Box::new(crate::test_support::native_event_loop)),
        ..Default::default()
    }, Box::new(move |creation| {
        let gl = creation.gl.as_ref().expect("OpenGL is required");
        // SAFETY: eframe supplies the current context to its creation callback.
        let graphics = unsafe { json!({"vendor":gl.get_parameter_string(glow::VENDOR),
            "renderer":gl.get_parameter_string(glow::RENDERER),"version":gl.get_parameter_string(glow::VERSION)}) };
        fs::write(output.join("graphics.json"),serde_json::to_vec_pretty(&graphics).unwrap()).unwrap();
        Ok(Box::new(Throughput { cases, output, completed:done, at:0, legacy:true,
            sample:0, pending:Arc::new(Mutex::new(None)),renderer:Shared::default(),
            baseline:None, captures:std::collections::BTreeMap::new(),results:Vec::new(),
            started:std::time::Instant::now() }))
    })).unwrap();
    assert!(
        completed.load(Ordering::SeqCst),
        "Inspect the native throughput receipt"
    );
}

struct Throughput {
    cases: Vec<NativeCase>,
    output: PathBuf,
    completed: Arc<AtomicBool>,
    at: usize,
    legacy: bool,
    sample: usize,
    pending: Arc<Mutex<Option<(egui::ColorImage, serde_json::Value)>>>,
    renderer: Shared,
    baseline: Option<egui::ColorImage>,
    captures: std::collections::BTreeMap<String, String>,
    results: Vec<serde_json::Value>,
    started: std::time::Instant,
}

impl Throughput {
    fn seconds(&self) -> f32 {
        let case = &self.cases[self.at];
        // Exercise running playback at distinct timestamps, ending at the pause timestamp.
        // Legacy and indexed submission replay the identical timeline.
        case.frame.seconds
            - if case.name.ends_with("-animated") {
                (9 - self.sample) as f32 / 60.0
            } else {
                0.0
            }
    }

    fn receive(&mut self, image: egui::ColorImage, measurements: serde_json::Value) {
        let name = &self.cases[self.at].name;
        self.results.push(
            json!({"case":name,"sample":self.sample,"seconds":self.seconds(),"warm":self.sample>=2,
            "legacy":self.legacy,"measurements":measurements}),
        );
        self.sample += 1;
        if self.sample < 10 {
            return;
        }
        self.sample = 0;
        let rgba: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect();
        let file = format!(
            "{name}-{}.png",
            if self.legacy { "legacy" } else { "indexed" }
        );
        let png = export::png(&rgba, image.width(), image.height()).unwrap();
        fs::write(self.output.join(&file), &png).unwrap();
        self.captures
            .insert(file, hex::encode(Sha256::digest(&png)));
        fs::write(
            self.output.join("samples.json"),
            serde_json::to_vec_pretty(&self.results).unwrap(),
        )
        .unwrap();
        fs::write(
            self.output.join("images.json"),
            serde_json::to_vec_pretty(&self.captures).unwrap(),
        )
        .unwrap();
        if self.legacy {
            self.baseline = Some(image);
            self.legacy = false;
        } else {
            let baseline = self.baseline.take().unwrap();
            assert_eq!(image.size, baseline.size);
            let difference = image
                .pixels
                .iter()
                .zip(&baseline.pixels)
                .flat_map(|(actual, expected)| {
                    actual
                        .to_array()
                        .into_iter()
                        .zip(expected.to_array())
                        .map(|(a, b)| a.abs_diff(b))
                })
                .max()
                .unwrap();
            assert!(
                difference <= 3,
                "{name}: GPU deformation or indexed drawing changed visible output by {difference}"
            );
            self.results.last_mut().unwrap()["max_image_difference"] = json!(difference);
            let background = image.pixels[0];
            assert!(
                image.pixels.iter().any(|&pixel| pixel != background),
                "{name}: empty image"
            );
            self.at += 1;
            self.legacy = true;
        }
    }

    fn finish(&self) {
        let digest = |name: &str| &self.captures[&format!("age-old-bond-{name}-indexed.png")];
        assert_ne!(
            digest("initial"),
            digest("animated"),
            "The native shell must still animate"
        );
        assert_eq!(digest("animated"), digest("paused"));
        assert_eq!(digest("initial"), digest("rewound"));
        let revision = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(
            revision.status.success(),
            "Source revision is required for the receipt"
        );
        fs::write(self.output.join("verification.json"),serde_json::to_vec_pretty(&json!({
            "revision":String::from_utf8_lossy(&revision.stdout).trim(),
            "executable_sha256":hex::encode(Sha256::digest(fs::read(std::env::current_exe().unwrap()).unwrap())),
            "cases":self.cases.len(),"samples":self.results,"images":self.captures,
            "legacy_image_tolerance":3,"animation_pause_rewind_verified":true,
            "repeat_filter":"model_preview::gpu::verification::throughput::native_weapon_playback_preserves_images_with_batched_gpu_submission",
            "limits":"GPU deformation and indexed submission compared with retained CPU deformation and legacy ordering. Shader binding is shared. Timings exclude model loading, PNG writing and software rendering. Legacy CPU pose sampling is separate from submission. No interactive FPS or gameplay claim."
        })).unwrap()).unwrap();
    }
}

impl eframe::App for Throughput {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        assert!(
            self.started.elapsed().as_secs() < 600,
            "Native preview verification timed out"
        );
        let captured = self.pending.lock().unwrap().take();
        if let Some((image, measurements)) = captured {
            self.receive(image, measurements);
        }
        if self.at == self.cases.len() {
            self.finish();
            self.completed.store(true, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let source = &self.cases[self.at].frame;
        let seconds = self.seconds();
        let sampling = std::time::Instant::now();
        let pose = self
            .legacy
            .then(|| self.renderer.pose(&source.model, seconds))
            .flatten();
        let pose_sample_ms = sampling.elapsed().as_secs_f64() * 1000.0;
        let frame = Frame {
            animate: !self.legacy,
            dyes: None,
            model: source.model.clone(),
            camera: source.camera,
            scene: source.scene,
            style: source.style,
            seconds,
            pose,
        };
        let state = self.renderer.0.clone();
        let pending = self.pending.clone();
        let legacy = self.legacy;
        egui::CentralPanel::default().show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(640.0, 420.0), egui::Sense::hover());
            let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                // SAFETY: the painter owns the current context, and readback storage exactly
                // matches its viewport. The renderer and reference share this context only.
                unsafe {
                    let mut state = state.lock().unwrap();
                    state.legacy_order = legacy;
                    let measurements = measure::draw(gl, &mut state, &info, &frame);
                    assert!(
                        state.fallback.is_none(),
                        "The required model fell back to software"
                    );
                    let Some(mut measurements) = measurements else {
                        return;
                    };
                    measurements["pose_sample_ms"] = json!(pose_sample_ms);
                    let viewport = info.viewport_in_pixels();
                    let (w, h) = (viewport.width_px as usize, viewport.height_px as usize);
                    let mut rgba = vec![0; w * h * 4];
                    gl.read_pixels(
                        viewport.left_px,
                        viewport.from_bottom_px,
                        viewport.width_px,
                        viewport.height_px,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    let mut flipped = vec![0; rgba.len()];
                    for y in 0..h {
                        flipped[y * w * 4..(y + 1) * w * 4]
                            .copy_from_slice(&rgba[(h - y - 1) * w * 4..(h - y) * w * 4]);
                    }
                    *pending.lock().unwrap() = Some((
                        egui::ColorImage::from_rgba_unmultiplied([w, h], &flipped),
                        measurements,
                    ));
                }
            });
            ui.painter().add(egui::Shape::Callback(egui::PaintCallback {
                rect,
                callback: Arc::new(callback),
            }));
        });
        ctx.request_repaint();
    }
}
