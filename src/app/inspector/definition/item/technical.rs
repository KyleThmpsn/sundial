use super::*;
use crate::app::inspector::look;

pub(super) fn draw_runtime_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        look::empty_state(ui, "No Package Metadata");
        return;
    };
    super::super::runtime::draw_item_runtime(ui, content.catalog, content.hash, metadata, runtime);
    if metadata
        .weapon_pattern_index
        .is_none_or(|index| index == u16::MAX)
        && metadata.sandbox_perks.is_empty()
    {
        look::empty_state(ui, "No Runtime References");
    }
}

pub(super) fn draw_technical_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        look::empty_state(ui, "No Package Metadata");
        return;
    };
    super::super::item_details::draw_classification_details(ui, content.hash, metadata);
    draw_hash_item_package_metadata(
        ui,
        content.catalog,
        content.hash,
        metadata,
        content.matches.item_material_requirement_set_indices,
    );
    super::super::item_details::draw_structural_details(
        ui,
        content.catalog,
        content.hash,
        metadata,
    );
    super::super::runtime::draw_dye_colors(ui, metadata, runtime);
    super::super::item_details::draw_stat_group(ui, content.catalog, content.hash);
    if let Some(inventory) = content.matches.inventory_metadata {
        look::section(
            ui,
            ("item_inventory", content.hash),
            "Inventory",
            None,
            true,
            |ui| {
                draw_hash_inventory_placement_summary(ui, inventory);
                draw_hash_inventory_capacity_summary(ui, inventory);
            },
        );
    }
}

pub(super) fn draw_hash_item_package_metadata(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
) {
    look::section(
        ui,
        ("item_package_definition", item_hash),
        "Package Definition",
        None,
        true,
        |ui| {
            let definition_tag = TagHash(metadata.definition_tag);
            if ui.available_width() >= 840.0 {
                ui.columns(2, |columns| {
                    look::properties(&mut columns[0], ("item_package_primary", item_hash), |p| {
                        draw_hash_item_package_primary_fields(
                            p,
                            catalog,
                            item_hash,
                            metadata,
                            definition_tag,
                        );
                    });
                    look::properties(
                        &mut columns[1],
                        ("item_package_references", item_hash),
                        |p| {
                            draw_hash_item_package_reference_fields(
                                p,
                                catalog,
                                metadata,
                                material_requirement_set_indices,
                            );
                        },
                    );
                });
            } else {
                look::properties(ui, ("item_package_rows", item_hash), |p| {
                    draw_hash_item_package_primary_fields(
                        p,
                        catalog,
                        item_hash,
                        metadata,
                        definition_tag,
                    );
                    draw_hash_item_package_reference_fields(
                        p,
                        catalog,
                        metadata,
                        material_requirement_set_indices,
                    );
                });
            }
        },
    );
    if let Some(index) = metadata.socket_entry_list_index {
        super::super::item_details::draw_item_hash_list(
            ui,
            catalog,
            ("socket-entry-list-sharers", item_hash),
            "Items Sharing This Socket Entry List",
            catalog.items_with_socket_entry_list(index),
        );
    }
}

pub(super) fn draw_hash_item_package_primary_fields(
    p: &mut look::Properties<'_>,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    definition_tag: TagHash,
) {
    p.mono("Definition Index", metadata.definition_index.to_string());
    p.mono(
        "Definition Tag",
        format_hash_hex(u64::from(metadata.definition_tag)),
    );
    p.mono(
        "Package ID",
        format!(
            "{} · 0x{:04X}",
            definition_tag.pkg_id(),
            definition_tag.pkg_id()
        ),
    );
    p.mono(
        "Package Entry Index",
        definition_tag.entry_index().to_string(),
    );
    if let Some(size) = metadata.definition_size {
        p.mono("Definition Record Size", format!("{size} bytes"));
    }
    p.text(
        "Package",
        catalog.item_package_name(item_hash).unwrap_or_default(),
    );
}

pub(super) fn draw_hash_item_package_reference_fields(
    p: &mut look::Properties<'_>,
    catalog: &Catalog,
    metadata: &ItemPackageMetadata,
    material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
) {
    if let Some(tag) = metadata.string_definition_tag {
        p.mono("String Definition Tag", format_hash_hex(u64::from(tag)));
    }
    if let Some(tag) = metadata.icon_container_tag {
        p.mono("Icon Container Tag", format_hash_hex(u64::from(tag)));
    }
    if let Some(category_hash) = metadata.plug_category_hash {
        p.hash("Plug Category", catalog, category_hash);
    }
    if let Some(slot) = metadata.equipment_slot {
        p.mono("Native Equipment Slot ID", slot.to_string());
    }
    if let Some(index) = metadata.socket_entry_list_index {
        p.mono("Socket Entry List Index", index.to_string());
    }
    if let Some(index) = metadata.roll_set_index {
        let meaning = match index {
            0 => "socketed directly",
            u16::MAX => "service-granted outside a roll set",
            _ => "server roll-set ordinal",
        };
        p.mono("Plug Roll Set Index", format!("{index} · {meaning}"));
    }
    if let Some(index) = metadata.linked_plug_index {
        p.mono("Linked Plug Definition Index", index.to_string());
    }
    if let Some(hash) = metadata.linked_plug_hash {
        p.link(
            "Linked Plug Definition",
            catalog,
            hash,
            super::super::item_details::item_name(catalog, hash),
        );
    }
    if let Some(index) = metadata.weapon_pattern_index {
        p.mono("Weapon Pattern Index", index.to_string());
    }
    let arrangements = ["Generic", "Titan", "Hunter", "Warlock"]
        .into_iter()
        .zip(metadata.art_arrangement_indices)
        .filter_map(|(label, index)| index.map(|index| format!("{label} {index}")))
        .collect::<Vec<_>>();
    if !arrangements.is_empty() {
        p.mono("Art Arrangement Indices", arrangements.join(" · "));
    }
    if !metadata.render_overrides.is_empty() {
        p.mono(
            "Active Material Override Count",
            metadata.render_overrides.len().to_string(),
        );
    }
    if let Some(indices) = material_requirement_set_indices {
        material_requirement_set_rows(p, catalog, "Insertion Materials", indices.insertion);
        material_requirement_set_rows(p, catalog, "Enabled Materials", indices.enabled);
    }
}
