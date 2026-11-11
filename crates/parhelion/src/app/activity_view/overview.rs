//! A converted map from above: the plan its newest finished conversion records, shaded by height
//! with water in blue, and its spawn points. The conversion is read and shaded on a worker.
use super::*;
use parhelion_import::halo_reach::level::native::Plan;

/// The direction toward the light, from the northwest and 45 degrees up, z up.
const LIGHT: [f32; 3] = [-0.5, 0.5, std::f32::consts::FRAC_1_SQRT_2];
/// A cell more than this many meters above a neighbor draws as a rim.
const RIM: f32 = 1.5;
const WATER: [f32; 3] = [0.22, 0.45, 0.76];
const SHORE: [f32; 3] = [0.55, 0.74, 0.92];
/// The tallest the overview draws, in points.
const MAX_HEIGHT: f32 = 420.0;
/// How far the view reaches past the spawn points, as a share of their wider extent, and at least
/// in meters.
const VIEW_MARGIN: (f32, f32) = (0.15, 25.0);

/// One spawn point in the map's coordinates, facing `yaw` radians about z.
#[derive(serde::Deserialize)]
pub(super) struct Spawn {
    pub position: [f32; 3],
    pub yaw: f32,
}

#[derive(serde::Deserialize)]
struct Area {
    spawns: Vec<Spawn>,
}

/// The parts of `conversion.json` the overview draws.
#[derive(serde::Deserialize)]
struct Recorded {
    spawns: Vec<Spawn>,
    #[serde(default)]
    areas: Vec<Area>,
    #[serde(default)]
    plan: Option<Plan>,
}

pub(super) struct Overview {
    pub folder: PathBuf,
    /// Every area's spawn points.
    pub spawns: Vec<Spawn>,
    pub plan: Option<Plan>,
    /// The part of the plan drawn, its lower left and upper right corners in map coordinates: the
    /// spawn points and a margin, within the plan.
    view: [[f32; 2]; 2],
    /// The shaded plan, until it is uploaded.
    image: Option<egui::ColorImage>,
}

/// The spawn points' bounds with [`VIEW_MARGIN`] around them, within `plan`, or the whole plan
/// without spawn points.
fn view(plan: &Plan, spawns: &[Spawn]) -> [[f32; 2]; 2] {
    let whole = [plan.minimum, plan.maximum];
    if spawns.is_empty() {
        return whole;
    }
    let mut bounds = [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]];
    for spawn in spawns {
        let [minimum, maximum] = &mut bounds;
        for ((minimum, maximum), position) in minimum.iter_mut().zip(maximum).zip(spawn.position) {
            *minimum = minimum.min(position);
            *maximum = maximum.max(position);
        }
    }
    let extent = (bounds[1][0] - bounds[0][0]).max(bounds[1][1] - bounds[0][1]);
    let margin = (extent * VIEW_MARGIN.0).max(VIEW_MARGIN.1);
    let view: [[f32; 2]; 2] = [
        std::array::from_fn(|k| (bounds[0][k] - margin).max(plan.minimum[k])),
        std::array::from_fn(|k| (bounds[1][k] + margin).min(plan.maximum[k])),
    ];
    if (0..2).all(|k| view[0][k] < view[1][k]) {
        view
    } else {
        whole
    }
}

/// The newest finished conversion of map `name`, or `None` when it has none.
pub(super) fn read(name: &str) -> Result<Option<Overview>, String> {
    let data = sundial::package_authoring::parhelion_data_directory()
        .ok_or("Could not locate Parhelion's data folder")?;
    let Some(folder) =
        crate::map::newest_conversion(&data, name).map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let bytes = std::fs::read(folder.join("conversion.json")).map_err(|error| error.to_string())?;
    let recorded: Recorded =
        serde_json::from_slice(&bytes).map_err(|error| format!("conversion.json: {error}"))?;
    let mut spawns = recorded.spawns;
    // The first area's spawn points are the map's own.
    spawns.extend(
        recorded
            .areas
            .into_iter()
            .skip(1)
            .flat_map(|area| area.spawns),
    );
    let image = recorded
        .plan
        .as_ref()
        .map(|plan| {
            let file = Path::new(&plan.image)
                .file_name()
                .ok_or("The plan names no image")?;
            let image = image::open(folder.join(file))
                .map_err(|error| format!("{}: {error}", plan.image))?
                .to_rgba8();
            if [image.width(), image.height()] != [plan.width, plan.height] {
                return Err(format!(
                    "{} is not {}x{}",
                    plan.image, plan.width, plan.height
                ));
            }
            Ok(shade(plan, &image))
        })
        .transpose()?;
    let view = recorded
        .plan
        .as_ref()
        .map_or([[0.0; 2]; 2], |plan| view(plan, &spawns));
    Ok(Some(Overview {
        folder,
        spawns,
        plan: recorded.plan,
        view,
        image,
    }))
}

