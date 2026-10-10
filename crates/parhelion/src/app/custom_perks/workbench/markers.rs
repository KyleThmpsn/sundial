//! Every named point the game's gear art carries, and where to find one.
//!
//! Two directions, because both questions get asked. From a marker: which objects carry it, and
//! where does it sit on each. From an object: which points does it have. About half of these
//! names are recovered; the rest are still hashes, so a row without one says which named marker
//! sits nearest, which is a fact about geometry rather than a guess at the name.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError, channel},
};
use sundial::package_authoring::gear_markers::{
    CANCELLED, MarkerEntry, MarkerIndex, MarkerObject, cached_index,
};
use sundial::package_authoring::tft;
use sundial::ui::catalog::BrowserList;

enum Event {
    Progress(usize, usize),
    Finished(Result<Arc<MarkerIndex>, String>),
}

struct Job {
    receiver: Receiver<Event>,
    worker: thread::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
enum Mode {
    #[default]
    Markers,
    Objects,
}

/// What a click in the list asked for.
enum Action {
    Copy(String),
    /// Show this object's own markers.
    Open(u32),
    /// Look this text up in Objects and Effects.
    Find(String),
}

#[derive(Default)]
pub(super) struct Markers {
    mode: Mode,
    query: String,
    unnamed_only: bool,
    index: Option<Arc<MarkerIndex>>,
    error: Option<String>,
    progress: (usize, usize),
    job: Option<Job>,
    /// The filtered rows, and the mode, query and filter they were built for.
    results: Option<((Mode, String, bool), Vec<usize>)>,
    reset: bool,
    /// Object tag to authored label, from the content paths the catalog scan already holds. A
    /// tag alone says nothing; `vandal.pattern` says what the thing is.
    labels: BTreeMap<u32, String>,
    /// A named carrier for each marker hash, when one can be found anywhere in the index.
    marker_context: BTreeMap<u32, (u32, String)>,
    labels_source: Option<(usize, usize)>,
    /// An object chosen from a marker's examples, to reveal in the object list.
    reveal: Option<u32>,
}

impl Markers {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The finished index, shared. Cloning the handle lets a caller hold it across a call that
    /// borrows this browser mutably, which is how the object page reaches it.
    pub(super) fn index(&self) -> Option<Arc<MarkerIndex>> {
        self.index.clone()
    }

    pub(super) fn invalidate(&mut self) {
        self.index = None;
        self.error = None;
        self.results = None;
        self.labels.clear();
        self.marker_context.clear();
        self.labels_source = None;
    }

    /// Starts the read once. A first read opens every object in the game, so it is only asked
    /// for while the view is on screen. Later ones come from the disk.
    pub(super) fn start(&mut self, packages: &Path, ctx: &egui::Context) {
        if self.job.is_some() || self.index.is_some() || self.error.is_some() {
            return;
        }
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
                    cached_index(
                        &packages,
                        &manager,
                        move |done, total| {
                            let _ = progress.send(Event::Progress(done, total));
                            ctx.request_repaint();
                        },
                        &stop,
                    )
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

    /// Asks a running read to stop. It stops within a few objects, keeps nothing, and starts
    /// over the next time the view opens.
    pub(super) fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Stops a running read and waits for it to let go of the packages.
    pub(super) fn stop(&mut self) {
        self.cancel();
        if let Some(job) = self.job.take() {
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
                    break Err("The marker reader stopped without a result".to_owned());
                }
            }
        };
        let Some(job) = self.job.take() else {
            return;
        };
        // Join before anything else touches the packages, as every other reader here does.
        if job.worker.join().is_err() {
            self.error = Some("The marker reader panicked while reading packages".to_owned());
            return;
        }
        match finished {
            Ok(index) => {
                self.index = Some(index);
                self.results = None;
                self.reset = true;
            }
            // Left on purpose, so there is nothing to report. The next visit starts again.
            Err(error) if error == CANCELLED => self.progress = (0, 0),
            Err(error) => self.error = Some(error),
        }
    }

