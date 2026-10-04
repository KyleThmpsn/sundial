//! The authored items the installation carries, read in the background when the app opens a
//! packages directory and again after every install, so a recipe list can mark the recipes
//! whose items are installed.
use super::*;

#[cfg(test)]
mod tests;

struct Job {
    receiver: Receiver<Result<BTreeSet<u32>, String>>,
    worker: thread::JoinHandle<()>,
    generation: u64,
}

#[derive(Default)]
pub(super) struct Installed {
    job: Option<Job>,
    /// A read was asked for and has not started.
    wanted: bool,
    /// The packages directory the items were read from.
    read_from: Option<PathBuf>,
    items: BTreeSet<u32>,
    generation: u64,
}

impl Installed {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Asks for the items to be read again, after an install or a library change.
    pub(super) fn request(&mut self) {
        self.wanted = true;
    }

    /// Whether the installation carries the item under `hash`.
    pub(super) fn contains(&self, hash: u32) -> bool {
        self.items.contains(&hash)
    }

    /// Takes the set another read produced, such as the pre-build check's.
    pub(super) fn replace(&mut self, items: BTreeSet<u32>) {
        self.generation = self.generation.wrapping_add(1);
        self.items = items;
    }

    fn set_source(&mut self, packages: &Path) {
        if self.read_from.as_deref() != Some(packages) {
            self.generation = self.generation.wrapping_add(1);
            self.read_from = Some(packages.to_path_buf());
            self.items.clear();
            self.wanted = true;
        }
    }

    /// Starts the read that was asked for, or the first read of a packages directory.
    pub(super) fn start(&mut self, packages: &Path, ctx: &egui::Context) {
        self.set_source(packages);
        if self.job.is_some() || !self.wanted || !packages.is_dir() {
            return;
        }
        self.wanted = false;
        let packages = packages.to_path_buf();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = sender.send(crate::install::installed_item_hashes(&packages));
            ctx.request_repaint();
        });
        self.job = Some(Job {
            receiver,
            worker,
            generation: self.generation,
        });
    }

    /// Takes a finished read. A failed one keeps the last set, since the pre-build check
    /// reports a failure of its own read.
    pub(super) fn poll(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(String::new()),
        };
        let current = job.generation == self.generation;
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
        if current && let Ok(items) = result {
            self.items = items;
        }
    }
}

impl PackageAuthoringApp {
    pub(super) fn refresh_installed(&mut self, ctx: &egui::Context) {
        self.installed.set_source(&self.packages);
        self.installed.poll();
        if !self.has_background_work() {
            self.installed.start(&self.packages, ctx);
        }
    }
}
