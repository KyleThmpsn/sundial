//! Checked component selection and explicit shared-owner effects. Package scans run off the UI thread.
use super::*;
use crate::runtime::compatibility::{
    ComponentCompatibilityReport, DonorCompatibility, assess_component_donors,
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Browser {
    picker: Option<Picker>,
    job: Option<Job>,
    generation: u64,
    reports: BTreeMap<u32, (RuntimeGraphKey, Arc<ComponentCompatibilityReport>)>,
}

struct Picker {
    binding_hash: u32,
    key: Option<RuntimeGraphKey>,
    baseline_hash: Option<u32>,
    query: String,
    experimental: bool,
    rejected: bool,
    selected: Option<u32>,
    error: Option<String>,
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
        self.job.is_some()
    }

    pub(super) fn close(&mut self) {
        self.picker = None;
        // A closing window does not detach a worker holding package handles.
    }

    pub(super) fn invalidate(&mut self) {
        self.close();
        self.generation = self.generation.wrapping_add(1);
        self.reports.clear();
    }
}

fn status_label(status: DonorCompatibility) -> &'static str {
    match status {
        DonorCompatibility::LowerRisk => "Lower-Risk Match",
        DonorCompatibility::Experimental => "Experimental / Unchecked",
        DonorCompatibility::Incompatible => "Structurally Incompatible",
    }
}

fn status_rank(status: DonorCompatibility) -> u8 {
    match status {
        DonorCompatibility::LowerRisk => 0,
        DonorCompatibility::Experimental => 1,
        DonorCompatibility::Incompatible => 2,
    }
}

fn can_apply(status: DonorCompatibility, experimental: bool) -> bool {
    match status {
        DonorCompatibility::LowerRisk => true,
        DonorCompatibility::Experimental => experimental,
        DonorCompatibility::Incompatible => false,
    }
}

fn binding_label(binding_hash: u32) -> String {
    runtime_component_control(binding_hash).map_or_else(
        || format!("Binding 0x{binding_hash:08X}"),
        |control| control.label.to_owned(),
    )
}

