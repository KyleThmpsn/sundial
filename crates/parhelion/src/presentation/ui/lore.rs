//! Loads inherited lore without blocking the editor.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};
use sundial::investment::LoreEntry;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    packages: PathBuf,
    item_hash: u32,
}
struct Job {
    source: Source,
    receiver: Receiver<Result<Option<LoreEntry>, String>>,
    worker: JoinHandle<()>,
}
/// The lore of the shown item, read on a worker. Switching items never waits for an earlier
/// read: it finishes on its own, and an install waits for it through its [`PackageRead`].
///
/// [`PackageRead`]: sundial::ui::model_preview::PackageRead
#[derive(Default)]
pub(super) struct Preview {
    source: Option<Source>,
    result: Option<Result<Option<LoreEntry>, String>>,
    job: Option<Job>,
}
impl Preview {
    pub(super) fn update(&mut self, ctx: &egui::Context, packages: &Path, item_hash: Option<u32>) {
        let source = item_hash
            .filter(|_| !packages.as_os_str().is_empty())
            .map(|item_hash| Source {
                packages: packages.to_owned(),
                item_hash,
            });
        if self.source != source {
            self.source = source;
            self.result = None;
        }
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.worker.is_finished())
        {
            let job = self.job.take().unwrap();
            let _ = job.worker.join();
            if self.source.as_ref() == Some(&job.source) {
                self.result = Some(
                    job.receiver
                        .try_recv()
                        .unwrap_or_else(|_| Err("Lore loading stopped unexpectedly".into())),
                );
            }
        }
        if self.job.is_none()
            && self.result.is_none()
            && let Some(source) = self.source.clone()
        {
            let (sender, receiver) = mpsc::channel();
            let worker_source = source.clone();
            let ctx = ctx.clone();
            let read = sundial::ui::model_preview::PackageRead::start();
            let worker = thread::spawn(move || {
                let _read = read;
                let result = sundial::investment::load_item_lore(
                    &worker_source.packages,
                    worker_source.item_hash,
                );
                let _ = sender.send(result);
                ctx.request_repaint();
            });
            self.job = Some(Job {
                source,
                receiver,
                worker,
            });
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    #[cfg(test)]
    pub(super) fn loaded(&self) -> bool {
        self.result.is_some()
    }

    pub(super) fn entry(&self) -> Option<&LoreEntry> {
        self.result
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(Option::as_ref)
    }
    pub(super) fn draw(&self, ui: &mut egui::Ui, kind: crate::ItemKind) {
        match &self.result {
            Some(Ok(Some(entry))) => {
                ui.strong(&entry.title);
                egui::ScrollArea::vertical()
                    .id_salt("inherited-lore-text")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(&entry.text).wrap().selectable(true));
                    });
            }
            Some(Ok(None)) => {
                ui.weak("Inherited: None");
            }
            Some(Err(error)) => {
                ui.weak("Lore Unavailable").on_hover_text(error);
            }
            _ if self.source.is_some() => {
                ui.spinner();
            }
            _ => {
                ui.weak(format!("No {} Selected", kind.label()));
            }
        }
    }
}
