//! Read-only native asset provenance and component details.
use crate::sandbox_perk::projectile;
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
pub fn asset_details(
    ui: &mut egui::Ui,
    entry: &projectile::catalog::Entry,
    catalog: &projectile::catalog::Catalog,
    usage: &str,
) {
    details(ui, entry, catalog, usage, true);
}

/// The resource page presents components in its dedicated structure inspector.
pub(super) fn resource_details(
    ui: &mut egui::Ui,
    entry: &projectile::catalog::Entry,
    catalog: &projectile::catalog::Catalog,
) {
    details(ui, entry, catalog, "", false);
}

fn details(
    ui: &mut egui::Ui,
    entry: &projectile::catalog::Entry,
    catalog: &projectile::catalog::Catalog,
    usage: &str,
    components: bool,
) {
    ui.separator();
    let summary = entry.discovery_summary();
    if !summary.is_empty() {
        ui.strong("Native Usage");
        ui.add(egui::Label::new(summary).wrap());
    }
    if !usage.is_empty() && entry.source_hint.as_deref() != Some(usage) {
        ui.label(usage);
    }
    egui::CollapsingHeader::new("Technical Details").show(ui, |ui| {
        ui.label(projectile::residency::classify(entry).label());
        ui.label(entry.kind_label());
        for path in entry.native_paths.iter().chain(entry.native_name.iter()).collect::<BTreeSet<_>>() {
            draw_path(ui, path);
        }
        ui.monospace(format!("0x{:08X}", entry.graph));
        ui.label(&entry.package);
        if components && !entry.owners.is_empty() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Component Resources");
                crate::ui_help::info(ui,
                    "These package resources store the components used by this effect. They are data owners, not the character or team that owns a spawned effect.");
            });
            for tag in &entry.owners {
                ui.push_id(tag, |ui| {
                    if let Some(owner) = catalog.owners.get(tag) {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("0x{tag:08X}"));
                            ui.weak(&owner.package);
                        });
                        for name in owner.names.iter().collect::<BTreeSet<_>>() {
                            ui.add(egui::Label::new(name).wrap());
                        }
                        let mut types = BTreeMap::<u32, BTreeSet<u32>>::new();
                        for component in &owner.components {
                            types.entry(component.class).or_default().insert(component.binding);
                        }
                        for (class, bindings) in types {
                            use crate::weapon_runtime::{component_binding_label, native_member_names, native_type_name};
                            let role = native_type_name(class);
                            if let Some(role) = role {
                                ui.label(role);
                                ui.small(format!("Type 0x{class:08X}"));
                            } else {
                                ui.label(format!("Component Type 0x{class:08X}"))
                                    .on_hover_text("The component type is decoded. Its gameplay role has not been identified.");
                            }
                            let members = native_member_names(class);
                            if !members.is_empty() {
                                ui.add(egui::Label::new(egui::RichText::new(format!("Known Fields: {}", members.join(", "))).small()).wrap());
                            }
                            egui::CollapsingHeader::new(format!("Known Bindings ({})", bindings.len()))
                                .id_salt(class).show(ui, |ui| {
                                    for binding in bindings {
                                        ui.add(egui::Label::new(component_binding_label(binding)).wrap())
                                            .on_hover_text(format!("Binding 0x{binding:08X}"));
                                    }
                                });
                        }
                    } else {
                        ui.monospace(format!("0x{tag:08X}"));
                    }
                    ui.add_space(6.0);
                });
            }
        }
        if ui.button("Copy Asset Tag").clicked() {
            ui.ctx().copy_text(format!("0x{:08X}", entry.graph));
        }
        let paths = entry.contexts.iter().filter(|context| !context.path.is_empty()).collect::<Vec<_>>();
        if !paths.is_empty() {
            ui.separator();
            ui.strong("Source References");
        }
        for context in paths {
            draw_path(ui, &context.path);
            let steps = if context.depth == 1 {
                "Direct Reference".to_owned()
            } else {
                format!("{} Steps via 0x{:08X}", context.depth, context.owner)
            };
            ui.small(format!("0x{:08X} · {steps}", context.graph));
            if let Some(evidence) = &context.name_evidence {
                ui.label(format!("Native name hash: 0x{:08X}", evidence.hash));
                ui.add(egui::Label::new(&evidence.source).wrap());
            }
        }
    });
}

/// Keep exact native paths readable and copyable in both selectors and the catalog.
pub fn draw_path(ui: &mut egui::Ui, path: &str) {
    ui.add(egui::Label::new(path).wrap()).context_menu(|ui| {
        if ui.button("Copy Path").clicked() {
            ui.ctx().copy_text(path.to_owned());
            ui.close_menu();
        }
    });
}