    /// Object labels from the content paths the catalog scan already found. Rebuilt only when
    /// that scan changes, because it walks every path in it.
    fn refresh_labels(&mut self, discovery: &discovery::Discovery, index: &MarkerIndex) {
        let source = (
            discovery
                .data
                .as_ref()
                .map_or(0, |data| Arc::as_ptr(&data.names) as usize),
            index as *const MarkerIndex as usize,
        );
        if self.labels_source == Some(source) {
            return;
        }
        let mut labels = index.tag_names.clone();
        if let Some(data) = &discovery.data {
            let names = data.names.names();
            let own = data.names.own_paths();
            let mut direct: BTreeMap<u32, String> = BTreeMap::new();
            for (tag, paths) in names.iter().chain(&own) {
                for path in paths {
                    let label = tft::asset_label(path);
                    // The shortest direct path is the least qualified and reads best.
                    if direct.get(tag).is_none_or(|kept| label.len() < kept.len()) {
                        direct.insert(*tag, label);
                    }
                }
            }
            labels.extend(direct.iter().map(|(&tag, label)| (tag, label.clone())));
            for reference in &data.names.entity_references {
                if labels.contains_key(&reference.target)
                    || index.object(reference.target).is_none()
                {
                    continue;
                }
                let context = own
                    .get(&reference.source)
                    .or_else(|| names.get(&reference.source))
                    .and_then(|paths| paths.first());
                if let Some(path) = context {
                    labels.insert(
                        reference.target,
                        format!(
                            "0x{:08X} · referenced by {}",
                            reference.target,
                            tft::asset_label(path)
                        ),
                    );
                }
            }
        }
        self.labels = labels;
        self.marker_context.clear();
        for object in &index.objects {
            if let Some(label) = self.labels.get(&object.entity)
                && !label.starts_with("0x")
            {
                for marker in &object.markers {
                    self.marker_context
                        .entry(marker.name)
                        .or_insert_with(|| (object.entity, label.clone()));
                }
            }
        }
        self.labels_source = Some(source);
        self.results = None;
    }

    /// What to call an object: its authored name, or its tag when the packages do not name it.
    fn object_label(&self, entity: u32) -> String {
        self.labels
            .get(&entity)
            .cloned()
            .unwrap_or_else(|| format!("0x{entity:08X}"))
    }

    fn marker_label(entry: &MarkerEntry, context: &BTreeMap<u32, (u32, String)>) -> String {
        let label = entry.label();
        if entry.name.is_some() || entry.neighbour.is_some() {
            return label;
        }
        context
            .get(&entry.hash)
            .map_or(label.clone(), |(_, name)| format!("{label} · on {name}"))
    }

