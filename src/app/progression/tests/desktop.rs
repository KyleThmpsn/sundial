//! A disposable native window for checking the same tables through the shipped GL renderer.
use super::*;

#[test]
#[ignore = "Opens a temporary native window and requires SUNDIAL_PROGRESSION_INSTALL"]
fn installed_progression_desktop_frames() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let install = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_PROGRESSION_INSTALL").expect("install path"),
    );
    let catalog = Catalog::load_or_scan_with_progress(
        &install,
        "examples/progression-ui-check/installed-catalog.json".into(),
        false,
        |_| {},
    )
    .unwrap();
    let counters = catalog
        .unlock_value_definitions()
        .iter()
        .filter(|d| d.bank() == 1)
        .filter_map(|d| d.compact_slot)
        .take(6000)
        .map(|slot| json!([slot, 3]))
        .collect::<Vec<_>>();
    let document = json!({"state":{"unlocks":{"objective_values":counters,"account_flag_runs":[[0,5000]],"account_progressions":[[0,500,0,0]]}}});
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_active(false),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed = finished.clone();
    eframe::run_native(
        "Sundial Progression Check",
        options,
        Box::new(move |creation| {
            crate::app::preferences::configure_destiny_symbol_fonts(&creation.egui_ctx, &install)
                .unwrap();
            Ok(Box::new(Check {
                catalog,
                document,
                state: UiState {
                    read_only: true,
                    ..Default::default()
                },
                collections: crate::app::collections_page::UiState::default(),
                frame: 0,
                times: Vec::new(),
                screenshots: 0,
                finished: completed,
            }))
        }),
    )
    .unwrap();
    assert!(
        finished.load(std::sync::atomic::Ordering::Relaxed),
        "The native check ended before completing all views"
    );
}

struct Check {
    catalog: Catalog,
    document: Value,
    state: UiState,
    collections: crate::app::collections_page::UiState,
    frame: usize,
    times: Vec<f32>,
    screenshots: usize,
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl eframe::App for Check {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        for event in ctx.input(|input| input.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let path = format!(
                    "examples/progression-round2-ui/desktop-{}.ppm",
                    self.screenshots
                );
                std::fs::create_dir_all("examples/progression-round2-ui").unwrap();
                let pixels = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| [pixel.r(), pixel.g(), pixel.b()])
                    .collect::<Vec<_>>();
                let mut data =
                    format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
                data.extend(pixels);
                std::fs::write(path, data).unwrap();
                self.screenshots += 1;
            }
        }
        let phase = self.frame / 120;
        if self.frame % 120 == 0 {
            self.state.reset_navigation();
            self.state.unlock_browser.tab = if phase == 1 {
                unlocks::Tab::Storage
            } else {
                unlocks::Tab::Entries
            };
            ctx.set_visuals(if phase < 2 {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            if phase == 1 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(640.0, 760.0)));
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if phase == 3 {
                self.collections.read_only = true;
                assert!(!crate::app::collections_page::draw_content(
                    ui,
                    &mut self.document,
                    &self.catalog,
                    &mut self.collections
                ));
            } else {
                assert!(!draw_content(
                    ui,
                    &mut self.document,
                    &self.catalog,
                    None,
                    &mut self.state,
                    if phase == 2 {
                        View::Triumphs
                    } else {
                        View::Unlocks
                    }
                ));
            }
        });
        if let Some(time) = frame.info().cpu_usage {
            self.times.push(time);
        }
        if self.frame % 120 == 100 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        self.frame += 1;
        if self.frame >= 480 {
            self.times.sort_by(f32::total_cmp);
            eprintln!(
                "Native GL: {} frames, {:.3} ms median CPU, {:.3} ms p95 CPU, {:.3} ms maximum CPU",
                self.times.len(),
                self.times[self.times.len() / 2] * 1000.0,
                self.times[self.times.len() * 95 / 100] * 1000.0,
                self.times.last().unwrap() * 1000.0
            );
            assert_eq!(self.screenshots, 4);
            self.finished
                .store(true, std::sync::atomic::Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }
}
