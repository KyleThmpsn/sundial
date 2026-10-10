//! Technical Build window state, caching and lifecycle.
//!
//! `report` renders saved recipes and staged identities as selectable text. `registry` renders
//! the current native runtime inputs. `markers` owns appearance reads and their report section.
//! The window combines those sections without starting a read during package installation.

use sundial::investment::WeaponDonor;
use sundial::package_authoring::gear_markers::{self, MarkerSet, marker_name};
use sundial::package_authoring::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeRoot,
    WeaponRuntimeValue, WeaponRuntimeValueKind,
};

use crate::ItemKind;
use crate::recipe::{MarkerOffsetRecipe, WeaponRecipe};
use crate::workflow::{BuildReport, WeaponBuildReport};

mod markers;
mod registry;
mod report;
pub(super) use markers::MarkerJob;
use markers::*;
use registry::*;
pub(super) use report::field;
pub(crate) use report::technical_build_report;
use report::*;

/// The effective runtime entity behind the report, once the background scan has produced it.
/// `None` while no scan has finished for the current recipe.
pub(super) type Registry<'a> = Option<Result<&'a WeaponRuntimeGraph, &'a str>>;

/// The gear-art markers behind the appearance, once the background read has produced them.
/// `None` while no read has finished for the arrangement on screen.
pub(super) type Markers<'a> = Option<Result<&'a [MarkerSet], &'a str>>;

/// A finished marker read and the arrangements it covers.
pub(super) type MarkerRead = (Vec<u16>, Result<Vec<MarkerSet>, String>);

/// What a rendered runtime-registry section was rendered from. A graph is held weakly, so its
/// address cannot be reused by another graph while the section is cached.
#[derive(Clone)]
enum RegistrySource {
    Pending,
    Failed(String),
    Graph(std::sync::Weak<WeaponRuntimeGraph>),
}

impl PartialEq for RegistrySource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pending, Self::Pending) => true,
            (Self::Failed(left), Self::Failed(right)) => left == right,
            (Self::Graph(left), Self::Graph(right)) => left.ptr_eq(right),
            _ => false,
        }
    }
}

/// The rendered runtime-registry section, with the source and field choice it was rendered for.
pub(super) struct RegistryCache {
    source: RegistrySource,
    fields: bool,
    text: String,
}

/// The inputs the report's donor and marker arrangements are derived from.
#[derive(Clone, Copy, PartialEq)]
struct DonorInputs {
    recipe: u64,
    donor: Option<u32>,
    stat_group: Option<u16>,
    catalog: u64,
    catalog_loaded: bool,
}

/// Everything the report text is rendered from.
#[derive(PartialEq)]
struct ReportKey {
    donor: DonorInputs,
    build: Option<std::path::PathBuf>,
    markers: u64,
    registry: RegistrySource,
    fields: bool,
    parts: String,
}

/// The rendered report, rebuilt only when one of its inputs changes.
pub(super) struct ReportCache {
    key: ReportKey,
    arrangements: Option<Vec<u16>>,
    text: String,
    lines: usize,
}

