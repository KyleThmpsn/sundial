//! Checked component selection and explicit shared-owner effects. Package scans run off the UI thread.
use super::*;
use crate::runtime::compatibility::{
    ComponentCompatibilityReport, ComponentDonorAssessment, DonorCompatibility,
    assess_component_donors,
};
mod preview;
use preview::{Review, ReviewJob};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Browser {
    picker: Option<Picker>,
    job: Option<Job>,
    generation: u64,
    reports: BTreeMap<u32, (RuntimeGraphKey, Arc<ComponentCompatibilityReport>)>,
    reviews: BTreeMap<(u32, u32), Arc<Review>>,
    review_job: Option<ReviewJob>,
    last_change: Option<(WeaponRecipe, WeaponRecipe)>,
    listing: Option<CachedListing>,
}

/// The donors a search matches, kept until the search, the report or the donor list changes.
/// The report is held weakly, so a new report cannot reuse its address.
struct CachedListing {
    query: String,
    report: std::sync::Weak<ComponentCompatibilityReport>,
    donors: (usize, usize),
    listing: Arc<Listing>,
}

/// The donors one search matches.
struct Listing {
    /// Positions in the donor list, one list per status in `status_rank` order, each by name.
    rows: [Vec<usize>; 3],
    /// Names more than one donor has. Their rows add the hash to tell them apart.
    repeated: BTreeSet<String>,
}

impl Listing {
    /// The rows of one status tab.
    fn for_status(&self, status: DonorCompatibility) -> &[usize] {
        &self.rows[usize::from(status_rank(status))]
    }
}

struct Picker {
    binding_hash: u32,
    key: Option<RuntimeGraphKey>,
    baseline_hash: Option<u32>,
    query: String,
    /// The status tab, chosen when the report arrives.
    status: Option<DonorCompatibility>,
    selected: Option<u32>,
    /// Selects `selected` in the list and scrolls to it on the next draw.
    reveal: bool,
    error: Option<String>,
    reset_unsupported: bool,
    reviewing: Option<(u32, WeaponRecipe)>,
}

impl Picker {
    fn new(
        binding_hash: u32,
        key: Option<RuntimeGraphKey>,
        baseline_hash: Option<u32>,
        query: String,
        selected: Option<u32>,
    ) -> Self {
        Self {
            binding_hash,
            key,
            baseline_hash,
            query,
            status: None,
            selected,
            reveal: true,
            error: None,
            reset_unsupported: false,
            reviewing: None,
        }
    }
}

struct Job {
    binding_hash: u32,
    key: RuntimeGraphKey,
    generation: u64,
    receiver: Receiver<Result<ComponentCompatibilityReport, String>>,
    worker: thread::JoinHandle<()>,
}

impl Browser {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some() || self.review_job.is_some()
    }

    pub(super) fn close(&mut self) {
        self.picker = None;
        // A closing window does not detach a worker holding package handles.
    }

    pub(super) fn invalidate(&mut self) {
        self.close();
        self.generation = self.generation.wrapping_add(1);
        self.reports.clear();
        self.reviews.clear();
        self.listing = None;
    }

    /// The donors `query` matches, rebuilt only when the search, the report or the donor list
    /// changes.
    fn listing_for(
        &mut self,
        donors: &[WeaponDonorSummary],
        report: &Arc<ComponentCompatibilityReport>,
        query: &str,
    ) -> Arc<Listing> {
        let donor_list = (donors.as_ptr() as usize, donors.len());
        if let Some(cached) = self.listing.as_ref().filter(|cached| {
            cached.query == query
                && cached.donors == donor_list
                && std::ptr::eq(cached.report.as_ptr(), Arc::as_ptr(report))
        }) {
            return Arc::clone(&cached.listing);
        }
        let listing = Arc::new(collect_listing(donors, report, query));
        self.listing = Some(CachedListing {
            query: query.to_owned(),
            report: Arc::downgrade(report),
            donors: donor_list,
            listing: Arc::clone(&listing),
        });
        listing
    }
}

/// The statuses in tab order.
const STATUSES: [DonorCompatibility; 3] = [
    DonorCompatibility::LowerRisk,
    DonorCompatibility::Experimental,
    DonorCompatibility::Incompatible,
];

const MIXING_WARNING: &str = "Component mixing can crash the game.";

fn status_label(status: DonorCompatibility) -> &'static str {
    match status {
        DonorCompatibility::LowerRisk => "Lower Risk",
        DonorCompatibility::Experimental => "Experimental Match",
        DonorCompatibility::Incompatible => "Rejected Donor",
    }
}

