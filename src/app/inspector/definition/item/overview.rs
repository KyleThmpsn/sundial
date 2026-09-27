use super::super::item_details::{Cell, ItemRowValue, draw_hash_item_rows, draw_table};
use super::*;
use crate::app::inspector::look;

pub(super) fn draw_overview(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if content.source_context.is_some() && ui.available_width() >= 840.0 {
        ui.columns(2, |columns| {
            draw_overview_source(&mut columns[0], content);
            draw_overview_stats(&mut columns[1], content);
        });
    } else {
        draw_overview_source(ui, content);
        draw_overview_stats(ui, content);
    }
    if let Some(metadata) = content.matches.item_package_metadata {
        draw_plug_summary(ui, content.catalog, content.hash, metadata);
        super::super::item_details::draw_item_traits(ui, content.catalog, content.hash, metadata);
    }
    if let Some(item) = content.matches.item {
        draw_hash_item_abilities(ui, content.catalog, item);
    }
}

pub(super) fn draw_overview_source(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if let Some(context) = content.source_context {
        draw_hash_item_source_comparison(
            ui,
            content.catalog,
            content.hash,
            content.matches.item,
            context,
        );
    }
}

pub(super) fn draw_overview_stats(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if let Some(metadata) = content.matches.item_package_metadata {
        draw_hash_item_investment_stats(ui, content.catalog, content.hash, metadata);
    }
}

/// The category a plug belongs to, which lists the other plugs in it.
fn draw_plug_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    let Some(category) = metadata
        .plug_category_hash
        .filter(|category| *category != 0 && *category != u64::from(u32::MAX))
        .filter(|_| catalog.item_kind_label(hash) == "Plug")
    else {
        return;
    };
    ui.add_space(8.0);
    look::properties(ui, ("item_plug_summary", hash), |p| {
        p.link(
            "Plug Category",
            catalog,
            category,
            catalog
                .display_name(category)
                .map_or_else(|| format_hash_hex(category), str::to_owned),
        );
    });
}

pub(super) fn draw_hash_inventory_placement_summary(
    ui: &mut egui::Ui,
    metadata: &InventoryMetadata,
) {
    look::subheading(ui, "Placement");
    look::properties(ui, "hash_inventory_metadata_placement", |p| {
        p.text("Storage Scope", metadata.scope.label());
        p.mono("Native Bucket ID", metadata.native_bucket_id.to_string());
    });
}

pub(super) fn draw_hash_inventory_capacity_summary(
    ui: &mut egui::Ui,
    metadata: &InventoryMetadata,
) {
    look::subheading(ui, "Capacity");
    look::properties(ui, "hash_inventory_metadata_capacity", |p| {
        p.text("Stackability", metadata.stackability.label());
        if let Some(value) = metadata.max_stack_size {
            p.mono("Maximum Stack Size", value.to_string());
        }
        if let Some(value) = metadata.bucket_capacity {
            p.mono("Inventory Bucket Capacity", value.to_string());
        }
    });
}

