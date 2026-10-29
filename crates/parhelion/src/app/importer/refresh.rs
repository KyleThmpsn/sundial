//! One advisory review after a library scan, with source batches using the normal importer.
use super::*;
use std::collections::VecDeque;

#[derive(Clone)]
pub(super) struct Item {
    path: PathBuf,
    pub name: String,
    namespace: String,
    identity: u32,
    revision: u32,
    /// Whether the recipe's conversion brought ornaments, which its reimport brings again.
    ornaments: bool,
}

impl Item {
    /// The importer that wrote the recipe. A reimport keeps the saved namespace, so the item keeps
    /// its identity in any of the schemes `Source::of_namespace` reads.
    fn source(&self) -> Option<Source> {
        Source::of_namespace(&self.namespace)
    }

    /// The source item hash that ends the namespace.
    fn named_source(&self) -> Option<u32> {
        Source::named_item(&self.namespace)
    }

    /// Whether `weapon`, a row of `source` whose origin is `origin`, is this recipe's source item.
    fn matches(&self, source: Source, weapon: &Weapon, origin: Option<&Origin>) -> bool {
        match source {
            Source::HaloReach => self.identity == weapon.hash,
            // Rows are keyed by the identity today's name derives. An earlier name holds the
            // source hash, or a word with no hash, which only the item's name can place.
            Source::Marathon => {
                self.identity == weapon.hash
                    || origin.is_some_and(|origin| match origin {
                        Origin::Marathon(entry) => match self.named_source() {
                            Some(hash) => hash == entry.item,
                            None => self.name.eq_ignore_ascii_case(&entry.name),
                        },
                        Origin::HaloReach(_) => false,
                    })
            }
            Source::D2Modern if self.namespace.starts_with("parhelion.bulk.") => {
                self.named_source() == Some(weapon.hash)
            }
            Source::D2Modern => service::destination_hash(weapon.hash, &weapon.name)
                .is_ok_and(|hash| hash == self.identity),
            Source::D1RiseOfIron => self.named_source() == Some(weapon.hash),
        }
    }

    /// The only row named as this recipe is, for a D2 Modern recipe that no row matches. Some
    /// earlier recipes, restored or made by an import test, end their namespace with a hash that
    /// names no source item. With no such row or several, how many there are, since the version
    /// the recipe came from is then unknown.
    fn named_row<'a>(&self, weapons: &'a [Weapon]) -> Result<&'a Weapon, usize> {
        let named: Vec<_> = weapons
            .iter()
            .filter(|weapon| weapon.name.trim().eq_ignore_ascii_case(self.name.trim()))
            .collect();
        match named.as_slice() {
            [weapon] => Ok(weapon),
            _ => Err(named.len()),
        }
    }
}

/// Records a reimport failure for the review's outcome and for the activity log, which keeps it
/// after the outcome is dismissed.
fn fail(log: &mut ActivityLog, state: &mut State, message: String) {
    log.push(LogEntry::error(message.clone()));
    state.failures.push(message);
}

#[derive(Default)]
pub(super) struct State {
    pub pending: Vec<Item>,
    /// Pending recipes left out of the next reimport. Every recipe starts selected.
    excluded: BTreeSet<PathBuf>,
    /// The revisions of each recipe the review already offered.
    seen: BTreeMap<PathBuf, BTreeSet<u32>>,
    queue: VecDeque<(Source, Vec<Item>)>,
    active: bool,
    importing: bool,
    added: usize,
    failures: Vec<String>,
}

impl State {
    /// Runs every frame, so the pending recipes are rebuilt only when the library changed them. A
    /// library of thousands of imported recipes is otherwise copied each frame.
    pub fn observe(&mut self, entries: &[RecipeLibraryEntry]) {
        if self.active {
            return;
        }
        let outdated = entries.iter().filter_map(|entry| {
            let revision = entry.import_revision?;
            (revision < parhelion_import::CONVERTER_REVISION
                && !self
                    .seen
                    .get(&entry.path)
                    .is_some_and(|seen| seen.contains(&revision)))
            .then_some((entry, revision))
        });
        let unchanged = outdated
            .clone()
            .map(|(entry, revision)| {
                (
                    &entry.path,
                    &entry.name,
                    &entry.namespace,
                    entry.identity_hash,
                    revision,
                    entry.import_ornaments,
                )
            })
            .eq(self.pending.iter().map(|item| {
                (
                    &item.path,
                    &item.name,
                    &item.namespace,
                    item.identity,
                    item.revision,
                    item.ornaments,
                )
            }));
        if unchanged {
            return;
        }
        self.pending = outdated
            .map(|(entry, revision)| Item {
                path: entry.path.clone(),
                name: entry.name.clone(),
                namespace: entry.namespace.clone(),
                identity: entry.identity_hash,
                revision,
                ornaments: entry.import_ornaments,
            })
            .collect();
        let pending: BTreeSet<_> = self.pending.iter().map(|item| &item.path).collect();
        self.excluded.retain(|path| pending.contains(path));
    }