/// A status as its tab names it.
fn tab_label(status: DonorCompatibility) -> &'static str {
    match status {
        DonorCompatibility::LowerRisk => "Lower Risk",
        DonorCompatibility::Experimental => "Experimental",
        DonorCompatibility::Incompatible => "Rejected",
    }
}

fn status_color(ui: &egui::Ui, status: DonorCompatibility) -> egui::Color32 {
    match status {
        DonorCompatibility::LowerRisk => crate::app::style::success_color(ui.visuals()),
        DonorCompatibility::Experimental => ui.visuals().warn_fg_color,
        DonorCompatibility::Incompatible => ui.visuals().error_fg_color,
    }
}

fn status_rank(status: DonorCompatibility) -> u8 {
    match status {
        DonorCompatibility::LowerRisk => 0,
        DonorCompatibility::Experimental => 1,
        DonorCompatibility::Incompatible => 2,
    }
}

/// Whether a donor of `status` can apply. The status sorts donors, and only a rejected one is
/// refused.
fn can_apply(status: DonorCompatibility) -> bool {
    status != DonorCompatibility::Incompatible
}

fn binding_label(binding_hash: u32) -> String {
    runtime_component_control(binding_hash).map_or_else(
        || format!("Binding 0x{binding_hash:08X}"),
        |control| control.label.to_owned(),
    )
}

/// A donor's name, or its hash when the donor list does not have it.
fn donor_name(donor_summaries: &[WeaponDonorSummary], hash: u32) -> String {
    donor_summaries
        .iter()
        .find(|donor| donor.hash == hash)
        .map_or_else(|| format!("0x{hash:08X}"), |donor| donor.name.clone())
}

/// One runtime component, as a row or a detail pane draws it.
pub(super) struct ComponentRow<'a> {
    pub(super) binding_hash: u32,
    pub(super) label: &'a str,
    /// What the component does, empty for a binding Parhelion does not name.
    pub(super) tooltip: &'a str,
    pub(super) baseline_hash: Option<u32>,
    pub(super) current_key: Option<&'a RuntimeGraphKey>,
}

impl ComponentRow<'_> {
    /// The name's hover: what the component does and its binding hash.
    fn hover(&self) -> String {
        if self.tooltip.is_empty() {
            format!("0x{:08X}", self.binding_hash)
        } else {
            format!("{}\n0x{:08X}", self.tooltip, self.binding_hash)
        }
    }
}

/// What a component's row shows and what its actions need.
struct ComponentState {
    source: String,
    /// The donor the recipe saved for the component.
    chosen: Option<u32>,
    /// Whether a saved choice or a shared owner moved the component off its baseline.
    off_baseline: bool,
    /// "Requested: X" when the saved donor is not the one the runtime uses.
    requested: Option<String>,
}

/// Whether a saved choice has no baseline to return to, so it can only be removed.
fn removable(row: &ComponentRow<'_>, state: &ComponentState) -> bool {
    row.baseline_hash.is_none() && state.chosen.is_some()
}

/// Where a component's runtime comes from now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ComponentSource {
    /// No runtime has been read for the current recipe.
    NotLoaded,
    /// The runtime has no such binding.
    Absent,
    /// The donors feeding the binding.
    Donors(Vec<SourceDonor>),
}

/// One donor feeding a component.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SourceDonor {
    /// The donor item, or `None` for a baseline no donor names.
    pub(super) hash: Option<u32>,
    pub(super) route: SourceRoute,
}

/// How a donor reaches a component.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum SourceRoute {
    /// The runtime's own component.
    Baseline,
    /// The recipe's choice for this component.
    Chosen,
    /// The recipe's choice for another binding that shares this one's owner.
    Shared(u32),
}

impl PackageAuthoringApp {
    /// One component as a part row: its name, and where it comes from now as a value that opens
    /// the compatibility review. Off its baseline, the reset opens the review on the baseline. A
    /// saved choice the runtime does not follow shows under it.
    pub(super) fn draw_runtime_component_row(&mut self, ui: &mut egui::Ui, row: &ComponentRow<'_>) {
        use crate::app::donor_view::parts;
        let state = self.component_state(row);
        let hover = row.hover();
        let part = parts::Part {
            column: &parts::GAMEPLAY_COLUMN,
            label: row.label,
            hint: hover.into(),
            value: &state.source,
            chosen: state.chosen,
            follow: "Use Baseline",
            blocked: None,
            detail: None,
        };
        let resettable = row.baseline_hash.is_some() && state.off_baseline;
        let (open, reset) = parts::draw_window_part(
            ui,
            self.catalog.as_ref(),
            &part,
            (state.off_baseline, resettable),
        );
        if open {
            self.review_component(row, state.chosen, false);
        } else if reset {
            self.review_component(row, row.baseline_hash, true);
        }
        if let Some(requested) = &state.requested {
            let color = crate::app::style::secondary(ui.visuals());
            parts::note(
                ui,
                &parts::GAMEPLAY_COLUMN,
                egui::RichText::new(requested).color(color),
            );
        }
        if removable(row, &state) {
            ui.horizontal(|ui| {
                ui.add_space(Self::component_label_width(ui) + ui.spacing().item_spacing.x);
                self.draw_remove_saved_choice(ui, row);
            });
        }
    }