/// The plan shaded by height and slope, with rims where it steps down, and water in blue with
/// lighter shores. Empty cells stay clear.
fn shade(plan: &Plan, image: &image::RgbaImage) -> egui::ColorImage {
    let [width, height] = [image.width() as usize, image.height() as usize];
    let cell = (plan.maximum[0] - plan.minimum[0]) / width as f32;
    let range = plan.ceiling - plan.floor;
    let sample = |x: isize, y: isize| -> Option<(f32, bool)> {
        if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
            return None;
        }
        let pixel = image.get_pixel(x as u32, y as u32);
        (pixel[3] == 255).then(|| {
            let level = f32::from(u16::from_be_bytes([pixel[0], pixel[1]])) / 65_535.0;
            (plan.floor + level * range, pixel[2] == 255)
        })
    };
    let mut pixels = vec![0; width * height * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let (x, y) = ((index % width) as isize, (index / width) as isize);
        let Some((z, water)) = sample(x, y) else {
            continue;
        };
        // Left, right, north (the row above) and south.
        let neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dx, dy)| sample(x + dx, y + dy));
        let [left, right, north, south] = neighbors.map(|n| n.map_or(z, |(z, _)| z));
        let normal = [
            -(right - left) / (2.0 * cell),
            -(north - south) / (2.0 * cell),
            1.0,
        ];
        let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
        let light = (normal.iter().zip(LIGHT).map(|(n, l)| n * l).sum::<f32>() / length).max(0.0);
        let color = if water {
            if neighbors.iter().flatten().any(|&(_, wet)| !wet) {
                SHORE
            } else {
                WATER.map(|channel| channel * (0.85 + 0.2 * light))
            }
        } else {
            let rim = neighbors.iter().flatten().any(|&(n, _)| z - n > RIM);
            let t = ((z - plan.floor) / range).clamp(0.0, 1.0);
            let value = (0.3 + 0.45 * t) * (0.45 + 0.8 * light) * if rim { 0.55 } else { 1.0 };
            [value; 3]
        };
        let [r, g, b] = color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        pixel.copy_from_slice(&[r, g, b, 255]);
    }
    egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels)
}

impl Editor {
    pub(super) fn draw_overview(&mut self, ui: &mut egui::Ui) {
        ui.strong("Overview");
        // The conversion follows the saved name.
        if self.overview_name != self.baseline.name {
            self.overview_name = self.baseline.name.clone();
            self.overview = None;
            self.overview_job = None;
            self.overview_texture = None;
        }
        if self.overview.is_none() && self.overview_job.is_none() {
            let (sender, receiver) = mpsc::channel();
            let name = self.overview_name.clone();
            let context = ui.ctx().clone();
            let spawned = thread::Builder::new()
                .name("activity-overview".into())
                .spawn(move || {
                    let _ = sender.send(read(&name));
                    context.request_repaint();
                });
            match spawned {
                Ok(_) => self.overview_job = Some(receiver),
                Err(error) => self.overview = Some(Err(error.to_string())),
            }
        }
        if let Some(receiver) = &self.overview_job {
            match receiver.try_recv() {
                Ok(overview) => {
                    self.overview = Some(overview);
                    self.overview_job = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.overview = Some(Err("The conversion reader stopped".into()));
                    self.overview_job = None;
                }
                Err(TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let overview = match &mut self.overview {
            None => {
                ui.weak("Reading the conversion…");
                return;
            }
            Some(Err(error)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Could not read the conversion.",
                )
                .on_hover_text(error.as_str());
                return;
            }
            Some(Ok(None)) => {
                ui.weak("No finished conversion. Stage the map to draw it.");
                return;
            }
            Some(Ok(Some(overview))) => overview,
        };
        let Some(plan) = &overview.plan else {
            ui.weak("This conversion has no overview. Stage the map again.");
            return;
        };
        if let Some(image) = overview.image.take() {
            self.overview_texture = Some(ui.ctx().load_texture(
                "activity-overview",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        // It fits the fields' visible height with its heading and caption, so it shows whole.
        let tallest =
            (ui.clip_rect().height() - 4.0 * ui.spacing().interact_size.y).clamp(120.0, MAX_HEIGHT);
        let [low, high] = overview.view;
        let span = [high[0] - low[0], high[1] - low[1]];
        let aspect = span[1] / span[0];
        let mut size = egui::vec2(ui.available_width(), ui.available_width() * aspect);
        if size.y > tallest {
            size = egui::vec2(tallest / aspect, tallest);
        }
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Map Overview")
        });
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        if let Some(texture) = &self.overview_texture {
            // The view's corners in the texture, whose rows run from the plan's largest y down.
            let whole = [
                plan.maximum[0] - plan.minimum[0],
                plan.maximum[1] - plan.minimum[1],
            ];
            let uv = egui::Rect::from_min_max(
                egui::pos2(
                    (low[0] - plan.minimum[0]) / whole[0],
                    (plan.maximum[1] - high[1]) / whole[1],
                ),
                egui::pos2(
                    (high[0] - plan.minimum[0]) / whole[0],
                    (plan.maximum[1] - low[1]) / whole[1],
                ),
            );
            painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
        }
        for spawn in &overview.spawns {
            let center = egui::pos2(
                rect.left() + (spawn.position[0] - low[0]) / span[0] * rect.width(),
                rect.top() + (high[1] - spawn.position[1]) / span[1] * rect.height(),
            );
            let facing = egui::vec2(spawn.yaw.cos(), -spawn.yaw.sin());
            painter.line_segment(
                [center, center + facing * 5.0],
                egui::Stroke::new(1.0, egui::Color32::WHITE),
            );
            painter.circle(
                center,
                2.0,
                egui::Color32::WHITE,
                egui::Stroke::new(0.5, egui::Color32::BLACK),
            );
        }
        ui.weak(match overview.spawns.len() {
            1 => "1 spawn point".to_owned(),
            count => format!("{count} spawn points"),
        })
        .on_hover_text(overview.folder.display().to_string());
    }
}