    pub fn dismiss(&mut self) {
        self.excluded.clear();
        for item in self.pending.drain(..) {
            self.seen
                .entry(item.path)
                .or_default()
                .insert(item.revision);
        }
    }

    /// Reimports the selected recipes. The rest wait for a later importer revision, as Later
    /// leaves them.
    fn begin(&mut self) {
        let mut groups = BTreeMap::<Source, Vec<Item>>::new();
        self.added = 0;
        self.failures.clear();
        let excluded = std::mem::take(&mut self.excluded);
        for item in self
            .pending
            .iter()
            .filter(|item| !excluded.contains(&item.path))
        {
            if let Some(source) = item.source() {
                groups.entry(source).or_default().push(item.clone());
            } else {
                self.failures.push(format!(
                    "{}: its recipe doesn't name a source item. Import it again from the importer.",
                    item.name
                ));
            }
        }
        self.dismiss();
        self.queue = groups.into_iter().collect();
        self.active = true;
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_import_refresh(&mut self, ctx: &egui::Context) {
        if self.importer.refresh.pending.is_empty()
            || self.importer.refresh.active
            || !self.importer_idle()
        {
            return;
        }
        let state = &self.importer.refresh;
        let outside_build = state
            .pending
            .iter()
            .filter(|item| !self.enabled_recipe_paths.contains(&item.path))
            .map(|item| item.path.clone())
            .collect::<BTreeSet<_>>();
        // One row per source, as the reimport runs, with the recipes it can't place last.
        let mut groups = BTreeMap::<Option<Source>, Vec<&Item>>::new();
        for item in &state.pending {
            groups.entry(item.source()).or_default().push(item);
        }
        let settings = &self.importer.settings;
        let missing = |source: Source| {
            match source {
                Source::D2Modern => &settings.modern_packages,
                Source::HaloReach => &settings.reach_maps,
                Source::Marathon => &settings.marathon_packages,
                Source::D1RiseOfIron => &settings.d1_roi,
            }
            .as_ref()
            .is_none_or(|path| !path.is_dir())
        };
        let mut requests = Requests::default();
        let available =
            (ctx.content_rect().size() - egui::vec2(48.0, 48.0)).max(egui::vec2(280.0, 220.0));
        let width = 480.0_f32.min(available.x - 40.0);
        egui::Window::new("Imported Recipe Updates")
            .id(egui::Id::new("parhelion-import-refresh"))
            .collapsible(false)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(width)
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(20))
            .show(ctx, |ui| {
                style::workbench_style(ui);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 8.0;
                let muted = style::secondary(ui.visuals());
                ui.label(egui::RichText::new("Importer · Library").color(muted));
                ui.label(egui::RichText::new("Reimport Recipes").size(24.0).strong());
                ui.label(format!(
                    "{} were converted by an older importer. Sockets and perks come from the new conversion, and a recipe they change is backed up first. Your other edits are kept.",
                    state.pending.len()
                ));
                choices(ui, state, &outside_build, &mut requests);
                egui::ScrollArea::vertical()
                    .id_salt("import-refresh-sources")
                    .max_height((available.y - 240.0).max(80.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let known = groups.iter().filter(|(source, _)| source.is_some());
                        let unknown = groups.iter().filter(|(source, _)| source.is_none());
                        for (source, items) in known.chain(unknown) {
                            group(ui, *source, items, state, &missing, &mut requests);
                        }
                    });
                ui.separator();
                actions(ui, state, &mut requests);
            });
        let Requests {
            refresh,
            dismiss,
            choose,
            excluded,
            changes,
        } = requests;
        let state = &mut self.importer.refresh;
        if let Some(excluded) = excluded {
            state.excluded = excluded;
        }
        for (path, selected) in changes {
            if selected {
                state.excluded.remove(&path);
            } else {
                state.excluded.insert(path);
            }
        }
        if let Some(source) = choose {
            self.importer.select_source(source);
            self.choose_source_folder(ctx);
        }
        if dismiss {
            self.importer.refresh.dismiss();
        }
        if refresh {
            self.importer.refresh.begin();
            // Recipes that name no source item fail before any import runs.
            for failure in &self.importer.refresh.failures {
                self.log.push(LogEntry::error(failure.clone()));
            }
            self.importer.enabled = true;
            self.importer.open = true;
        }
    }