    /// The selected binding in Advanced Runtime Bindings: what it is, where it comes from now,
    /// and its actions.
    pub(super) fn draw_runtime_component_detail(
        &mut self,
        ui: &mut egui::Ui,
        row: &ComponentRow<'_>,
    ) {
        let state = self.component_state(row);
        let secondary = crate::app::style::secondary(ui.visuals());
        let heading = ui.heading(row.label);
        if !row.tooltip.is_empty() {
            heading.on_hover_text(row.tooltip);
        }
        ui.label(egui::RichText::new(format!("0x{:08X}", row.binding_hash)).color(secondary));
        section(ui, "Source");
        ui.label(&state.source);
        if let Some(requested) = &state.requested {
            ui.label(egui::RichText::new(requested).color(secondary));
        }
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| self.draw_component_actions(ui, row, &state));
    }

    /// Where a component comes from now, as its row says it.
    pub(super) fn component_source_text(
        &self,
        binding_hash: u32,
        baseline_hash: Option<u32>,
        current_key: Option<&RuntimeGraphKey>,
    ) -> String {
        self.source_text(&self.component_source(binding_hash, baseline_hash, current_key))
    }

    fn component_state(&self, row: &ComponentRow<'_>) -> ComponentState {
        let source = self.component_source(row.binding_hash, row.baseline_hash, row.current_key);
        let chosen = self
            .recipe
            .runtime_component_donor(row.binding_hash)
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let donors = match &source {
            ComponentSource::Donors(donors) => donors.as_slice(),
            ComponentSource::NotLoaded | ComponentSource::Absent => &[],
        };
        let requested = chosen
            .filter(|&hash| {
                !donors.contains(&SourceDonor {
                    hash: Some(hash),
                    route: SourceRoute::Chosen,
                })
            })
            .map(|hash| format!("Requested: {}", donor_name(&self.donor_summaries, hash)));
        ComponentState {
            source: self.source_text(&source),
            chosen,
            off_baseline: chosen.is_some()
                || donors
                    .iter()
                    .any(|donor| donor.route != SourceRoute::Baseline),
            requested,
        }
    }

    /// A detail pane's actions: Change…, and a way back to the baseline once the component is
    /// off it.
    fn draw_component_actions(
        &mut self,
        ui: &mut egui::Ui,
        row: &ComponentRow<'_>,
        state: &ComponentState,
    ) {
        let change = ui.add_enabled(
            self.catalog.is_some(),
            crate::app::style::primary(ui, "Change…"),
        );
        if named_control(change, format!("Change {} Donor", row.label)).clicked() {
            self.review_component(row, state.chosen, false);
        }
        if row.baseline_hash.is_some()
            && state.off_baseline
            && named_control(
                ui.small_button("Use Baseline…"),
                format!("Use Baseline for {}", row.label),
            )
            .clicked()
        {
            self.review_component(row, row.baseline_hash, true);
        }
        if removable(row, state) {
            self.draw_remove_saved_choice(ui, row);
        }
    }

    /// Opens the compatibility review for `row` on `selected`: the saved choice with the row's
    /// last search, or the baseline it offers to restore.
    fn review_component(&mut self, row: &ComponentRow<'_>, selected: Option<u32>, baseline: bool) {
        let query = if baseline {
            String::new()
        } else {
            self.runtime_component_queries
                .get(&row.binding_hash)
                .cloned()
                .unwrap_or_default()
        };
        self.runtime_donors.picker = Some(Picker::new(
            row.binding_hash,
            row.current_key.cloned(),
            row.baseline_hash,
            query,
            selected,
        ));
    }

    /// Remove Saved Choice, which drops a saved choice that has no baseline to return to.
    fn draw_remove_saved_choice(&mut self, ui: &mut egui::Ui, row: &ComponentRow<'_>) {
        if named_control(
            ui.small_button("Remove Saved Choice"),
            format!("Remove Saved Choice for {}", row.label),
        )
        .clicked()
        {
            self.recipe
                .set_runtime_component_donor(row.binding_hash, None);
            self.runtime_graph = None;
            self.runtime_value_text.clear();
            self.runtime_donors.invalidate();
        }
    }

    /// Where a component's runtime comes from now: the compatibility report's sources when one
    /// was read for this runtime, otherwise the owners the runtime graph shows.
    fn component_source(
        &self,
        binding_hash: u32,
        baseline_hash: Option<u32>,
        current_key: Option<&RuntimeGraphKey>,
    ) -> ComponentSource {
        if let Some(sources) = self
            .runtime_donors
            .reports
            .values()
            .filter(|(key, report)| Some(key) == current_key && report.current_error.is_none())
            .find_map(|(_, report)| report.current_sources.get(&binding_hash))
        {
            return ComponentSource::Donors(
                sources
                    .iter()
                    .map(|source| {
                        self.source_donor(
                            binding_hash,
                            source.donor_item_hash,
                            source.via_binding_hash,
                        )
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            );
        }
        let Some((_, graph)) = self
            .runtime_graph
            .as_ref()
            .filter(|(key, _)| Some(key) == current_key)
        else {
            return ComponentSource::NotLoaded;
        };
        let owners = graph
            .bindings
            .iter()
            .filter(|binding| binding.binding_hash == binding_hash)
            .map(|binding| binding.owner_tag)
            .collect::<BTreeSet<_>>();
        if owners.is_empty() {
            return ComponentSource::Absent;
        }
        let mut sources = BTreeSet::new();
        for owner in owners {
            let requests = self
                .recipe
                .runtime_component_donors
                .iter()
                .filter_map(|component| {
                    let via = component.binding_hash.parse_u32().ok()?;
                    let hash = component.donor.item_hash.parse_u32().ok()?;
                    let donor_pattern = self
                        .donor_summaries
                        .iter()
                        .find(|donor| donor.hash == hash)
                        .and_then(|donor| donor.weapon_pattern_index);
                    if Some(hash) == baseline_hash
                        || donor_pattern.is_some_and(|pattern| {
                            current_key.is_some_and(|key| key.pattern_index == Some(pattern))
                        })
                    {
                        return None;
                    }
                    graph
                        .bindings
                        .iter()
                        .any(|binding| binding.binding_hash == via && binding.owner_tag == owner)
                        .then_some((via, hash))
                })
                .collect::<Vec<_>>();
            if requests.is_empty() {
                sources.insert(SourceDonor {
                    hash: baseline_hash,
                    route: SourceRoute::Baseline,
                });
            } else {
                let mut donors = BTreeMap::new();
                for (via, hash) in requests {
                    donors.entry(hash).or_insert(via);
                }
                for (hash, via) in donors {
                    sources.insert(self.source_donor(binding_hash, hash, Some(via)));
                }
            }
        }
        ComponentSource::Donors(sources.into_iter().collect())
    }

    /// How donor `hash` feeds `binding`: as the baseline when it arrives through no binding,
    /// otherwise as the recipe's choice for `binding` or through the binding `via`.
    fn source_donor(&self, binding: u32, hash: u32, via: Option<u32>) -> SourceDonor {
        let route = match via {
            None => SourceRoute::Baseline,
            Some(_)
                if self
                    .recipe
                    .runtime_component_donor(binding)
                    .is_some_and(|donor| donor.item_hash.parse_u32() == Ok(hash)) =>
            {
                SourceRoute::Chosen
            }
            Some(via) => SourceRoute::Shared(via),
        };
        SourceDonor {
            hash: Some(hash),
            route,
        }
    }

    /// A component's source as text: "Age-Old Bond", "Ace of Spades" or "Ace of Spades via
    /// Weapon Stat Translator". The baseline reads as the weapon the runtime starts from, as the
    /// other part rows do, not the item that owns its pattern row, which can be a hidden one.
    fn source_text(&self, source: &ComponentSource) -> String {
        let baseline = self
            .runtime_base()
            .map(|hash| donor_name(&self.donor_summaries, hash));
        match source {
            ComponentSource::NotLoaded => "Not Loaded".to_owned(),
            ComponentSource::Absent => "Not in This Runtime".to_owned(),
            ComponentSource::Donors(donors) => donors
                .iter()
                .map(|donor| {
                    let name = donor.hash.map_or_else(
                        || "Runtime Baseline".to_owned(),
                        |hash| donor_name(&self.donor_summaries, hash),
                    );
                    match donor.route {
                        SourceRoute::Baseline => baseline.clone().unwrap_or(name),
                        SourceRoute::Chosen => name,
                        SourceRoute::Shared(via) => format!("{name} via {}", binding_label(via)),
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub(super) fn poll_runtime_donors(&mut self) {
        self.poll_runtime_swap();
        let Some(job) = &self.runtime_donors.job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The donor compatibility scan stopped without a result.".into())
            }
        };
        let job = self.runtime_donors.job.take().expect("job checked above");
        let joined = job.worker.join();
        if job.generation != self.runtime_donors.generation
            || Some(&job.key) != self.runtime_graph_key().as_ref()
        {
            return;
        }
        let result = if joined.is_err() {
            Err("The donor compatibility scan failed unexpectedly.".into())
        } else {
            result
        }
        .and_then(|report| {
            if report.binding_hash == job.binding_hash {
                Ok(report)
            } else {
                Err("The donor compatibility scan returned a different binding.".into())
            }
        });
        match result {
            Ok(report) => {
                self.runtime_donors
                    .reports
                    .insert(job.binding_hash, (job.key, Arc::new(report)));
            }
            Err(error) => {
                if let Some(picker) = self
                    .runtime_donors
                    .picker
                    .as_mut()
                    .filter(|picker| picker.binding_hash == job.binding_hash)
                {
                    picker.error = Some(error);
                }
            }
        }
    }

    fn ensure_runtime_donor_report(&mut self, ctx: &egui::Context) {
        let key = self.runtime_graph_key();
        let Some(picker) = self.runtime_donors.picker.as_mut() else {
            return;
        };
        if picker.key != key {
            picker.selected = None;
            picker.error = Some("The runtime changed. Reopen this window.".into());
            return;
        }
        let Some(key) = key else {
            picker.error = Some("Choose a runtime first.".into());
            return;
        };
        if picker.error.is_some()
            || self.runtime_donors.job.is_some()
            || self
                .runtime_donors
                .reports
                .get(&picker.binding_hash)
                .is_some_and(|(loaded, _)| *loaded == key)
        {
            return;
        }
        let binding_hash = picker.binding_hash;
        let packages = self.packages.clone();
        let donors = self.donor_summaries.clone();
        let worker_key = key.clone();
        let worker_context = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = assess_component_donors(&packages, &worker_key, binding_hash, &donors);
            let _ = sender.send(result);
            worker_context.request_repaint();
        });
        self.runtime_donors.job = Some(Job {
            binding_hash,
            key,
            generation: self.runtime_donors.generation,
            receiver,
            worker,
        });
    }

    /// Draws the body of the donor browser: the current source, the search and status tabs, and
    /// the donors beside the selected one. Returns true when a rescan was requested.
    fn draw_donor_browser_body(
        &mut self,
        ui: &mut egui::Ui,
        picker: &mut Picker,
        report: Option<&Arc<ComponentCompatibilityReport>>,
        current_key: Option<&RuntimeGraphKey>,
        review: Option<&Review>,
    ) -> bool {
        if let Some(error) = &picker.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return picker.key.as_ref() == current_key && ui.button("Retry Scan").clicked();
        }
        let Some(shared_report) = report else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking donors…");
            });
            return false;
        };
        let report: &ComponentCompatibilityReport = shared_report;
        let source =
            self.component_source_text(picker.binding_hash, picker.baseline_hash, current_key);
        draw_header(ui, &source);
        if let Some(error) = &report.current_error {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Current combination needs repair.",
            )
            .on_hover_text(error);
        }
        ui.add_space(4.0);
        // A search remembered from the last review must not hide the donor this one opened on,
        // or the list would select another donor in its place.
        if picker.reveal
            && let Some(hash) = picker.selected
            && report.candidates.contains_key(&hash)
            && !self
                .runtime_donors
                .listing_for(&self.donor_summaries, shared_report, &picker.query)
                .rows
                .iter()
                .flatten()
                .any(|&position| self.donor_summaries[position].hash == hash)
        {
            picker.query.clear();
        }
        let listing =
            self.runtime_donors
                .listing_for(&self.donor_summaries, shared_report, &picker.query);
        let opened = picker.status.is_none();
        if opened {
            picker.status = Some(opening_status(report, &listing, picker.selected));
        }
        let changed = draw_toolbar(ui, picker, &listing, report, opened);
        // A search typed this frame lists different donors.
        let listing =
            self.runtime_donors
                .listing_for(&self.donor_summaries, shared_report, &picker.query);
        let rows = listing.for_status(picker.status.unwrap_or(DonorCompatibility::LowerRisk));
        ui.add_space(4.0);
        if rows.is_empty() {
            picker.selected = None;
        }
        let keys = rows
            .iter()
            .map(|&position| u64::from(self.donor_summaries[position].hash))
            .collect::<Vec<_>>();
        if picker.reveal && picker.selected.is_none() {
            // A review opened without a donor starts on the first row, not on the row the last
            // review of another component left selected.
            let id = ui.make_persistent_id("inspected-choice");
            ui.data_mut(|data| data.remove::<u64>(id));
        }
        let select = picker.selected.filter(|_| picker.reveal).map(u64::from);
        let reset = changed || picker.reveal;
        picker.reveal = false;
        let donors = &self.donor_summaries;
        let catalog = self.catalog.as_ref();
        let baseline = picker.baseline_hash;
        let retry_settings = crate::app::pickers::BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(160.0),
            reset,
            row_height: sundial::investment::authoring_choice_row_height(ui),
            select,
        }
        .draw_body(
            ui,
            |ui, index, selected| {
                let donor = &donors[rows[index]];
                let detail = row_detail(
                    donor,
                    listing.repeated.contains(&donor.name),
                    baseline == Some(donor.hash),
                );
                draw_donor_row(ui, catalog, donor, &detail, selected)
            },
            |ui, index| {
                let donor = &donors[rows[index]];
                let assessment = report.candidates.get(&donor.hash)?;
                if picker.selected != Some(donor.hash) {
                    picker.selected = Some(donor.hash);
                    picker.reset_unsupported = false;
                }
                draw_donor_detail(ui, donor, assessment, report, picker, review)
                    .then_some(donor.hash)
            },
        );
        if let Some(hash) = retry_settings {
            self.runtime_donors
                .reviews
                .remove(&(picker.binding_hash, hash));
        }
        false
    }

    pub(super) fn draw_runtime_donor_browser(&mut self, ctx: &egui::Context) {
        if !self.show_experimental_options
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        self.ensure_runtime_donor_report(ctx);
        self.ensure_runtime_swap(ctx);
        let Some(mut picker) = self.runtime_donors.picker.take() else {
            return;
        };
        let current_key = self.runtime_graph_key();
        let review = self.current_runtime_swap(&picker);
        let report = self
            .runtime_donors
            .reports
            .get(&picker.binding_hash)
            .filter(|(key, _)| {
                Some(key) == current_key.as_ref() && Some(key) == picker.key.as_ref()
            })
            .map(|(_, report)| Arc::clone(report));
        let mut open = true;
        let mut apply = false;
        let mut retry = false;
        let screen = ctx.content_rect();
        let width = (screen.width() - 40.0).clamp(280.0, 900.0);
        let height = (screen.height() - 64.0).clamp(240.0, 780.0);
        egui::Window::new(format!("{} Donors", binding_label(picker.binding_hash)))
            .id(egui::Id::new("parhelion-runtime-donor-browser"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(width)
            .default_height(height)
            .min_width(width.min(480.0))
            .min_height(height.min(360.0))
            .show(ctx, |ui| {
                workbench_style(ui);
                // The footer holds Apply Donor wherever the body is, so the body takes the
                // rest of the window.
                let footer_height =
                    ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0 + 8.0;
                let body_height = (ui.available_height() - footer_height).max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_height(body_height);
                        retry = self.draw_donor_browser_body(
                            ui,
                            &mut picker,
                            report.as_ref(),
                            current_key.as_ref(),
                            review.as_deref(),
                        );
                    },
                );
                let selected_assessment = picker
                    .selected
                    .and_then(|hash| report.as_ref()?.candidates.get(&hash));
                let apply_enabled = picker.error.is_none()
                    && selected_assessment.is_some_and(|assessment| {
                        can_apply(assessment.status)
                            && preview::can_apply(review.as_deref(), &self.recipe, &picker)
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    apply = ui
                        .add_enabled(apply_enabled, crate::app::style::primary(ui, "Apply Donor"))
                        .clicked();
                    if let Some(hint) = donor_apply_hint(
                        &picker,
                        report.is_some(),
                        selected_assessment,
                        review.as_deref(),
                        apply_enabled,
                    ) {
                        ui.label(
                            egui::RichText::new(hint)
                                .color(crate::app::style::secondary(ui.visuals())),
                        );
                    }
                });
            });
        self.runtime_component_queries
            .insert(picker.binding_hash, picker.query.clone());
        if retry {
            picker.error = None;
            self.runtime_donors.reports.remove(&picker.binding_hash);
        }
        let mut applied = false;
        if apply
            && picker.key == self.runtime_graph_key()
            && let Some(assessment) = picker
                .selected
                .and_then(|hash| report.as_ref()?.candidates.get(&hash))
            && can_apply(assessment.status)
            && preview::can_apply(review.as_deref(), &self.recipe, &picker)
            && let Some(Review {
                result: Ok(plan), ..
            }) = review.as_deref()
        {
            self.runtime_donors.last_change = Some((self.recipe.clone(), plan.after.clone()));
            self.recipe = plan.after.clone();
            self.runtime_graph = None;
            self.runtime_value_text.clear();
            self.runtime_donors.invalidate();
            applied = true;
        }
        if open && !applied {
            self.runtime_donors.picker = Some(picker);
        }
    }
}

