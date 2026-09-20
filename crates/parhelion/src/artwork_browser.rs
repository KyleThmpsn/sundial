//! A virtualized icon grid with optional local artwork, loaded away from the UI thread.
use crate::app::pickers;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
};
use sundial::investment::InvestmentCatalog;
mod library;
pub(crate) mod perk_quality;
pub(crate) mod preview;
mod purpose;
mod selection;
pub(crate) use purpose::Purpose;
#[cfg(test)]
mod tests;
mod view;
use crate::{icon_edit::package_icons, perk::Icon};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};
pub(crate) use view::Browser;

struct Row {
    icon: Icon,
    local: Option<PathBuf>,
    white: bool,
    label: String,
    search: String,
    source: u8,
    image: egui::ColorImage,
}

enum Event {
    Native(package_icons::Entry),
    Progress(usize, usize),
    Local(Result<(Vec<library::Entry>, usize), String>),
    Downloaded,
    RevealLocal(u8),
    ShowColors,
    Error(String),
    Preview(Icon, Result<egui::ColorImage, String>),
}

#[derive(Default)]
pub(crate) struct Picker {
    purpose: Purpose,
    rows: Vec<Row>,
    textures: HashMap<usize, egui::TextureHandle>,
    source: u8,
    all_colors: bool,
    packages: Option<PathBuf>,
    attempted: bool,
    local_loading: bool,
    downloaded: bool,
    error: Option<String>,
    skipped: usize,
    progress: Option<(usize, usize)>,
    receiver: Option<Receiver<Event>>,
    sender: Option<mpsc::Sender<Event>>,
    workers: Vec<thread::JoinHandle<()>>,
    local_workers: Vec<thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    preview: Option<(Icon, Result<egui::ColorImage, String>)>,
    preview_texture: Option<egui::TextureHandle>,
    preview_pending: Option<Icon>,
    clear_query: bool,
}

pub(crate) enum Selection {
    Icon(Icon),
    Perk(u32),
    Local(PathBuf),
}

impl Picker {
    #[allow(clippy::field_reassign_with_default)] // Picker owns workers and implements Drop.
    pub fn for_purpose(purpose: Purpose) -> Self {
        let mut picker = Self::default();
        picker.purpose = purpose;
        picker
    }
    pub fn busy(&self) -> bool {
        !self.workers.is_empty() || !self.local_workers.is_empty()
    }

    pub fn invalidate(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.receiver = None;
        self.sender = None;
        self.rows.clear();
        self.textures.clear();
        self.attempted = false;
        self.local_loading = false;
        self.progress = None;
        self.preview = None;
        self.preview_texture = None;
        self.preview_pending = None;
    }

