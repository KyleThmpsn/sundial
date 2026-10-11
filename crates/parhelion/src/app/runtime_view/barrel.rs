//! Gameplay Barrel controls, read from the same composed owner the build edits.
use super::*;
use crate::item::BarrelDefaults;
use crate::weapon::burst::{Column, ROUNDS_PER_MINUTE_HASH};
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
    crate::item::barrel_defaults(
        &manager,
        &entity.payload,
        entity.weapon_translation_group_hash,
        &overrides,
        &splices,
    )
    .map_err(|error| error.to_string())
}

impl PackageAuthoringApp {
    fn barrel_source(&self) -> Result<Option<Source>, String> {
        let Some(key) = self.runtime_graph_key() else {
            return Ok(None);
        };
        let splices = self.component_splice_list()?;
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

    /// The Barrel Settings card, for a weapon. `donor` holds the stats the weapon is built with.
    pub(in crate::app) fn draw_barrel_controls(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        if self.recipe.kind.is_weapon() {
            crate::app::style::card(ui, |ui| self.draw_barrel_settings(ui, donor));
        }
    }

    /// Barrel Settings' contents, in the card its caller draws.
    pub(in crate::app) fn draw_barrel_settings(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        let defaults = match self.barrel_source() {
            Ok(Some(source)) => self.barrel_controls.poll(
                ui.ctx(),
                &source,
                self.install_receiver.is_none() && self.catalog.is_some(),
            ),
            Ok(None) => Some(Ok(None)),
            Err(error) => Some(Err(error)),
        };
        let burst = match &defaults {
            Some(Ok(Some(defaults))) => defaults
                .bullets_per_shot
                .as_ref()
                .and_then(|column| stock_burst(column, &self.recipe.overrides, donor)),
            _ => None,
        };
        let custom = egui::Id::new(("barrel-custom-pattern", self.recipe_panel_scope()));
        let saved = &mut self.recipe.overrides.barrel;
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
                        ui.data_mut(|data| data.remove::<bool>(custom));
                    }
                });
            }
        });
        match defaults {
            Some(Ok(Some(defaults))) => {
                ui.push_id("barrel-controls", |ui| {
                    controls::draw(ui, saved, &defaults, custom, burst.as_ref());
                });
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
    }
}

/// The bullets per shot the weapon fires as built, from `column` at the stat `donor` and
/// `overrides` give it where the bullets follow one. None where the weapon lacks that stat, so its
/// bullets are unknown.
fn stock_burst(
    column: &Column,
    overrides: &WeaponRecipeOverrides,
    donor: Option<&WeaponDonor>,
) -> Option<controls::Burst> {
    if let Some(bullets) = column.fixed() {
        return Some(controls::Burst {
            bullets,
            follows: None,
        });
    }
    let hash = column.stat?;
    let stat = donor?
        .investment_stats
        .iter()
        .find(|stat| stat.definition_hash == Some(hash))?;
    if overrides
        .removed_investment_stats
        .contains(&stat.definition_index)
    {
        return None;
    }
    let value = overrides
        .investment_stats
        .iter()
        .find(|value| value.definition_index == stat.definition_index)
        .map_or(stat.value, |value| value.value);
    let shown = stat.in_game_display_value(value);
    let reading = if hash == ROUNDS_PER_MINUTE_HASH {
        format!("{shown} RPM")
    } else {
        format!("{} {shown}", stat.name)
    };
    Some(controls::Burst {
        bullets: column.at(value),
        follows: Some((stat.name.clone(), reading)),
    })
}
