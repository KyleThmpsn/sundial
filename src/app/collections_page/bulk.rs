use std::collections::HashSet;

use super::*;

#[derive(Debug, Default, PartialEq, Eq)]
struct Outcome {
    changed: usize,
    unchanged: usize,
    skipped: usize,
}

// The candidate stays isolated until every selected condition has been checked.
#[derive(Debug)]
pub(super) struct Job {
    source: Value,
    candidate: Value,
    initial: CollectionStateSnapshot,
    current: CollectionStateSnapshot,
    definitions: Vec<CollectibleDef>,
    targets: Vec<usize>,
    cursor: usize,
    acquired: bool,
    outcome: Outcome,
    issues: Vec<(String, String)>,
    review: Option<crate::app::progression::impact::Review>,
    conflict: Option<String>,
}

impl Job {
    fn new(
        document: &Value,
        definitions: Vec<CollectibleDef>,
        acquired: bool,
    ) -> Result<Self, String> {
        let initial =
            collection_state_snapshot(document).ok_or("The progression settings are invalid")?;
        Ok(Self {
            source: document.clone(),
            candidate: document.clone(),
            current: initial.clone(),
            initial,
            definitions,
            targets: Vec::new(),
            cursor: 0,
            acquired,
            outcome: Outcome::default(),
            issues: Vec::new(),
            review: None,
            conflict: None,
        })
    }

    fn step(&mut self, catalog: &Catalog, limit: usize) -> Result<bool, String> {
        let start = std::time::Instant::now();
        let end = self
            .cursor
            .saturating_add(limit)
            .min(self.definitions.len());
        while self.cursor < end {
            let definition = &self.definitions[self.cursor];
            if collectible_acquired_state(definition, &self.current, catalog) == Some(self.acquired)
            {
                self.targets.push(self.cursor);
            } else {
                match set_collectible_acquisition_state(
                    &mut self.candidate,
                    definition,
                    &self.current,
                    catalog,
                    self.acquired,
                ) {
                    Ok(()) => {
                        self.targets.push(self.cursor);
                        self.current = collection_state_snapshot(&self.candidate)
                            .ok_or("The progression settings are invalid")?;
                    }
                    Err(reason) => {
                        self.outcome.skipped += 1;
                        self.issues.push((item_name(definition), reason));
                    }
                }
            }
            self.cursor += 1;
            if start.elapsed() >= std::time::Duration::from_millis(8) {
                break;
            }
        }
        if self.cursor == self.definitions.len() && self.review.is_none() {
            let after = collection_state_snapshot(&self.candidate)
                .ok_or("The updated progression settings are invalid")?;
            let conflicts = self
                .targets
                .iter()
                .filter_map(|index| {
                    let definition = &self.definitions[*index];
                    (collectible_acquired_state(definition, &after, catalog) != Some(self.acquired))
                        .then(|| item_name(definition))
                })
                .collect::<Vec<_>>();
            if !conflicts.is_empty() {
                self.conflict = Some(format!(
                    "Conflicting acquisition conditions: {}. Adjust the selection before applying.",
                    conflicts.join(", ")
                ));
            }
            self.review = Some(crate::app::progression::impact::Review::build(
                &self.initial,
                &after,
                catalog,
                &self.definitions.iter().map(|entry| entry.hash).collect(),
            ));
        }
        Ok(self.cursor == self.definitions.len())
    }

    fn finish(mut self, document: &mut Value, catalog: &Catalog) -> Result<Outcome, String> {
        if document != &self.source {
            return Err(
                "The account changed during the bulk edit. No bulk changes were applied.".into(),
            );
        }
        let final_state = collection_state_snapshot(&self.candidate)
            .ok_or("The updated progression settings are invalid")?;
        for index in &self.targets {
            let definition = &self.definitions[*index];
            if collectible_acquired_state(definition, &final_state, catalog) != Some(self.acquired)
            {
                return Err(format!(
                    "Conflicting shared state for {}. No bulk changes were applied.",
                    definition.name
                ));
            }
            if collectible_acquired_state(definition, &self.initial, catalog) == Some(self.acquired)
            {
                self.outcome.unchanged += 1;
            } else {
                self.outcome.changed += 1;
            }
        }
        *document = self.candidate;
        Ok(self.outcome)
    }
}

