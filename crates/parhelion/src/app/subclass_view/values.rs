//! An ability's own values and those of the graphs it spawns: each graph's fields, as the
//! weapon's Runtime Values list them, edited on copies that only this ability uses. Graphs load
//! on a worker, once each, with the graphs they spawn named by the object catalog. The ability's
//! bank is left out, since the build keeps banks stock and Parameters edits them.
use super::*;
use std::sync::mpsc::{self, Receiver};
use sundial::package_authoring::sandbox_perk::entity::catalog::{self as objects, Catalog};

const HELP: &str = "Fields of this graph. The build changes a copy only this ability uses.";

/// A loaded graph and the graphs it spawns, each with its name.
pub(super) struct Loaded {
    pub(super) graph: Arc<WeaponRuntimeGraph>,
    pub(super) spawns: Vec<(u32, String)>,
}

type Load = Result<(Loaded, Option<Arc<Catalog>>), String>;

/// The graphs loaded so far and each list's own filter, cache and text.
#[derive(Default)]
pub(super) struct Values {
    graphs: BTreeMap<u32, Result<Arc<Loaded>, String>>,
    loading: Option<(u32, Receiver<Load>)>,
    objects: Option<Arc<Catalog>>,
    query: String,
    cache: Option<RuntimeValuesCache>,
    text: BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    show_technical: bool,
}

impl Values {
    /// Takes a finished load, and starts one for `graph` when it has none. Returns the graph
    /// once it is loaded.
    pub(super) fn poll(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        graph: u32,
    ) -> Option<Result<Arc<Loaded>, String>> {
        if let Some((loading, receiver)) = &self.loading {
            let loading = *loading;
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The loader stopped.".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(finished) = finished {
                let finished = finished.map(|(loaded, objects)| {
                    if objects.is_some() {
                        self.objects = objects;
                    }
                    Arc::new(loaded)
                });
                self.graphs.insert(loading, finished);
                self.loading = None;
            }
        }
        if let Some(loaded) = self.graphs.get(&graph) {
            return Some(loaded.clone());
        }
        if self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            let objects = self.objects.clone();
            std::thread::spawn(move || {
                let _ = sender.send(load(&packages, graph, objects));
            });
            self.loading = Some((graph, receiver));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        None
    }

    /// A loaded graph's name among the graphs its parent spawns.
    pub(super) fn spawn_name(&self, parent: u32, graph: u32) -> Option<&str> {
        let loaded = self.graphs.get(&parent)?.as_ref().ok()?;
        loaded
            .spawns
            .iter()
            .find(|(tag, _)| *tag == graph)
            .map(|(_, name)| name.as_str())
    }
}

fn load(packages: &Path, graph_tag: u32, objects: Option<Arc<Catalog>>) -> Load {
    let manager = open_shadowkeep_package_manager(packages)?;
    let payload = manager
        .read_tag(tiger_pkg::TagHash(graph_tag))
        .map_err(|error| error.to_string())?;
    let mut graph = load_weapon_runtime_graph_for_entity(&manager, 0, 0, graph_tag, &payload)?;
    graph.scope_fields();
    let is_bank = sundial::package_authoring::ability_modifier::is_bank;
    graph
        .resources
        .retain(|resource| !is_bank(resource.owner_tag));
    graph.owners.retain(|owner| !is_bank(owner.owner_tag));
    let objects = match objects {
        Some(objects) => Some(objects),
        None => objects::cached_only(packages).ok().flatten(),
    };
    let graphs =
        sundial::package_authoring::ability_spawns::reached_graphs(&manager, graph_tag, &payload)?;
    let names =
        sundial::package_authoring::ability_spawns::names(&manager, &graphs, objects.as_deref());
    let spawns = graphs.into_iter().zip(names).collect::<Vec<_>>();
    Ok((
        Loaded {
            graph: Arc::new(graph),
            spawns,
        },
        objects,
    ))
}

/// The values that belong to `graph`: scoped to it, or unscoped when it is the ability's own.
pub(super) fn values_of(
    values: &[WeaponRuntimeValueOverride],
    graph: u32,
    entity: u32,
) -> Vec<WeaponRuntimeValueOverride> {
    values
        .iter()
        .filter(|value| {
            value
                .locator
                .graph_tag
                .map(|tag| tag.get())
                .unwrap_or(entity)
                == graph
        })
        .cloned()
        .collect()
}

impl PackageAuthoringApp {
    /// One graph's values in the list the weapon's Runtime Values use. Returns the entry's whole
    /// value list once one of this graph's changes.
    pub(super) fn draw_graph_values(
        &self,
        ui: &mut egui::Ui,
        (loaded, entity): (&Loaded, u32),
        place: Place,
        values: &[WeaponRuntimeValueOverride],
        state: &mut Values,
    ) -> Option<Vec<WeaponRuntimeValueOverride>> {
        let tag = loaded.graph.entity_tag;
        let own = values_of(values, tag, entity);
        let mut edited = own.clone();
        draw_value_panel(
            ui,
            &loaded.graph,
            ValuePanel {
                title: "",
                help: HELP,
                scope: egui::Id::new(("subclass-ability-value-list", place, tag)),
                query: &mut state.query,
                cache: &mut state.cache,
                text: &mut state.text,
                overrides: &mut edited,
                show_experimental: self.show_experimental_options,
                show_technical: &mut state.show_technical,
                readable_first: true,
            },
        );
        (edited != own).then(|| {
            values
                .iter()
                .filter(|value| {
                    value
                        .locator
                        .graph_tag
                        .map(|tag| tag.get())
                        .unwrap_or(entity)
                        != tag
                })
                .cloned()
                .chain(edited)
                .collect()
        })
    }
}
