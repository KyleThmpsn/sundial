//! Parameter editing: load native data, edit an isolated draft, then apply or discard.
use super::*;
mod actions;
mod behavior;
pub(super) mod conversion;
mod fields;
mod guided;
mod movement;
mod native;
mod projectiles;
mod references;
mod structure;
#[cfg(test)]
pub(in crate::app) mod tests;
mod validation;

pub(in crate::app) use guided::has_guided_profile;
pub(super) use native::property_changes;

pub(super) fn load_entity_parameters(
    packages: &Path,
    entity: u32,
) -> Result<PrivatePerkRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let payload = manager
        .read_tag(tiger_pkg::TagHash(entity))
        .map_err(|error| error.to_string())?;
    let mut graph = load_weapon_runtime_graph_for_entity(&manager, 0, 0, entity, &payload)?;
    graph.scope_fields();
    let names = sundial::package_authoring::tft::cached_only(packages)?;
    Ok(PrivatePerkRuntimeGraph {
        action_tag: 0,
        action_payload: Vec::new(),
        summary: None,
        program: None,
        graphs: vec![(entity, graph)],
        graph_errors: Vec::new(),
        warnings: match names.as_ref() {
            None => vec!["Asset references have not been indexed for these packages yet. Open asset discovery to build the index. Effect properties are available now.".into()],
            Some(names) if !names.errors.is_empty() => vec![format!("{} resources could not be read. Asset references may be incomplete.", names.errors.len())],
            Some(_) => Vec::new(),
        },
        loading_issues: loading_issues(&manager, [("Asset", entity, false)]),
        projectile_slots: Vec::new(),
        projectile_catalog: Arc::default(),
        native_assets: names
            .iter()
            .flat_map(|names| &names.references)
            .filter(|reference| reference.target == entity || reference.source == entity)
            .cloned()
            .collect(),
    })
}

/// Validate the source closure without requiring it to be in the stock loading index.
/// The package builder enrolls missing prerequisites for both direct and cloned graphs.
fn loading_issues(
    manager: &tiger_pkg::PackageManager,
    assets: impl IntoIterator<Item = (&'static str, u32, bool)>,
) -> Vec<String> {
    assets
        .into_iter()
        .filter_map(
            |(role, graph, _cloned)| match projectile::residency::inspect(manager, graph) {
                Ok(_) => None,
                Err(error) => Some(format!(
                    "{role} 0x{graph:08X} could not be checked against the loading index: {error}"
                )),
            },
        )
        .collect()
}

pub(in crate::app) fn load_private_perk_runtime_graph(
    packages: &Path,
    key: PerkEditorKey,
    projectiles: &[ProjectileSelection],
) -> Result<PrivatePerkRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals = manager
        .read_tag(globals_tag)
        .map_err(|error| error.to_string())?;
    let action =
        load_sandbox_perk_runtime_action(&manager, &globals, usize::from(key.source_perk_index))?;
    let graphs = projectile::resolve(&manager, &action, projectiles)?;
    let mut projectile_slots = Vec::new();
    for (source, effective) in action.graphs.iter().zip(&graphs) {
        if projectile::kind(&source.payload)?.is_some() {
            projectile_slots.push((source.tag.0, effective.tag.0));
        }
    }
    let projectile_catalog = if projectile_slots.is_empty() {
        Arc::default()
    } else {
        projectile::catalog::cached(packages, &manager)?
    };
    let names = sundial::package_authoring::tft::cached_only(packages)?;
    let native_assets = names
        .iter()
        .flat_map(|names| &names.references)
        .filter(|reference| {
            reference.source == action.action_tag.0
                || reference.target == action.action_tag.0
                || graphs.iter().any(|graph| reference.target == graph.tag.0)
        })
        .cloned()
        .collect();
    // Decoding is read only. A perk whose action cannot be decoded still edits normally.
    let decoded =
        sundial::package_authoring::sandbox_perk::action::decode(&action.action_payload).ok();
    let summary = decoded
        .as_ref()
        .map(sundial::package_authoring::sandbox_perk::action::ActionSummary::new);
    let program = Some(
        sundial::package_authoring::sandbox_perk::program::Program::from_native(
            &action.action_payload,
            "Custom Effect",
        ),
    );
    let mut loaded = PrivatePerkRuntimeGraph {
        action_tag: action.action_tag.0,
        action_payload: action.action_payload,
        summary,
        program,
        graphs: Vec::new(),
        graph_errors: Vec::new(),
        warnings: match names.as_ref() {
            None => vec!["Asset references have not been indexed for these packages yet. Open asset discovery to build the index. Effect properties are available now.".into()],
            Some(names) if !names.errors.is_empty() => vec![format!("{} resources could not be read. Asset references may be incomplete.", names.errors.len())],
            Some(_) => Vec::new(),
        },
        loading_issues: loading_issues(
            &manager,
            projectiles
                .iter()
                .map(|selection| ("Projectile", selection.donor_graph, true)),
        ),
        projectile_slots,
        projectile_catalog,
        native_assets,
    };
    // Action-only perks are valid. A failed graph must not hide editable action values.
    for source in graphs {
        match load_weapon_runtime_graph_for_entity(
            &manager,
            key.source_plug_hash,
            0,
            source.tag.0,
            &source.payload,
        ) {
            Ok(mut graph) => {
                graph.scope_fields();
                loaded.graphs.push((source.tag.0, graph));
            }
            Err(error) => loaded
                .graph_errors
                .push(format!("Graph {}: {error}", source.tag)),
        }
    }
    Ok(loaded)
}