    pub(super) fn advance_import_refresh(&mut self, ctx: &egui::Context) {
        if !self.importer.refresh.active || !self.importer_idle() {
            return;
        }
        if self.importer.refresh.importing {
            if let Some(outcome) = self.importer.outcome.take() {
                self.importer.refresh.added += outcome.added;
                self.importer.refresh.failures.extend(outcome.failures);
                if outcome.cancelled {
                    self.importer.refresh.queue.clear();
                    fail(
                        &mut self.log,
                        &mut self.importer.refresh,
                        "The remaining reimports were cancelled.".into(),
                    );
                }
            }
            self.importer.refresh.importing = false;
        }
        let Some((source, items)) = self.importer.refresh.queue.front().cloned() else {
            self.importer.refresh.active = false;
            self.importer.outcome = Some(Outcome {
                added: self.importer.refresh.added,
                failures: std::mem::take(&mut self.importer.refresh.failures),
                show_failures: true,
                cancelled: false,
            });
            return;
        };
        if self.importer.source != source {
            self.importer.select_source(source);
            ctx.request_repaint();
            return;
        }
        let problem = if self.importer.folder().is_none_or(|path| !path.is_dir()) {
            Some(format!(
                "Choose the {} source folder before reimporting.",
                source.label()
            ))
        } else if let Some(error) = &self.importer.scan_error {
            Some(error.clone())
        } else if self.importer.tools_checking() || !self.importer.read_requested {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        } else if !self.importer.tools_ready() {
            Some("Configure the missing import tools before reimporting.".into())
        } else {
            None
        };
        if let Some(problem) = problem {
            for item in &items {
                fail(
                    &mut self.log,
                    &mut self.importer.refresh,
                    format!("{}: {problem}", item.name),
                );
            }
            self.importer.refresh.queue.pop_front();
            ctx.request_repaint();
            return;
        }
        if !self.importer_blockers().is_empty() {
            return;
        }
        let deferred = self.select_reimports(source, items);
        self.importer.refresh.queue.pop_front();
        if !deferred.is_empty() {
            self.importer.refresh.queue.push_front((source, deferred));
        }
        if !self.importer.selected.is_empty() {
            self.import_selected(ctx);
            self.importer.refresh.importing = self.importer.busy();
        }
        ctx.request_repaint();
    }

    /// Selects each recipe's source row for one batch, with the saved namespace that keeps its
    /// identity, and records the recipes no row can take. Returns the recipes that wait for a
    /// later batch.
    fn select_reimports(&mut self, source: Source, items: Vec<Item>) -> Vec<Item> {
        self.importer.selected.clear();
        self.importer.reimport.clear();
        let mut deferred = Vec::new();
        for item in items {
            let found = self
                .importer
                .weapons
                .iter()
                .find(|weapon| {
                    item.matches(source, weapon, self.importer.origins.get(&weapon.hash))
                })
                .ok_or(0)
                .or_else(|_| match source {
                    Source::D2Modern => item.named_row(&self.importer.weapons),
                    _ => Err(0),
                });
            match found {
                // Each recipe keeps its own saved namespace, so a second recipe of the same
                // source item waits for a batch of its own.
                Ok(weapon) if self.importer.selected.contains(&weapon.hash) => deferred.push(item),
                Ok(weapon) if self.importer.browser.importable(weapon) => {
                    self.importer.selected.insert(weapon.hash);
                    // The saved namespace reproduces the saved identity, so the import
                    // updates this recipe and keeps its edits. It brings ornaments only if
                    // the recipe had them.
                    self.importer.reimport.insert(
                        weapon.hash,
                        Saved {
                            namespace: item.namespace.clone(),
                            ornaments: item.ornaments,
                        },
                    );
                }
                Ok(_) => fail(
                    &mut self.log,
                    &mut self.importer.refresh,
                    format!("{}: no Shadowkeep weapon can host its type.", item.name),
                ),
                Err(0) => fail(
                    &mut self.log,
                    &mut self.importer.refresh,
                    format!(
                        "{}: no item in the current {} packages matches its recipe.",
                        item.name,
                        source.label()
                    ),
                ),
                Err(count) => fail(
                    &mut self.log,
                    &mut self.importer.refresh,
                    format!(
                        "{}: {count} {} items have this name, and its recipe doesn't record which one it came from. Import the right one from the importer.",
                        item.name,
                        source.label()
                    ),
                ),
            }
        }
        deferred
    }
}

