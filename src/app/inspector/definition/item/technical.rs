use super::*;

pub(super) fn draw_runtime_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        ui.weak("Package metadata is unavailable. Runtime references cannot be resolved.");
        return;
    };
    super::super::runtime::draw_item_runtime(ui, content.catalog, content.hash, metadata, runtime);
    if metadata
        .weapon_pattern_index
        .is_none_or(|index| index == u16::MAX)
        && metadata.sandbox_perks.is_empty()
    {
        ui.weak("No weapon pattern or sandbox-perk references are decoded for this item.");
    }
}

pub(super) fn draw_technical_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        ui.weak("Package metadata is unavailable for this item.");
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
        draw_hash_inventory_placement_summary(ui, inventory);
        draw_hash_inventory_capacity_summary(ui, inventory);
    }
}

pub(super) fn draw_hash_item_package_metadata(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
) {
    metadata_subsection(ui, "Package Definition", |ui| {
        let definition_tag = TagHash(metadata.definition_tag);
        if ui.available_width() >= 840.0 {
            ui.columns(2, |columns| {
                columns[0].spacing_mut().item_spacing.x = 8.0;
                egui::Grid::new(("hash_item_package_metadata_primary", item_hash))
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(&mut columns[0], |ui| {
                        draw_hash_item_package_primary_fields(
                            ui,
                            catalog,
                            item_hash,
                            metadata,
                            definition_tag,
                        );
                    });

                columns[1].spacing_mut().item_spacing.x = 8.0;
                egui::Grid::new(("hash_item_package_references", item_hash))
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(&mut columns[1], |ui| {
                        draw_hash_item_package_reference_fields(
                            ui,
                            catalog,
                            metadata,
                            material_requirement_set_indices,
                        );
                    });
            });
        } else {
            egui::Grid::new(("hash_item_package_metadata_rows", item_hash))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    draw_hash_item_package_primary_fields(
                        ui,
                        catalog,
                        item_hash,
                        metadata,
                        definition_tag,
                    );
                    draw_hash_item_package_reference_fields(
                        ui,
                        catalog,
                        metadata,
                        material_requirement_set_indices,
                    );
                });
        }
    });
}

pub(super) fn draw_hash_item_package_primary_fields(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    definition_tag: TagHash,
) {
    hash_detail_field(
        ui,
        "Definition Index",
        metadata.definition_index.to_string(),
        true,
    );
    hash_detail_field(
        ui,
        "Definition Tag",
        format_hash_hex(u64::from(metadata.definition_tag)),
        true,
    );
    hash_detail_field(
        ui,
        "Package ID",
        format!(
            "{} · 0x{:04X}",
            definition_tag.pkg_id(),
            definition_tag.pkg_id()
        ),
        true,
    );
    hash_detail_field(
        ui,
        "Package Entry Index",
        definition_tag.entry_index().to_string(),
        true,
    );
    if let Some(size) = metadata.definition_size {
        hash_detail_field(ui, "Definition Record Size", format!("{size} bytes"), true);
    }
    hash_detail_field(
        ui,
        "Package",
        catalog
            .item_package_name(item_hash)
            .unwrap_or("<not present>"),
        false,
    );
}

pub(super) fn draw_hash_item_package_reference_fields(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    metadata: &ItemPackageMetadata,
    material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
) {
    if let Some(tag) = metadata.string_definition_tag {
        hash_detail_field(
            ui,
            "String Definition Tag",
            format_hash_hex(u64::from(tag)),
            true,
        );
    }
    if let Some(tag) = metadata.icon_container_tag {
        hash_detail_field(
            ui,
            "Icon Container Tag",
            format_hash_hex(u64::from(tag)),
            true,
        );
    }
    if let Some(category_hash) = metadata.plug_category_hash {
        catalog_hash_hex_and_decimal_field(ui, catalog, "Plug Category Hash", category_hash);
    }
    if let Some(slot) = metadata.equipment_slot {
        hash_detail_field(ui, "Native Equipment Slot ID", slot.to_string(), true);
    }
    if let Some(index) = metadata.socket_entry_list_index {
        hash_detail_field(ui, "Socket Entry List Index", index.to_string(), true);
    }
    if let Some(index) = metadata.roll_set_index {
        let meaning = match index {
            0 => "socketed directly",
            u16::MAX => "service-granted outside a roll set",
            _ => "server roll-set ordinal",
        };
        hash_detail_field(
            ui,
            "Plug Roll Set Index",
            format!("{index} · {meaning}"),
            true,
        );
    }
    if let Some(index) = metadata.linked_plug_index {
        hash_detail_field(ui, "Linked Plug Definition Index", index.to_string(), true);
    }
    if let Some(hash) = metadata.linked_plug_hash {
        ui.label(metadata_label_text(ui, "Linked Plug Definition"));
        ui.horizontal_wrapped(|ui| {
            draw_named_catalog_hash_link(
                ui,
                catalog,
                hash,
                catalog.package_item_name(hash).unwrap_or("Unnamed plug"),
            );
            draw_catalog_hash_link(ui, catalog, hash, format_hash_hex(hash));
        });
        ui.end_row();
    }
    if let Some(index) = metadata.weapon_pattern_index {
        hash_detail_field(ui, "Weapon Pattern Index", index.to_string(), true);
    }
    let arrangements = ["Generic", "Titan", "Hunter", "Warlock"]
        .into_iter()
        .zip(metadata.art_arrangement_indices)
        .filter_map(|(label, index)| index.map(|index| format!("{label} {index}")))
        .collect::<Vec<_>>();
    if !arrangements.is_empty() {
        hash_detail_field(
            ui,
            "Art Arrangement Indices",
            arrangements.join(" · "),
            true,
        );
    }
    if !metadata.render_overrides.is_empty() {
        hash_detail_field(
            ui,
            "Active Material Override Count",
            metadata.render_overrides.len().to_string(),
            true,
        );
    }
    if let Some(indices) = material_requirement_set_indices {
        draw_item_material_requirement_set_link(
            ui,
            catalog,
            "Insertion Material Requirement Set",
            indices.insertion,
        );
        draw_item_material_requirement_set_link(
            ui,
            catalog,
            "Enabled Material Requirement Set",
            indices.enabled,
        );
    }
}
