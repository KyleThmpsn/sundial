use super::*;

pub(super) fn draw_hash_item_source_comparison(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    item: Option<&ItemDef>,
    context: &DefinitionInspectionContext,
) {
    ui.add_space(8.0);
    hash_metadata_section(
        ui,
        if context.instance_id.is_some() {
            "Selected Instance"
        } else {
            "Source Snapshot"
        },
        true,
        |ui| {
            ui.strong(&context.source);
            ui.weak(
                "Snapshot captured when opened. Reopen the item after editing its account data.",
            );
            ui.add_space(6.0);
            egui::Grid::new(("hash_item_instance", hash))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    if catalog.item_has_power_stat(hash)
                        && let Some(level) = context.authored_level
                    {
                        let cap = catalog.item_power_cap(hash).map_or_else(
                            || "No catalog cap".into(),
                            |value| format!("Catalog cap {value}"),
                        );
                        hash_detail_field(
                            ui,
                            "Power",
                            format!(
                                "{} · authored level {level} · {cap}",
                                displayed_item_power(level)
                            ),
                            true,
                        );
                    }
                    if let Some(instance_id) = &context.instance_id {
                        hash_detail_field(ui, "Instance ID", instance_id, true);
                    }
                    if let Some(quantity) = context.quantity {
                        hash_detail_field(ui, "Quantity", quantity.to_string(), true);
                    }
                    if let Some(plugs) = &context.plugs {
                        hash_detail_field(
                            ui,
                            "Plug Source",
                            super::super::instance::plug_source(plugs),
                            false,
                        );
                    }
                    if let Some(plug_count) = context.plug_count {
                        let sockets = item.map_or_else(
                            || "Catalog sockets unavailable".into(),
                            |item| format!("{} catalog sockets", item.sockets.len()),
                        );
                        hash_detail_field(
                            ui,
                            "Saved Plugs",
                            format!("{plug_count} · {sockets}"),
                            true,
                        );
                    }
                    if let Some(flags) = context.flags {
                        hash_detail_field(ui, "Instance Flags", format!("0x{flags:02X}"), true);
                    }
                });
        },
    );
}

