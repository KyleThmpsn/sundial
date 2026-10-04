//! Real viewport, resize, shader and close checks using read-only installed assets.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "opens a native viewer and requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
fn native_popout_resizes_tracks_shaders_and_closes_independently() {
    let packages = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
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
                output,
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
    output: PathBuf,
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
                        std::fs::write(self.output.join("model-preview-launcher.png"), png)
                            .unwrap();
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
                    self.output
                        .join(format!("model-popout-after-{}.ppm", self.phase)),
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
        if self.phase == 0 && !crate::model_preview::gpu::available() {
            preview.playing = true;
            if preview.texture.is_none() {
                return;
            }
        }
        if self.phase == 1 {
            if std::env::var_os("SUNDIAL_PREVIEW_APPEARANCE").is_none() {
                assert!(preview.model.as_ref().unwrap().has_shader_animation());
            }
            preview.playing = false;
            preview.seconds = 2.5;
        }
        if self.settled > 3 {
            if self.phase == 0 && !crate::model_preview::gpu::available() {
                let size = preview.texture.as_ref().unwrap().size();
                assert!(size[0] >= 600, "playing preview is undersized: {size:?}");
                std::fs::write(
                    self.output.join("software-resolution.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "playing": preview.playing, "frame_size": size,
                        "artifact": "model-popout-after-0.ppm"
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
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
