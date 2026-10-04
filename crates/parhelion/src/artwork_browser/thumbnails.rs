//! Thumbnails for the tiles on screen. The browser lists every texture in the packages that
//! fits its purpose, often thousands, so a tile asks for its thumbnail as it comes on screen,
//! the loader decodes the ones still there, and a tile that leaves drops its thumbnail.
use super::*;
use std::{
    sync::{Mutex, mpsc::RecvTimeoutError},
    time::Duration,
};

/// How long the loader keeps the packages open without a request.
const IDLE: Duration = Duration::from_secs(10);

/// Where a row's artwork is read from, which also names its thumbnail.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum Origin {
    Texture(u32),
    /// A file of the local library, and the name a perk icon made from it keeps.
    File {
        path: PathBuf,
        name: String,
    },
}

impl Origin {
    /// Whether `icon` is this artwork: the same texture, or a perk icon made from this file.
    pub(super) fn is(&self, icon: Option<&Icon>) -> bool {
        match (self, icon) {
            (Self::Texture(tag), Some(Icon::Texture { tag: other })) => {
                other.parse_u32().ok() == Some(*tag)
            }
            (Self::File { name, .. }, Some(Icon::Image { name: other, .. })) => name == other,
            _ => false,
        }
    }
}

/// The size a thumbnail is drawn at: 64 pixels on the artwork's long side.
pub(super) fn extent([width, height]: [usize; 2]) -> egui::Vec2 {
    let long = width.max(height).max(1) as f32;
    egui::vec2(64.0 * width as f32 / long, 64.0 * height as f32 / long)
}

/// A tile's thumbnail.
pub(super) enum Thumbnail {
    /// Asked for and not back yet.
    Pending,
    Decoded(egui::ColorImage),
    Shown(egui::TextureHandle),
    /// The artwork could not be read. The tile stays empty rather than asking again.
    Unreadable,
}

impl Thumbnail {
    /// The texture to draw, uploading a decoded thumbnail the first frame it is drawn.
    pub(super) fn texture(
        &mut self,
        ctx: &egui::Context,
        index: usize,
    ) -> Option<&egui::TextureHandle> {
        if matches!(self, Self::Decoded(_))
            && let Self::Decoded(image) = std::mem::replace(self, Self::Unreadable)
        {
            *self = Self::Shown(ctx.load_texture(
                format!("perk-icon-{index}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        match &*self {
            Self::Shown(texture) => Some(texture),
            _ => None,
        }
    }
}

/// What the loader made of one request.
pub(super) enum Loaded {
    Ready(egui::ColorImage),
    Failed,
    /// The tile had left the screen by the time its turn came.
    Skipped,
}

pub(super) struct Loader {
    requests: mpsc::Sender<Origin>,
    /// The tiles on screen, which the loader checks each request against.
    wanted: Arc<Mutex<HashSet<Origin>>>,
    worker: thread::JoinHandle<()>,
}

impl Loader {
    fn spawn(
        packages: Option<PathBuf>,
        purpose: Purpose,
        events: mpsc::Sender<Event>,
        repaint: egui::Context,
    ) -> Self {
        let (requests, received) = mpsc::channel::<Origin>();
        let wanted = Arc::new(Mutex::new(HashSet::new()));
        let on_screen = Arc::clone(&wanted);
        let worker = thread::spawn(move || {
            let mut manager = None;
            loop {
                let origin = match received.recv_timeout(IDLE) {
                    Ok(origin) => origin,
                    // Nothing on screen needs the packages until a tile asks again.
                    Err(RecvTimeoutError::Timeout) => {
                        manager = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                let wanted = match on_screen.lock() {
                    Ok(tiles) => tiles.contains(&origin),
                    Err(_) => true,
                };
                let loaded = if wanted {
                    match load(&origin, purpose, packages.as_deref(), &mut manager) {
                        Ok(image) => Loaded::Ready(image),
                        Err(_) => Loaded::Failed,
                    }
                } else {
                    Loaded::Skipped
                };
                if events.send(Event::Thumbnail(origin, loaded)).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        Self {
            requests,
            wanted,
            worker,
        }
    }

    /// Lets the loader finish the thumbnail it is decoding, then waits for it to stop.
    pub(super) fn stop(self) {
        drop(self.requests);
        let _ = self.worker.join();
    }
}

fn load(
    origin: &Origin,
    purpose: Purpose,
    packages: Option<&Path>,
    manager: &mut Option<sundial::package_authoring::PackageManager>,
) -> Result<egui::ColorImage, String> {
    match origin {
        Origin::Texture(tag) => {
            if manager.is_none() {
                let packages = packages.ok_or("No packages to read")?;
                *manager = Some(sundial::package_authoring::open_shadowkeep_package_manager(
                    packages,
                )?);
            }
            let manager = manager.as_ref().ok_or("No packages to read")?;
            package_icons::load_for(manager, tiger_pkg::TagHash(*tag), purpose)
                .map(|image| package_icons::thumbnail(&image))
        }
        Origin::File { path, .. } => library::thumbnail(path, purpose),
    }
}

impl Picker {
    /// Tells the loader which tiles are on screen, then asks for the thumbnails of the ones
    /// that just came on screen.
    pub(super) fn request_thumbnails(
        &mut self,
        ctx: &egui::Context,
        on_screen: HashSet<Origin>,
        requested: Vec<Origin>,
    ) {
        if self.loader.is_none() {
            if requested.is_empty() {
                return;
            }
            let events = self.events();
            self.loader = Some(Loader::spawn(
                self.packages.clone(),
                self.purpose,
                events,
                ctx.clone(),
            ));
        }
        let Some(loader) = &self.loader else {
            return;
        };
        if let Ok(mut wanted) = loader.wanted.lock() {
            *wanted = on_screen;
        }
        for origin in requested {
            if loader.requests.send(origin).is_err() {
                break;
            }
        }
    }

    /// Takes a thumbnail the loader sent back for a tile that is still waiting for it.
    pub(super) fn receive_thumbnail(&mut self, origin: Origin, loaded: Loaded) {
        let Some(slot) = self.thumbnails.get_mut(&origin) else {
            return;
        };
        if !matches!(slot, Thumbnail::Pending) {
            return;
        }
        match loaded {
            Loaded::Ready(image) => *slot = Thumbnail::Decoded(image),
            Loaded::Failed => *slot = Thumbnail::Unreadable,
            // Its tile asks again next frame if it is still on screen.
            Loaded::Skipped => {
                self.thumbnails.remove(&origin);
            }
        }
    }
}
