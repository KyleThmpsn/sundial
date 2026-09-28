use eframe::egui;

use crate::{
    app::inspector::look,
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
    look::section(
        ui,
        ("inspector-sandbox-perks", item_hash),
        "Sandbox Perks",
        Some(metadata.sandbox_perks.len()),
        true,
        |ui| {
            for (row, perk) in metadata.sandbox_perks.iter().enumerate() {
                if row > 0 {
                    ui.add_space(8.0);
                }
                // A declaration-only row still reaches the client's perk bank, so it is shown
                // with its liveness rather than hidden.
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Perk {}", perk.perk_index)).strong());
                    if !perk.active {
                        ui.label(
                            egui::RichText::new("Declaration Only")
                                .small()
                                .color(look::muted(ui)),
                        );
                    }
                });
                if let Some(description) = catalog
                    .perk_description(perk.perk_index)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    ui.add(egui::Label::new(crate::app::ui::destiny_text(ui, description)).wrap());
                }
                let carriers = catalog
                    .items_with_sandbox_perk(perk.perk_index)
                    .iter()
                    .copied()
                    .filter(|hash| *hash != item_hash)
                    .collect::<Vec<_>>();
                if !carriers.is_empty() {
                    let id = egui::Id::new(("inspector-sandbox-perk-carriers", item_hash, row));
                    egui::CollapsingHeader::new(format!(
                        "Carried by {} Other {}",
                        carriers.len(),
                        if carriers.len() == 1 { "Item" } else { "Items" }
                    ))
                    .id_salt(id)
                    .default_open(false)
                    .show(ui, |ui| {
                        super::super::item_details::draw_hash_item_rows(
                            ui,
                            catalog,
                            id,
                            carriers.iter().copied(),
                            &[],
                        );
                    });
                }
                egui::CollapsingHeader::new("Runtime Action")
                    .id_salt((
                        "inspector-sandbox-perk-action",
                        item_hash,
                        row,
                        perk.perk_index,
                    ))
                    .default_open(false)
                    .show(ui, |ui| {
                        let loaded = state.draw_request(ui, RuntimeTarget::Perk(perk.perk_index));
                        if let Some(Ok(LoadedDetails::Perk(details))) = loaded.as_deref() {
                            draw_details(ui, catalog, details, &mut state.view);
                        }
                    });
            }
        },
    );
}

fn draw_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    details: &PerkDetails,
    options: &mut view::RuntimeViewOptions,
) {
    let row = details.row;
    look::properties(ui, ("inspector-perk-row", row.index), |p| {
        p.mono("Finished Perk", row.index.to_string());
        p.mono("Perk Hash", format!("0x{:08X}", row.perk_hash));
        p.mono("Runtime Key", format!("0x{:08X}", row.runtime_key));
        if let Some(name) = catalog.display_name(u64::from(row.perk_hash)) {
            p.text("Name", name);
        }
    });
    match &details.action {
        Ok(action) => {
            look::properties(ui, ("inspector-perk-action", row.index), |p| {
                p.mono("Action", format!("0x{:08X}", action.tag));
                p.mono("Direct Graphs", action.graphs.len().to_string());
            });
            for graph in &action.graphs {
                egui::CollapsingHeader::new(format!(
                    "Runtime Graph {}",
                    format_hash_hex(u64::from(graph.tag))
                ))
                .id_salt(graph.tag)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Action reference offsets: {}",
                            graph
                                .action_offsets
                                .iter()
                                .map(|offset| format!("0x{offset:X}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                        .color(look::muted(ui)),
                    );
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
            look::empty_state(ui, "Runtime Action Unavailable");
            ui.label(egui::RichText::new(error.as_str()).color(look::muted(ui)));
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
        } else {
            look::empty_state(ui, "No Native Detail Row");
        }
    });
}
