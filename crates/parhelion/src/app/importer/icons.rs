use super::*;
use parhelion_import::d2_mot::icon;
mod reader;
use reader::Reader;

// Older thumbnails contain only the primary artwork. The package stamp cannot
// invalidate those when our layer composition changes.
const CACHE_VERSION: &str = "v3";

#[cfg(test)]
mod tests;

enum Loaded {
    Icon(usize, Result<egui::ColorImage, String>),
    /// Indices of a batch a newer request replaced before it was read.
    Dropped(Vec<usize>),
}

#[derive(Default)]
pub(super) struct Icons {
    cache: BTreeMap<usize, Result<egui::TextureHandle, String>>,
    requests: Option<mpsc::SyncSender<Vec<usize>>>,
    results: Option<Receiver<Loaded>>,
    /// Indices sent to the worker and not yet answered.
    pending: BTreeSet<usize>,
}

impl Icons {
    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut disconnected = false;
        if let Some(results) = &self.results {
            loop {
                let loaded = match results.try_recv() {
                    Ok(loaded) => loaded,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                };
                match loaded {
                    Loaded::Icon(index, result) => {
                        self.pending.remove(&index);
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
                    Loaded::Dropped(indices) => {
                        for index in indices {
                            self.pending.remove(&index);
                        }
                    }
                }
            }
        }
        if disconnected {
            self.requests = None;
            self.results = None;
            for index in std::mem::take(&mut self.pending) {
                self.cache.insert(
                    index,
                    Err(
                        "The icon loader stopped unexpectedly. Refresh the importer to retry."
                            .into(),
                    ),
                );
            }
        }
    }

    pub fn request(
        &mut self,
        ctx: &egui::Context,
        source: Source,
        settings: &Settings,
        visible: &[usize],
    ) {
        let folder = match source {
            Source::Modern => settings.modern_packages.as_deref(),
            Source::Roi => settings.d1_roi.as_deref(),
            _ => None,
        };
        let Some(folder) = folder else { return };
        let missing: Vec<_> = visible
            .iter()
            .copied()
            .filter(|index| !self.cache.contains_key(index) && !self.pending.contains(index))
            .collect();
        if missing.is_empty() {
            return;
        }
        // Bound GPU memory while retaining the current viewport. Package IO stays on the worker.
        if self.cache.len() > 256 {
            self.cache.retain(|index, _| visible.contains(index));
        }
        if self.requests.is_none() {
            let Ok(root) = data_root() else { return };
            let folder = folder.to_owned();
            let xg = settings.d1_xg_library.clone();
            let ctx = ctx.clone();
            let (requests, incoming) = mpsc::sync_channel::<Vec<usize>>(1);
            let (sender, results) = mpsc::channel();
            self.requests = Some(requests);
            self.results = Some(results);
            thread::spawn(move || {
                let generation = service::package_stamp(&folder).ok().and_then(|stamp| {
                    parhelion_import::cache::Generation::directory(
                        &root.join("importer/icons").join(CACHE_VERSION).join(
                            if source == Source::Roi {
                                "d1-roi"
                            } else {
                                "d2-mot"
                            },
                        ),
                        &stamp,
                    )
                });
                let cache = generation
                    .as_ref()
                    .map(parhelion_import::cache::Generation::path);
                let mut reader = None;
                loop {
                    let mut indices = match incoming.recv_timeout(Duration::from_secs(10)) {
                        Ok(indices) => indices,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            reader = None;
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    // Prioritize the latest viewport after fast scrolling.
                    for latest in incoming.try_iter() {
                        let mut dropped = std::mem::replace(&mut indices, latest);
                        dropped.retain(|index| !indices.contains(index));
                        if sender.send(Loaded::Dropped(dropped)).is_err() {
                            return;
                        }
                    }
                    for index in indices {
                        let path = cache
                            .as_ref()
                            .map(|cache| cache.join(format!("{index}.png")));
                        if let Some(image) = path.as_ref().and_then(|path| cached_image(path).ok())
                        {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                image.as_raw(),
                            );
                            if sender.send(Loaded::Icon(index, Ok(image))).is_err() {
                                return;
                            }
                            ctx.request_repaint();
                            continue;
                        }
                        let reader = reader.get_or_insert_with(|| {
                            Reader::open(source, &folder, &root.join("importer/icons"), xg.clone())
                        });
                        let result = reader
                            .as_mut()
                            .map_err(|error| error.clone())
                            .and_then(|reader| reader.read(index))
                            .map(|image| {
                                let image = thumbnail(&image);
                                if let Some(path) = &path
                                    && let Some(parent) = path.parent()
                                    && let Ok(temporary) = tempfile::NamedTempFile::new_in(parent)
                                    && image
                                        .save_with_format(temporary.path(), image::ImageFormat::Png)
                                        .is_ok()
                                {
                                    let _ = temporary.persist(path);
                                }
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [image.width() as usize, image.height() as usize],
                                    image.as_raw(),
                                )
                            });
                        if sender.send(Loaded::Icon(index, result)).is_err() {
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
            self.pending.extend(missing);
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

fn cached_image(path: &Path) -> Result<image::RgbaImage, String> {
    let bytes = crate::image_import::read_path(path)?;
    crate::image_import::decode_png(&bytes).map(|image| thumbnail(&image))
}

fn thumbnail(image: &image::RgbaImage) -> image::RgbaImage {
    // Bound GPU bytes as well as the number of cached viewport thumbnails.
    let scale = 128.0 / f64::from(image.width().max(image.height()).max(128));
    let width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
    let height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
    crate::image_import::resize(image, width, height)
}

/// Use the source-independent browser composition, while native exports remain primary-only.
pub(super) fn composite(layers: &[icon::Layer]) -> Result<([usize; 2], Vec<u8>), String> {
    let image = icon::composite(layers).map_err(|error| format!("{error:#}"))?;
    Ok((
        [usize::from(image.width), usize::from(image.height)],
        image.rgba,
    ))
}
