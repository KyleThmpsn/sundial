use eframe::egui;

use crate::{
    catalog::{Catalog, ItemPackageMetadata},
    hash::format_hash_hex,
};

use super::{
    RuntimeInspectionState,
    loader::{LoadedDetails, PerkDetails, RuntimeTarget},
    view,
};

pub(super) fn draw_perks(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    state: &mut RuntimeInspectionState,
) {
    if metadata.sandbox_perks.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Sandbox Perks ({})", metadata.sandbox_perks.len()))
        .id_salt(("inspector-sandbox-perks", item_hash))
        .show(ui, |ui| {
            ui.weak("Perks stored on this definition. Inspect a socket plug separately for its perks. These are not the combined effects of equipped plugs.");
            for (row, perk) in metadata.sandbox_perks.iter().enumerate() {
                // A declaration-only row still reaches the client's perk bank, so it is shown
                // with its liveness rather than hidden.
                let heading = if perk.active {
                    format!("Perk Index {}", perk.perk_index)
                } else {
                    format!("Perk Index {} · Declaration Only", perk.perk_index)
                };
                egui::CollapsingHeader::new(heading)
                    .id_salt((row, perk.perk_index))
                    .show(ui, |ui| {
                        if !perk.active {
                            ui.weak("Inactive in the native registry's own lookup. The row carries no runtime action but is still projected into the replicated perk bank.");
                        }
                        let loaded = state.draw_request(ui, RuntimeTarget::Perk(perk.perk_index));
                        if let Some(Ok(LoadedDetails::Perk(details))) = loaded.as_deref() {
                            draw_details(ui, catalog, details, &mut state.view);
                        }
                    });
            }
        });
}

fn draw_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    details: &PerkDetails,
    options: &mut view::RuntimeViewOptions,
) {
    let row = details.row;
    ui.monospace(format!(
        "Finished perk {} · hash 0x{:08X} · runtime key 0x{:08X}",
        row.index, row.perk_hash, row.runtime_key
    ));
    if let Some(name) = catalog.display_name(u64::from(row.perk_hash)) {
        ui.label(crate::app::ui::destiny_text(ui, name));
    }
    match &details.action {
        Ok(action) => {
            ui.monospace(format!(
                "Action 0x{:08X} · {} direct graph(s)",
                action.tag,
                action.graphs.len()
            ));
            ui.weak("Package references only. This does not confirm that the action is loaded or active in game.");
            if action.graphs.is_empty() {
                ui.weak("No directly referenced weapon-entity graph was found. The action may still implement behavior through other native nodes.");
            }
            for graph in &action.graphs {
                egui::CollapsingHeader::new(format!(
                    "Runtime Graph {}",
                    format_hash_hex(u64::from(graph.tag))
                ))
                .id_salt(graph.tag)
                .show(ui, |ui| {
                    ui.weak(format!(
                        "Action reference offsets: {}",
                        graph
                            .action_offsets
                            .iter()
                            .map(|offset| format!("0x{offset:X}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    match &graph.decoded {
                        Ok(decoded) => view::draw_graph(ui, decoded, options),
                        Err(error) => {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("Graph could not be decoded: {error}"),
                            );
                        }
                    }
                });
            }
        }
        Err(error) => {
            ui.label("Runtime action unavailable. The finished-perk row was read successfully.");
            ui.weak(error);
        }
    }
    egui::CollapsingHeader::new("Raw Finished Perk Record").show(ui, |ui| {
        ui.monospace(format!("Trailing value 0x{:016X}", row.trailing));
        if let Some(bytes) = row.detail {
            ui.monospace(
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            ui.weak("Native detail bytes. Not all fields have decoded semantics.");
        } else {
            ui.weak("No native detail row.");
        }
    });
}
