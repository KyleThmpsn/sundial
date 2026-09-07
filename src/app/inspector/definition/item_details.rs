//! Native classifications and complete ordered metadata, separate from the item summary.

use eframe::egui;

use crate::{
    catalog::{Catalog, ItemDamageProfile, ItemPackageMetadata},
    hash::format_hash_hex,
};

pub(super) fn draw_item_traits(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if metadata.trait_indices.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Native Item Traits ({})",
        metadata.trait_indices.len()
    ))
    .id_salt(("inspector-item-traits", hash))
    .show(ui, |ui| {
        ui.weak("Native item classifications, separate from socket perks and objective traits.");
        egui::Grid::new(("item-traits", hash))
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Trait Index");
                ui.strong("Classification");
                ui.strong("Hash");
                ui.end_row();
                for &index in &metadata.trait_indices {
                    ui.monospace(index.to_string());
                    if let Some(definition) = catalog.trait_definitions().get(usize::from(index)) {
                        let name = if definition.name.trim().is_empty() {
                            "Unnamed trait"
                        } else {
                            &definition.name
                        };
                        ui.label(crate::app::ui::destiny_text(ui, name))
                            .on_hover_text(&definition.description);
                        ui.monospace(format_hash_hex(definition.hash));
                    } else {
                        ui.weak("Not resolved in this catalog");
                        ui.weak("—");
                    }
                    ui.end_row();
                }
            });
    });
}

pub(super) fn draw_structural_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    ui.push_id(("item-structural-details", hash), |ui| {
        if !metadata.power_cap_groups.is_empty() {
            egui::CollapsingHeader::new(format!("Power Cap Version Rows ({})", metadata.power_cap_groups.len()))
                .show(ui, |ui| {
                    ui.weak("Stored order is preserved. Each value indexes the installed power-cap table. The summary uses the highest cap only when every row resolves.");
                    let rows = metadata.power_cap_groups.iter().enumerate().map(|(row, group)| vec![
                        (row + 1).to_string(), group.to_string(), version_cap_text(catalog.power_cap_for_version_group(*group)),
                        catalog.power_cap_definitions().get(usize::from(*group)).map_or_else(
                            || "Unresolved".into(), |definition| format_hash_hex(u64::from(definition.hash))),
                    ]).collect();
                    draw_rows(ui, "versions", &["Row", "Cap Table Index", "Decoded Power Cap", "Definition Hash"], rows);
                });
        }
        if !metadata.art_arrangements.is_empty() {
            egui::CollapsingHeader::new(format!("Art Arrangement Rows ({})", metadata.art_arrangements.len()))
                .show(ui, |ui| {
                    ui.weak("Complete stored rows, including repeated classes and unassigned arrangements.");
                    let rows = metadata.art_arrangements.iter().enumerate().map(|(row, art)| vec![
                        (row + 1).to_string(), art_class_text(art.character_class),
                        if art.arrangement == u16::MAX { "65535 · unassigned".into() } else { art.arrangement.to_string() },
                    ]).collect();
                    draw_rows(ui, "art", &["Row", "Class", "Arrangement Index"], rows);
                });
        }
        let rows = dye_rows(metadata);
        if !rows.is_empty() {
            egui::CollapsingHeader::new(format!("Dye Reference Rows ({})", rows.len()))
                .show(ui, |ui| {
                    ui.weak("Custom, default, and locked lanes in stored order. Disabled rows are retained; indices are not item hashes.");
                    draw_rows(ui, "dyes", &["Override Lane", "Row", "Channel Index", "Dye Reference", "State"], rows);
                });
        }
    });
}

fn version_cap_text(cap: Option<u32>) -> String {
    cap.map_or_else(|| "No fixed cap decoded".into(), |cap| cap.to_string())
}

