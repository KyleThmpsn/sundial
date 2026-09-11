//! Background release checks and the staged update state machine.
mod archive;
mod download;
mod files;
mod handoff;
mod helper;
mod release;
mod transaction;
mod view;

use download::{Prepared, Progress};
use eframe::egui;
use handoff::Handoff;
pub(crate) use helper::startup;
use release::Release;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};
pub(crate) use view::Action;

pub(crate) const RELEASES_URL: &str = "https://github.com/kylethmpsn/sundial/releases";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/kylethmpsn/sundial/releases/latest";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    NotStarted,
    Checking,
    Current,
    Available(Release),
    Failed,
}

enum InstallState {
    Idle,
    Downloading,
    Ready(Prepared),
    Preparing,
    Waiting(Handoff),
    Failed(String),
    Restarting,
}

enum Work {
    Downloaded(Prepared),
    Prepared(Handoff),
}

pub(crate) struct UpdateCheck {
    status: UpdateStatus,
    receiver: Option<Receiver<Result<Option<Release>, String>>>,
    check_error: Option<String>,
    pub(crate) window_open: bool,
    install: InstallState,
    worker: Option<Receiver<Result<Work, String>>>,
    progress: Arc<Progress>,
    notes_cache: egui_commonmark::CommonMarkCache,
}

impl Default for UpdateCheck {
    fn default() -> Self {
        Self {
            status: UpdateStatus::NotStarted,
            receiver: None,
            check_error: None,
            window_open: false,
            install: InstallState::Idle,
            worker: None,
            progress: Arc::new(Progress::default()),
            notes_cache: egui_commonmark::CommonMarkCache::default(),
        }
    }
}

impl UpdateCheck {
    pub(crate) fn start_if_needed(&mut self, ctx: &egui::Context) {
        if self.status == UpdateStatus::NotStarted {
            self.start(ctx);
        }
    }

    pub(crate) fn retry(&mut self, ctx: &egui::Context) {
        if self.status != UpdateStatus::Checking
            && matches!(self.install, InstallState::Idle | InstallState::Failed(_))
        {
            self.start(ctx);
        }
    }

    pub(crate) fn poll(&mut self) {
        if let Some(receiver) = &self.receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    self.status = match result {
                        Ok(Some(release)) => UpdateStatus::Available(release),
                        Ok(None) => UpdateStatus::Current,
                        Err(error) => {
                            self.check_error = Some(error);
                            UpdateStatus::Failed
                        }
                    };
                    self.receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.status = UpdateStatus::Failed;
                    self.receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.poll_worker();
    }

    fn poll_worker(&mut self) {
        let Some(receiver) = &self.worker else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("The update worker stopped unexpectedly. Sundial was not replaced.".into())
            }
        };
        self.worker = None;
        self.install = match result {
            Ok(Work::Downloaded(prepared)) => InstallState::Ready(prepared),
            Ok(Work::Prepared(handoff)) => InstallState::Waiting(handoff),
            Err(error) => InstallState::Failed(error),
        };
    }

    pub(crate) const fn status(&self) -> &UpdateStatus {
        &self.status
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        self.status = UpdateStatus::Checking;
        self.check_error = None;
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = crate::http::get(LATEST_RELEASE_URL, MAX_RESPONSE_BYTES)
                .and_then(|body| release::parse(&body, env!("CARGO_PKG_VERSION")));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn download(&mut self, ctx: &egui::Context) {
        let UpdateStatus::Available(release) = &self.status else {
            return;
        };
        if !matches!(self.install, InstallState::Idle | InstallState::Failed(_)) {
            return;
        }
        let release = release.clone();
        let (sender, receiver) = mpsc::channel();
        self.progress = Arc::new(Progress::default());
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.install = InstallState::Downloading;
        self.worker = Some(receiver);
        thread::spawn(move || {
            let _ = sender.send(download::prepare(&release, &progress).map(Work::Downloaded));
            ctx.request_repaint();
        });
    }

    pub(crate) fn prepare_restart(&mut self, ctx: &egui::Context, install: PathBuf) {
        if !matches!(self.install, InstallState::Ready(_)) {
            return;
        }
        let InstallState::Ready(prepared) =
            std::mem::replace(&mut self.install, InstallState::Preparing)
        else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        self.worker = Some(receiver);
        thread::spawn(move || {
            let _ = sender.send(handoff::begin(prepared, install).map(Work::Prepared));
            ctx.request_repaint();
        });
    }

    pub(crate) fn commit_restart(&mut self) -> bool {
        let InstallState::Waiting(handoff) = &mut self.install else {
            return false;
        };
        match handoff.commit() {
            Ok(()) => {
                self.install = InstallState::Restarting;
                true
            }
            Err(error) => {
                self.install = InstallState::Failed(error);
                false
            }
        }
    }
}

impl Drop for UpdateCheck {
    fn drop(&mut self) {
        self.progress
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