#[cfg(test)]
fn apply_rows<'a>(
    document: &mut Value,
    catalog: &Catalog,
    definitions: impl IntoIterator<Item = &'a CollectibleDef>,
    acquired: bool,
) -> Result<Outcome, String> {
    let mut job = Job::new(
        document,
        definitions.into_iter().cloned().collect(),
        acquired,
    )?;
    while !job.step(catalog, 8)? {}
    job.finish(document, catalog)
}

pub(super) fn jobs(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if state.bulk_job.is_none() && state.bulk_ready.is_none() {
        return false;
    }
    let (changed, close) = crate::app::ui::edit_modal(ui, "collection_bulk_edit", |ui| {
        draw_job(ui, document, catalog, state)
    });
    if close {
        state.bulk_job = None;
        state.bulk_ready = None;
    }
    changed
}

fn draw_job(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if state.read_only {
        state.bulk_job = None;
        state.bulk_ready = None;
    }
    if let Some(mut job) = state.bulk_job.take() {
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.label(format!(
                "Updating {} / {}…",
                job.cursor,
                job.definitions.len()
            ));
            cancel = ui.button("Cancel Bulk Edit").clicked();
        });
        if cancel {
            return false;
        }
        let result = match job.step(catalog, 8) {
            Ok(true)
                if job.issues.is_empty()
                    && job.conflict.is_none()
                    && job
                        .review
                        .as_ref()
                        .is_none_or(|review| review.related.is_empty()) =>
            {
                job.finish(document, catalog)
            }
            Ok(true) => {
                state.bulk_ready = Some(job);
                ui.ctx().request_repaint();
                return false;
            }
            Ok(false) => {
                state.bulk_job = Some(job);
                ui.ctx().request_repaint();
                return false;
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(outcome) => {
                state.bulk_feedback = Some((
                    false,
                    format!(
                        "{} updated · {} unchanged · {} unsupported",
                        outcome.changed, outcome.unchanged, outcome.skipped
                    ),
                ));
                return outcome.changed > 0;
            }
            Err(error) => {
                state.bulk_feedback = Some((true, error));
                return false;
            }
        }
    }
    if let Some(job) = &state.bulk_ready {
        ui.strong("Review Changes");
        if let Some(reason) = &job.conflict {
            ui.colored_label(ui.visuals().error_fg_color, reason);
        }
        if let Some(review) = &job.review {
            review.draw(ui);
        }
        if !job.issues.is_empty() {
            ui.label(format!("{} items could not be changed", job.issues.len()));
        }
        egui::ScrollArea::vertical()
            .id_salt("collection_edit_issues")
            .max_height(160.0)
            .show_rows(
                ui,
                TABLE_CELL_HEIGHT * 2.0,
                job.issues.len(),
                |ui, range| {
                    for index in range {
                        let (name, reason) = &job.issues[index];
                        ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
                        ui.add(egui::Label::new(reason).truncate())
                            .on_hover_text(reason);
                    }
                },
            );
        let (apply, cancel) = ui
            .horizontal(|ui| {
                (
                    ui.add_enabled(
                        !job.targets.is_empty() && job.conflict.is_none(),
                        egui::Button::new(format!("Apply {} Supported", job.targets.len())),
                    )
                    .clicked(),
                    ui.button("Cancel").clicked(),
                )
            })
            .inner;
        if cancel {
            state.bulk_ready = None;
        } else if apply {
            match state
                .bulk_ready
                .take()
                .expect("ready bulk edit")
                .finish(document, catalog)
            {
                Ok(outcome) => {
                    state.bulk_feedback = Some((
                        false,
                        format!(
                            "{} updated · {} unchanged · {} unsupported",
                            outcome.changed, outcome.unchanged, outcome.skipped
                        ),
                    ));
                    return outcome.changed > 0;
                }
                Err(error) => state.bulk_feedback = Some((true, error)),
            }
        }
        return false;
    }
    if let Some((error, message)) = &state.bulk_feedback {
        ui.colored_label(
            if *error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            },
            message,
        );
    }
    false
}