    /// Rows matching the query, cached until the query, filter or mode changes.
    fn rows(&mut self) -> Vec<usize> {
        let key = (self.mode, self.query.to_lowercase(), self.unnamed_only);
        let Some(index) = self.index.clone() else {
            return Vec::new();
        };
        if self.results.as_ref().is_none_or(|(built, _)| *built != key) {
            let text = key.1.clone();
            let mut matched: Vec<usize> = match self.mode {
                Mode::Markers => index
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| !self.unnamed_only || entry.name.is_none())
                    .filter(|(_, entry)| {
                        text.is_empty()
                            || Self::marker_label(entry, &self.marker_context)
                                .to_lowercase()
                                .contains(&text)
                            || format!("{:08x}", entry.hash).contains(&text)
                            || entry.examples.iter().any(|entity| {
                                self.object_label(*entity).to_lowercase().contains(&text)
                            })
                    })
                    .map(|(row, _)| row)
                    .collect(),
                Mode::Objects => index
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, object)| {
                        !self.unnamed_only
                            || object
                                .markers
                                .iter()
                                .any(|marker| marker.label().starts_with("0x"))
                    })
                    .filter(|(_, object)| {
                        text.is_empty()
                            || format!("{:08x}", object.entity).contains(&text)
                            || self
                                .object_label(object.entity)
                                .to_lowercase()
                                .contains(&text)
                    })
                    .map(|(row, _)| row)
                    .collect(),
            };
            if self.mode == Mode::Objects {
                matched.sort_by_key(|&row| {
                    let object = &index.objects[row];
                    let label = self.object_label(object.entity);
                    (label.starts_with("0x"), label.to_lowercase(), object.entity)
                });
            }
            self.results = Some((key, matched));
        }
        self.results
            .as_ref()
            .map_or_else(Vec::new, |(_, rows)| rows.clone())
    }

    /// Draws the view. Returns text to look up in Objects and Effects when a row asks for it.
    pub(super) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
    ) -> Option<String> {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if ui.button("Retry").clicked() {
                self.error = None;
            }
            return None;
        }
        if self.index.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                let (done, total) = self.progress;
                if !self.busy() && discovery.busy() {
                    ui.label("Waiting for the content scan…");
                } else if let Some(percent) = (done * 100).checked_div(total) {
                    ui.label(format!("Reading objects… {percent}%"));
                } else {
                    ui.label("Reading objects…");
                }
            });
            return None;
        }
        let index = self.index.clone().expect("index was checked");
        self.refresh_labels(discovery, &index);
        ui.horizontal_wrapped(|ui| {
            let before = self.mode;
            ui.selectable_value(&mut self.mode, Mode::Markers, "By Marker");
            ui.selectable_value(&mut self.mode, Mode::Objects, "By Object");
            if before != self.mode {
                self.reset = true;
            }
            ui.separator();
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(200.0));
            if ui.button("Clear").clicked() {
                self.query.clear();
            }
            ui.checkbox(&mut self.unnamed_only, "Unnamed Only");
            ui.weak(match self.mode {
                Mode::Markers => {
                    format!("{} names, {} recovered", index.entries.len(), index.named())
                }
                Mode::Objects => format!(
                    "{} objects carry markers, of {} read",
                    index.objects.len(),
                    index.scanned
                ),
            });
        });
        ui.separator();
        let reset = std::mem::take(&mut self.reset);
        let rows = self.rows();
        let reveal = self.reveal.take();
        let keys: Vec<u64> = match self.mode {
            Mode::Markers => rows
                .iter()
                .map(|row| u64::from(index.entries[*row].hash))
                .collect(),
            Mode::Objects => rows
                .iter()
                .map(|row| u64::from(index.objects[*row].entity))
                .collect(),
        };
        let list = BrowserList {
            keys: &keys,
            height: ui.available_height().max(120.0),
            reset,
            row_height: sundial::investment::authoring_choice_row_height(ui),
            select: reveal.map(u64::from),
        };
        let labels = &self.labels;
        let marker_context = &self.marker_context;
        let name_of = |entity: u32| {
            labels
                .get(&entity)
                .cloned()
                .unwrap_or_else(|| format!("0x{entity:08X}"))
        };
        let action = match self.mode {
            Mode::Markers => list.draw(
                ui,
                |ui, row, selected| {
                    let entry = &index.entries[rows[row]];
                    sundial::investment::draw_asset_choice_row_plain(
                        ui,
                        &Self::marker_label(entry, marker_context),
                        &count(entry.objects, "Object"),
                        selected,
                    )
                },
                |ui, row| {
                    let entry = &index.entries[rows[row]];
                    marker_detail(
                        ui,
                        &index,
                        entry,
                        &name_of,
                        marker_context.get(&entry.hash).map(|(entity, _)| *entity),
                    )
                },
            ),
            Mode::Objects => list.draw(
                ui,
                |ui, row, selected| {
                    let object = &index.objects[rows[row]];
                    sundial::investment::draw_asset_choice_row_plain(
                        ui,
                        &name_of(object.entity),
                        &count(object.markers.len(), "Marker"),
                        selected,
                    )
                },
                |ui, row| object_detail(ui, &index.objects[rows[row]], &name_of),
            ),
        };
        let mut find = None;
        match action {
            Some(Action::Copy(text)) => ui.ctx().copy_text(text),
            Some(Action::Open(entity)) => {
                self.mode = Mode::Objects;
                self.query.clear();
                self.results = None;
                self.reveal = Some(entity);
                self.reset = true;
            }
            Some(Action::Find(text)) => find = Some(text),
            None => {}
        }
        find
    }
}