/// What the review window asked for this frame.
#[derive(Default)]
struct Requests {
    refresh: bool,
    dismiss: bool,
    choose: Option<Source>,
    /// A new set of recipes to leave out, from All or In Build.
    excluded: Option<BTreeSet<PathBuf>>,
    /// Recipes checked or cleared one at a time, and whether each is now selected.
    changes: Vec<(PathBuf, bool)>,
}

fn selected(state: &State, item: &Item) -> bool {
    !state.excluded.contains(&item.path)
}

/// Every pending recipe, or only those the build includes.
fn choices(
    ui: &mut egui::Ui,
    state: &State,
    outside_build: &BTreeSet<PathBuf>,
    requests: &mut Requests,
) {
    let in_build = state.pending.len() - outside_build.len();
    ui.horizontal(|ui| {
        let all = format!("All · {}", state.pending.len());
        if ui
            .add(egui::Button::new(all).selected(state.excluded.is_empty()))
            .clicked()
        {
            requests.excluded = Some(BTreeSet::new());
        }
        let build = egui::Button::new(format!("In Build · {in_build}"))
            .selected(in_build > 0 && state.excluded == *outside_build);
        if ui.add_enabled(in_build > 0, build).clicked() {
            requests.excluded = Some(outside_build.clone());
        }
    });
}

/// One source's row: a checkbox for all of its recipes, its count and any folder it needs, and
/// its recipes when opened.
fn group(
    ui: &mut egui::Ui,
    source: Option<Source>,
    items: &[&Item],
    state: &State,
    missing: &dyn Fn(Source) -> bool,
    requests: &mut Requests,
) {
    let label = source.map_or("Earlier Importer", Source::label);
    let muted = style::secondary(ui.visuals());
    let picked = items.iter().filter(|item| selected(state, item)).count();
    style::block(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let id = egui::Id::new(("import-refresh", label));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| {
                let mut all = picked == items.len();
                let some = picked > 0 && picked < items.len();
                if ui
                    .add(egui::Checkbox::without_text(&mut all).indeterminate(some))
                    .changed()
                {
                    requests
                        .changes
                        .extend(items.iter().map(|item| (item.path.clone(), all)));
                }
                ui.strong(label);
                ui.label(egui::RichText::new(format!("{picked} of {}", items.len())).color(muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    note(ui, source, missing, requests);
                });
            })
            .body(|ui| {
                // Only the rows in view are laid out. A source can hold thousands of recipes.
                let row_height = ui
                    .spacing()
                    .interact_size
                    .y
                    .max(ui.text_style_height(&egui::TextStyle::Body));
                egui::ScrollArea::vertical()
                    .id_salt(("import-refresh-names", label))
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, items.len(), |ui, rows| {
                        for item in &items[rows] {
                            let mut on = selected(state, item);
                            if ui.checkbox(&mut on, &item.name).changed() {
                                requests.changes.push((item.path.clone(), on));
                            }
                        }
                    });
            });
    });
}

/// The folder a source still needs, or where recipes of no known source reimport.
fn note(
    ui: &mut egui::Ui,
    source: Option<Source>,
    missing: &dyn Fn(Source) -> bool,
    requests: &mut Requests,
) {
    match source {
        Some(source) if missing(source) => {
            if ui.button("Choose Folder…").clicked() {
                requests.choose = Some(source);
            }
        }
        Some(_) => {}
        None => {
            ui.label(
                egui::RichText::new("Import again in the importer")
                    .color(style::secondary(ui.visuals())),
            );
        }
    }
}

fn actions(ui: &mut egui::Ui, state: &State, requests: &mut Requests) {
    let chosen = state
        .pending
        .iter()
        .filter(|item| selected(state, item))
        .count();
    let label = if chosen == 1 {
        "Reimport 1 Recipe".to_owned()
    } else {
        format!("Reimport {chosen} Recipes")
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
        requests.refresh = ui
            .add_enabled(chosen > 0, style::primary(ui, &label))
            .clicked();
        requests.dismiss = ui.button("Later").clicked();
    });
}