/// The donor the component comes from now, with the crash warning on the same line when it
/// fits and under it when it does not.
fn draw_header(ui: &mut egui::Ui, sources: &str) {
    let secondary = crate::app::style::secondary(ui.visuals());
    let warn = ui.visuals().warn_fg_color;
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let text_width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font_id.clone(), warn)
            .size()
            .x
    };
    let fits = text_width("Current Source")
        + text_width(sources)
        + text_width(MIXING_WARNING)
        + ui.spacing().item_spacing.x * 2.0
        + 24.0
        <= ui.available_width();
    if fits {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Current Source").color(secondary));
            ui.label(sources);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(warn, MIXING_WARNING);
            });
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Current Source").color(secondary));
            ui.label(sources);
        });
        ui.colored_label(warn, MIXING_WARNING);
    }
}

/// A status tab's name and how many donors it holds. The count is in the secondary colour except
/// on the selected tab, whose accent fill the secondary colour does not read on.
fn tab_text(
    ui: &egui::Ui,
    status: DonorCompatibility,
    count: usize,
    selected: bool,
) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        tab_label(status),
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            // The tab's own text colour, which follows its hover and selected states.
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job.append(
        &format!(" {count}"),
        0.0,
        egui::TextFormat {
            font_id,
            color: if selected {
                egui::Color32::PLACEHOLDER
            } else {
                crate::app::style::secondary(ui.visuals())
            },
            ..Default::default()
        },
    );
    job
}