impl super::PackageAuthoringApp {
    /// Draws the window while it is open, with or without a staged build.
    pub(super) fn draw_technical_build_window(&mut self, ctx: &egui::Context) {
        if !self.technical_build_open {
            return;
        }
        // A window drawn earlier this frame may have edited the recipe. Observing it here keeps
        // the cached report from lagging the recipe on screen.
        self.synchronize_recipe_dirty();
        let inputs = DonorInputs {
            recipe: self.recipe_revision,
            donor: self.recipe.donor.item_hash.parse_u32().ok(),
            stat_group: self.recipe.overrides.stat_group_index,
            catalog: self.catalog_revision,
            catalog_loaded: self.catalog.is_some(),
        };
        // The donor is resolved only when the report has to be rendered again.
        let mut donor = None;
        let arrangements = match self
            .technical_report
            .as_ref()
            .filter(|report| report.key.donor == inputs)
        {
            Some(report) => report.arrangements.clone(),
            None => {
                let current = self.current_donor();
                // Markers belong to the art the weapon wears, the appearance's when it has one.
                // Subclasses, emblems and shaders wear none.
                let arrangements = if wears_gear_art(self.recipe.kind) {
                    self.technical_marker_arrangements(self.current_geometry_donor().as_ref())
                } else {
                    None
                };
                donor = Some(current);
                arrangements
            }
        };
        // Started before anything borrows the app, so the read is already in flight by the
        // time the rest of the report is assembled. No read starts while an installation
        // replaces packages.
        if self.install_receiver.is_none()
            && let Some(arrangements) = &arrangements
        {
            self.ensure_technical_markers(ctx, arrangements);
        }
        // The rig, hold and type the part rows choose. It can start the type read.
        let parts = self.technical_parts(ctx);
        let build = self
            .latest_build
            .as_ref()
            .and_then(|build| build.as_ref().ok());
        // Only the graph scanned for the recipe on screen. A stale one would describe a
        // different runtime baseline than every other section of this report.
        let target = self.runtime_graph_target.as_ref();
        let graph = self
            .runtime_graph
            .as_ref()
            .and_then(|(key, graph)| (Some(key) == target).then_some(graph));
        let registry = match graph {
            Some(graph) => Some(Ok(graph.as_ref())),
            None => self
                .runtime_graph_error
                .as_ref()
                .and_then(|(key, error)| (Some(key) == target).then_some(Err(error.as_str()))),
        };
        let fields = graph.map_or(0, |graph| runtime_registry_field_count(graph));
        // A weapon carries a few thousand readable fields. Rendering them on every frame would
        // cost more than the rest of the report put together, so the section is kept until the
        // graph, the scan result or the choice changes.
        let source = match (graph, registry) {
            (Some(graph), _) => RegistrySource::Graph(std::sync::Arc::downgrade(graph)),
            (None, Some(Err(error))) => RegistrySource::Failed(error.to_owned()),
            (None, _) => RegistrySource::Pending,
        };
        if self.technical_registry.as_ref().is_none_or(|cache| {
            cache.source != source || cache.fields != self.technical_registry_fields
        }) {
            self.technical_registry = Some(RegistryCache {
                text: runtime_registry_section(registry, self.technical_registry_fields),
                source: source.clone(),
                fields: self.technical_registry_fields,
            });
        }
        // The whole report is about a megabyte of text with the field values on, so it is
        // rendered again only when something it reads has changed.
        let key = ReportKey {
            donor: inputs,
            build: build.map(|build| build.run_directory.clone()),
            markers: self.technical_marker_revision,
            registry: source,
            fields: self.technical_registry_fields,
            parts,
        };
        if self
            .technical_report
            .as_ref()
            .is_none_or(|report| report.key != key)
        {
            let donor = donor.unwrap_or_else(|| self.current_donor());
            let section = self
                .technical_registry
                .as_ref()
                .map_or("", |cache| cache.text.as_str());
            let markers = if wears_gear_art(self.recipe.kind) {
                marker_section(
                    arrangements.as_ref().and_then(|arrangements| {
                        let (read, result) = self.technical_markers.as_ref()?;
                        (read == arrangements).then_some(match result {
                            Ok(sets) => Ok(sets.as_slice()),
                            Err(error) => Err(error.as_str()),
                        })
                    }),
                    &self.recipe.overrides.marker_offsets,
                )
            } else {
                String::new()
            };
            let art = format!("{}{markers}", key.parts);
            let text = technical_build_report(build, &self.recipe, donor.as_ref(), &art, section);
            let lines = text.lines().count();
            self.technical_report = Some(ReportCache {
                key,
                arrangements,
                text,
                lines,
            });
        }
        let Some(report) = self.technical_report.as_ref() else {
            return;
        };
        let mut open = self.technical_build_open;
        let mut fields_shown = self.technical_registry_fields;
        egui::Window::new("Technical Build")
            .open(&mut open)
            .default_width(760.0)
            .default_height(520.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Copy Everything").clicked() {
                        ui.ctx().copy_text(report.text.clone());
                    }
                    ui.add_enabled(
                        fields > 0,
                        egui::Checkbox::new(&mut fields_shown, "Field Values"),
                    )
                    .on_hover_text(format!(
                        "Add every readable runtime field value ({fields}) to the registry."
                    ))
                    .on_disabled_hover_text("No runtime registry has been read for this weapon.");
                    ui.weak(format!("{} lines", report.lines));
                });
                ui.separator();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Selectable so a single figure can be lifted out without copying it all.
                        ui.add(
                            egui::TextEdit::multiline(&mut report.text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        );
                    });
            });
        if fields_shown != self.technical_registry_fields {
            ctx.request_repaint();
        }
        self.technical_build_open = open;
        self.technical_registry_fields = fields_shown;
    }
}

#[cfg(test)]
mod tests;
