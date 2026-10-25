//! Every use of a resource over the whole installation, read once when a resource page asks
//! for it. The first read opens every package, so it runs on a thread with progress and can
//! be stopped. It is kept on the disk by package, so later reads come from there.
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError, channel},
    },
    thread,
};

use sundial::package_authoring::referrers::{CANCELLED, Referrers as Index, read};
use sundial::ui::catalog::content::UsesState;

enum Event {
    Progress(usize, usize),
    Finished(Result<Arc<Index>, String>),
}

struct Job {
    receiver: Receiver<Event>,
    worker: thread::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct Referrers {
    index: Option<Arc<Index>>,
    error: Option<String>,
    progress: (usize, usize),
    job: Option<Job>,
    /// A resource page asked for the index and the read has not started.
    wanted: bool,
}

impl Referrers {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Whether a read should start: a page asked, and neither an index nor a read exists.
    pub(super) fn wanted(&self) -> bool {
        self.wanted && self.index.is_none() && self.job.is_none()
    }

    pub(super) fn request(&mut self) {
        self.wanted = true;
        self.error = None;
    }

    /// The finished index, shared, so a caller can hold it across a call that borrows this
    /// state mutably.
    pub(super) fn index(&self) -> Option<Arc<Index>> {
        self.index.clone()
    }

    /// How the resource page reads this state, over an index handle the caller holds.
    pub(super) fn state<'a>(&'a self, index: Option<&'a Index>) -> UsesState<'a> {
        if let Some(index) = index {
            return UsesState::Ready(index);
        }
        if let Some(error) = &self.error {
            return UsesState::Failed(error);
        }
        if self.job.is_some() {
            return UsesState::Reading(self.progress.0, self.progress.1);
        }
        UsesState::Idle
    }

    /// Drops the index. It describes the packages that were open, so a reload drops it.
    pub(super) fn invalidate(&mut self) {
        self.index = None;
        self.error = None;
        self.wanted = false;
    }

    pub(super) fn start(&mut self, packages: &Path, ctx: &egui::Context) {
        if !self.wanted() {
            return;
        }
        self.wanted = false;
        let packages = packages.to_path_buf();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let (sender, receiver) = channel();
        let worker = thread::spawn(move || {
            let result = sundial::package_authoring::open_shadowkeep_package_manager(&packages)
                .and_then(|manager| {
                    let progress = sender.clone();
                    let ctx = ctx.clone();
                    read(&manager, &stop, move |done, total| {
                        let _ = progress.send(Event::Progress(done, total));
                        ctx.request_repaint();
                    })
                    .map(Arc::new)
                });
            let _ = sender.send(Event::Finished(result));
            ctx.request_repaint();
        });
        self.job = Some(Job {
            receiver,
            worker,
            cancel,
        });
    }

    /// Stops a running read and waits for it to let go of the packages. The packages it
    /// finished stay on the disk, so asking again resumes after them.
    pub(super) fn stop(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            let _ = job.worker.join();
        }
        self.progress = (0, 0);
    }

    pub(super) fn poll(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let finished = loop {
            match job.receiver.try_recv() {
                Ok(Event::Progress(done, total)) => self.progress = (done, total),
                Ok(Event::Finished(result)) => break result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    break Err("The reference index reader stopped without a result".to_owned());
                }
            }
        };
        let Some(job) = self.job.take() else {
            return;
        };
        // Join before anything else touches the packages, as every other reader here does.
        if job.worker.join().is_err() {
            self.error =
                Some("The reference index reader panicked while reading packages".to_owned());
            return;
        }
        match finished {
            Ok(index) => self.index = Some(index),
            Err(error) if error == CANCELLED => self.progress = (0, 0),
            Err(error) => self.error = Some(error),
        }
    }
}
