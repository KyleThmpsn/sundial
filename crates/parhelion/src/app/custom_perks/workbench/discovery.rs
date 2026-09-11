use super::*;
use sundial::package_authoring::{sandbox_perk::dependencies, tft};

pub(super) struct Data {
    pub names: Arc<tft::Index>,
    pub effects: Arc<projectile::catalog::Catalog>,
    pub asset_choices: Vec<program::AssetChoice>,
    pub perks: Arc<dependencies::Index>,
    pub perk_assets: Vec<dependencies::content::PerkAssets>,
    perk_search: BTreeMap<u16, String>,
}

struct Row {
    index: usize,
    label: String,
    search: String,
}

enum Event {
    Progress(usize, usize),
    Ready(Result<Box<Data>, String>),
}

#[derive(Clone, Copy, Default, PartialEq, Hash)]
enum View {
    #[default]
    Projectiles,
    Emitters,
    Perks,
    AllPaths,
    AllReferences,
}

#[derive(Default)]
pub(super) struct Discovery {
    pub open: bool,
    pub data: Option<Data>,
    receiver: Option<Receiver<Event>>,
    worker: Option<thread::JoinHandle<()>>,
    error: Option<String>,
    progress: Option<(usize, usize)>,
    attempted: bool,
    query: String,
    view: View,
    selected: Option<usize>,
    rows: Vec<Row>,
    row_key: Option<(View, usize)>,
    filtered: Vec<usize>,
    filter_query: Option<String>,
}

impl Discovery {
    pub fn busy(&self) -> bool {
        self.receiver.is_some() || self.worker.is_some()
    }
    pub fn invalidate(&mut self) {
        self.data = None;
        self.attempted = false;
        self.error = None;
        self.row_key = None;
        self.rows.clear();
    }

    pub fn start(&mut self, packages: &Path, ctx: &egui::Context) {
        if self.attempted || self.busy() {
            return;
        }
        self.attempted = true;
        let packages = packages.to_owned();
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let result = (|| {
                let manager = open_shadowkeep_package_manager(&packages)?;
                let names = tft::cached(&packages, &manager, |current, total| {
                    let _ = sender.send(Event::Progress(current, total));
                    repaint.request_repaint();
                })?;
                let perks = dependencies::cached(&packages, &manager, |_, _| {})?;
                let effects = projectile::catalog::cached(&packages, &manager)?;
                let asset_choices = program::asset_choices(&effects);
                let perk_assets = dependencies::content::map(&perks, &names);
                let perk_search = perk_assets
                    .iter()
                    .filter_map(|assets| {
                        let index = u16::try_from(assets.perk_index).ok()?;
                        Some((
                            index,
                            assets
                                .references()
                                .map(|index| names.references[index].path.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                                .to_ascii_lowercase(),
                        ))
                    })
                    .collect();
                Ok(Data {
                    names,
                    effects,
                    asset_choices,
                    perks,
                    perk_assets,
                    perk_search,
                })
            })();
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
                Some(Event::Progress(current, total)) => self.progress = Some((current, total)),
                Some(Event::Ready(result)) => {
                    self.receiver = None;
                    if let Some(worker) = self.worker.take() {
                        let _ = worker.join();
                    }
                    self.progress = None;
                    self.row_key = None;
                    match result {
                        Ok(data) => {
                            self.data = Some(*data);
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                    break;
                }
                None => break,
            }
        }
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

mod browser;
