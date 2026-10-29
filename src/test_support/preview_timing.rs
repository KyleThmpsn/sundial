//! Times model previews in a real OpenGL window: each preview's wait to its first drawn frame, the
//! stages its load and GPU preparation spent, and how long the window's frames stalled meanwhile.
use crate::model_preview::timing::{self, Event};
use crate::ui::model_preview::{Appearance, LocalAppearance, still};
use eframe::egui;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// What one preview shows, through the same still a workbench page draws.
pub enum Subject {
    /// A native item appearance, as a stock item's page shows it.
    Appearance(Appearance),
    /// A whole object, such as the vehicle a Sparrow summons.
    Object(u32),
    /// An imported model through its private reader.
    Imported(LocalAppearance),
}

pub struct Case {
    pub name: String,
    pub subject: Subject,
}

/// How long a preview may take to draw before the run records it as timed out.
const LIMIT: Duration = Duration::from_secs(240);
/// Frames are still watched this long after the first drawn one, for stalls an upload leaves.
const SETTLE: Duration = Duration::from_secs(2);
/// How long a case waits for its window capture.
const CAPTURE: Duration = Duration::from_secs(5);
/// A frame slower than this is a visible stall.
const STALL_MS: f64 = 50.0;

/// Shows each case in turn, `passes` times, and reports what each took. The first pass reads
/// every model for the first time in this process. Later passes show them again in new previews,
/// as returning to a recipe does. Each case's window, as it stood once settled, goes to
/// `captures` as a PNG.
pub fn run(packages: &Path, cases: Vec<Case>, passes: usize, captures: &Path) -> Value {
    timing::record(true);
    timing::salt_shaders(format!(
        "preview timing {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos())
    ));
    let warm = warm(packages);
    let report = Arc::new(Mutex::new(json!({
        "packages": packages,
        "warm_up": warm,
        "cases": [],
    })));
    let output = Arc::clone(&report);
    let packages = packages.to_owned();
    let captures = captures.to_owned();
    eframe::run_native(
        "Model Preview Timing",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([760.0, 600.0])
                .with_active(false),
            event_loop_builder: Some(Box::new(event_loop)),
            ..Default::default()
        },
        Box::new(move |creation| {
            let gl = creation
                .gl
                .as_ref()
                .ok_or("the timing run requires OpenGL")?;
            crate::model_preview::gpu::set_available(true);
            output.lock().unwrap_or_else(PoisonError::into_inner)["graphics"] = graphics(gl);
            Ok(Box::new(Timing {
                packages,
                captures,
                cases,
                passes,
                index: 0,
                current: None,
                report: output,
            }))
        }),
    )
    .expect("the timing window runs");
    let report = report
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    timing::record(false);
    report
}

/// Opens the stock and installed readers once before any preview, so the cases time a preview
/// opened after the catalog has read the packages, as the workbench does.
fn warm(packages: &Path) -> Value {
    timing::take();
    let stock = timing::measure(
        || "warm stock packages".to_owned(),
        "worker",
        "package open",
        Duration::ZERO,
        || {
            crate::package_runtime::reader::PackageManager::new(
                packages,
                tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
                None,
            )
            .map(drop)
        },
    );
    let installed = timing::measure(
        || "warm installed packages".to_owned(),
        "worker",
        "package open",
        Duration::ZERO,
        || crate::investment::discovery::open_packages(packages).map(drop),
    );
    json!({
        "stock_error": stock.err(),
        "installed_error": installed.err(),
        "events": timing::take(),
    })
}

fn event_loop<T: 'static>(builder: &mut winit::event_loop::EventLoopBuilder<T>) {
    #[cfg(windows)]
    use winit::platform::windows::EventLoopBuilderExtWindows;
    #[cfg(target_os = "linux")]
    use winit::platform::x11::EventLoopBuilderExtX11;
    builder.with_any_thread(true);
}

fn graphics(gl: &eframe::glow::Context) -> Value {
    use eframe::glow::HasContext;
    // SAFETY: eframe provides its live GL context during app creation.
    unsafe {
        let extensions = gl.supported_extensions();
        json!({
            "vendor": gl.get_parameter_string(eframe::glow::VENDOR),
            "renderer": gl.get_parameter_string(eframe::glow::RENDERER),
            "version": gl.get_parameter_string(eframe::glow::VERSION),
            "parallel_shader_compile": extensions.contains("GL_KHR_parallel_shader_compile")
                || extensions.contains("GL_ARB_parallel_shader_compile"),
        })
    }
}

struct Timing {
    packages: PathBuf,
    captures: PathBuf,
    cases: Vec<Case>,
    passes: usize,
    /// The case being shown, counted across passes.
    index: usize,
    current: Option<Watch>,
    report: Arc<Mutex<Value>>,
}

/// What one case's preview has done so far.
struct Watch {
    shown: Instant,
    shown_ms: f64,
    last_frame: Instant,
    gaps: Vec<f64>,
    events: Vec<Event>,
    finished: Option<(Instant, String)>,
    /// When the window capture was asked for, and the file it went to.
    capture: Option<(Instant, Option<String>)>,
}

