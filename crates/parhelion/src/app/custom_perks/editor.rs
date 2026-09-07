//! Parameter editing: load native data, edit an isolated draft, then apply or discard.
use super::*;
mod actions;
mod fields;
mod guided;
#[cfg(test)]
mod tests;
mod validation;

pub(in crate::app) use guided::{guided_support_notice, has_guided_profile};

pub(in crate::app) fn load_private_perk_runtime_graph(
    packages: &Path,
    key: PerkEditorKey,
) -> Result<PrivatePerkRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals = manager
        .read_tag(globals_tag)
        .map_err(|error| error.to_string())?;
    let action =
        load_sandbox_perk_runtime_action(&manager, &globals, usize::from(key.source_perk_index))?;
    let mut loaded = PrivatePerkRuntimeGraph {
        action_tag: action.action_tag.0,
        action_payload: action.action_payload,
        graphs: Vec::new(),
        warnings: Vec::new(),
    };
    // Action-only perks are valid. A failed graph must not hide editable action values.
    for source in action.graphs {
        match load_weapon_runtime_graph_for_entity(
            &manager,
            key.source_plug_hash,
            0,
            source.tag.0,
            &source.payload,
        ) {
            Ok(graph) => loaded.graphs.push((source.tag.0, graph)),
            Err(error) => loaded
                .warnings
                .push(format!("Graph {}: {error}", source.tag)),
        }
    }
    Ok(loaded)
}

impl PerkEditor {
    pub(in crate::app) fn is_loading(&self) -> bool {
        self.receiver.is_some()
    }

    pub(in crate::app) fn has_background_work(&self) -> bool {
        self.receiver.is_some() || self.worker.is_some()
    }

    pub(in crate::app) fn open(
        packages: PathBuf,
        key: PerkEditorKey,
        plug_label: String,
        draft: Vec<WeaponRuntimeValueOverride>,
        action_draft: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
        ctx: &egui::Context,
    ) -> Self {
        let mut editor = Self {
            key,
            plug_label,
            packages,
            original_draft: draft.clone(),
            original_action_draft: action_draft.clone(),
            draft,
            action_draft,
            parameter_error: None,
            graph: None,
            error: None,
            receiver: None,
            worker: None,
            query: String::new(),
            value_text: BTreeMap::new(),
            show_all_native_values: false,
        };
        editor.start_load(ctx);
        editor
    }

    pub(in crate::app) fn start_load(&mut self, ctx: &egui::Context) {
        if self.receiver.is_some() {
            return;
        }
        self.graph = None;
        self.error = None;
        let packages = self.packages.clone();
        let key = self.key;
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let result = load_private_perk_runtime_graph(&packages, key);
            let _ = sender.send(PrivatePerkGraphEvent::Finished(result));
            repaint.request_repaint();
        }));
    }

    pub(in crate::app) fn poll(&mut self) {
        let event = self
            .receiver
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(event) => Some(event),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(PrivatePerkGraphEvent::Finished(Err(
                    "The custom perk reader stopped without returning a result".into(),
                ))),
            });
        let Some(PrivatePerkGraphEvent::Finished(result)) = event else {
            return;
        };
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        match result {
            Ok(graph) => self.graph = Some(Arc::new(graph)),
            Err(error) => self.error = Some(error),
        }
    }

    pub(in crate::app) fn reset_all(&mut self) {
        self.draft.clear();
        self.action_draft.clear();
        self.value_text.clear();
        self.parameter_error = None;
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        experimental: bool,
    ) -> Option<PerkEditorAction> {
        self.poll();
        let mut open = true;
        let mut action = None;
        let available = ctx.screen_rect().size();
        egui::Window::new("Custom Perks · Parameters")
            .id(super::window::custom_perks_window_id())
            .open(&mut open).default_width(760.0)
            .default_height((available.y - 100.0).clamp(280.0, 640.0))
            .min_width(320.0).min_height(240.0)
            .max_width((available.x - 32.0).max(320.0))
            .max_height((available.y - 48.0).max(240.0))
            .resizable(true).show(ctx, |ui| {
                workbench_style(ui);
                ui.label("Parameter draft — changes are not applied until you choose Apply & Back.");
                ui.strong(&self.plug_label);
                ui.label("Scope: This recipe's custom perk. Stock perks and other weapons are unchanged.");
                guided_support_notice(ui);
                ui.separator();
                let body_height = (ui.available_height() - 110.0).max(80.0);
                egui::ScrollArea::vertical().id_salt("private-parameters-body")
                    .max_height(body_height).auto_shrink([false, false]).show(ui, |ui| {
                    if self.receiver.is_some() {
                        ui.horizontal(|ui| { ui.spinner(); ui.label("Reading Perk Parameters…"); });
                    } else if let Some(error) = self.error.clone() {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                        if ui.button("Retry").clicked() { self.start_load(ctx); }
                    } else if let Some(loaded) = self.graph.as_ref().map(Arc::clone) {
                        for warning in &loaded.warnings {
                            ui.colored_label(ui.visuals().warn_fg_color, warning);
                        }
                        self.draw_verified_parameters(ui, &loaded);
                        egui::CollapsingHeader::new("Advanced — Unverified Package Fields")
                            .default_open(!self.draft.is_empty() || !self.action_draft.is_empty())
                            .show(ui, |ui| {
                                self.draw_action_values(ui, &loaded, experimental);
                                self.draw_runtime_fields(ui, &loaded, experimental);
                            });
                    }
                });
                ui.separator();
                let errors = self.validation_errors();
                if let Some(error) = errors.first().filter(|_| self.graph.is_some()) {
                    ui.colored_label(ui.visuals().error_fg_color, error)
                        .on_hover_text(errors.join("\n"));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(errors.is_empty() && self.has_changes(), egui::Button::new("Apply & Back"))
                        .on_disabled_hover_text("Resolve invalid, ambiguous or unfinished edits before applying.")
                        .clicked() {
                        action = Some(PerkEditorAction::Apply {
                            key: self.key, values: self.draft.clone(), action_values: self.action_draft.clone(),
                        });
                    }
                    if ui.add_enabled(!self.draft.is_empty() || !self.action_draft.is_empty(), egui::Button::new("Restore Parameter Defaults")).on_hover_text("Clears parameter overrides in this draft only. Keeps activation conditions, custom display, stat bonuses and added effects. Choose Apply & Back to update the recipe.").clicked() {
                        self.reset_all();
                    }
                    if ui.button(if self.has_changes() { "Discard & Back" } else { "Back" }).clicked() {
                        action = Some(PerkEditorAction::Cancel);
                    }
                });
            });
        if !open && action.is_none() {
            action = Some(PerkEditorAction::Cancel);
        }
        action
    }

    fn has_changes(&self) -> bool {
        self.draft != self.original_draft || self.action_draft != self.original_action_draft
    }
}
