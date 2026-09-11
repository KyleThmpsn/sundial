//! Read-only dependency evidence. Nothing in this browser rejects an authored combination.
use super::*;
mod details;
#[cfg(test)]
mod tests;
use sundial::{
    investment::PerkPatternUse,
    package_authoring::sandbox_perk::dependencies::{self, Entity, Index, Perk},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Perks,
    Patterns,
}

#[derive(Clone, Copy, Default)]
struct Request(Option<usize>);

pub(super) fn request(ctx: &egui::Context, perk: Option<usize>) {
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("pattern-dependency-request"), Request(perk))
    });
}

enum Event {
    Progress(usize, usize),
    Finished(Result<Arc<Index>, String>),
}
struct Job {
    generation: u64,
    receiver: Receiver<Event>,
    worker: thread::JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct Browser {
    open: bool,
    page: Page,
    query: String,
    selected: usize,
    history: Vec<(Page, usize)>,
    show_unnamed: bool,
    reveal_selection: bool,
    index: Option<Arc<Index>>,
    uses: Vec<PerkPatternUse>,
    error: Option<String>,
    progress: (usize, usize),
    job: Option<Job>,
    generation: u64,
}

impl Browser {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(super) fn invalidate(&mut self) {
        self.open = false;
        self.index = None;
        self.uses.clear();
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(super) fn poll(&mut self) {
        let mut finished = None;
        if let Some(job) = &self.job {
            loop {
                match job.receiver.try_recv() {
                    Ok(Event::Progress(done, total)) => self.progress = (done, total),
                    Ok(Event::Finished(result)) => {
                        finished = Some(result);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(Err(
                            "The dependency reader stopped without returning a result.".into(),
                        ));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            let job = self.job.take().expect("finished job exists");
            let _ = job.worker.join();
            if job.generation == self.generation {
                match result {
                    Ok(index) => self.index = Some(index),
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }

    fn start(&mut self, packages: PathBuf, ctx: &egui::Context) {
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.error = None;
        self.progress = (0, 0);
        self.job = Some(Job {
            generation: self.generation,
            receiver,
            worker: thread::spawn(move || {
                let result = open_shadowkeep_package_manager(&packages).and_then(|manager| {
                    dependencies::cached(&packages, &manager, |done, total| {
                        if done % 25 == 0 || done == total {
                            let _ = sender.send(Event::Progress(done, total));
                            repaint.request_repaint();
                        }
                    })
                });
                let _ = sender.send(Event::Finished(result));
                repaint.request_repaint();
            }),
        });
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_runtime_dependencies(&mut self, ctx: &egui::Context) {
        if !self.show_experimental_options {
            ctx.data_mut(|data| {
                data.remove_temp::<Request>(egui::Id::new("pattern-dependency-request"));
            });
            self.runtime_dependencies.open = false;
            return;
        }
        if let Some(Request(perk)) = ctx.data_mut(|data| {
            data.remove_temp::<Request>(egui::Id::new("pattern-dependency-request"))
        }) {
            self.runtime_dependencies.open = true;
            self.runtime_dependencies.query.clear();
            if let Some(perk) = perk {
                self.runtime_dependencies.page = Page::Perks;
                self.runtime_dependencies.selected = perk;
                self.runtime_dependencies.reveal_selection = true;
                self.runtime_dependencies.show_unnamed = true;
            }
        }
        if !self.runtime_dependencies.open {
            return;
        }
        if self.install_receiver.is_some() || self.build_receiver.is_some() {
            return;
        }
        if self.runtime_dependencies.index.is_none()
            && self.runtime_dependencies.error.is_none()
            && !self.runtime_dependencies.busy()
        {
            self.runtime_dependencies.uses = self
                .catalog
                .as_ref()
                .map_or_else(Vec::new, InvestmentCatalog::perk_pattern_uses);
            self.runtime_dependencies.start(self.packages.clone(), ctx);
        }
        let mut open = true;
        let target = self.recipe.overrides.weapon_pattern_index.or_else(|| {
            self.current_donor()
                .and_then(|donor| donor.summary.weapon_pattern_index)
        });
        egui::Window::new("Perks & Patterns")
            .id(egui::Id::new("perk-pattern-browser"))
            .open(&mut open)
            .default_size([1100.0, 720.0])
            .resizable(true)
            .max_width((ctx.screen_rect().width() - 32.0).max(320.0))
            .max_height((ctx.screen_rect().height() - 64.0).max(240.0))
            .show(ctx, |ui| {
                workbench_style(ui);
                self.runtime_dependencies.show(
                    ui,
                    &self.sandbox_perk_choices,
                    &self.donor_summaries,
                    target,
                );
            });
        self.runtime_dependencies.open = open;
    }
}

fn perk_label(index: usize, choices: &[WeaponSandboxPerkChoice]) -> String {
    choices
        .iter()
        .find(|choice| usize::from(choice.perk_index) == index)
        .map_or_else(
            || format!("Effect {index}"),
            |choice| format!("{} · {index}", choice.representative_name),
        )
}

fn pattern_label(index: usize, donors: &[WeaponDonorSummary]) -> String {
    let mut names = donors
        .iter()
        .filter(|donor| donor.weapon_pattern_index.map(usize::from) == Some(index))
        .map(|donor| donor.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    pattern_names_label(index, &names)
}

fn pattern_names_label(index: usize, names: &[&str]) -> String {
    if names.is_empty() {
        format!("Pattern {index}")
    } else {
        let more = if names.len() > 1 {
            format!(" (+{})", names.len() - 1)
        } else {
            String::new()
        };
        format!("{}{more} · Pattern {index}", names[0])
    }
}

fn selector_rows(
    page: Page,
    count: usize,
    choices: &[WeaponSandboxPerkChoice],
    donors: &[WeaponDonorSummary],
    show_unnamed: bool,
    query: &str,
) -> Vec<(usize, String)> {
    let mut names = BTreeMap::<usize, Vec<&str>>::new();
    match page {
        Page::Perks => {
            for choice in choices {
                names
                    .entry(usize::from(choice.perk_index))
                    .or_default()
                    .push(&choice.representative_name);
            }
        }
        Page::Patterns => {
            for donor in donors {
                if let Some(index) = donor.weapon_pattern_index {
                    names
                        .entry(usize::from(index))
                        .or_default()
                        .push(&donor.name);
                }
            }
        }
    }
    (0..count)
        .filter_map(|row| {
            let mut names = names.remove(&row).unwrap_or_default();
            if names.is_empty() && !show_unnamed {
                return None;
            }
            names.sort_unstable();
            names.dedup();
            let label = match page {
                Page::Perks => names
                    .first()
                    .map_or_else(|| format!("Effect {row}"), |name| format!("{name} · {row}")),
                Page::Patterns => pattern_names_label(row, &names),
            };
            (label.to_lowercase().contains(query)
                || names.iter().any(|name| name.to_lowercase().contains(query)))
            .then_some((row, label))
        })
        .collect()
}

impl Browser {
    fn draw_navigation(&mut self, ui: &mut egui::Ui, index: &Index, target: Option<u16>) {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.history.is_empty(), egui::Button::new("Back")).clicked() {
                if let Some((page, selected)) = self.history.pop() {
                    self.page = page;
                    self.selected = selected;
                    self.query.clear();
                    self.reveal_selection = true;
                }
            }
            if ui
                .selectable_label(self.page == Page::Perks, "Perks")
                .clicked()
            {
                self.navigate(Page::Perks, 0);
            }
            if ui
                .selectable_label(self.page == Page::Patterns, "Patterns")
                .clicked()
            {
                self.navigate(Page::Patterns, target.map_or(0, usize::from));
            }
            if let Some(target) = target {
                if ui.button("Recipe Pattern").clicked() {
                    self.navigate(Page::Patterns, usize::from(target));
                }
            }
            if ui.button("Refresh").clicked() {
                self.index = None;
            }
            sundial::investment::draw_authoring_info_icon(ui,
                format!("Patterns contain weapon behavior. Perks may add actions or depend on that behavior.\n\nStock pairings are examples, not requirements. Package data cannot confirm gameplay compatibility. Recipe component overrides are not included.\n\n{} patterns · {} perks\n{} unreadable patterns · {} perk errors", index.patterns.len(), index.perks.len(), index.patterns.iter().filter(|row| row.error.is_some()).count(), index.perks.iter().filter(|row| row.error.is_some()).count()));
        });
        ui.separator();
    }

    fn draw_selector(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        choices: &[WeaponSandboxPerkChoice],
        donors: &[WeaponDonorSummary],
    ) -> bool {
        if self.reveal_selection {
            let named = match self.page {
                Page::Perks => choices
                    .iter()
                    .any(|choice| usize::from(choice.perk_index) == self.selected),
                Page::Patterns => donors.iter().any(|donor| {
                    donor.weapon_pattern_index.map(usize::from) == Some(self.selected)
                }),
            };
            self.show_unnamed |= !named;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .desired_width(f32::INFINITY)
                .hint_text("Search by name or number"),
        );
        ui.checkbox(&mut self.show_unnamed, "Include Unnamed")
            .on_hover_text("Show internal entries that have no named stock weapon or perk.");
        let query = self.query.trim().to_lowercase();
        let count = if self.page == Page::Perks {
            index.perks.len()
        } else {
            index.patterns.len()
        };
        let labels = selector_rows(self.page, count, choices, donors, self.show_unnamed, &query);
        ui.weak(format!("{} Results", labels.len()));
        if labels.is_empty() {
            ui.weak("No matches. Try another name or number.");
            return false;
        }
        if !labels.iter().any(|(value, _)| *value == self.selected) && !self.reveal_selection {
            if let Some((value, _)) = labels.first() {
                self.selected = *value;
            }
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("dependency-rows", self.page as u8))
            .auto_shrink([false, false]);
        if self.reveal_selection {
            if let Some(row) = labels.iter().position(|(value, _)| *value == self.selected) {
                scroll = scroll
                    .vertical_scroll_offset(row as f32 * (26.0 + ui.spacing().item_spacing.y));
            }
            self.reveal_selection = false;
        }
        scroll.show_rows(ui, 26.0, labels.len(), |ui, range| {
            for row in range {
                let (value, label) = &labels[row];
                if ui
                    .add(
                        egui::Button::new(label)
                            .selected(self.selected == *value)
                            .truncate()
                            .min_size(egui::vec2(ui.available_width(), 26.0)),
                    )
                    .on_hover_text(label)
                    .clicked()
                {
                    self.history.push((self.page, self.selected));
                    self.selected = *value;
                }
            }
        });
        true
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        donors: &[WeaponDonorSummary],
        target: Option<u16>,
    ) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if ui.button("Retry Inspection").clicked() {
                self.error = None;
            }
            return;
        }
        let Some(index) = self.index.clone() else {
            ui.spinner();
            ui.label(format!(
                "Inspecting patterns and perks · {} / {}",
                self.progress.0, self.progress.1
            ));
            return;
        };
        self.draw_navigation(ui, &index, target);
        if ui.available_width() >= 700.0 {
            let height = ui.available_height().max(300.0);
            ui.horizontal_top(|ui| {
                let has_results = ui
                    .allocate_ui_with_layout(
                        egui::vec2(280.0, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.draw_selector(ui, &index, choices, donors),
                    )
                    .inner;
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if has_results {
                            self.draw_details(ui, &index, choices, donors, target);
                        }
                    },
                );
            });
        } else {
            let has_results = ui
                .allocate_ui(egui::vec2(ui.available_width(), 180.0), |ui| {
                    self.draw_selector(ui, &index, choices, donors)
                })
                .inner;
            ui.separator();
            if has_results {
                self.draw_details(ui, &index, choices, donors, target);
            }
        }
    }

    fn draw_details(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        choices: &[WeaponSandboxPerkChoice],
        donors: &[WeaponDonorSummary],
        target: Option<u16>,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(("dependency-details", self.page as u8, self.selected))
            .auto_shrink([false, false])
            .show(ui, |ui| match self.page {
                Page::Perks => self.draw_perk(ui, index, choices, donors, target),
                Page::Patterns => self.draw_pattern(ui, index, choices, donors, target),
            });
    }
}

fn perk_explanation(perk: &Perk) -> &'static str {
    if perk.error.is_some() {
        "Could not read this effect completely. Requirements are unknown."
    } else if perk.action.is_none() {
        "This is a marker. Adding it alone may not add its behavior."
    } else if perk.graphs.is_empty() {
        "Has an action, but no direct entity graph. It may use the weapon's resources."
    } else {
        "Supplies its own resources. Host requirements may still apply."
    }
}

fn draw_entity(ui: &mut egui::Ui, entity: &Entity) {
    egui::CollapsingHeader::new(format!(
        "Entity {:08X} · {} Component Bindings",
        entity.tag,
        entity.components.len()
    ))
    .id_salt(entity.tag)
    .show(ui, |ui| {
        for component in &entity.components {
            let name = runtime_component_control(component.binding).map_or_else(
                || format!("Binding {:08X}", component.binding),
                |control| control.label.to_owned(),
            );
            ui.label(format!(
                "{name} · Class {:08X} · Owner {:08X}",
                component.class, component.owner
            ));
        }
    });
}
