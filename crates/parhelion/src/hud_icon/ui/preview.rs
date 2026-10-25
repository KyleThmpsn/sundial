//! Keeps package reads off the UI thread and discards results for a previous appearance.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, PartialEq, Eq)]
struct Source {
    packages: PathBuf,
    pattern_index: u16,
}

struct Job {
    source: Source,
    receiver: Receiver<Result<Option<egui::ColorImage>, String>>,
    worker: JoinHandle<()>,
}

/// The inherited ammunition HUD icon, read on a worker. Switching appearances never waits for an
/// earlier read: it finishes on its own, and an install waits for it through its [`PackageRead`].
///
/// [`PackageRead`]: sundial::ui::model_preview::PackageRead
#[derive(Default)]
pub(super) struct Preview {
    source: Option<Source>,
    result: Option<Result<Option<egui::TextureHandle>, String>>,
    job: Option<Job>,
}

impl Preview {
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        pattern_index: Option<u16>,
    ) {
        let source = pattern_index
            .filter(|index| *index != u16::MAX && !packages.as_os_str().is_empty())
            .map(|pattern_index| Source {
                packages: packages.to_owned(),
                pattern_index,
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
                let result = job
                    .receiver
                    .try_recv()
                    .unwrap_or_else(|_| Err("HUD preview loading stopped unexpectedly".into()));
                self.result = Some(result.map(|image| {
                    image.map(|image| {
                        ctx.load_texture(
                            "inherited-ammo-hud",
                            super::squared(image),
                            egui::TextureOptions::LINEAR,
                        )
                    })
                }));
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
                let result = sundial::package_authoring::open_shadowkeep_package_manager(
                    &worker_source.packages,
                )
                .and_then(|manager| {
                    super::super::preview::load(&manager, worker_source.pattern_index)
                });
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

    pub(super) fn texture(&self) -> Option<&egui::TextureHandle> {
        self.result
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(Option::as_ref)
    }

    pub(super) fn status(&self) -> &'static str {
        match &self.result {
            Some(Ok(None)) => "No Override",
            Some(Err(_)) => "Unavailable",
            _ if self.source.is_some() => "Loading…",
            _ => "No Appearance",
        }
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.result
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .map(String::as_str)
    }
}