impl eframe::App for Timing {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        let total = self.cases.len() * self.passes;
        if self.index >= total {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let now = Instant::now();
        let watch = self.current.get_or_insert_with(|| {
            timing::take();
            Watch {
                shown: now,
                shown_ms: timing::now_ms(),
                last_frame: now,
                gaps: Vec::new(),
                events: Vec::new(),
                finished: None,
                capture: None,
            }
        });
        // A capture's readback is not the preview's, so its frames are left out.
        if now > watch.last_frame && watch.capture.is_none() {
            watch.gaps.push(millis(now - watch.last_frame));
        }
        watch.last_frame = now;
        for event in timing::take() {
            if watch.finished.is_none() {
                if event.name == "first frame" {
                    watch.finished = Some((now, "drawn".into()));
                } else if let Some(error) = event.name.strip_prefix("load failed: ") {
                    watch.finished = Some((now, format!("failed: {error}")));
                }
            }
            watch.events.push(event);
        }
        if watch.finished.is_none() && now - watch.shown > LIMIT {
            watch.finished = Some((now, "timed out".into()));
        }
        let case = &self.cases[self.index % self.cases.len()];
        let pass = self.index / self.cases.len();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(format!("{} (pass {})", case.name, pass + 1));
            let id = egui::Id::new(("preview-timing", self.index));
            let size = egui::vec2(640.0, 480.0);
            match &case.subject {
                Subject::Appearance(appearance) => {
                    still::show(ui, id, &self.packages, Some(appearance.clone()), &[], size)
                }
                Subject::Object(tag) => {
                    still::show_object(ui, id, &self.packages, Some(*tag), size)
                }
                Subject::Imported(appearance) => {
                    still::show_local(ui, id, &self.packages, appearance.clone(), size)
                }
            };
        });
        if let Some((at, _)) = &watch.finished
            && now - *at >= SETTLE
        {
            let file = format!("{:02}-{}.png", self.index, file_name(&case.name));
            match &mut watch.capture {
                capture @ None => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                    *capture = Some((now, None));
                    return;
                }
                Some((asked, saved @ None)) => {
                    if let Some(image) = screenshot(&ctx) {
                        let rgba: Vec<u8> = image
                            .pixels
                            .iter()
                            .flat_map(|pixel| pixel.to_array())
                            .collect();
                        let png =
                            crate::model_preview::export::png(&rgba, image.width(), image.height())
                                .expect("the capture encodes");
                        std::fs::create_dir_all(&self.captures).expect("the capture folder");
                        std::fs::write(self.captures.join(&file), png).expect("the capture saves");
                        *saved = Some(file);
                    } else if now - *asked < CAPTURE {
                        return;
                    }
                }
                Some((_, Some(_))) => {}
            }
            let watch = self.current.take().expect("a watched case");
            let summary = summarize(&case.name, pass, watch);
            self.report.lock().unwrap_or_else(PoisonError::into_inner)["cases"]
                .as_array_mut()
                .expect("a case list")
                .push(summary);
            self.index += 1;
        }
    }
}

/// The window image a screenshot request delivered this frame.
fn screenshot(ctx: &egui::Context) -> Option<Arc<egui::ColorImage>> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
            _ => None,
        })
    })
}

fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The case's waits, its load's stages, the paint thread's stages and the window's frame gaps.
fn summarize(name: &str, pass: usize, watch: Watch) -> Value {
    let (finished, outcome) = watch.finished.expect("a finished case");
    let first = |name: &str| {
        watch
            .events
            .iter()
            .find(|event| event.name == name)
            .map(|event| event.start_ms - watch.shown_ms)
    };
    let (first_frame, first_software_frame) = (first("first frame"), first("first software frame"));
    let load = watch
        .events
        .iter()
        .find(|event| event.thread == "worker" && event.name.starts_with("load "));
    let mut paint: Vec<(&'static str, f64, u32)> = Vec::new();
    let mut paint_steps = Vec::new();
    for event in watch
        .events
        .iter()
        .filter(|event| matches!(event.thread, "paint" | "ui") && event.total_ms > 0.0)
    {
        paint_steps.push(event.total_ms);
        for &(stage, ms, count) in &event.stages {
            match paint.iter_mut().find(|(name, ..)| *name == stage) {
                Some(total) => {
                    total.1 += ms;
                    total.2 += count;
                }
                None => paint.push((stage, ms, count)),
            }
        }
    }
    // Gaps up to the first frame and through the settling time after it.
    let mut gaps = watch.gaps.clone();
    gaps.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        gaps.get(((gaps.len() as f64 - 1.0) * fraction).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let stalls: Vec<f64> = watch
        .gaps
        .iter()
        .copied()
        .filter(|gap| *gap > STALL_MS)
        .collect();
    json!({
        "name": name,
        "pass": pass + 1,
        "outcome": outcome,
        "first_frame_ms": first_frame,
        "first_software_frame_ms": first_software_frame,
        "watched_ms": millis(finished - watch.shown),
        "capture": watch.capture.and_then(|(_, file)| file),
        "load": load.map(|load| json!({
            "ready_ms": load.start_ms + load.total_ms - watch.shown_ms,
            "total_ms": load.total_ms,
            "stages": load.stages,
        })),
        "paint_thread": {
            "steps": paint_steps.len(),
            "longest_step_ms": paint_steps.iter().copied().fold(0.0, f64::max),
            "stages": paint,
        },
        "frames": {
            "count": watch.gaps.len(),
            "median_ms": percentile(0.5),
            "p95_ms": percentile(0.95),
            "longest_ms": gaps.last().copied().unwrap_or(0.0),
            "stalls_over_50_ms": stalls.len(),
            "stalled_ms": stalls.iter().sum::<f64>(),
        },
        "events": watch.events,
    })
}
