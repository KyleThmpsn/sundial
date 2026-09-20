use super::*;
use sundial::package_authoring::sandbox_perk::dependencies;
use sundial::package_authoring::sandbox_perk::program::properties;

pub(super) use sundial::investment::discovery::Catalog as Data;

enum Event {
    Progress(usize, usize),
    Keys(Result<Arc<properties::KeyIndex>, String>),
    Labels(Result<Arc<sundial::investment::discovery::labels::Registry>, String>),
    Ready(Result<Box<Data>, String>),
}

#[derive(Default)]
pub(super) struct Discovery {
    pub data: Option<Data>,
    pub keys: Option<Arc<properties::KeyIndex>>,
    pub key_error: Option<String>,
    pub labels: Option<Arc<sundial::investment::discovery::labels::Registry>>,
    pub label_error: Option<String>,
    packages: Option<PathBuf>,
    discard_result: bool,
    receiver: Option<Receiver<Event>>,
    worker: Option<thread::JoinHandle<()>>,
    pub(super) error: Option<String>,
    pub(super) progress: Option<(usize, usize)>,
    attempted: bool,
}

impl Discovery {
    pub fn packages(&self) -> Option<&Path> {
        self.packages.as_deref()
    }
    pub fn busy(&self) -> bool {
        self.receiver.is_some() || self.worker.is_some()
    }
    pub fn invalidate(&mut self) {
        self.data = None;
        self.keys = None;
        self.key_error = None;
        self.labels = None;
        self.label_error = None;
        self.discard_result = true;
        self.attempted = false;
        self.error = None;
    }

    pub fn start(&mut self, packages: &Path, ctx: &egui::Context) {
        if self.packages.as_deref() != Some(packages) {
            self.invalidate();
            self.packages = Some(packages.to_owned());
        }
        if self.attempted || self.busy() {
            return;
        }
        self.attempted = true;
        self.discard_result = false;
        let packages = packages.to_owned();
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let result = sundial::investment::discovery::discover(&packages, |event| {
                use sundial::investment::discovery::DiscoveryEvent;
                let event = match event {
                    DiscoveryEvent::Progress(current, total) => Event::Progress(current, total),
                    DiscoveryEvent::Keys(result) => Event::Keys(result),
                    DiscoveryEvent::Labels(result) => Event::Labels(result),
                };
                let _ = sender.send(event);
                repaint.request_repaint();
            });
            let _ = sender.send(Event::Ready(result.map(Box::new)));
            repaint.request_repaint();
        }));
    }

    pub fn poll(&mut self) {
        loop {
            let event = self
                .receiver
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(event) => Some(event),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Event::Ready(Err(
                        "The content reader stopped before finishing".into(),
                    ))),
                });
            match event {
                Some(Event::Labels(result)) => {
                    if !self.discard_result {
                        match result {
                            Ok(labels) => {
                                self.labels = Some(labels);
                                self.label_error = None;
                            }
                            Err(error) => self.label_error = Some(error),
                        }
                    }
                }
                Some(Event::Progress(current, total)) => {
                    if !self.discard_result {
                        self.progress = Some((current, total));
                    }
                }
                Some(Event::Keys(result)) => {
                    if !self.discard_result {
                        match result {
                            Ok(keys) => {
                                self.keys = Some(keys);
                                self.key_error = None;
                            }
                            Err(error) => self.key_error = Some(error),
                        }
                    }
                }
                Some(Event::Ready(result)) => {
                    self.receiver = None;
                    if let Some(worker) = self.worker.take() {
                        let _ = worker.join();
                    }
                    self.progress = None;
                    if self.discard_result {
                        break;
                    }
                    match result {
                        Ok(data) => {
                            self.data = Some(*data);
                            self.error = None;
                        }
                        Err(error) => {
                            if self.labels.is_none() && self.label_error.is_none() {
                                self.label_error = Some(error.clone());
                            }
                            if self.keys.is_none() && self.key_error.is_none() {
                                self.key_error = Some(error.clone());
                            }
                            self.error = Some(error);
                        }
                    }
                    break;
                }
                None => break,
            }
        }
    }

    /// Cached reading of a perk's action, when the dependency index has one.
    pub fn behavior(&self, index: u16) -> Option<&dependencies::Behavior> {
        let data = self.data.as_ref()?;
        data.perks
            .perks
            .get(usize::from(index))
            .filter(|perk| perk.index == usize::from(index))
            .and_then(|perk| perk.behavior.as_ref())
    }

    pub fn perk_issue(&self, index: u16) -> Option<&str> {
        let data = self.data.as_ref()?;
        let Some(perk) = data
            .perks
            .perks
            .get(usize::from(index))
            .filter(|perk| perk.index == usize::from(index))
        else {
            return Some("This effect is missing from the installed packages.");
        };
        if let Some(error) = &perk.error {
            return Some(error);
        }
        perk.action.is_none().then_some(
            "This entry has no standalone action. It cannot be added as a custom effect yet.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_lookup_is_available_before_the_asset_scan_finishes() {
        let (sender, receiver) = mpsc::channel();
        let mut discovery = Discovery {
            receiver: Some(receiver),
            ..Discovery::default()
        };
        sender
            .send(Event::Keys(Ok(Arc::new(properties::KeyIndex::default()))))
            .unwrap();
        discovery.poll();
        assert!(discovery.keys.is_some());
        assert!(discovery.busy());
        assert!(discovery.data.is_none());
        sender
            .send(Event::Ready(Err("unrelated asset scan failure".into())))
            .unwrap();
        discovery.poll();
        assert!(discovery.keys.is_some());
        assert!(discovery.key_error.is_none());
    }

    #[test]
    fn invalidation_discards_results_from_the_previous_installation() {
        let (sender, receiver) = mpsc::channel();
        let mut discovery = Discovery {
            receiver: Some(receiver),
            attempted: true,
            ..Discovery::default()
        };
        discovery.invalidate();
        sender
            .send(Event::Keys(Ok(Arc::new(properties::KeyIndex::default()))))
            .unwrap();
        sender.send(Event::Ready(Err("old scan".into()))).unwrap();
        discovery.poll();
        assert!(discovery.keys.is_none());
        assert!(discovery.data.is_none());
        assert!(discovery.error.is_none());
        assert!(discovery.key_error.is_none());
        assert!(!discovery.attempted);
        assert!(!discovery.busy());
    }
}
