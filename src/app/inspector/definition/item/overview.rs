use super::*;

pub(super) fn draw_overview(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if let Some(description) = content
        .catalog
        .description(content.hash)
        .filter(|text| !text.trim().is_empty())
    {
        ui.label(crate::app::ui::destiny_text(ui, description));
    }
    if content.source_context.is_some() && ui.available_width() >= 840.0 {
        ui.columns(2, |columns| {
            draw_overview_source(&mut columns[0], content);
            columns[1].add_space(6.0);
            draw_overview_stats(&mut columns[1], content);
        });
    } else {
        draw_overview_source(ui, content);
        draw_overview_stats(ui, content);
    }
    if let Some(metadata) = content.matches.item_package_metadata {
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

pub(super) fn draw_hash_inventory_placement_summary(
    ui: &mut egui::Ui,
    metadata: &InventoryMetadata,
) {
    metadata_subsection(ui, "Placement", |ui| {
        egui::Grid::new("hash_inventory_metadata_placement")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Storage Scope", metadata.scope.label(), false);
                hash_detail_field(
                    ui,
                    "Native Bucket ID",
                    metadata.native_bucket_id.to_string(),
                    true,
                );
            });
    });
}

pub(super) fn draw_hash_inventory_capacity_summary(
    ui: &mut egui::Ui,
    metadata: &InventoryMetadata,
) {
    metadata_subsection(ui, "Capacity", |ui| {
        egui::Grid::new("hash_inventory_metadata_capacity")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Stackability", metadata.stackability.label(), false);
                hash_detail_field(
                    ui,
                    "Maximum Stack Size",
                    metadata
                        .max_stack_size
                        .map_or_else(|| "<none>".into(), |value| value.to_string()),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Inventory Bucket Capacity",
                    metadata
                        .bucket_capacity
                        .map_or_else(|| "<none>".into(), |value| value.to_string()),
                    true,
                );
            });
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
    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Stats ({})", metadata.investment_stats.len()))
        .id_salt(("hash_item_investment_stats", item_hash))
        .default_open(true)
        .show(ui, |ui| {
            let technical_id = ui.id().with("stat_technical_ids");
            let mut technical = ui.data_mut(|data| data.get_temp::<bool>(technical_id).unwrap_or(false));
            ui.checkbox(&mut technical, "Show Technical IDs");
            ui.data_mut(|data| data.insert_temp(technical_id, technical));
            egui::ScrollArea::horizontal().id_salt(("stat_columns", item_hash)).show(ui, |ui| {
            egui::Grid::new(("hash_item_investment_stat_rows", item_hash))
                .num_columns(if technical { 6 } else { 3 })
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Stat");
                    ui.strong("Investment Value")
                        .on_hover_text("Raw value stored in the investment block");
                    ui.strong("Display Value").on_hover_text(
                        "Value calculated from this item's decoded stat-group display curve, not a live gameplay measurement",
                    );
                    if technical {
                        ui.strong("Index");
                        ui.strong("Hex");
                        ui.strong("Decimal");
                    }
                    ui.end_row();
                    for stat in &metadata.investment_stats {
                        let definition = catalog.item_stat_definition(stat.definition_index);
                        let stat_name = definition
                            .map(|definition| definition.name.as_str())
                            .filter(|name| !name.trim().is_empty());
                        if let Some(definition) = definition {
                            draw_named_catalog_hash_link(ui, catalog, definition.hash, stat_name.unwrap_or("Unnamed Stat"));
                        } else {
                            ui.weak("-");
                        }
                        ui.monospace(stat.value.to_string());
                        ui.monospace(catalog.item_in_game_stat_display(item_hash, stat));
                        if technical {
                            ui.monospace(stat.definition_index.to_string());
                            if let Some(definition) = definition {
                                draw_hash_hex_and_decimal_cells(ui, definition.hash);
                            } else {
                                ui.label("-");
                                ui.label("-");
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
}

pub(super) fn draw_hash_item_stat_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
) {
    let Some(definition) = matches.item_stat_definition else {
        return;
    };
    ui.add_space(8.0);
    hash_metadata_section(ui, "Investment Stat Definition", true, |ui| {
        egui::Grid::new(("hash_item_stat_definition", definition.hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "Name",
                    if definition.name.trim().is_empty() {
                        "<not present>"
                    } else {
                        &definition.name
                    },
                    false,
                );
                hash_detail_field(
                    ui,
                    "Definition Index",
                    definition.definition_index.to_string(),
                    true,
                );
            });

        let references = &matches.investment_stat_references;
        if references.is_empty() {
            return;
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(format!("Items Using This Stat ({})", references.len()))
            .id_salt(("hash_item_stat_references", definition.hash))
            .default_open(references.len() <= 12)
            .show(ui, |ui| {
                egui::Grid::new(("hash_item_stat_reference_rows", definition.hash))
                    .num_columns(5)
                    .spacing([16.0, 3.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Item");
                        ui.strong("Hex");
                        ui.strong("Decimal");
                        ui.strong("Investment Value")
                            .on_hover_text("Raw value stored in the investment block");
                        ui.strong("Display Value").on_hover_text(
                            "Value produced by each item's decoded stat-group display curve",
                        );
                        ui.end_row();
                        for (item_hash, stat) in references {
                            ui.label(
                                catalog
                                    .package_item_name(*item_hash)
                                    .unwrap_or("<not present>"),
                            );
                            draw_hash_hex_and_decimal_cells(ui, *item_hash);
                            ui.monospace(stat.value.to_string());
                            ui.monospace(catalog.item_in_game_stat_display(*item_hash, stat));
                            ui.end_row();
                        }
                    });
            });
    });
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

    ui.add_space(8.0);
    hash_metadata_section(ui, "Inventory Bucket", false, |ui| {
        egui::Grid::new(("hash_inventory_bucket", hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "Definition Hash",
                    format_hash_hex_and_decimal(hash),
                    true,
                );
                hash_detail_field(ui, "Items", items.len().to_string(), true);
                hash_detail_field(
                    ui,
                    "Native Bucket IDs",
                    if native_buckets.is_empty() {
                        "<not resolved>".into()
                    } else {
                        native_buckets.join(" · ")
                    },
                    false,
                );
            });
        egui::CollapsingHeader::new(format!("Items ({})", items.len()))
            .id_salt(("hash_inventory_bucket_items", hash))
            .default_open(items.len() <= 20)
            .show(ui, |ui| {
                draw_hash_item_rows(
                    ui,
                    catalog,
                    ("bucket_items", hash, 0_usize),
                    items.iter().map(|item| item.hash),
                );
            });
    });
}

pub(super) fn draw_hash_item_rows(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: (&'static str, u64, usize),
    hashes: impl IntoIterator<Item = u64>,
) {
    let hashes = hashes.into_iter().collect::<Vec<_>>();
    let filter_id = ui.id().with(("item_rows_filter", id));
    let mut query = ui.data_mut(|data| data.get_temp::<String>(filter_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Filter by Name, Type, or Hash")
                .desired_width(260.0),
        );
        if !query.is_empty() && ui.small_button("Clear").clicked() {
            query.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(filter_id, query.clone()));
    let query = query.trim().to_lowercase();
    let matching = hashes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, hash)| {
            query.is_empty()
                || format_hash_hex(*hash).to_lowercase().contains(&query)
                || catalog
                    .package_item_name(*hash)
                    .is_some_and(|name| name.to_lowercase().contains(&query))
                || catalog
                    .package_item_type_name(*hash)
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    ui.weak(format!("{} of {} members", matching.len(), hashes.len()));
    if matching.is_empty() {
        ui.weak("No members match this filter.");
        return;
    }
    const MAX_VISIBLE_ROWS: usize = 18;
    let row_height = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y)
        + 3.0;
    let visible_rows = matching.len().clamp(1, MAX_VISIBLE_ROWS) + 1;
    let table_height = row_height * visible_rows as f32;
    egui::ScrollArea::vertical()
        .id_salt(("hash_item_rows", id))
        .min_scrolled_height(table_height)
        .max_height(table_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new(("hash_item_row_grid", id))
                .num_columns(4)
                .striped(true)
                .spacing([16.0, 3.0])
                .show(ui, |ui| {
                    ui.strong("Row");
                    ui.strong("Hash");
                    ui.strong("Name");
                    ui.strong("Type");
                    ui.end_row();
                    for (index, hash) in matching {
                        ui.monospace((index + 1).to_string());
                        draw_catalog_hash_link(ui, catalog, hash, format_hash_hex(hash));
                        let name = catalog
                            .package_item_name(hash)
                            .unwrap_or("Name not resolved");
                        crate::app::item_editor::catalog_item_tooltip(
                            ui.label(name),
                            catalog,
                            hash,
                        );
                        ui.label(
                            catalog
                                .package_item_type_name(hash)
                                .unwrap_or("Type not resolved"),
                        );
                        ui.end_row();
                    }
                });
        });
}
