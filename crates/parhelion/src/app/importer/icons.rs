use super::*;
use parhelion_import::d2_mot::{icon, reader::Reader};

type Loaded = (usize, Result<egui::ColorImage, String>);

#[derive(Default)]
pub(super) struct Icons {
    cache: BTreeMap<usize, Result<egui::TextureHandle, String>>,
    requests: Option<mpsc::SyncSender<Vec<usize>>>,
    results: Option<Receiver<Loaded>>,
    last_requested: Vec<usize>,
}

impl Icons {
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(results) = &self.results {
            while let Ok((index, result)) = results.try_recv() {
                self.cache.insert(
                    index,
                    result.map(|image| {
                        ctx.load_texture(
                            format!("d2-importer-icon-{index}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    }),
                );
            }
        }
    }

    pub fn request(&mut self, ctx: &egui::Context, modern: &Path, visible: &[usize]) {
        let missing: Vec<_> = visible
            .iter()
            .copied()
            .filter(|index| !self.cache.contains_key(index))
            .collect();
        if missing.is_empty() || missing == self.last_requested {
            return;
        }
        // Bound GPU memory while retaining the current viewport. Package IO stays on the worker.
        if self.cache.len() > 256 {
            self.cache.retain(|index, _| visible.contains(index));
        }
        if self.requests.is_none() {
            let Ok(root) = data_root() else { return };
            let modern = modern.to_owned();
            let ctx = ctx.clone();
            let (requests, incoming) = mpsc::sync_channel::<Vec<usize>>(1);
            let (sender, results) = mpsc::channel();
            self.requests = Some(requests);
            self.results = Some(results);
            thread::spawn(move || {
                let cache = service::package_stamp(&modern)
                    .ok()
                    .map(|stamp| root.join("importer/icons").join(stamp));
                if let Some(cache) = &cache {
                    let _ = std::fs::create_dir_all(cache);
                }
                let mut reader = None;
                while let Ok(mut indices) = incoming.recv() {
                    // Prioritize the latest viewport after fast scrolling.
                    for latest in incoming.try_iter() {
                        indices = latest;
                    }
                    for index in indices {
                        let path = cache
                            .as_ref()
                            .map(|cache| cache.join(format!("{index}.png")));
                        if let Some(image) = path.as_ref().and_then(|path| image::open(path).ok()) {
                            let image = image.into_rgba8();
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                image.as_raw(),
                            );
                            if sender.send((index, Ok(image))).is_err() {
                                return;
                            }
                            ctx.request_repaint();
                            continue;
                        }
                        let reader = reader.get_or_insert_with(|| {
                            Reader::discovery(&modern, &root.join("importer/icons"), true)
                                .map_err(|error| format!("{error:#}"))
                        });
                        let result = reader
                            .as_mut()
                            .map_err(|error| error.clone())
                            .and_then(|reader| {
                                icon::read_layers(reader, index)
                                    .map_err(|error| format!("{error:#}"))
                            })
                            .and_then(|layers| composite(&layers))
                            .map(|(size, rgba)| {
                                if let Some(path) = &path {
                                    let _ = image::save_buffer(
                                        path,
                                        &rgba,
                                        size[0] as u32,
                                        size[1] as u32,
                                        image::ColorType::Rgba8,
                                    );
                                }
                                egui::ColorImage::from_rgba_unmultiplied(size, &rgba)
                            });
                        if sender.send((index, result)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            });
        }
        if self
            .requests
            .as_ref()
            .is_some_and(|sender| sender.try_send(missing.clone()).is_ok())
        {
            self.last_requested = missing;
        }
    }

    pub fn draw(&self, ui: &mut egui::Ui, rect: egui::Rect, index: Option<usize>) {
        match index.and_then(|index| self.cache.get(&index)) {
            Some(Ok(texture)) => egui::Image::new(texture).paint_at(ui, rect),
            Some(Err(error)) => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "!",
                    egui::TextStyle::Heading.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
                ui.interact(rect, ui.id().with("icon"), egui::Sense::hover())
                    .on_hover_text(error);
            }
            None if index.is_some() => {
                ui.put(rect, egui::Spinner::new());
            }
            None => {}
        }
    }
}

/// Blends the layers bottom-up at their native size, anchored top-left, on a canvas as large
/// as the largest layer.
pub(super) fn composite(layers: &[icon::Layer]) -> Result<([usize; 2], Vec<u8>), String> {
    let size = [
        layers
            .iter()
            .map(|layer| usize::from(layer.width))
            .max()
            .unwrap_or(0),
        layers
            .iter()
            .map(|layer| usize::from(layer.height))
            .max()
            .unwrap_or(0),
    ];
    if size[0] == 0 || size[1] == 0 {
        return Err("no icon layers".into());
    }
    let mut rgba = vec![0_u8; size[0] * size[1] * 4];
    for layer in layers {
        let (width, height) = (usize::from(layer.width), usize::from(layer.height));
        let Ok(pixels) = decode(layer) else { continue };
        for y in 0..height.min(size[1]) {
            for x in 0..width.min(size[0]) {
                let from = (y * width + x) * 4;
                let to = (y * size[0] + x) * 4;
                sundial::image_processing::blend_rgba_pixel(
                    &mut rgba[to..to + 4],
                    [
                        pixels[from],
                        pixels[from + 1],
                        pixels[from + 2],
                        pixels[from + 3],
                    ],
                );
            }
        }
    }
    Ok((size, rgba))
}

pub(super) fn decode(layer: &icon::Layer) -> Result<Vec<u8>, String> {
    let (width, height) = (usize::from(layer.width), usize::from(layer.height));
    match layer.format {
        28 | 29 => Ok(layer.data.clone()),
        71 | 72 => sundial::image_processing::decode_bc1(&layer.data, width, height),
        74 | 75 | 77 | 78 | 98 | 99 => {
            let columns = width.div_ceil(4);
            let mut rgba = vec![0; width * height * 4];
            for (index, block) in layer.data.chunks_exact(16).enumerate() {
                let mut pixels = [0; 64];
                match layer.format {
                    74 | 75 => bcdec_rs::bc2(block, &mut pixels, 16),
                    77 | 78 => bcdec_rs::bc3(block, &mut pixels, 16),
                    _ => bcdec_rs::bc7(block, &mut pixels, 16),
                }
                for y in 0..4 {
                    for x in 0..4 {
                        let px = (index % columns) * 4 + x;
                        let py = (index / columns) * 4 + y;
                        if px < width && py < height {
                            let to = (py * width + px) * 4;
                            let from = (y * 4 + x) * 4;
                            rgba[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                        }
                    }
                }
            }
            Ok(rgba)
        }
        format => Err(format!("unsupported icon texture format {format}")),
    }
}
