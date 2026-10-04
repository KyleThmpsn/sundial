//! Cancellable serialization with one atomic publication after the complete snapshot exists.
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread,
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::app::custom_perks::workbench) struct Export {
    worker: Option<thread::JoinHandle<()>>,
    receiver: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    cancel: Arc<AtomicBool>,
    publication: Arc<AtomicU8>,
    pub result: Option<Result<PathBuf, String>>,
}

struct Writer<'a> {
    bytes: Vec<u8>,
    cancel: &'a AtomicBool,
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("Export stopped."));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Export {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn stop(&mut self) {
        // State 0 is cancellable, 1 is cancelled, and 2 has started atomic publication.
        // The UI never waits on file I/O when requesting cancellation.
        if self
            .publication
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub fn start<T: serde::Serialize + Send + 'static>(
        &mut self,
        path: PathBuf,
        value: T,
        ctx: &egui::Context,
    ) {
        if self.busy() {
            return;
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.publication = Arc::new(AtomicU8::new(0));
        self.result = None;
        let cancel = self.cancel.clone();
        let publication = self.publication.clone();
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let run = || -> Result<PathBuf, String> {
                let baseline = match std::fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.to_string()),
                };
                let mut writer = Writer {
                    bytes: Vec::new(),
                    cancel: &cancel,
                };
                serde_json::to_writer_pretty(&mut writer, &value).map_err(|e| e.to_string())?;
                if publication
                    .compare_exchange(0, 2, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
                {
                    return Err("Export stopped. The previous file was preserved.".into());
                }
                match baseline {
                    Some(baseline) => {
                        sundial::package_authoring::replace_authoring_file_if_unchanged(
                            &path,
                            &writer.bytes,
                            &baseline,
                        )
                    }
                    None => sundial::storage::create_file(&path, &writer.bytes),
                }
                .map_err(|e| e.to_string())?;
                Ok(path)
            };
            let _ = sender.send(run());
            repaint.request_repaint();
        }));
    }
    pub fn poll(&mut self) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "The native map exporter stopped before finishing.".into(),
                )),
            });
        if let Some(result) = result {
            self.receiver = None;
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            self.result = Some(result);
        }
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        self.stop();
    }
}