/// The search and the status tabs, each tab counting the donors the search matches. Returns
/// true when the list to show changed.
fn draw_toolbar(
    ui: &mut egui::Ui,
    picker: &mut Picker,
    listing: &Listing,
    report: &ComponentCompatibilityReport,
    opened: bool,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let width = (ui.available_width() * 0.35).clamp(160.0, 280.0);
        changed |=
            sundial::ui::catalog::search(ui, &mut picker.query, opened, width, "Search Weapons");
        ui.add_space(12.0);
        for status in STATUSES {
            let count = listing.for_status(status).len();
            let selected = picker.status == Some(status);
            if ui
                .selectable_label(selected, tab_text(ui, status, count, selected))
                .clicked()
                && picker.status != Some(status)
            {
                picker.status = Some(status);
                changed = true;
            }
        }
        if let Some(hash) = picker.baseline_hash
            && picker.selected != Some(hash)
            && let Some(assessment) = report.candidates.get(&hash)
        {
            ui.add_space(12.0);
            if ui.button("Select Baseline").clicked() {
                picker.query.clear();
                picker.status = Some(assessment.status);
                picker.selected = Some(hash);
                picker.reveal = true;
                picker.reset_unsupported = false;
                changed = true;
            }
        }
    });
    changed
}

/// The tab a review opens on: the selected donor's, else the first that lists any donor.
fn opening_status(
    report: &ComponentCompatibilityReport,
    listing: &Listing,
    selected: Option<u32>,
) -> DonorCompatibility {
    selected
        .and_then(|hash| report.candidates.get(&hash))
        .map(|assessment| assessment.status)
        .or_else(|| {
            STATUSES
                .into_iter()
                .find(|&status| !listing.for_status(status).is_empty())
        })
        .unwrap_or(DonorCompatibility::LowerRisk)
}