pub(super) fn draw_classification_details(
    ui: &mut egui::Ui,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    super::metadata_subsection(ui, "Native Classifications", |ui| {
        egui::Grid::new(("item-classifications", hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                super::hash_detail_field(
                    ui,
                    "Damage Profile",
                    damage_profile_text(metadata.damage_profile),
                    false,
                );
                super::hash_detail_field(
                    ui,
                    "Animation Translation Group",
                    metadata.weapon_translation_group.map_or_else(
                        || "Not Decoded".into(),
                        |group| format!("{group} · 0x{group:08X}"),
                    ),
                    true,
                );
                super::hash_detail_field(
                    ui,
                    "Weapon Inventory Slot",
                    metadata
                        .weapon_inventory_slot
                        .map_or("Not Decoded", |slot| match slot {
                            crate::catalog::ItemWeaponInventorySlot::Kinetic => "Kinetic",
                            crate::catalog::ItemWeaponInventorySlot::Energy => "Energy",
                            crate::catalog::ItemWeaponInventorySlot::Power => "Power",
                        }),
                    false,
                );
                super::hash_detail_field(
                    ui,
                    "Client Ammo Classification",
                    metadata
                        .weapon_ammo_type
                        .map_or("Not Decoded", |ammo| ammo.label()),
                    false,
                );
            });
        crate::ui_help::info(
            ui,
            "Slot, damage profile, and client ammo classification are separate package facts. They do not prove the loaded runtime's damage or ammunition consumption.",
        );
    });
}

fn damage_profile_text(profile: ItemDamageProfile) -> String {
    match profile {
        ItemDamageProfile::KineticEmpty => "Kinetic · Empty Damage Descriptor".into(),
        ItemDamageProfile::ModernFixed { damage_type } => {
            format!("{} · Modern Fixed", damage_type.label())
        }
        ItemDamageProfile::LegacyFixed { damage_type } => {
            format!("{} · Legacy Fixed", damage_type.label())
        }
        ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type } => format!(
            "{} · Default-Plug / Empty Descriptor Inference",
            damage_type.map_or("Unresolved", |damage| damage.label())
        ),
        ItemDamageProfile::Variable => "Variable Element".into(),
        ItemDamageProfile::Unknown => "Unknown / Not Decoded".into(),
    }
}

pub(super) fn draw_stat_group(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let Some(metadata) = catalog.item_package_metadata(hash) else {
        return;
    };
    egui::CollapsingHeader::new("Stat Display Curves").id_salt(("item-curves", hash)).show(ui, |ui| {
        let Some(group) = catalog.item_stat_group(hash) else {
            ui.weak(metadata.stat_group_index.map_or_else(|| "No stat-group reference is decoded.".into(), |index| format!("Stat-group index {index} is referenced but could not be resolved.")));
            return;
        };
        ui.label(format!("Group {} · {} · Maximum {}", metadata.stat_group_index.map_or_else(|| "Unknown".into(), |index| index.to_string()), format_hash_hex(group.hash), group.maximum_value));
        ui.weak("These curves produce presentation values from investment stats, not final gameplay values after plugs or runtime effects.");
        for (row, stat) in group.scaled_stats.iter().enumerate() {
            let definition = catalog.item_stat_definition(stat.definition_index);
            let name = definition.map_or("Unresolved Stat", |definition| definition.name.as_str());
            egui::CollapsingHeader::new(format!("{name} · Index {}", stat.definition_index)).id_salt(row).show(ui, |ui| {
                ui.label(format!("{} · {}", if stat.is_linear { "Linear" } else { "Interpolated" }, if stat.display_as_numeric { "Numeric Display" } else { "Bar Display" }));
                if stat.display_interpolation.is_empty() { ui.weak("No interpolation points stored."); }
                let rows = stat.display_interpolation.iter().map(|point| vec![point.investment_value.to_string(), point.display_value.to_string()]).collect();
                draw_rows(ui, "curve-points", &["Investment Value", "Display Value"], rows);
            });
        }
    });
}

