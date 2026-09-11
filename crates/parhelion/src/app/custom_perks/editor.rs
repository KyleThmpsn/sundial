//! Parameter editing: load native data, edit an isolated draft, then apply or discard.
use super::*;
mod actions;
mod fields;
mod guided;
mod movement;
mod projectiles;
mod references;
#[cfg(test)]
pub(in crate::app) mod tests;
mod validation;

pub(in crate::app) use guided::has_guided_profile;

fn load_entity_parameters(packages: &Path, entity: u32) -> Result<PrivatePerkRuntimeGraph, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let payload = manager
        .read_tag(tiger_pkg::TagHash(entity))
        .map_err(|error| error.to_string())?;
    let graph = load_weapon_runtime_graph_for_entity(&manager, 0, 0, entity, &payload)?;
    let names = sundial::package_authoring::tft::cached_only(packages)?;
    Ok(PrivatePerkRuntimeGraph {
        action_tag: 0,
        action_payload: Vec::new(),
        graphs: vec![(entity, graph)],
        warnings: match names.as_ref() {
            None => vec!["Asset references have not been indexed for these packages yet. Open asset discovery to build the index. Effect properties are available now.".into()],
            Some(names) if !names.errors.is_empty() => vec![format!("{} resources could not be read. Asset references may be incomplete.", names.errors.len())],
            Some(_) => Vec::new(),
        },
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
    let mut loaded = PrivatePerkRuntimeGraph {
        action_tag: action.action_tag.0,
        action_payload: action.action_payload,
        graphs: Vec::new(),
        warnings: match names.as_ref() {
            None => vec!["Asset references have not been indexed for these packages yet. Open asset discovery to build the index. Effect properties are available now.".into()],
            Some(names) if !names.errors.is_empty() => vec![format!("{} resources could not be read. Asset references may be incomplete.", names.errors.len())],
            Some(_) => Vec::new(),
        },
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
        projectile_draft: Vec<ProjectileSelection>,
        ctx: &egui::Context,
    ) -> Self {
        let mut editor = Self {
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
        let Some(PrivatePerkGraphEvent::Finished(result)) = event else {
            return;
        };
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        match result {
            Ok(graph) => {
                self.carry_movement(&graph);
                self.graph = Some(Arc::new(graph));
            }
            Err(error) => self.error = Some(error),
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
        guided::guided_support_notice(ui);
        if self.receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading Effect…");
            });
            ui.small(if self.entity_source.is_some() {
                "Reading the selected effect and any cached asset references."
            } else {
                "Loading effect data. Unchanged package scans are reused when available."
            });
        } else if let Some(error) = self.error.clone() {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if ui.button("Retry").clicked() {
                self.start_load(ctx);
            }
        } else if let Some(loaded) = self.graph.as_ref().map(Arc::clone) {
            for warning in &loaded.warnings {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
            }
            if self.entity_source.is_some() {
                self.draw_movement(ui, &loaded);
                egui::CollapsingHeader::new("Advanced Fields").show(ui, |ui| {
                    ui.weak("Gameplay meaning and safe ranges are not verified.");
                    self.draw_runtime_fields(ui, &loaded, experimental);
                });
                return;
            }
            if self.draw_projectiles(ui, &loaded) {
                self.start_load(ctx);
                return;
            }
            self.draw_movement(ui, &loaded);
            ui.add_space(8.0);
            references::draw(ui, &loaded.native_assets);
            egui::CollapsingHeader::new("Advanced Fields")
                .default_open(false)
                .show(ui, |ui| {
                    ui.weak("Gameplay meaning and safe ranges are not verified.");
                    self.draw_action_values(ui, &loaded, experimental);
                    self.draw_runtime_fields(ui, &loaded, experimental);
                });
        }
    }

    pub(super) fn has_changes(&self) -> bool {
        self.draft != self.original_draft
            || self.action_draft != self.original_action_draft
            || self.projectile_draft != self.original_projectile_draft
    }
}