/// The donors matching `query`, by status and then name, as positions in `donor_summaries`.
fn collect_listing(
    donor_summaries: &[WeaponDonorSummary],
    report: &ComponentCompatibilityReport,
    query: &str,
) -> Listing {
    let query = query.trim().to_ascii_lowercase();
    let mut rows: [Vec<usize>; 3] = Default::default();
    for (position, donor) in donor_summaries.iter().enumerate() {
        let Some(assessment) = report.candidates.get(&donor.hash) else {
            continue;
        };
        if query.is_empty()
            || donor.name.to_ascii_lowercase().contains(&query)
            || donor.type_name.to_ascii_lowercase().contains(&query)
            || format!("0x{:08x}", donor.hash).contains(&query)
        {
            rows[usize::from(status_rank(assessment.status))].push(position);
        }
    }
    for rows in &mut rows {
        rows.sort_by(|&left, &right| {
            let (left, right) = (&donor_summaries[left], &donor_summaries[right]);
            left.name
                .cmp(&right.name)
                .then_with(|| left.hash.cmp(&right.hash))
        });
    }
    let mut seen = BTreeSet::new();
    let repeated = donor_summaries
        .iter()
        .filter(|donor| !seen.insert(donor.name.as_str()))
        .map(|donor| donor.name.clone())
        .collect();
    Listing { rows, repeated }
}

