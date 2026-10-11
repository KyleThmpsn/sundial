//! Gameplay's Projectile section: the property cards of the projectile the weapon fires, set on
//! a private copy of it.
use super::*;
use crate::app::subclass_view::GraphCards;

/// What decides the graph a weapon fires: the runtime it reads with the recipe's component
/// splices, as binding, donor pattern and donor item, or the graph a requested behavior brings in
/// place of its own.
type Firing = (
    Option<(RuntimeGraphKey, Vec<(u32, Option<u16>, u32)>)>,
    Option<u32>,
);

/// The projectile a firing fires, or `None` for a graph that is not a projectile.
type Fired = Result<Option<u32>, String>;

/// The projectile the open weapon fires, read in the background, and its cards.
#[derive(Default)]
pub(in crate::app) struct FiredProjectile {
    read: Option<(Firing, Fired)>,
    loading: Option<(Firing, Receiver<Fired>)>,
    cards: GraphCards,
}

impl FiredProjectile {
    /// Takes a finished read, and starts one for `firing` when none runs and `start` allows it.
    /// Returns the projectile once `firing` has been read.
    fn poll(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        firing: &Firing,
        start: bool,
    ) -> Option<Fired> {
        if let Some((loading, receiver)) = &self.loading {
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err("The reader stopped.".to_owned())),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = finished {
                self.read = Some((loading.clone(), result));
                self.loading = None;
            }
        }
        if let Some((read, result)) = &self.read
            && read == firing
        {
            return Some(result.clone());
        }
        if start && self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            let worker = firing.clone();
            thread::spawn(move || {
                let _ = sender.send(read_fired(&packages, &worker));
            });
            self.loading = Some((firing.clone(), receiver));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        None
    }
}

/// The projectile a weapon fires: the graph a requested behavior brings, else the one the block
/// its runtime selects names, as the build finds it.
fn read_fired(packages: &Path, (key, requested): &Firing) -> Fired {
    use sundial::package_authoring::runtime::{
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager,
    };
    use sundial::package_authoring::sandbox_perk::entity::{Kind, kind};
    let manager = open_shadowkeep_package_manager(packages)?;
    let graph = match (requested, key) {
        (Some(graph), _) => Some(*graph),
        (None, Some((key, splices))) => {
            let source = crate::runtime::load_effective_runtime_entity(&manager, key)?;
            let donors = splices
                .iter()
                .map(|&(binding, pattern, item)| {
                    let donor = if let Some(pattern) = pattern {
                        load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern)?
                    } else {
                        load_weapon_runtime_entity_with_manager(&manager, item)?
                    };
                    Ok((binding, donor.payload))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let copied = crate::item::splice_writes(&manager, &source.payload, &donors)
                .map_err(|error| error.to_string())?;
            crate::weapon::behavior::fired_graph(
                &manager,
                &source.payload,
                source.weapon_content_group_hash,
                &copied,
            )
            .map_err(|error| error.to_string())?
        }
        (None, None) => None,
    };
    Ok(graph.filter(|graph| {
        manager
            .read_tag(TagHash(*graph))
            .is_ok_and(|payload| kind(&payload).ok().flatten() == Some(Kind::Projectile))
    }))
}

impl PackageAuthoringApp {
    /// Gameplay's Projectile section, including the imported carrier's launch adaptation.
    pub(in crate::app) fn draw_fired_projectile(&mut self, ui: &mut egui::Ui) {
        if !self.recipe.kind.is_weapon() {
            return;
        }
        if let Some(fired) = &self.recipe.overrides.fired_graph {
            if fired.imported.is_some() {
                ui.add_space(8.0);
                draw_donor_section_label(ui, "Projectile", None);
                crate::app::donor_view::draw_projectile_speed(ui, &mut self.recipe.overrides);
            }
            return;
        }
        let behaviors = self
            .recipe
            .overrides
            .additional_behaviors
            .iter()
            .map(|entry| entry.behavior.clone())
            .collect::<Vec<_>>();
        let firing = match crate::weapon::behavior::requested_graphs(&behaviors).first() {
            Some(graph) => (None, Some(*graph)),
            None => (
                self.runtime_graph_key()
                    .map(|key| (key, self.component_splice_sources())),
                None,
            ),
        };
        let packages = self.packages.clone();
        // The installer replaces the packages a read would open.
        let fired = if firing == (None, None) {
            Some(Ok(None))
        } else {
            let start = self.install_receiver.is_none();
            self.fired_projectile
                .poll(ui.ctx(), &packages, &firing, start)
        };
        let saved = self.recipe.overrides.projectile.clone();
        let graph = match fired {
            Some(Ok(Some(graph))) => graph,
            Some(Ok(None)) => {
                self.draw_unfired_projectile_values(ui, saved.as_ref(), "");
                return;
            }
            Some(Err(error)) => {
                self.draw_unfired_projectile_values(ui, saved.as_ref(), &error);
                return;
            }
            None => {
                if saved.is_some() {
                    ui.add_space(8.0);
                    draw_donor_section_label(ui, "Projectile", None);
                    ui.weak("Loading…");
                }
                return;
            }
        };
        let cards = self
            .fired_projectile
            .cards
            .has_cards(ui.ctx(), &packages, graph);
        let stale = saved.as_ref().filter(|saved| saved.graph != graph);
        if cards != Some(true) && stale.is_none() && saved.is_none() {
            return;
        }
        // Each card names its part, the projectile's own first.
        ui.add_space(8.0);
        if let Some(stale) = stale
            && crate::app::style::missing(
                ui,
                "Values for Another Projectile",
                &format!(
                    "Set for 0x{:08X}, which the weapon no longer fires",
                    stale.graph
                ),
            )
        {
            self.recipe.overrides.projectile = None;
        }
        let mut values = saved
            .filter(|saved| saved.graph == graph)
            .map(|saved| saved.values)
            .unwrap_or_default();
        let before = values.clone();
        self.fired_projectile
            .cards
            .draw(ui, &packages, graph, &mut values);
        if values != before {
            self.recipe.overrides.projectile =
                (!values.is_empty()).then_some(crate::weapon::projectile::Edits { graph, values });
        }
    }

    /// Saved projectile values while the weapon fires no projectile that can take them, with
    /// Remove.
    fn draw_unfired_projectile_values(
        &mut self,
        ui: &mut egui::Ui,
        saved: Option<&crate::weapon::projectile::Edits>,
        error: &str,
    ) {
        let Some(saved) = saved else {
            return;
        };
        ui.add_space(8.0);
        draw_donor_section_label(ui, "Projectile", None);
        let detail = if error.is_empty() {
            format!(
                "Set for 0x{:08X}, which the weapon no longer fires",
                saved.graph
            )
        } else {
            error.to_owned()
        };
        if crate::app::style::missing(ui, "Projectile Values", &detail) {
            self.recipe.overrides.projectile = None;
        }
    }
}