pub(super) fn draw_hash_item_identity_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    resolved_name: &Option<String>,
    item: Option<&ItemDef>,
    package_metadata: Option<&ItemPackageMetadata>,
    inventory_metadata: Option<&InventoryMetadata>,
) {
    ui.horizontal_top(|ui| {
        if let Some(icon) = catalog.icon_texture(ui.ctx(), hash) {
            ui.add(
                egui::Image::new(&icon)
                    .fit_to_exact_size(egui::vec2(72.0, 72.0))
                    .maintain_aspect_ratio(true),
            );
            ui.add_space(8.0);
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            crate::app::item_editor::catalog_item_tooltip(
                ui.label(
                    crate::app::ui::destiny_text(
                        ui,
                        resolved_name.as_deref().unwrap_or("Unnamed item"),
                    )
                        .strong()
                        .size(18.0),
                ),
                catalog,
                hash,
            );
            ui.label(metadata_label_text(
                ui,
                catalog
                    .package_item_type_name(hash)
                    .unwrap_or("Type not present"),
            ));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let mut drew = false;
                draw_inline_item_hash(ui, &mut drew, catalog, hash);
                if let Some(metadata) = package_metadata {
                    if let Some(ammo) = metadata.weapon_ammo_type {
                        draw_inline_item_fact_with_tooltip(
                            ui, &mut drew, "Ammo", ammo.label(), false,
                            "Primary/Special/Heavy client classification. This does not establish which ammo pool the runtime weapon consumes.",
                        );
                    }
                    if metadata.rarity != ItemRarity::Unknown {
                        draw_inline_item_fact_with_tooltip(
                            ui,
                            &mut drew,
                            "Rarity",
                            metadata.rarity.label(),
                            false,
                            format!(
                                "Stored directly in the item definition as package rarity value {}. This field has no separate definition hash.",
                                metadata.rarity.package_value().unwrap_or_default()
                            ),
                        );
                    }
                    if let Some(damage_type) = metadata.damage_type {
                        draw_inline_item_fact_with_tooltip(
                            ui,
                            &mut drew,
                            "Damage",
                            damage_type.label(),
                            false,
                            "Package-derived damage classification, not a live measurement. The Technical tab distinguishes modern, legacy, and default-plug inference.",
                        );
                    }
                    if let Some(power_cap) = metadata.power_cap {
                        draw_inline_item_fact_with_tooltip(
                            ui,
                            &mut drew,
                            "Power Cap",
                            power_cap.to_string(),
                            true,
                            "Read from the installed power-cap definition table using this item's ordered version indices. See Technical for the individual rows and definition hashes.",
                        );
                    }
                }
                if let Some(item) = item {
                    draw_inline_item_fact_with_tooltip(
                        ui,
                        &mut drew,
                        "Class",
                        format!(
                            "{} ({})",
                            item_class_type_label(item.class_type),
                            item.class_type
                        ),
                        false,
                        format!(
                            "Stored directly as item class type {}. This field has no separate definition hash.",
                            item.class_type
                        ),
                    );
                }
            });
            if let Some(metadata) = inventory_metadata {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let mut drew = false;
                    let bucket_label = format!(
                        "{} ({})",
                        metadata.bucket_label(),
                        metadata.native_bucket_id
                    );
                    if let Some(bucket_hash) = item.map(|item| item.bucket_hash).filter(|hash| *hash != 0)
                    {
                        draw_inline_item_definition_fact(
                            ui,
                            &mut drew,
                            catalog,
                            "Bucket",
                            bucket_label,
                            bucket_hash,
                        );
                    } else {
                        draw_inline_item_fact(
                            ui,
                            &mut drew,
                            "Bucket",
                            bucket_label,
                            false,
                        );
                    }
                    draw_inline_item_fact(
                        ui,
                        &mut drew,
                        "Storage",
                        format!(
                            "{} · {}",
                            metadata.scope.label(),
                            metadata.stackability.label()
                        ),
                        false,
                    );
                    if let Some(maximum) = metadata.max_stack_size {
                        draw_inline_item_fact(
                            ui,
                            &mut drew,
                            "Max Stack",
                            maximum.to_string(),
                            true,
                        );
                    }
                    if let Some(capacity) = metadata.bucket_capacity {
                        draw_inline_item_fact(
                            ui,
                            &mut drew,
                            "Bucket Capacity",
                            capacity.to_string(),
                            true,
                        );
                    }
                });
            }
        });
    });
}

pub(super) fn draw_inline_item_hash(
    ui: &mut egui::Ui,
    drew: &mut bool,
    catalog: &Catalog,
    hash: u64,
) {
    if *drew {
        ui.weak("·");
    }
    ui.label(metadata_label_text(ui, "Hash"));
    crate::app::item_editor::catalog_item_tooltip(
        ui.label(
            egui::RichText::new(format_hash_hex(hash))
                .strong()
                .monospace(),
        ),
        catalog,
        hash,
    );
    *drew = true;
}

pub(super) fn draw_inline_item_fact(
    ui: &mut egui::Ui,
    drew: &mut bool,
    label: &str,
    value: impl Into<String>,
    monospace: bool,
) {
    if *drew {
        ui.weak("·");
    }
    ui.label(metadata_label_text(ui, label));
    let value = egui::RichText::new(value.into()).strong();
    ui.label(if monospace { value.monospace() } else { value });
    *drew = true;
}

pub(super) fn draw_inline_item_fact_with_tooltip(
    ui: &mut egui::Ui,
    drew: &mut bool,
    label: &str,
    value: impl Into<String>,
    monospace: bool,
    tooltip: impl Into<String>,
) {
    if *drew {
        ui.weak("·");
    }
    ui.label(metadata_label_text(ui, label));
    let value = egui::RichText::new(value.into()).strong();
    ui.label(if monospace { value.monospace() } else { value })
        .on_hover_text(tooltip.into());
    *drew = true;
}

pub(super) fn draw_inline_item_definition_fact(
    ui: &mut egui::Ui,
    drew: &mut bool,
    catalog: &Catalog,
    label: &str,
    value: impl Into<String>,
    hash: u64,
) {
    if *drew {
        ui.weak("·");
    }
    ui.label(metadata_label_text(ui, label));
    draw_named_catalog_hash_link(ui, catalog, hash, value);
    *drew = true;
}