/// A row's line under the name: the weapon type, the hash when another donor has the same
/// name, and whether it is the runtime baseline.
fn row_detail(donor: &WeaponDonorSummary, repeated: bool, baseline: bool) -> String {
    let hash = format!("0x{:08X}", donor.hash);
    [
        Some(donor.type_name.as_str()).filter(|type_name| !type_name.is_empty()),
        repeated.then_some(hash.as_str()),
        baseline.then_some("Baseline"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// One donor in the list, with its weapon icon when the catalog has one.
fn draw_donor_row(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    donor: &WeaponDonorSummary,
    detail: &str,
    selected: bool,
) -> egui::Response {
    match catalog {
        Some(catalog) => catalog.draw_authoring_choice_row(
            ui,
            Some(donor.hash),
            &donor.name,
            Some(detail),
            selected,
        ),
        None => sundial::investment::draw_asset_choice_row_plain(ui, &donor.name, detail, selected),
    }
}

/// A titled part of the selected donor's details.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.strong(title);
}

/// The selected donor: its status and the reasons for it, what applying it changes and how
/// your settings carry over. Returns true when its settings check should run again.
fn draw_donor_detail(
    ui: &mut egui::Ui,
    donor: &WeaponDonorSummary,
    assessment: &ComponentDonorAssessment,
    report: &ComponentCompatibilityReport,
    picker: &mut Picker,
    review: Option<&Review>,
) -> bool {
    ui.heading(&donor.name);
    ui.label(
        egui::RichText::new(format!("{} · 0x{:08X}", donor.type_name, donor.hash))
            .color(crate::app::style::secondary(ui.visuals())),
    );
    ui.colored_label(
        status_color(ui, assessment.status),
        status_label(assessment.status),
    );
    if !assessment.reasons.is_empty() {
        section(ui, "Compatibility");
        for reason in &assessment.reasons {
            ui.label(reason);
        }
    }
    if assessment
        .affected_bindings
        .iter()
        .any(|&affected| affected != picker.binding_hash)
    {
        section(ui, "Changes Together");
        for affected in &assessment.affected_bindings {
            let label = report
                .affected_bindings
                .iter()
                .find(|(hash, _)| hash == affected)
                .map_or_else(|| binding_label(*affected), |(_, label)| label.clone());
            ui.label(label).on_hover_text(format!("0x{affected:08X}"));
        }
        ui.weak("Other donor choices in this group will be replaced.");
    }
    if assessment.status == DonorCompatibility::Incompatible {
        return false;
    }
    let review = review.filter(|review| Some(review.donor_hash) == picker.selected);
    preview::draw_review(ui, picker, review)
}

/// Names what still blocks Apply Donor. Nothing while Apply is enabled, or while the blocker
/// shows beside it already: a rejected status or a failed check shown with the review.
fn donor_apply_hint(
    picker: &Picker,
    has_report: bool,
    selected_assessment: Option<&ComponentDonorAssessment>,
    review: Option<&Review>,
    apply_enabled: bool,
) -> Option<&'static str> {
    if apply_enabled {
        return None;
    }
    if picker.error.is_some() {
        return Some("Resolve the scan error before applying.");
    }
    if !has_report {
        return Some("Waiting for the compatibility scan.");
    }
    let Some(assessment) = selected_assessment else {
        return Some("Select a donor.");
    };
    match assessment.status {
        DonorCompatibility::Incompatible => None,
        _ => settings_hint(picker, review),
    }
}

/// What the settings check still needs before Apply Donor. A failed check and a recipe
/// conflict are shown with the review, so they add nothing here.
fn settings_hint(picker: &Picker, review: Option<&Review>) -> Option<&'static str> {
    let Some(review) = review.filter(|review| Some(review.donor_hash) == picker.selected) else {
        return Some("Checking settings…");
    };
    match &review.result {
        Ok(plan)
            if plan.error.is_none() && !plan.resets.is_empty() && !picker.reset_unsupported =>
        {
            Some("Confirm Reset Listed Edits.")
        }
        _ => None,
    }
}