pub(super) fn resolved_socket_pools(
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
) -> Vec<serde_json::Value> {
    item.sockets
        .iter()
        .enumerate()
        .map(|(index, socket)| {
            serde_json::json!({
                "socket_index": index,
                "socket_type": socket.socket_type,
                "normalized_options": catalog.socket_options(socket),
                "sources": socket.sources.iter().map(|source| serde_json::json!({
                    "source": source,
                    "normalized_options": catalog.socket_source_options(source),
                    "ordered_members": source.ordered_members,
                    "valid": source.valid,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn art_class_text(class: i8) -> String {
    let label = match class {
        -1 => "Generic",
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Unknown class",
    };
    format!("{label} ({class})")
}

fn dye_rows(metadata: &ItemPackageMetadata) -> Vec<Vec<String>> {
    let complete = metadata
        .translation_dye_rows
        .iter()
        .any(|rows| !rows.is_empty());
    let mut rows = Vec::new();
    if complete {
        for (stage, lane) in metadata.translation_dye_rows.iter().enumerate() {
            for (index, row) in lane.iter().enumerate() {
                rows.push(dye_row(stage, index, row.key, row.value));
            }
        }
    } else {
        // Older in-memory fixtures may contain only the active-row summary.
        for (index, row) in metadata.render_overrides.iter().enumerate() {
            rows.push(dye_row(usize::from(row.stage), index, row.key, row.value));
        }
    }
    rows
}

fn dye_row(stage: usize, index: usize, key: i8, value: u16) -> Vec<String> {
    vec![
        ["Custom", "Default", "Locked"].get(stage).map_or_else(
            || format!("Unknown lane {stage}"),
            |label| (*label).to_owned(),
        ),
        (index + 1).to_string(),
        key.to_string(),
        value.to_string(),
        if key == -1 {
            "Disabled".into()
        } else {
            "Enabled".into()
        },
    ]
}

fn draw_rows(ui: &mut egui::Ui, id: &str, headings: &[&str], rows: Vec<Vec<String>>) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(240.0)
        .show(ui, |ui| {
            egui::Grid::new(id)
                .num_columns(headings.len())
                .striped(true)
                .show(ui, |ui| {
                    for heading in headings {
                        ui.strong(*heading);
                    }
                    ui.end_row();
                    for row in rows {
                        for cell in row {
                            ui.monospace(cell);
                        }
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_class_labels_do_not_invent_unknown_meanings() {
        assert_eq!(version_cap_text(Some(1740)), "1740");
        assert_eq!(version_cap_text(None), "No fixed cap decoded");
        assert_eq!(art_class_text(-1), "Generic (-1)");
        assert_eq!(art_class_text(7), "Unknown class (7)");
    }

    #[test]
    fn damage_profile_labels_distinguish_fixed_inferred_and_unknown() {
        use crate::catalog::ItemDamageType;
        let modern = damage_profile_text(ItemDamageProfile::ModernFixed {
            damage_type: ItemDamageType::Arc,
        });
        let legacy = damage_profile_text(ItemDamageProfile::LegacyFixed {
            damage_type: ItemDamageType::Arc,
        });
        assert_ne!(modern, legacy);
        assert!(
            damage_profile_text(ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None })
                .contains("Unresolved")
        );
        assert!(damage_profile_text(ItemDamageProfile::Unknown).contains("Unknown"));
        assert_eq!(
            damage_profile_text(ItemDamageProfile::Variable),
            "Variable Element"
        );
    }

    #[test]
    fn complete_dye_rows_preserve_order_and_disabled_rows() {
        let metadata: ItemPackageMetadata = serde_json::from_value(serde_json::json!({
            "definition_index": 0, "definition_tag": 0,
            "translation_dye_rows": [[
                {"stage":0,"key":2,"value":80}, {"stage":0,"key":-1,"value":65535},
                {"stage":0,"key":2,"value":90}
            ], [], [{"stage":2,"key":0,"value":5}]]
        }))
        .unwrap();
        let rows = dye_rows(&metadata);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0][3], "80");
        assert_eq!(rows[1][4], "Disabled");
        assert_eq!(rows[2][3], "90");
        assert_eq!(rows[3][0], "Locked");
    }
}