impl PackageAuthoringApp {
    pub(super) fn draw_checked_runtime_donor_header(
        &mut self,
        ui: &mut egui::Ui,
        binding_hash: u32,
        selected_text: &str,
        current_hash: Option<u32>,
        baseline_hash: Option<u32>,
    ) {
        ui.label(format!("Requested: {selected_text}"));
        for source in self.effective_runtime_source_labels(binding_hash, baseline_hash) {
            ui.weak(source);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.catalog.is_some(), egui::Button::new("Review Donors…"))
                .on_hover_text("Check compatibility and review every affected component before applying a donor.")
                .clicked()
            {
                self.runtime_donors.picker = Some(Picker {
                    binding_hash,
                    key: self.runtime_graph_key(),
                    baseline_hash,
                    query: self.runtime_component_queries.get(&binding_hash).cloned().unwrap_or_default(),
                    experimental: false,
                    rejected: false,
                    selected: current_hash,
                    error: None,
                });
            }
            if current_hash.is_some() && ui.small_button("Follow Baseline").clicked() {
                self.recipe.set_runtime_component_donor(binding_hash, None);
                self.runtime_graph = None;
                self.runtime_value_text.clear();
                self.runtime_donors.invalidate();
            }
        });
    }

    fn effective_runtime_source_labels(
        &self,
        binding_hash: u32,
        baseline_hash: Option<u32>,
    ) -> Vec<String> {
        let current_key = self.runtime_graph_key();
        if let Some(sources) = self
            .runtime_donors
            .reports
            .values()
            .filter(|(key, report)| {
                Some(key) == current_key.as_ref() && report.current_error.is_none()
            })
            .find_map(|(_, report)| report.current_sources.get(&binding_hash))
        {
            return sources
                .iter()
                .map(|source| {
                    let name = self
                        .donor_summaries
                        .iter()
                        .find(|donor| donor.hash == source.donor_item_hash)
                        .map_or_else(
                            || format!("0x{:08X}", source.donor_item_hash),
                            |donor| donor.name.clone(),
                        );
                    source.via_binding_hash.map_or_else(
                        || format!("Effective: {name} (baseline)"),
                        |via| {
                            format!(
                                "Effective: {name} via {} (shared owner)",
                                binding_label(via)
                            )
                        },
                    )
                })
                .collect();
        }
        let Some((_, graph)) = self
            .runtime_graph
            .as_ref()
            .filter(|(key, _)| Some(key) == current_key.as_ref())
        else {
            return vec!["Effective source: Waiting for the current runtime graph.".into()];
        };
        let owners = graph
            .bindings
            .iter()
            .filter(|binding| binding.binding_hash == binding_hash)
            .map(|binding| binding.owner_tag)
            .collect::<BTreeSet<_>>();
        if owners.is_empty() {
            return vec![
                "Effective source: This binding is absent from the current runtime.".into(),
            ];
        }
        let mut labels = BTreeSet::new();
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
                            current_key
                                .as_ref()
                                .is_some_and(|key| key.pattern_index == Some(pattern))
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
                let name = baseline_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or("Runtime Baseline", |donor| donor.name.as_str());
                labels.insert(format!("Effective: {name} (baseline)"));
            } else {
                for (via, hash) in requests {
                    let name = self
                        .donor_summaries
                        .iter()
                        .find(|donor| donor.hash == hash)
                        .map_or_else(|| format!("0x{hash:08X}"), |donor| donor.name.clone());
                    labels.insert(format!(
                        "Effective: {name} via {} (shared owner)",
                        binding_label(via)
                    ));
                }
            }
        }
        labels.into_iter().collect()
    }

    pub(super) fn poll_runtime_donors(&mut self) {
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
            picker.error = Some("The runtime selection changed. Close this window and reopen Review Donors for the new baseline.".into());
            return;
        }
        let Some(key) = key else {
            picker.error =
                Some("Select a runtime baseline before choosing component donors.".into());
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

    pub(super) fn draw_runtime_donor_browser(&mut self, ctx: &egui::Context) {
        if !self.show_experimental_options
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        self.ensure_runtime_donor_report(ctx);
        let Some(mut picker) = self.runtime_donors.picker.take() else {
            return;
        };
        let current_key = self.runtime_graph_key();
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
        let width = (ctx.screen_rect().width() - 40.0).clamp(280.0, 900.0);
        egui::Window::new("Review Component Donors")
            .id(egui::Id::new("parhelion-runtime-donor-browser"))
            .open(&mut open).collapsible(false).resizable(true).default_width(width)
            .default_height((ctx.screen_rect().height() - 64.0).clamp(240.0, 780.0))
            .show(ctx, |ui| {
                workbench_style(ui);
                // Keep the explicit action reachable even when warnings and shared-owner
                // details need more room than a small viewport can provide. The window
                // may be shorter than the screen, so reserve its actual footer space.
                let footer_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                let body_height = (ui.available_height() - footer_height)
                    .min(ctx.screen_rect().height() - 120.0).max(0.0);
                egui::ScrollArea::vertical().id_salt("component-donor-browser-body")
                    .max_height(body_height).min_scrolled_height(0.0)
                    .show(ui, |ui| {
                ui.heading(binding_label(picker.binding_hash));
                ui.colored_label(ui.visuals().warn_fg_color,
                    "Component mixing has a high risk of crashes. Lower-risk matches are not gameplay-tested guarantees.");
                ui.weak("Checks compare against the runtime baseline and other component donors. Value edits, binary patches and automatic ammo/HUD edits still need in-game testing.");
                if let Some(error) = &picker.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    retry = picker.key == current_key && ui.button("Retry Scan").clicked();
                    return;
                }
                let Some(report) = report.as_ref() else {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Checking donor structures and shared dependencies…"); });
                    return;
                };
                if let Some(error) = &report.current_error {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("Current combination: {error}"));
                }
                if let Some(sources) = report.current_sources.get(&picker.binding_hash) {
                    let source_names = sources.iter().map(|source| {
                        let name = self.donor_summaries.iter().find(|donor| donor.hash == source.donor_item_hash)
                            .map_or_else(|| format!("0x{:08X}", source.donor_item_hash), |donor| donor.name.clone());
                        source.via_binding_hash.map_or_else(|| format!("{name} (baseline)"),
                            |via| format!("{name} via {}", binding_label(via)))
                    }).collect::<Vec<_>>().join(", ");
                    ui.weak(format!("Current Effective Source: {source_names}"));
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Filter");
                    named_control(ui.add(egui::TextEdit::singleline(&mut picker.query)
                        .hint_text("Weapon name, family or hash").desired_width(270.0)), "Filter Component Donors");
                    ui.checkbox(&mut picker.experimental, "Show Experimental Matches");
                    ui.checkbox(&mut picker.rejected, "Show Rejected Donors");
                });
                let counts = report.candidates.values().fold([0_usize; 3], |mut counts, assessment| {
                    counts[usize::from(status_rank(assessment.status))] += 1; counts
                });
                ui.weak(format!("{} lower risk · {} experimental · {} rejected", counts[0], counts[1], counts[2]));
                let query = picker.query.trim().to_ascii_lowercase();
                let mut candidates = self.donor_summaries.iter().filter_map(|donor| {
                    let assessment = report.candidates.get(&donor.hash)?;
                    if (assessment.status == DonorCompatibility::Experimental && !picker.experimental)
                        || (assessment.status == DonorCompatibility::Incompatible && !picker.rejected) { return None; }
                    (query.is_empty() || donor.name.to_ascii_lowercase().contains(&query)
                        || donor.type_name.to_ascii_lowercase().contains(&query)
                        || format!("0x{:08x}", donor.hash).contains(&query)).then_some((donor, assessment))
                }).collect::<Vec<_>>();
                candidates.sort_by(|(left, left_assessment), (right, right_assessment)| {
                    status_rank(left_assessment.status).cmp(&status_rank(right_assessment.status))
                        .then_with(|| left.name.cmp(&right.name)).then_with(|| left.hash.cmp(&right.hash))
                });
                let height = (ctx.screen_rect().height() * 0.28).clamp(100.0, 270.0);
                egui::ScrollArea::vertical().id_salt("component-donor-candidates")
                    .max_height(height).auto_shrink([false, false])
                    .show(ui, |ui| {
                        if candidates.is_empty() { ui.weak("No donors match these filters. Experimental matches require the explicit option above."); }
                        for (donor, assessment) in candidates {
                            let label = format!("{} · {} · {}", donor.name, donor.type_name, status_label(assessment.status));
                            if ui.selectable_label(picker.selected == Some(donor.hash), label)
                                .on_hover_text(format!("0x{:08X}\n{}", donor.hash, assessment.reasons.join("\n"))).clicked()
                            { picker.selected = Some(donor.hash); }
                        }
                    });
                ui.separator();
                if let Some((hash, assessment)) = picker.selected.and_then(|hash| report.candidates.get(&hash).map(|assessment| (hash, assessment))) {
                    let name = self.donor_summaries.iter().find(|donor| donor.hash == hash)
                        .map_or_else(|| format!("0x{hash:08X}"), |donor| donor.name.clone());
                    ui.strong(format!("{name}: {}", status_label(assessment.status)));
                    egui::ScrollArea::vertical().id_salt("component-donor-review")
                        .max_height((ctx.screen_rect().height() * 0.2).clamp(70.0, 170.0)).show(ui, |ui| {
                            for reason in &assessment.reasons { ui.label(reason); }
                            if !assessment.affected_bindings.is_empty() {
                                ui.label("This selection affects the complete shared owner:");
                                for affected in &assessment.affected_bindings {
                                    let label = report.affected_bindings.iter().find(|(hash, _)| hash == affected)
                                        .map_or_else(|| binding_label(*affected), |(_, label)| label.clone());
                                    ui.monospace(format!("{label} · 0x{affected:08X}"));
                                }
                            }
                        });
                } else {
                    ui.weak("Select a donor to review the result before applying it.");
                }
                });
                if picker.error.is_none()
                    && let Some(assessment) = picker.selected.and_then(|hash| report.as_ref()?.candidates.get(&hash))
                {
                    apply = ui.add_enabled(can_apply(assessment.status, picker.experimental),
                        egui::Button::new("Apply Donor")).clicked();
                }
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
            && let Some((hash, assessment)) = picker.selected.and_then(|hash| {
                report
                    .as_ref()?
                    .candidates
                    .get(&hash)
                    .map(|assessment| (hash, assessment))
            })
            && can_apply(assessment.status, picker.experimental)
            && let Some(donor) = self.donor_summaries.iter().find(|donor| donor.hash == hash)
        {
            let reference = (Some(hash) != picker.baseline_hash).then(|| WeaponDonorReference {
                item_hash: hash.into(),
                expected_name: Some(donor.name.clone()),
            });
            self.recipe
                .set_runtime_component_donor(picker.binding_hash, reference);
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
