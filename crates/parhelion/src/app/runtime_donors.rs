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
    reset_unsupported: bool,
    reviewing: Option<(u32, WeaponRecipe)>,
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
    }
}

fn status_label(status: DonorCompatibility) -> &'static str {
    match status {
        DonorCompatibility::LowerRisk => "Lower Risk",
        DonorCompatibility::Experimental => "Experimental Match",
        DonorCompatibility::Incompatible => "Rejected Donor",
    }
}

fn status_color(ui: &egui::Ui, status: DonorCompatibility) -> egui::Color32 {
    match status {
        DonorCompatibility::LowerRisk => crate::app::style::success_color(ui.visuals()),
        DonorCompatibility::Experimental => ui.visuals().warn_fg_color,
        DonorCompatibility::Incompatible => ui.visuals().error_fg_color,
    }
}

fn draw_donor_group_label(ui: &mut egui::Ui, status: DonorCompatibility) {
    ui.horizontal(|ui| {
        ui.colored_label(
            status_color(ui, status),
            egui::RichText::new(status_label(status)).strong(),
        );
        ui.separator();
    });
}

fn draw_donor_row(
    ui: &mut egui::Ui,
    donor: &WeaponDonorSummary,
    assessment: &ComponentDonorAssessment,
    selected: bool,
    baseline: bool,
) -> egui::Response {
    let name_size = egui::TextStyle::Body.resolve(ui.style()).size.max(15.0);
    let row_height = (name_size + 34.0).max(56.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::hover(),
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("runtime-donor-row", donor.hash)),
        egui::Sense::click(),
    );
    let visuals = ui.style().interact_selectable(&response, selected);
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else if response.hovered() || response.has_focus() {
        visuals.weak_bg_fill
    } else {
        ui.visuals().faint_bg_color
    };
    ui.painter().rect_filled(rect, visuals.corner_radius, fill);
    ui.painter().rect_stroke(
        rect,
        visuals.corner_radius,
        egui::Stroke::new(
            if selected { 1.5 } else { 1.0 },
            if selected {
                ui.visuals().selection.stroke.color
            } else {
                visuals.bg_stroke.color
            },
        ),
        egui::StrokeKind::Inside,
    );

    let text_rect = rect.shrink2(egui::vec2(10.0, 5.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
        let trailing_width = if baseline { 72.0 } else { 0.0 };
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(
                    (ui.available_width() - trailing_width).max(40.0),
                    ui.available_height(),
                ),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&donor.name).size(name_size).strong())
                            .truncate()
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "{} · 0x{:08X}",
                                donor.type_name, donor.hash
                            ))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if baseline {
                    ui.label(egui::RichText::new("Baseline").small().strong());
                }
            });
        });
    });
    let response = response.on_hover_text(format!(
        "0x{:08X}\n{}",
        donor.hash,
        assessment.reasons.join("\n")
    ));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            format!(
                "{} · {} · 0x{:08X}",
                donor.name, donor.type_name, donor.hash
            ),
        )
    });
    response
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
                    reset_unsupported: false,
                    reviewing: None,
                });
            }
            if let Some(baseline) = baseline_hash
                && ui.small_button("Use Baseline…").on_hover_text("Restore this entire component group and check your saved settings.").clicked() {
                self.runtime_donors.picker = Some(Picker {
                    binding_hash, key: self.runtime_graph_key(), baseline_hash,
                    query: String::new(), experimental: false, rejected: false,
                    selected: Some(baseline), error: None, reset_unsupported: false, reviewing: None,
                });
            }
            if baseline_hash.is_none() && current_hash.is_some()
                && ui.small_button("Remove Saved Choice").clicked() {
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
                    self.runtime_source_label(
                        binding_hash,
                        source.donor_item_hash,
                        source.via_binding_hash,
                    )
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
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
                let mut donors = BTreeMap::new();
                for (via, hash) in requests {
                    donors.entry(hash).or_insert(via);
                }
                for (hash, via) in donors {
                    labels.insert(self.runtime_source_label(binding_hash, hash, Some(via)));
                }
            }
        }
        labels.into_iter().collect()
    }

    fn runtime_source_label(&self, binding: u32, hash: u32, via: Option<u32>) -> String {
        let name = self
            .donor_summaries
            .iter()
            .find(|donor| donor.hash == hash)
            .map_or_else(|| format!("0x{hash:08X}"), |donor| donor.name.clone());
        let Some(via) = via else {
            return format!("Effective: {name} (baseline)");
        };
        if self
            .recipe
            .runtime_component_donor(binding)
            .is_some_and(|donor| donor.item_hash.parse_u32() == Ok(hash))
        {
            format!("Effective: {name}")
        } else {
            format!(
                "Effective: {name} via {} (shared owner)",
                binding_label(via)
            )
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
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "Component mixing can cause crashes. Review details before applying and test in-game.",
                        );
                        if let Some(error) = &picker.error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            retry = picker.key == current_key && ui.button("Retry Scan").clicked();
                            return;
                        }
                        let Some(report) = report.as_ref() else {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking donor structures and shared dependencies…");
                            });
                            return;
                        };
                        if let Some(error) = &report.current_error {
                            egui::Frame::group(ui.style())
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.colored_label(
                                        ui.visuals().warn_fg_color,
                                        "The current combination needs repair. Selecting a donor replaces conflicting choices in this group.",
                                    )
                                    .on_hover_text(error);
                                });
                        }
                        if let Some(sources) = report.current_sources.get(&picker.binding_hash) {
                            let source_names = sources
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
                                        || format!("{name} (baseline)"),
                                        |via| format!("{name} via {}", binding_label(via)),
                                    )
                                })
                                .collect::<Vec<_>>();
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new("Current Source").strong());
                                for source in source_names {
                                    ui.weak(source);
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Current Source").strong());
                                ui.weak("Inherited from the current runtime baseline.");
                            });
                        }
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new("Search").strong());
                                    named_control(
                                        ui.add(
                                            egui::TextEdit::singleline(&mut picker.query)
                                                .hint_text("Weapon name, family or hash")
                                                .desired_width(260.0),
                                        ),
                                        "Filter Component Donors",
                                    );
                                    ui.checkbox(&mut picker.experimental, "Show Experimental Matches");
                                    ui.checkbox(&mut picker.rejected, "Show Rejected Donors");
                                    if let Some(hash) = picker.baseline_hash
                                        && report.candidates.contains_key(&hash)
                                        && ui.button("Select Baseline").clicked()
                                    {
                                        picker.selected = Some(hash);
                                        picker.query.clear();
                                        picker.reset_unsupported = false;
                                    }
                                });
                            });
                        let counts = report.candidates.values().fold([0_usize; 3], |mut counts, assessment| {
                            counts[usize::from(status_rank(assessment.status))] += 1;
                            counts
                        });
                        let query = picker.query.trim().to_ascii_lowercase();
                        let mut candidates = self
                            .donor_summaries
                            .iter()
                            .filter_map(|donor| {
                                let assessment = report.candidates.get(&donor.hash)?;
                                if (assessment.status == DonorCompatibility::Experimental && !picker.experimental)
                                    || (assessment.status == DonorCompatibility::Incompatible && !picker.rejected)
                                {
                                    return None;
                                }
                                (query.is_empty()
                                    || donor.name.to_ascii_lowercase().contains(&query)
                                    || donor.type_name.to_ascii_lowercase().contains(&query)
                                    || format!("0x{:08x}", donor.hash).contains(&query))
                                    .then_some((donor, assessment))
                            })
                            .collect::<Vec<_>>();
                        candidates.sort_by(|(left, left_assessment), (right, right_assessment)| {
                            status_rank(left_assessment.status)
                                .cmp(&status_rank(right_assessment.status))
                                .then_with(|| left.name.cmp(&right.name))
                                .then_with(|| left.hash.cmp(&right.hash))
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.weak(format!(
                                "{} donors shown · {} Lower Risk · {} Experimental · {} Rejected",
                                candidates.len(), counts[0], counts[1], counts[2]
                            ));
                        });
                        let height = if picker.selected.is_some() {
                            (ctx.screen_rect().height() * 0.2).clamp(100.0, 180.0)
                        } else {
                            (ctx.screen_rect().height() * 0.28).clamp(120.0, 300.0)
                        };
                        egui::ScrollArea::vertical()
                            .id_salt("component-donor-candidates")
                            .max_height(height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                if candidates.is_empty() {
                                    ui.weak("No donors match these filters. Enable additional match types above to broaden the list.");
                                }
                                let mut last_status = None;
                                for (donor, assessment) in candidates {
                                    if last_status != Some(assessment.status) {
                                        draw_donor_group_label(ui, assessment.status);
                                        last_status = Some(assessment.status);
                                    }
                                    if draw_donor_row(
                                        ui,
                                        donor,
                                        assessment,
                                        picker.selected == Some(donor.hash),
                                        picker.baseline_hash == Some(donor.hash),
                                    )
                                    .clicked()
                                    {
                                        picker.selected = Some(donor.hash);
                                        picker.reset_unsupported = false;
                                    }
                                    ui.add_space(4.0);
                                }
                            });
                        ui.separator();
                        if let Some((hash, assessment)) = picker
                            .selected
                            .and_then(|hash| report.candidates.get(&hash).map(|assessment| (hash, assessment)))
                        {
                            let donor = self.donor_summaries.iter().find(|donor| donor.hash == hash);
                            egui::Frame::group(ui.style())
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new("Selected Donor").strong());
                                        let color = status_color(ui, assessment.status);
                                        ui.colored_label(color, status_label(assessment.status));
                                    });
                                    if let Some(donor) = donor {
                                        ui.label(egui::RichText::new(&donor.name).size(16.0).strong());
                                        ui.weak(format!("{} · 0x{:08X}", donor.type_name, donor.hash));
                                    } else {
                                        ui.label(format!("0x{hash:08X}"));
                                    }
                                    egui::ScrollArea::vertical()
                                        .id_salt("component-donor-review")
                                        .max_height((ctx.screen_rect().height() * 0.2).clamp(70.0, 170.0))
                                        .show(ui, |ui| {
                                            if !assessment.affected_bindings.is_empty() {
                                                ui.strong("Changes Together");
                                                let named = assessment
                                                    .affected_bindings
                                                    .iter()
                                                    .filter_map(|hash| runtime_component_control(*hash))
                                                    .map(|control| control.label)
                                                    .collect::<Vec<_>>();
                                                if !named.is_empty() {
                                                    ui.label(named.join(", "));
                                                }
                                                ui.weak("Other donor choices in this group will be replaced.");
                                            }
                                            egui::CollapsingHeader::new("Compatibility Details")
                                                .default_open(assessment.status == DonorCompatibility::Incompatible)
                                                .show(ui, |ui| {
                                                    for reason in &assessment.reasons {
                                                        ui.label(reason);
                                                    }
                                                    for affected in &assessment.affected_bindings {
                                                        let label = report
                                                            .affected_bindings
                                                            .iter()
                                                            .find(|(hash, _)| hash == affected)
                                                            .map_or_else(|| binding_label(*affected), |(_, label)| label.clone());
                                                        ui.label(label).on_hover_text(format!("0x{affected:08X}"));
                                                    }
                                                });
                                        });
                                    if assessment.status != DonorCompatibility::Incompatible {
                                        let current_review = review
                                            .as_deref()
                                            .filter(|review| Some(review.donor_hash) == picker.selected);
                                        if preview::draw_review(ui, &mut picker, current_review) {
                                            self.runtime_donors.reviews.remove(&(picker.binding_hash, hash));
                                        }
                                    }
                                });
                        } else {
                            ui.weak("Select a donor to review the result before applying it.");
                        }
                });
                let selected_assessment = picker
                    .selected
                    .and_then(|hash| report.as_ref()?.candidates.get(&hash));
                let apply_enabled = picker.error.is_none()
                    && selected_assessment.is_some_and(|assessment| {
                        can_apply(assessment.status, picker.experimental)
                            && preview::can_apply(review.as_deref(), &self.recipe, &picker)
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    apply = ui
                        .add_enabled(apply_enabled, egui::Button::new("Apply Donor"))
                        .clicked();
                    let hint = if picker.error.is_some() {
                        "Resolve the scan error before applying."
                    } else if report.is_none() {
                        "Waiting for the compatibility scan."
                    } else if picker.selected.is_none() {
                        "Select a donor to enable apply."
                    } else if selected_assessment.is_some_and(|assessment| assessment.status == DonorCompatibility::Incompatible) {
                        "This donor is rejected because its structure is incompatible."
                    } else if !picker.experimental
                        && selected_assessment.is_some_and(|assessment| assessment.status == DonorCompatibility::Experimental)
                    {
                        "Enable Experimental Matches to apply this donor."
                    } else {
                        "Apply after the settings check completes."
                    };
                    ui.weak(hint);
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
            && can_apply(assessment.status, picker.experimental)
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