    pub fn poll(&mut self) {
        while let Some(event) = self.receiver.as_ref().and_then(|r| r.try_recv().ok()) {
            match event {
                Event::Native(entry) => {
                    let label = format!(
                        "{}\n{}\n{} × {}\n{}",
                        entry.name, entry.package, entry.size[0], entry.size[1], entry.tag
                    );
                    let search = format!("{label} {:08x}", entry.tag.0).to_lowercase();
                    self.rows.push(Row {
                        icon: Icon::Texture {
                            tag: entry.tag.0.into(),
                        },
                        local: None,
                        white: entry.white,
                        label,
                        search,
                        source: 1,
                        image: entry.thumbnail,
                    });
                }
                Event::Progress(done, total) => {
                    self.progress = (done < total).then_some((done, total))
                }
                Event::Local(result) => {
                    self.local_loading = false;
                    match result {
                        Ok((entries, skipped)) => {
                            self.rows.retain(|row| row.source == 1);
                            self.textures.clear();
                            self.skipped = skipped;
                            self.rows.extend(entries.into_iter().map(|entry| {
                                Row {
                                    source: if entry
                                        .path
                                        .components()
                                        .any(|part| part.as_os_str() == "destiny-icons")
                                    {
                                        3
                                    } else {
                                        2
                                    },
                                    local: Some(entry.path.clone()),
                                    white: entry.white,
                                    search: entry.name.to_lowercase(),
                                    label: format!(
                                        "{}\n{} × {}",
                                        entry.name, entry.size[0], entry.size[1]
                                    ),
                                    icon: entry.icon,
                                    image: entry.thumbnail,
                                }
                            }));
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Event::Downloaded => {
                    self.downloaded = true;
                    self.source = 3;
                    self.clear_query = true;
                }
                Event::ShowColors => self.all_colors = true,
                Event::RevealLocal(source) => {
                    self.source = source;
                    self.clear_query = true;
                }
                Event::Error(error) => self.error = Some(error),
                Event::Preview(icon, result) => {
                    self.preview = Some((icon, result));
                    self.preview_texture = None;
                    self.preview_pending = None;
                }
            }
        }
        for workers in [&mut self.workers, &mut self.local_workers] {
            let mut index = 0;
            while index < workers.len() {
                if workers[index].is_finished() {
                    if workers.swap_remove(index).join().is_err() {
                        self.error = Some(
                            "The icon reader stopped unexpectedly. Reopen the browser to retry."
                                .into(),
                        );
                    }
                } else {
                    index += 1;
                }
            }
        }
    }

    fn start(&mut self, packages: Option<&Path>, ctx: &egui::Context) {
        if self.packages.as_deref() != packages {
            self.invalidate();
            self.packages = packages.map(Path::to_owned);
        }
        if self.attempted || self.busy() {
            return;
        }
        self.attempted = true;
        self.local_loading = true;
        self.error = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        let purpose = self.purpose;
        let cancel = self.cancel.clone();
        let sender = self.events();
        let packages = self.packages.clone();
        let repaint = ctx.clone();
        self.downloaded = library::directory().is_ok_and(|root| library::downloaded(&root));
        self.workers.push(thread::spawn(move || {
            let local = library::directory().and_then(|root| library::scan(&root, purpose));
            let _ = sender.send(Event::Local(local));
            repaint.request_repaint();
            if let Some(packages) = packages.filter(|path| path.is_dir()) {
                match sundial::package_authoring::open_shadowkeep_package_manager(&packages) {
                    Ok(manager) => {
                        package_icons::scan_for(&manager, purpose, |done, total, entry| {
                            if cancel.load(Ordering::Relaxed) {
                                return false;
                            }
                            let matched = entry.is_some();
                            if let Some(entry) = entry {
                                let _ = sender.send(Event::Native(entry));
                            }
                            if done % 128 == 0 || done == total || matched {
                                if sender.send(Event::Progress(done, total)).is_err() {
                                    return false;
                                }
                                repaint.request_repaint();
                            }
                            true
                        })
                    }
                    Err(error) => {
                        let _ = sender.send(Event::Error(error));
                    }
                }
            }
            repaint.request_repaint();
        }));
    }

    fn events(&mut self) -> mpsc::Sender<Event> {
        if let Some(sender) = &self.sender {
            return sender.clone();
        }
        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender.clone());
        self.receiver = Some(receiver);
        sender
    }

    pub fn preview(
        &mut self,
        ui: &mut egui::Ui,
        packages: Option<&Path>,
        icon: Option<&Icon>,
    ) -> bool {
        let Some(icon) = icon else {
            return false;
        };
        if self.packages.as_deref() != packages {
            self.invalidate();
            self.packages = packages.map(Path::to_owned);
        }
        if self
            .preview
            .as_ref()
            .is_none_or(|(current, _)| current != icon)
            && self.preview_pending.is_none()
        {
            self.preview_texture = None;
            if let Some(row) = self.rows.iter().find(|row| &row.icon == icon) {
                self.preview = Some((icon.clone(), Ok(row.image.clone())));
            } else {
                match icon {
                    Icon::Image { image, .. } => {
                        let image = image.fit_to(64, 64);
                        self.preview = Some((
                            icon.clone(),
                            Ok(egui::ColorImage::from_rgba_unmultiplied(
                                [64, 64],
                                image.as_raw(),
                            )),
                        ));
                    }
                    Icon::Texture { tag } => {
                        if let (Some(packages), Ok(tag)) = (packages, tag.parse_u32()) {
                            let packages = packages.to_owned();
                            let sender = self.events();
                            let icon = icon.clone();
                            self.preview_pending = Some(icon.clone());
                            let repaint = ui.ctx().clone();
                            self.workers.push(thread::spawn(move || {
                                let result =
                                    sundial::package_authoring::open_shadowkeep_package_manager(
                                        &packages,
                                    )
                                    .and_then(|manager| {
                                        package_icons::load(&manager, tiger_pkg::TagHash(tag))
                                    });
                                let _ = sender.send(Event::Preview(icon, result));
                                repaint.request_repaint();
                            }));
                        }
                    }
                }
            }
        }
        let size = egui::Vec2::splat(ui.spacing().interact_size.y);
        if let Some((current, result)) = &self.preview
            && current == icon
        {
            match result {
                Ok(image) => {
                    let texture = self.preview_texture.get_or_insert_with(|| {
                        ui.ctx().load_texture(
                            "selected-perk-icon",
                            image.clone(),
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    ui.add(egui::Image::new(&*texture).fit_to_exact_size(size));
                }
                Err(error) => {
                    ui.label("?").on_hover_text(error);
                }
            }
        } else {
            ui.add_sized(size, egui::Spinner::new());
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        true
    }

    fn local_job(&mut self, ctx: &egui::Context, download: bool) {
        let Some(sender) = self.sender.clone() else {
            return;
        };
        self.error = None;
        let repaint = ctx.clone();
        let purpose = self.purpose;
        self.local_workers.push(thread::spawn(move || {
            let result = (|| {
                let root = library::directory()?;
                if download {
                    library::download(&root)?;
                    let _ = sender.send(Event::Downloaded);
                } else {
                    let dialog = rfd::FileDialog::new().set_title("Add Icon");
                    let dialog = if purpose == Purpose::Badge {
                        dialog.add_filter("Images", &["png", "jpg", "jpeg"])
                    } else {
                        dialog.add_filter("Transparent PNG", &["png"])
                    };
                    let Some(path) = dialog.pick_file() else {
                        return Ok(());
                    };
                    let entry = library::add(&root, &path, purpose)?;
                    if !entry.white {
                        let _ = sender.send(Event::ShowColors);
                    }
                    let _ = sender.send(Event::RevealLocal(2));
                }
                let _ = sender.send(Event::Local(library::scan(&root, purpose)));
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                let _ = sender.send(Event::Error(error));
            }
            repaint.request_repaint();
        }));
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