impl PerkEditor {
    /// Takes a requested conversion, if the user asked for one this frame.
    pub(in crate::app::custom_perks) fn take_conversion(
        &mut self,
    ) -> Option<sundial::package_authoring::sandbox_perk::program::Program> {
        self.conversion.take()
    }

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
        projectile_draft: Vec<ProjectileSelection>,
        ctx: &egui::Context,
    ) -> Self {
        let mut editor = Self {
            activation: None,
            preview: None,
            entity_source: None,
            key,
            plug_label,
            packages,
            original_draft: draft.clone(),
            original_action_draft: action_draft.clone(),
            draft,
            action_draft,
            original_projectile_draft: projectile_draft.clone(),
            projectile_draft,
            projectile_labels: BTreeMap::new(),
            item_names: BTreeMap::new(),
            projectile_query: String::new(),
            pending_movement: None,
            parameter_error: None,
            graph: None,
            error: None,
            receiver: None,
            worker: None,
            query: String::new(),
            value_text: BTreeMap::new(),
            show_all_native_values: false,
            conversion: None,
        };
        editor.start_load(ctx);
        editor
    }

    pub(in crate::app) fn open_entity(
        packages: PathBuf,
        key: PerkEditorKey,
        label: String,
        entity: u32,
        values: Vec<WeaponRuntimeValueOverride>,
        ctx: &egui::Context,
    ) -> Self {
        // Build the same isolated editor without starting a stock-perk load.
        let mut editor = Self {
            activation: None,
            preview: None,
            entity_source: Some(entity),
            key,
            plug_label: label,
            packages,
            original_draft: values.clone(),
            draft: values,
            action_draft: Vec::new(),
            original_action_draft: Vec::new(),
            projectile_draft: Vec::new(),
            original_projectile_draft: Vec::new(),
            projectile_labels: BTreeMap::new(),
            item_names: BTreeMap::new(),
            projectile_query: String::new(),
            pending_movement: None,
            parameter_error: None,
            graph: None,
            error: None,
            receiver: None,
            worker: None,
            query: String::new(),
            value_text: BTreeMap::new(),
            show_all_native_values: false,
            conversion: None,
        };
        editor.start_load(ctx);
        editor
    }

    pub(in crate::app) fn start_load(&mut self, ctx: &egui::Context) {
        if self.receiver.is_some() {
            return;
        }
        self.graph = None;
        self.preview = None;
        self.error = None;
        let packages = self.packages.clone();
        let key = self.key;
        let projectiles = self.projectile_draft.clone();
        let entity = self.entity_source;
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let result = if let Some(entity) = entity {
                load_entity_parameters(&packages, entity)
            } else {
                load_private_perk_runtime_graph(&packages, key, &projectiles)
            };
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
        let Some(event) = event else {
            return;
        };
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        match event {
            PrivatePerkGraphEvent::Preview(input, result) => self.preview = Some((input, result)),
            PrivatePerkGraphEvent::Finished(Ok(graph)) => {
                self.carry_movement(&graph);
                self.graph = Some(Arc::new(graph));
            }
            PrivatePerkGraphEvent::Finished(Err(error)) => self.error = Some(error),
        }
    }

    pub(in crate::app) fn reset_all(&mut self) {
        self.draft.clear();
        self.action_draft.clear();
        self.projectile_draft.clear();
        self.pending_movement = None;
        self.value_text.clear();
        self.parameter_error = None;
    }

    pub(in crate::app::custom_perks) fn draw_parameters(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        experimental: bool,
    ) {
        if self.receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading Effect…");
            });
            ui.small("The first load after a package change takes longer.");
        } else if let Some(error) = self.error.clone() {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if ui.button("Retry").clicked() {
                self.start_load(ctx);
            }
        } else if let Some(loaded) = self.graph.as_ref().map(Arc::clone) {
            for warning in &loaded.warnings {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
            }
            for error in &loaded.graph_errors {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if self.entity_source.is_some() {
                self.draw_movement(ui, &loaded);
                let named = self.draw_component_properties(ui, &loaded)
                    || loaded
                        .graphs
                        .iter()
                        .any(|(_, graph)| !projectile::parameters::discover(graph).is_empty());
                if !named {
                    ui.label("Gameplay property names have not been identified for this asset.");
                }
                egui::CollapsingHeader::new("Native Structure")
                    .default_open(!named)
                    .show(ui, |ui| {
                        self.draw_runtime_fields(ui, &loaded, experimental);
                    });
                return;
            }
            // The reading of the effect comes first. Converting it is an offer that follows.
            if self.draw_stock_canvas(ui, &loaded) {
                self.start_load(ctx);
                return;
            }
            self.draw_component_properties(ui, &loaded);
            ui.add_space(8.0);
            self.draw_conversion(ui, ctx, &loaded, experimental);
            references::draw(ui, &loaded.native_assets);
            egui::CollapsingHeader::new("Native Structure")
                .default_open(false)
                .show(ui, |ui| {
                    ui.small("Edit the stored values in each component and linked record.");
                    self.draw_action_values(ui, &loaded, experimental);
                    self.draw_runtime_fields(ui, &loaded, experimental);
                });
        }
    }

    /// Draws the stock action on the program canvas with the projectile pickers and mapped
    /// properties placed on the effect blocks that reference them. Slots the summary does not
    /// reference follow in a plain list. Returns whether a projectile swap needs a reload.
    fn draw_stock_canvas(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &Arc<PrivatePerkRuntimeGraph>,
    ) -> bool {
        use super::workbench::canvas;
        let Some(summary) = &loaded.summary else {
            // An action that did not decode still edits through the plain lists.
            if self.draw_projectiles(ui, loaded) {
                return true;
            }
            self.draw_movement(ui, loaded);
            return false;
        };
        if !loaded.projectile_slots.is_empty() {
            self.draw_projectile_notes(ui, loaded);
        }
        let editable = matches!(loaded.program, Some(Ok(_)));
        let name = self.plug_label.clone();
        let mut change = None;
        let mut placed = Vec::new();
        let mut place = |ui: &mut egui::Ui, asset: u32| {
            let Some(&(source, effective)) = loaded
                .projectile_slots
                .iter()
                .find(|(source, _)| *source == asset)
            else {
                return;
            };
            placed.push(source);
            ui.horizontal_wrapped(|ui| {
                ui.label("Asset")
                    .on_hover_text("Choose a different projectile or emitter for this effect.");
                if let Some(selected) = self.draw_projectile_slot(ui, loaded, source) {
                    change = Some((source, selected));
                }
            });
            self.draw_movement_for(ui, loaded, &[effective]);
        };
        canvas::draw(
            ui,
            canvas::Canvas {
                name: &name,
                backend: canvas::Backend::Stock {
                    summary,
                    action_tag: loaded.action_tag,
                    editable,
                },
                header: None,
                place: Some(&mut place),
                footer: None,
            },
        );
        let unplaced = loaded
            .projectile_slots
            .iter()
            .filter(|(source, _)| !placed.contains(source))
            .copied()
            .collect::<Vec<_>>();
        if !unplaced.is_empty() {
            ui.add_space(8.0);
            ui.strong("Other Projectiles and Emitters").on_hover_text(
                "Assets the action references outside the effect blocks shown above.",
            );
            for (source, effective) in &unplaced {
                ui.push_id(source, |ui| {
                    if let Some(selected) = self.draw_projectile_slot(ui, loaded, *source) {
                        change = Some((*source, selected));
                    }
                    self.draw_movement_for(ui, loaded, &[*effective]);
                });
            }
        }
        match change {
            Some((source, selected)) => {
                self.select_projectile(loaded, source, selected);
                true
            }
            None => false,
        }
    }

    pub(super) fn has_changes(&self) -> bool {
        self.draft != self.original_draft
            || self.action_draft != self.original_action_draft
            || self.projectile_draft != self.original_projectile_draft
    }
}