/// "1 Object" or "N Objects".
fn count(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The place a marker takes on one object, as two columns: where, and which way it faces.
fn placement_columns(ui: &mut egui::Ui, marker: &sundial::package_authoring::gear_markers::Marker) {
    let [x, y, z] = marker.position;
    ui.monospace(format!("{x:>8.4} {y:>8.4} {z:>8.4}"));
    if marker.is_aligned() {
        ui.weak("");
    } else {
        let [i, j, k, w] = marker.orientation;
        ui.weak(format!("facing {i:.3} {j:.3} {k:.3} {w:.3}"));
    }
}

/// One marker: what it is, how widely it is used, and the objects that carry it with the place
/// it takes on each.
fn marker_detail(
    ui: &mut egui::Ui,
    index: &MarkerIndex,
    entry: &MarkerEntry,
    name_of: &impl Fn(u32) -> String,
    named_carrier: Option<u32>,
) -> Option<Action> {
    let mut action = None;
    egui::Grid::new("marker-detail")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Hash");
            if ui
                .button(format!("0x{:08X}", entry.hash))
                .on_hover_text("Copy")
                .clicked()
            {
                action = Some(Action::Copy(format!("0x{:08X}", entry.hash)));
            }
            ui.end_row();
            ui.label("Name");
            match entry.name {
                Some(name) => ui.label(name),
                None => ui.weak("Not recovered"),
            };
            ui.end_row();
            ui.label("Objects");
            ui.label(entry.objects.to_string());
            ui.end_row();
            if let Some(near) = entry.neighbour {
                ui.label("Nearest Named");
                ui.label(near.to_string())
                    .on_hover_text("Median across every object carrying both");
                ui.end_row();
            }
        });
    if entry.examples.is_empty() {
        return action;
    }
    ui.add_space(4.0);
    ui.weak("On These Objects");
    egui::Grid::new("marker-examples")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            let extra = named_carrier.filter(|entity| !entry.examples.contains(entity));
            for entity in entry.examples.iter().copied().chain(extra) {
                let label = name_of(entity);
                if ui
                    .button(&label)
                    .on_hover_text(format!("0x{entity:08X} · show its markers"))
                    .clicked()
                {
                    action = Some(Action::Open(entity));
                }
                match index.placement(entity, entry.hash) {
                    Some(marker) => placement_columns(ui, marker),
                    None => {
                        ui.weak("not placed");
                        ui.weak("");
                    }
                }
                ui.end_row();
            }
        });
    let first = name_of(entry.examples[0]);
    if !first.starts_with("0x")
        && ui
            .button("Find in Objects and Effects")
            .on_hover_text(&first)
            .clicked()
    {
        action = Some(Action::Find(first));
    }
    action
}

/// One object and every point on it.
fn object_detail(
    ui: &mut egui::Ui,
    object: &MarkerObject,
    name_of: &impl Fn(u32) -> String,
) -> Option<Action> {
    let mut action = None;
    let label = name_of(object.entity);
    ui.horizontal_wrapped(|ui| {
        ui.strong(&label);
        if ui
            .button(format!("0x{:08X}", object.entity))
            .on_hover_text("Copy")
            .clicked()
        {
            action = Some(Action::Copy(format!("0x{:08X}", object.entity)));
        }
        if !label.starts_with("0x") && ui.button("Find in Objects and Effects").clicked() {
            action = Some(Action::Find(label.clone()));
        }
    });
    ui.add_space(4.0);
    ui.weak(format!("{} markers", object.markers.len()));
    egui::Grid::new("object-markers")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for marker in &object.markers {
                ui.label(marker.label());
                placement_columns(ui, marker);
                ui.end_row();
            }
        });
    action
}
