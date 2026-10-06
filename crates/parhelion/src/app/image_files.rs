//! A page's pictures in and out of files: importing a picture for one part of an item and
//! exporting a part as a PNG, each with its file dialog on another thread, and drawing a picture
//! fitted to a frame. The emblem's nameplate and a subclass's screen art share these.
use super::*;
use crate::image_import::EmbeddedImage;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// A file dialog and the work after it, running on another thread.
enum Task {
    Import(Receiver<Result<Option<EmbeddedImage>, String>>),
    /// Whether a file was saved.
    Export(Receiver<Result<bool, String>>),
}

/// A running import or export, with the part it was started for, and how the last one went.
pub(super) struct ImageFiles<Part> {
    task: Option<(Part, Task)>,
    pub(super) outcome: Option<Result<&'static str, String>>,
}

impl<Part> Default for ImageFiles<Part> {
    fn default() -> Self {
        Self {
            task: None,
            outcome: None,
        }
    }
}

impl<Part: Copy> ImageFiles<Part> {
    pub(super) fn busy(&self) -> bool {
        self.task.is_some()
    }

    /// Picks a picture for `part` with a dialog titled `title`, and decodes it off the frame on
    /// a thread named `thread`.
    pub(super) fn import(
        &mut self,
        part: Part,
        title: String,
        thread: &str,
        context: egui::Context,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.spawn(part, Task::Import(receiver), thread, move || {
            let picked = rfd::FileDialog::new()
                .set_title(title)
                .add_filter("PNG or JPEG Image", &["png", "jpg", "jpeg"])
                .pick_file()
                .map(|path| EmbeddedImage::from_path(&path))
                .transpose();
            let _ = sender.send(picked);
            context.request_repaint();
        });
    }

    /// Asks where to save `part`, offering `file_name`, and writes the pixels `pixels` makes
    /// there as a PNG once a place is chosen.
    pub(super) fn export(
        &mut self,
        part: Part,
        (title, file_name): (String, String),
        thread: &str,
        pixels: impl FnOnce() -> Result<image::RgbaImage, String> + Send + 'static,
        context: egui::Context,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.spawn(part, Task::Export(receiver), thread, move || {
            let saved = rfd::FileDialog::new()
                .set_title(title)
                .add_filter("PNG Image", &["png"])
                .set_file_name(file_name)
                .save_file()
                .map_or(Ok(false), |path| write_png(&path, pixels()?).map(|()| true));
            let _ = sender.send(saved);
            context.request_repaint();
        });
    }

    fn spawn(
        &mut self,
        part: Part,
        task: Task,
        thread: &str,
        work: impl FnOnce() + Send + 'static,
    ) {
        self.task = Some((part, task));
        self.outcome = None;
        if let Err(error) = std::thread::Builder::new()
            .name(thread.to_owned())
            .spawn(work)
        {
            self.task = None;
            self.outcome = Some(Err(format!("Could not open the file dialog: {error}")));
        }
    }

    /// A finished import's picture with the part it was started for, for the page to take into
    /// its recipe. How a finished export went is kept in `outcome`.
    pub(super) fn poll(&mut self) -> Option<(Part, EmbeddedImage)> {
        let (part, task) = self.task.as_ref()?;
        let part = *part;
        let (imported, outcome) = match task {
            Task::Import(receiver) => match receiver.try_recv() {
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    (None, Some(Err("The import stopped.".to_owned())))
                }
                Ok(Ok(Some(image))) => (Some((part, image)), None),
                Ok(Ok(None)) => (None, None),
                Ok(Err(error)) => (None, Some(Err(error))),
            },
            Task::Export(receiver) => match receiver.try_recv() {
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    (None, Some(Err("The export stopped.".to_owned())))
                }
                Ok(Ok(true)) => (None, Some(Ok("Image saved"))),
                Ok(Ok(false)) => (None, None),
                Ok(Err(error)) => (None, Some(Err(error))),
            },
        };
        self.task = None;
        self.outcome = outcome;
        imported
    }
}

/// Writes `pixels` to `path` as a PNG, replacing what is there only once it is whole.
pub(super) fn write_png(path: &Path, pixels: image::RgbaImage) -> Result<(), String> {
    let mut png = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("Could not encode the image: {error}"))?;
    sundial::package_authoring::replace_authoring_file(path, png.get_ref())
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}

/// A picture as a texture covering `width` by `height`, kept while the picture and size stay the
/// same. `namespace` keeps each page's textures apart.
pub(super) fn covered_texture(
    ctx: &egui::Context,
    namespace: &str,
    image: &EmbeddedImage,
    (width, height): (u32, u32),
) -> egui::TextureHandle {
    let id = egui::Id::new((namespace, image.fingerprint(), width, height));
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let covered = crate::image_import::cover(image.pixels(), width, height);
    let texture = ctx.load_texture(
        format!("{namespace}-{:016x}-{width}x{height}", image.fingerprint()),
        egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            covered.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

/// `texture` fitted inside `frame` at its own aspect, over a checkerboard that shows where it is
/// clear. While it loads, or where there is none, the frame is an outline.
pub(super) fn draw_contained(
    ui: &egui::Ui,
    frame: egui::Rect,
    texture: Option<&egui::TextureHandle>,
) {
    let Some(texture) = texture else {
        ui.painter().rect_stroke(
            frame,
            3.0,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        return;
    };
    let size = texture.size_vec2();
    let scale = (frame.width() / size.x.max(1.0)).min(frame.height() / size.y.max(1.0));
    let rect = egui::Rect::from_center_size(frame.center(), size * scale);
    style::transparency_backdrop(ui, rect);
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}
