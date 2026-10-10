//! Gameplay Barrel controls, read from the same composed owner the build edits.
use super::*;
use crate::item::BarrelDefaults;
mod controls;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Source {
    packages: PathBuf,
    key: RuntimeGraphKey,
    splices: Vec<(u32, Option<u16>, u32)>,
    overrides: WeaponRecipeOverrides,
}

type Read = Result<Option<BarrelDefaults>, String>;

#[derive(Default)]
pub(in crate::app) struct BarrelControls {
    generation: u64,
    read: Option<(Source, Read)>,
    loading: Option<(u64, Source, Receiver<Read>)>,
}

impl BarrelControls {
    pub(in crate::app) fn busy(&self) -> bool {
        self.loading.is_some()
    }

    pub(in crate::app) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.read = None;
    }

    /// Drain on every app frame, including while another page is open, so installation can wait
    /// for native package readers to close. An obsolete generation never becomes current data.
    pub(in crate::app) fn poll_finished(&mut self) {
        let Some((generation, source, receiver)) = &self.loading else {
            return;
        };
        let finished = match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Disconnected) => Some(Err("The Barrel reader stopped.".into())),
            Err(TryRecvError::Empty) => None,
        };
        if let Some(result) = finished {
            if *generation == self.generation {
                self.read = Some((source.clone(), result));
            }
            self.loading = None;
        }
    }

    fn poll(&mut self, ctx: &egui::Context, source: &Source, start: bool) -> Option<Read> {
        self.poll_finished();
        if let Some((key, result)) = &self.read
            && key == source
        {
            return Some(result.clone());
        }
        if start && self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let source = source.clone();
            let worker = source.clone();
            let repaint = ctx.clone();
            thread::spawn(move || {
                let _ = sender.send(read_defaults(&worker));
                repaint.request_repaint();
            });
            self.loading = Some((self.generation, source, receiver));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        None
    }
}

fn read_defaults(source: &Source) -> Read {
    use sundial::package_authoring::runtime::{
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager,
    };
    let manager = open_shadowkeep_package_manager(&source.packages)?;
    let entity = crate::runtime::load_effective_runtime_entity(&manager, &source.key)?;
    let splices = source
        .splices
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
    let overrides = source
        .overrides
        .to_compiler()
        .map_err(|error| error.to_string())?;
    crate::item::barrel_defaults(&manager, &entity.payload, &overrides, &splices)
        .map_err(|error| error.to_string())
}

impl PackageAuthoringApp {
    fn barrel_source(&self) -> Result<Option<Source>, String> {
        let Some(key) = self.runtime_graph_key() else {
            return Ok(None);
        };
        let splices = self
            .recipe
            .overrides
            .component_splices
            .iter()
            .map(|splice| {
                let item = splice
                    .donor
                    .item_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let binding = splice
                    .binding_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let pattern = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == item)
                    .and_then(|donor| donor.weapon_pattern_index);
                Ok((binding, pattern, item))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Source {
            packages: self.packages.clone(),
            key,
            splices,
            overrides: WeaponRecipeOverrides {
                runtime_values: self.recipe.overrides.runtime_values.clone(),
                runtime_resource_patches: self.recipe.overrides.runtime_resource_patches.clone(),
                raw_payload_patches: self.recipe.overrides.raw_payload_patches.clone(),
                ..Default::default()
            },
        }))
    }

    pub(in crate::app) fn draw_barrel_controls(&mut self, ui: &mut egui::Ui) {
        if !self.recipe.kind.is_weapon() {
            return;
        }
        let defaults = match self.barrel_source() {
            Ok(Some(source)) => self.barrel_controls.poll(
                ui.ctx(),
                &source,
                self.install_receiver.is_none() && self.catalog.is_some(),
            ),
            Ok(None) => Some(Ok(None)),
            Err(error) => Some(Err(error)),
        };
        let saved = &mut self.recipe.overrides.barrel;
        crate::app::style::card(ui, |ui| {
            ui.horizontal(|ui| {
                draw_donor_section_label(
                    ui,
                    "Barrel Settings",
                    Some("Pellets, spread and pattern of each shot."),
                );
                if saved.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::app::style::reset_icon(ui, "Reset Barrel") {
                            *saved = None;
                        }
                    });
                }
            });
            match defaults {
                Some(Ok(Some(defaults))) => {
                    ui.push_id("barrel-controls", |ui| controls::draw(ui, saved, &defaults));
                }
                Some(Ok(None)) => {
                    ui.weak("Choose a base weapon with a supported Barrel.");
                }
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().warn_fg_color, error);
                }
                None => {
                    ui.weak("Loading…");
                }
            }
        });
    }
}
