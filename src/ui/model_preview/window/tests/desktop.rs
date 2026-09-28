//! Real viewport, resize, shader and close checks using read-only installed assets.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn air_weak_effect_window_screenshot() {
    capture_effect(0x80BC_12EB, "Air Weak 2 FX Sequence", "air-weak-window.png");
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn point_emitter_window_screenshot() {
    capture_effect(
        0x80BB_AEFC,
        "Point Emitter Sample",
        "point-emitter-window.png",
    );
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn meshless_effect_graph_window_screenshot() {
    capture_effect(0x80C0_D581, "Harpy Eye Flash", "harpy-eye-flash-window.png");
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn meshless_particle_sequence_window_screenshot() {
    capture_effect(0x80C0_16E3, "Seeker Firing", "seeker-firing-window.png");
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn sound_event_window_screenshot() {
    capture_asset(
        0x80BB_E621,
        "Air Weak Sound Event",
        "air-weak-sound-window.png",
        CaptureKind::Sound,
    );
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn light_volume_window_screenshot() {
    capture_asset(
        0x80FD_E2F9,
        "Shadowing Light",
        "shadow-light-window.png",
        CaptureKind::Light,
    );
}

#[derive(Clone, Copy)]
enum CaptureKind {
    Effect,
    Sound,
    Light,
}

fn capture_effect(tag: u32, label: &'static str, filename: &'static str) {
    capture_asset(tag, label, filename, CaptureKind::Effect);
}

fn capture_asset(tag: u32, label: &'static str, filename: &'static str, kind: CaptureKind) {
    let packages = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let finished = completed.clone();
    eframe::run_native(
        "Asset Preview Capture",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([360.0, 160.0]),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |creation| {
            assert!(
                creation.gl.is_some(),
                "the capture must exercise the OpenGL app path"
            );
            crate::model_preview::gpu::set_available(creation.gl.is_some());
            let state = shared(&creation.egui_ctx);
            let mut preview = state.lock().unwrap();
            preview.open = true;
            preview.owner = Some(egui::Id::new("effect-capture"));
            preview.source_viewport = Some(egui::ViewportId::ROOT);
            preview.request = Some(Request::new(
                &packages,
                Target::Object(tag),
                label,
                None,
                false,
            ));
            drop(preview);
            Ok(Box::new(EffectCapture {
                output,
                label,
                filename,
                kind,
                completed: finished,
                requested: false,
                settled: 0,
                started: std::time::Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(completed.load(Ordering::SeqCst));
}

struct EffectCapture {
    output: PathBuf,
    label: &'static str,
    filename: &'static str,
    kind: CaptureKind,
    completed: Arc<AtomicBool>,
    requested: bool,
    settled: usize,
    started: std::time::Instant,
}

impl eframe::App for EffectCapture {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        assert!(
            self.started.elapsed().as_secs() < 90,
            "effect preview timed out"
        );
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(format!("Capturing {}", self.label));
        });
        show(ctx);
        let viewport = egui::ViewportId::from_hash_of("model-preview-window");
        if self.requested {
            let image = ctx.data_mut(|data| {
                data.remove_temp::<Arc<egui::ColorImage>>(egui::Id::new("model-preview-test-image"))
            });
            if let Some(image) = image {
                let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let png = crate::model_preview::export::png(&rgba, image.width(), image.height())
                    .unwrap();
                std::fs::write(self.output.join(self.filename), png).unwrap();
                self.completed.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            let state = shared(ctx);
            let mut preview = state.lock().unwrap();
            assert!(preview.error.is_none(), "{:?}", preview.error);
            if self.label == "Air Weak 2 FX Sequence"
                && preview.model.is_some()
                && (preview.playing || (preview.seconds - 0.17).abs() > 0.001)
            {
                preview.playing = false;
                preview.seconds = 0.17;
                preview.rendered_seconds = None;
            }
            if let Some(model) = &preview.model {
                let supported = match self.kind {
                    CaptureKind::Effect => {
                        !model.particle_sources.is_empty() || !model.assets.effect_nodes.is_empty()
                    }
                    CaptureKind::Sound => !model.assets.sounds.is_empty(),
                    CaptureKind::Light => model.light_geometry,
                };
                assert!(
                    supported,
                    "{} did not expose its expected asset",
                    self.label
                );
            }
            if preview.model.is_some() && preview.pending.is_none() {
                self.settled += 1;
            }
            drop(preview);
            if self.settled > 10 {
                ctx.send_viewport_cmd_to(
                    viewport,
                    egui::ViewportCommand::Screenshot(Default::default()),
                );
                self.requested = true;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[test]
#[ignore = "opens a native viewer and reads SUNDIAL_PREVIEW_PACKAGES"]
fn native_popout_resizes_tracks_shaders_and_closes_independently() {
    let packages = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let completed = Arc::new(AtomicBool::new(false));
    let finished = completed.clone();
    eframe::run_native(
        "Model Preview Design Check",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([820.0, 680.0])
                .with_active(false),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |creation| {
            crate::model_preview::gpu::set_available(
                creation.gl.is_some() && std::env::var_os("SUNDIAL_PREVIEW_CPU").is_none(),
            );
            let ctx = &creation.egui_ctx;
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            ctx.set_fonts(fonts);
            let state = shared(ctx);
            let mut preview = state.lock().unwrap();
            preview.open = true;
            preview.owner = Some(weapon_source(ctx));
            preview.source_viewport = Some(egui::ViewportId::ROOT);
            Ok(Box::new(Check {
                packages,
                completed: finished,
                phase: 0,
                root_requested: false,
                root_captured: false,
                requested: false,
                settled: 0,
                started: std::time::Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(completed.load(Ordering::SeqCst));
}

struct Check {
    packages: PathBuf,
    completed: Arc<AtomicBool>,
    phase: usize,
    root_requested: bool,
    root_captured: bool,
    requested: bool,
    settled: usize,
    started: std::time::Instant,
}

impl eframe::App for Check {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        assert!(
            !ctx.embed_viewports(),
            "the check requires a native pop-out"
        );
        assert!(
            self.started.elapsed().as_secs() < 90,
            "native preview timed out at phase {}, requested {}, settled {}",
            self.phase,
            self.requested,
            self.settled
        );
        let viewport = egui::ViewportId::from_hash_of("model-preview-window");
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Weapon Preview");
            if self.phase < 2 {
                weapon(
                    ui,
                    &self.packages,
                    appearance(self.phase),
                    "Current Appearance",
                );
            }
        });
        show(ctx);
        if !self.root_captured {
            if self.root_requested {
                for event in ctx.input(|input| input.events.clone()) {
                    if let egui::Event::Screenshot { image, .. } = event {
                        let rgba: Vec<_> = image
                            .pixels
                            .iter()
                            .flat_map(|pixel| pixel.to_array())
                            .collect();
                        let png =
                            crate::model_preview::export::png(&rgba, image.width(), image.height())
                                .unwrap();
                        let output = std::env::var_os("SUNDIAL_PROBE_OUT")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("examples"));
                        std::fs::create_dir_all(&output).unwrap();
                        std::fs::write(output.join("model-preview-launcher.png"), png).unwrap();
                        self.root_captured = true;
                    }
                }
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.root_requested = true;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        if self.phase == 2 {
            if !shared(ctx).lock().unwrap().open {
                self.completed.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            self.capture(ctx, viewport);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

impl Check {
    fn capture(&mut self, ctx: &egui::Context, viewport: egui::ViewportId) {
        ctx.request_repaint_of(viewport);
        if self.requested {
            let image = ctx.data_mut(|data| {
                data.remove_temp::<Arc<egui::ColorImage>>(egui::Id::new("model-preview-test-image"))
            });
            if let Some(image) = image {
                let mut bytes =
                    format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
                bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                std::fs::write(
                    format!("examples/model-popout-after-{}.ppm", self.phase),
                    bytes,
                )
                .unwrap();
                self.phase += 1;
                self.requested = false;
                self.settled = 0;
                let command = if self.phase == 1 {
                    egui::ViewportCommand::InnerSize(egui::vec2(360.0, 320.0))
                } else {
                    egui::ViewportCommand::Close
                };
                ctx.send_viewport_cmd_to(viewport, command);
            }
            return;
        }
        let state = shared(ctx);
        let mut preview = state.lock().unwrap();
        assert!(preview.error.is_none(), "{:?}", preview.error);
        if preview.pending.is_some()
            || preview.model.is_none()
            || preview.request.as_ref().map(|r| &r.selection) != preview.selection.as_ref()
        {
            return;
        }
        self.settled += 1;
        if self.phase == 1 {
            if std::env::var_os("SUNDIAL_PREVIEW_APPEARANCE").is_none() {
                assert!(preview.model.as_ref().unwrap().has_shader_animation());
            }
            preview.playing = false;
            preview.seconds = 2.5;
        }
        if self.settled > 3 {
            ctx.send_viewport_cmd_to(
                viewport,
                egui::ViewportCommand::Screenshot(Default::default()),
            );
            self.requested = true;
        }
    }
}

/// `SUNDIAL_PREVIEW_APPEARANCE="2474:4=7656,5=7657,6=7658"` previews another weapon in both
/// phases; the default is Better Devils, undyed then with one shader applied.
fn appearance(phase: usize) -> Appearance {
    if let Some(spec) = std::env::var_os("SUNDIAL_PREVIEW_APPEARANCE") {
        let spec = spec.to_string_lossy();
        let (arrangement, dyes) = spec.split_once(':').unwrap_or((&spec, ""));
        return Appearance {
            arrangement: arrangement.parse().unwrap(),
            dye_textures: Vec::new(),
            dyes: dyes
                .split(',')
                .filter(|d| !d.is_empty())
                .map(|d| {
                    let (channel, dye) = d.split_once('=').unwrap();
                    (channel.parse().unwrap(), dye.parse().unwrap())
                })
                .collect(),
        };
    }
    Appearance {
        arrangement: 930,
        dye_textures: Vec::new(),
        dyes: if phase == 1 {
            vec![(4, 8054), (5, 8054), (6, 8054)]
        } else {
            vec![]
        },
    }
}