pub(super) fn draw_hash_item_investment_stats(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if metadata.investment_stats.is_empty() {
        return;
    }
    look::section(
        ui,
        ("hash_item_investment_stats", item_hash),
        "Stats",
        Some(metadata.investment_stats.len()),
        true,
        |ui| {
            let technical_id = egui::Id::new(("stat_technical_ids", item_hash));
            let mut technical =
                ui.data_mut(|data| data.get_temp::<bool>(technical_id).unwrap_or(false));
            ui.checkbox(&mut technical, "Show Technical IDs");
            ui.data_mut(|data| data.insert_temp(technical_id, technical));
            egui::ScrollArea::horizontal()
                .id_salt(("stat_columns", item_hash))
                .show(ui, |ui| {
                    egui::Grid::new(("hash_item_investment_stat_rows", item_hash))
                        .num_columns(if technical { 5 } else { 3 })
                        .spacing([16.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Stat");
                            ui.strong("Investment Value");
                            ui.strong("Display Value");
                            if technical {
                                ui.strong("Index");
                                ui.strong("Hash");
                            }
                            ui.end_row();
                            for stat in &metadata.investment_stats {
                                let definition =
                                    catalog.item_stat_definition(stat.definition_index);
                                if let Some(definition) = definition {
                                    let name = definition.name.trim();
                                    let stat_name = if name.is_empty() {
                                        format_hash_hex(definition.hash)
                                    } else {
                                        name.to_owned()
                                    };
                                    draw_named_catalog_hash_link(
                                        ui,
                                        catalog,
                                        definition.hash,
                                        stat_name,
                                    );
                                } else {
                                    ui.label(format!("Stat #{}", stat.definition_index));
                                }
                                ui.monospace(stat.value.to_string());
                                ui.monospace(catalog.item_in_game_stat_display(item_hash, stat));
                                if technical {
                                    ui.monospace(stat.definition_index.to_string());
                                    if let Some(definition) = definition {
                                        ui.label(
                                            egui::RichText::new(format_hash_hex_and_decimal(
                                                definition.hash,
                                            ))
                                            .monospace()
                                            .color(look::muted(ui)),
                                        );
                                    } else {
                                        ui.label("");
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
        },
    );
}

pub(super) fn draw_hash_item_stat_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
) {
    let Some(definition) = matches.item_stat_definition else {
        return;
    };
    look::properties(ui, ("hash_item_stat_definition", definition.hash), |p| {
        p.mono("Definition Index", definition.definition_index.to_string());
    });

    let groups = catalog.stat_groups_with_stat(definition.definition_index);
    if !groups.is_empty() {
        let rows = groups
            .iter()
            .filter_map(|&group_index| {
                let group = catalog.item_stat_group_by_index(u16::try_from(group_index).ok()?)?;
                let stat = group
                    .scaled_stats
                    .iter()
                    .find(|stat| stat.definition_index == definition.definition_index)?;
                Some(vec![
                    Cell::Link(group.hash, format!("Group {group_index}")),
                    Cell::mono(group.maximum_value),
                    Cell::text(if stat.is_linear {
                        "Linear"
                    } else {
                        "Interpolated"
                    }),
                    Cell::mono(stat.display_interpolation.len()),
                ])
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("hash_item_stat_groups", definition.hash),
            "Stat Groups",
            Some(rows.len()),
            rows.len() <= super::super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("hash_item_stat_groups", definition.hash),
                    &[
                        "Stat Group",
                        "Maximum Value",
                        "Interpolation",
                        "Curve Points",
                    ],
                    &rows,
                );
            },
        );
    }

    let references = &matches.investment_stat_references;
    if references.is_empty() {
        return;
    }
    look::section(
        ui,
        ("hash_item_stat_references", definition.hash),
        "Items Using This Stat",
        Some(references.len()),
        references.len() <= 12,
        |ui| {
            let investment = |position: usize| references[position].1.value.to_string();
            let display = |position: usize| {
                let (item_hash, stat) = references[position];
                catalog.item_in_game_stat_display(item_hash, stat)
            };
            let values: [ItemRowValue<'_>; 2] = [
                ("Investment Value", &investment),
                ("Display Value", &display),
            ];
            draw_hash_item_rows(
                ui,
                catalog,
                egui::Id::new(("hash_item_stat_references", definition.hash)),
                references.iter().map(|(item_hash, _)| *item_hash),
                &values,
            );
        },
    );
}

pub(super) fn draw_hash_inventory_bucket(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    items: &[&crate::catalog::ItemDef],
) {
    let mut native_buckets = items
        .iter()
        .filter_map(|item| catalog.inventory_metadata(item.hash))
        .map(|metadata| metadata.bucket_label())
        .collect::<Vec<_>>();
    native_buckets.sort();
    native_buckets.dedup();
    look::properties(ui, ("hash_inventory_bucket", hash), |p| {
        p.text("Native Buckets", native_buckets.join(" · "));
    });
    look::section(
        ui,
        ("hash_inventory_bucket_items", hash),
        "Items",
        Some(items.len()),
        true,
        |ui| {
            draw_hash_item_rows(
                ui,
                catalog,
                egui::Id::new(("bucket_items", hash)),
                items.iter().map(|item| item.hash),
                &[],
            );
        },
    );
}