pub(super) fn selection(
    ui: &mut egui::Ui,
    document: &Value,
    catalog: &Catalog,
    indices: &[u16],
    state: &mut UiState,
) {
    let visible = indices.iter().copied().collect::<HashSet<_>>();
    // Filtering narrows the selection as well as the operation's scope.
    state.selected.retain(|index| visible.contains(index));
    if let Some((true, message)) = &state.bulk_feedback {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
    let mut desired = None;
    if ui
        .add_enabled(!visible.is_empty(), egui::Button::new("Select Filtered"))
        .clicked()
    {
        state.selected = visible;
    }
    if state.selected.is_empty() {
        return;
    }
    if ui.button("Clear Selection").clicked() {
        state.selected.clear();
        return;
    }
    ui.label(format!("{} selected", state.selected.len()));
    let enabled = !state.read_only && state.bulk_job.is_none() && state.bulk_ready.is_none();
    if ui
        .add_enabled(enabled, egui::Button::new("Acquire Selected"))
        .clicked()
    {
        desired = Some(true);
    }
    if ui
        .add_enabled(enabled, egui::Button::new("Mark Selected Not Acquired"))
        .clicked()
    {
        desired = Some(false);
    }
    if let Some(desired) = desired {
        let definitions = catalog
            .collectibles()
            .iter()
            .filter(|row| state.selected.contains(&row.index))
            .cloned()
            .collect();
        match Job::new(document, definitions, desired) {
            Ok(job) => {
                state.bulk_job = Some(job);
                state.bulk_feedback = None;
                ui.ctx().request_repaint();
            }
            Err(error) => state.bulk_feedback = Some((true, error)),
        }
    }
}

fn item_name(definition: &CollectibleDef) -> String {
    if definition.name.trim().is_empty() {
        format!("Collection Item #{}", definition.index)
    } else {
        definition.name.clone()
    }
}

pub(super) fn checkbox(ui: &mut egui::Ui, indices: &[u16], selected: &mut HashSet<u16>) {
    let count = indices
        .iter()
        .filter(|index| selected.contains(index))
        .count();
    let mut checked = count == indices.len() && count > 0;
    ui.allocate_ui_with_layout(
        egui::vec2(20.0, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if ui
                .add(
                    egui::Checkbox::without_text(&mut checked)
                        .indeterminate(count > 0 && count < indices.len()),
                )
                .on_hover_text("Select for Bulk Acquisition")
                .changed()
            {
                for index in indices {
                    if checked {
                        selected.insert(*index);
                    } else {
                        selected.remove(index);
                    }
                }
            }
        },
    );
}

pub(super) fn branch_indices(branch: &hierarchy::CollectionBranch<'_>) -> Vec<u16> {
    let mut indices = branch
        .leaves
        .iter()
        .map(|leaf| leaf.definition.index)
        .collect::<Vec<_>>();
    for child in &branch.branches {
        indices.extend(branch_indices(child));
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::app) fn benchmark(document: &Value, catalog: &Catalog) {
    let mut job = Job::new(
        document,
        catalog.collectibles().iter().take(1000).cloned().collect(),
        true,
    )
    .unwrap();
    let start = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    let mut frames = 0;
    loop {
        let slice = std::time::Instant::now();
        let done = job.step(catalog, 8).unwrap();
        worst = worst.max(slice.elapsed());
        frames += 1;
        if done {
            break;
        }
    }
    eprintln!(
        "Collections bulk: {} targets, {} unsupported, {} related entries, {} slices, {:?} total, {:?} slowest slice",
        job.targets.len(),
        job.issues.len(),
        job.review.as_ref().unwrap().related.len(),
        frames,
        start.elapsed(),
        worst
    );
}
