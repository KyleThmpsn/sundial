//! Selected-candidate preflight and atomic application are separate from the broad donor scan.
use super::*;
use crate::runtime::swap;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Review {
    pub recipe: WeaponRecipe,
    pub binding_hash: u32,
    pub donor_hash: u32,
    pub result: Result<swap::Preview, String>,
}

pub(super) struct ReviewJob {
    recipe: WeaponRecipe,
    binding_hash: u32,
    donor_hash: u32,
    generation: u64,
    receiver: Receiver<Result<swap::Preview, String>>,
    worker: thread::JoinHandle<()>,
}

pub(super) fn can_apply(review: Option<&Review>, recipe: &WeaponRecipe, picker: &Picker) -> bool {
    let Some(review) = review else {
        return false;
    };
    let Ok(plan) = &review.result else {
        return false;
    };
    review.recipe == *recipe
        && plan.before == *recipe
        && review.binding_hash == picker.binding_hash
        && plan.binding_hash == picker.binding_hash
        && Some(review.donor_hash) == picker.selected
        && Some(plan.donor_hash) == picker.selected
        && plan.error.is_none()
        && (plan.resets.is_empty() || picker.reset_unsupported)
}

pub(super) fn draw_review(ui: &mut egui::Ui, picker: &mut Picker, review: Option<&Review>) -> bool {
    ui.separator();
    ui.strong("Your Settings");
    let Some(review) = review else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Checking your settings…");
        });
        return false;
    };
    let plan = match &review.result {
        Ok(plan) => plan,
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("This donor change could not be checked: {error}"),
            );
            return ui.button("Retry Settings Check").clicked();
        }
    };
    ui.label(format!(
        "{} saved {} kept · {} {} adapted",
        plan.kept,
        if plan.kept == 1 {
            "setting"
        } else {
            "settings"
        },
        plan.transferred.len(),
        if plan.transferred.len() == 1 {
            "setting"
        } else {
            "settings"
        }
    ));
    if !plan.transferred.is_empty() {
        egui::CollapsingHeader::new("Adapted Settings").show(ui, |ui| {
            for label in &plan.transferred {
                ui.label(label);
            }
        });
    }
    if !plan.resets.is_empty() {
        ui.label(
            "These edits cannot be carried to this donor. Reset them to use the donor's settings:",
        );
        egui::ScrollArea::vertical()
            .id_salt("swap-reset-settings")
            .max_height(100.0)
            .show(ui, |ui| {
                for label in &plan.resets {
                    ui.label(label);
                }
            });
        ui.checkbox(&mut picker.reset_unsupported, "Reset Listed Edits");
    }
    if let Some(error) = &plan.error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("Resolve this recipe issue before applying: {error}"),
        );
    } else {
        ui.weak(
            "Runtime edits checked, including ammo and HUD changes. Gameplay still needs testing.",
        );
    }
    false
}

impl PackageAuthoringApp {
    pub(super) fn current_runtime_swap(&self, picker: &Picker) -> Option<Arc<Review>> {
        self.runtime_donors
            .reviews
            .get(&(picker.binding_hash, picker.selected?))
            .filter(|review| review.recipe == self.recipe)
            .cloned()
    }

    pub(super) fn ensure_runtime_swap(&mut self, ctx: &egui::Context) {
        let key = self.runtime_graph_key();
        let Some(picker) = self.runtime_donors.picker.as_mut() else {
            return;
        };
        let Some(selected) = picker.selected else {
            return;
        };
        if picker.key != key || picker.error.is_some() {
            return;
        }
        let Some(key) = key else {
            return;
        };
        if picker
            .reviewing
            .as_ref()
            .is_none_or(|(donor, recipe)| *donor != selected || *recipe != self.recipe)
        {
            picker.reset_unsupported = false;
            picker.reviewing = Some((selected, self.recipe.clone()));
        }
        let binding_hash = picker.binding_hash;
        let assessed = self
            .runtime_donors
            .reports
            .get(&binding_hash)
            .filter(|(loaded, _)| *loaded == key)
            .and_then(|(_, report)| report.candidates.get(&selected));
        if assessed.is_none_or(|assessment| assessment.status == DonorCompatibility::Incompatible)
            || self.runtime_donors.review_job.is_some()
            || self
                .runtime_donors
                .reviews
                .get(&(binding_hash, selected))
                .is_some_and(|review| review.recipe == self.recipe)
        {
            return;
        }
        let Some(donor) = self
            .donor_summaries
            .iter()
            .find(|donor| donor.hash == selected)
            .cloned()
        else {
            return;
        };
        let recipe = self.recipe.clone();
        let worker_recipe = recipe.clone();
        let packages = self.packages.clone();
        let donors = self.donor_summaries.clone();
        let context = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = swap::preview(
                &packages,
                &worker_recipe,
                &key,
                binding_hash,
                &donor,
                &donors,
            );
            let _ = sender.send(result);
            context.request_repaint();
        });
        self.runtime_donors.review_job = Some(ReviewJob {
            recipe,
            binding_hash,
            donor_hash: selected,
            generation: self.runtime_donors.generation,
            receiver,
            worker,
        });
    }

    pub(super) fn poll_runtime_swap(&mut self) {
        let Some(job) = &self.runtime_donors.review_job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The settings check stopped without a result".into())
            }
        };
        let job = self
            .runtime_donors
            .review_job
            .take()
            .expect("job checked above");
        let joined = job.worker.join();
        if job.generation != self.runtime_donors.generation || job.recipe != self.recipe {
            return;
        }
        let result = if joined.is_err() {
            Err("The settings check failed unexpectedly".into())
        } else {
            result
        };
        // Keep a few recently selected candidates without retaining an entire catalog of recipes.
        if self.runtime_donors.reviews.len() >= 8 {
            self.runtime_donors.reviews.clear();
        }
        self.runtime_donors.reviews.insert(
            (job.binding_hash, job.donor_hash),
            Arc::new(Review {
                recipe: job.recipe,
                binding_hash: job.binding_hash,
                donor_hash: job.donor_hash,
                result,
            }),
        );
    }

    pub(in crate::app) fn draw_runtime_donor_undo(&mut self, ui: &mut egui::Ui) {
        let Some((_, after)) = &self.runtime_donors.last_change else {
            return;
        };
        if *after != self.recipe {
            self.runtime_donors.last_change = None;
            return;
        }
        if ui.small_button("Undo Donor Change").clicked()
            && let Some((before, _)) = self.runtime_donors.last_change.take()
        {
            self.recipe = before;
            self.runtime_graph = None;
            self.runtime_value_text.clear();
            self.runtime_donors.invalidate();
        }
    }
}
