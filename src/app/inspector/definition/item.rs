use super::*;

pub(super) fn draw_hash_item_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    resolved_name: &Option<String>,
    matches: &CatalogHashMatches<'_>,
) {
    let item = matches.item;
    let item_package_metadata = matches.item_package_metadata;
    let inventory_metadata = matches.inventory_metadata;
    let bucket_items = &matches.bucket_items;
    let item_material_requirement_set_indices = matches.item_material_requirement_set_indices;

    if item_package_metadata.is_some() || item.is_some() {
        ui.add_space(8.0);
        hash_metadata_section(ui, "Inventory item definition", true, |ui| {
            if let Some(metadata) = item_package_metadata
                && hash_inspector_uses_wide_summary(ui.available_width())
            {
                ui.columns(2, |columns| {
                    draw_hash_item_identity_summary(
                        &mut columns[0],
                        catalog,
                        hash,
                        resolved_name,
                        item,
                    );
                    draw_hash_item_package_metadata(
                        &mut columns[1],
                        catalog,
                        hash,
                        metadata,
                        item_material_requirement_set_indices,
                    );
                });
            } else {
                draw_hash_item_identity_summary(ui, catalog, hash, resolved_name, item);
                if let Some(metadata) = item_package_metadata {
                    ui.add_space(8.0);
                    draw_hash_item_package_metadata(
                        ui,
                        catalog,
                        hash,
                        metadata,
                        item_material_requirement_set_indices,
                    );
                }
            }
            if let Some(description) = catalog
                .description(hash)
                .filter(|description| !description.trim().is_empty())
            {
                ui.add_space(6.0);
                ui.label(description);
            }
            if let Some(metadata) = item_package_metadata {
                draw_hash_item_investment_stats(ui, catalog, hash, metadata);
                draw_hash_item_intrinsic_perks(ui, hash, metadata);
            }
            if let Some(item) = item {
                draw_hash_item_sockets(ui, catalog, item);
                draw_hash_item_abilities(ui, item);
            }
        });
    }

    draw_hash_item_stat_matches(ui, catalog, matches);
    draw_hash_intrinsic_perk_matches(ui, catalog, hash, matches);

    if let Some(metadata) = inventory_metadata {
        ui.add_space(8.0);
        hash_metadata_section(ui, "Inventory metadata", true, |ui| {
            if hash_inspector_uses_wide_summary(ui.available_width()) {
                ui.columns(2, |columns| {
                    draw_hash_inventory_placement_summary(&mut columns[0], metadata);
                    draw_hash_inventory_capacity_summary(&mut columns[1], metadata);
                });
            } else {
                draw_hash_inventory_placement_summary(ui, metadata);
                ui.add_space(8.0);
                draw_hash_inventory_capacity_summary(ui, metadata);
            }
        });
    }

    if !bucket_items.is_empty() {
        draw_hash_inventory_bucket(ui, catalog, hash, bucket_items);
    }
}

fn draw_hash_item_identity_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    resolved_name: &Option<String>,
    item: Option<&ItemDef>,
) {
    metadata_subsection(ui, "Item", |ui| {
        ui.horizontal_top(|ui| {
            if let Some(icon) = catalog.icon_texture(ui.ctx(), hash) {
                ui.add(
                    egui::Image::new(&icon)
                        .fit_to_exact_size(egui::vec2(88.0, 88.0))
                        .maintain_aspect_ratio(true),
                );
                ui.add_space(10.0);
            }
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(resolved_name.as_deref().unwrap_or("Unnamed item"))
                        .strong()
                        .size(18.0),
                );
                ui.label(
                    egui::RichText::new(
                        catalog
                            .package_item_type_name(hash)
                            .unwrap_or("Type not present"),
                    )
                    .weak(),
                );
                egui::Grid::new(("hash_inventory_item_summary", hash))
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        hash_detail_field(ui, "Definition hash", format_hash_hex(hash), true);
                        if let Some(item) = item {
                            hash_hex_and_decimal_field(ui, "Bucket hash", item.bucket_hash);
                            hash_detail_field(
                                ui,
                                "Class type",
                                format!(
                                    "{} ({})",
                                    item_class_type_label(item.class_type),
                                    item.class_type
                                ),
                                false,
                            );
                        }
                    });
            });
        });
    });
}

fn draw_hash_item_package_metadata(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
) {
    metadata_subsection(ui, "Package definition", |ui| {
        egui::Grid::new(("hash_item_package_metadata_rows", item_hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "Definition index",
                    metadata.definition_index.to_string(),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Definition tag",
                    format_hash_hex(u64::from(metadata.definition_tag)),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Package",
                    catalog
                        .item_package_name(item_hash)
                        .unwrap_or("<not present>"),
                    false,
                );
                if let Some(category_hash) = metadata.plug_category_hash {
                    hash_hex_and_decimal_field(ui, "Plug category hash", category_hash);
                }
                if let Some(indices) = material_requirement_set_indices {
                    draw_item_material_requirement_set_link(
                        ui,
                        catalog,
                        "Insertion material requirement set",
                        indices.insertion,
                    );
                    draw_item_material_requirement_set_link(
                        ui,
                        catalog,
                        "Enabled material requirement set",
                        indices.enabled,
                    );
                }
            });
    });
}

fn draw_hash_inventory_placement_summary(ui: &mut egui::Ui, metadata: &InventoryMetadata) {
    metadata_subsection(ui, "Placement", |ui| {
        egui::Grid::new("hash_inventory_metadata_placement")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Scope", metadata.scope.label(), false);
                hash_detail_field(
                    ui,
                    "Native bucket",
                    metadata.native_bucket_id.to_string(),
                    true,
                );
            });
    });
}

fn draw_hash_inventory_capacity_summary(ui: &mut egui::Ui, metadata: &InventoryMetadata) {
    metadata_subsection(ui, "Capacity", |ui| {
        egui::Grid::new("hash_inventory_metadata_capacity")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Stackability", metadata.stackability.label(), false);
                hash_detail_field(
                    ui,
                    "Maximum stack",
                    metadata
                        .max_stack_size
                        .map_or_else(|| "<none>".into(), |value| value.to_string()),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Bucket capacity",
                    metadata
                        .bucket_capacity
                        .map_or_else(|| "<none>".into(), |value| value.to_string()),
                    true,
                );
            });
    });
}

fn draw_hash_item_investment_stats(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if metadata.investment_stats.is_empty() {
        return;
    }
    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!(
        "Investment stats ({})",
        metadata.investment_stats.len()
    ))
    .id_salt(("hash_item_investment_stats", item_hash))
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new(("hash_item_investment_stat_rows", item_hash))
            .num_columns(5)
            .spacing([16.0, 3.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Stat");
                ui.strong("Hex");
                ui.strong("Decimal");
                ui.strong("Value");
                ui.end_row();
                for stat in &metadata.investment_stats {
                    let definition = catalog.item_stat_definition(stat.definition_index);
                    ui.monospace(stat.definition_index.to_string());
                    let stat_name = definition
                        .map(|definition| definition.name.as_str())
                        .filter(|name| !name.trim().is_empty());
                    ui.label(stat_name.unwrap_or("-"));
                    if let Some(definition) = definition {
                        draw_hash_hex_and_decimal_cells(ui, definition.hash);
                    } else {
                        ui.label(egui::RichText::new("-").weak());
                        ui.label(egui::RichText::new("-").weak());
                    }
                    ui.monospace(stat.value.to_string());
                    ui.end_row();
                }
            });
    });
}

fn draw_hash_item_intrinsic_perks(
    ui: &mut egui::Ui,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if metadata.intrinsic_perks.is_empty() {
        return;
    }
    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!(
        "Intrinsic perks ({})",
        metadata.intrinsic_perks.len()
    ))
    .id_salt(("hash_item_intrinsic_perks", item_hash))
    .default_open(metadata.intrinsic_perks.len() <= 4)
    .show(ui, |ui| {
        egui::Grid::new(("hash_item_intrinsic_perk_rows", item_hash))
            .num_columns(4)
            .spacing([16.0, 3.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Hex");
                ui.strong("Decimal");
                ui.strong("Name");
                ui.end_row();
                for perk in &metadata.intrinsic_perks {
                    ui.monospace(perk.definition_index.to_string());
                    draw_hash_hex_and_decimal_cells(ui, perk.hash);
                    unresolved_name_cell(ui, 220.0, SANDBOX_PERK_NAME_UNRESOLVED_HELP);
                    ui.end_row();
                }
            });
    });
}

fn draw_hash_item_stat_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
) {
    let Some(definition) = matches.item_stat_definition else {
        return;
    };
    ui.add_space(8.0);
    hash_metadata_section(ui, "Investment stat definition", true, |ui| {
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
                    "Definition index",
                    definition.definition_index.to_string(),
                    true,
                );
            });

        let references = &matches.investment_stat_references;
        if references.is_empty() {
            return;
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(format!("Item references ({})", references.len()))
            .id_salt(("hash_item_stat_references", definition.hash))
            .default_open(references.len() <= 12)
            .show(ui, |ui| {
                egui::Grid::new(("hash_item_stat_reference_rows", definition.hash))
                    .num_columns(4)
                    .spacing([16.0, 3.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Item");
                        ui.strong("Hex");
                        ui.strong("Decimal");
                        ui.strong("Value");
                        ui.end_row();
                        for (item_hash, stat) in references {
                            ui.label(
                                catalog
                                    .package_item_name(*item_hash)
                                    .unwrap_or("<not present>"),
                            );
                            draw_hash_hex_and_decimal_cells(ui, *item_hash);
                            ui.monospace(stat.value.to_string());
                            ui.end_row();
                        }
                    });
            });
    });
}

fn draw_hash_intrinsic_perk_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let references = matches.intrinsic_perk_item_references;
    if references.is_empty() {
        return;
    }
    ui.add_space(8.0);
    hash_metadata_section(
        ui,
        &format!("Intrinsic perk references ({})", references.len()),
        references.len() <= 12,
        |ui| {
            egui::Grid::new(("hash_intrinsic_perk_reference_rows", hash))
                .num_columns(3)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Item");
                    ui.strong("Hex");
                    ui.strong("Decimal");
                    ui.end_row();
                    for item_hash in references {
                        ui.label(
                            catalog
                                .package_item_name(*item_hash)
                                .unwrap_or("<not present>"),
                        );
                        draw_hash_hex_and_decimal_cells(ui, *item_hash);
                        ui.end_row();
                    }
                });
        },
    );
}

fn draw_item_material_requirement_set_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &str,
    index: Option<u16>,
) {
    let Some(index) = index else {
        return;
    };
    hash_detail_field(ui, &format!("{label} index"), index.to_string(), true);
    if let Some(set) = catalog.material_requirement_set(usize::from(index)) {
        ui.label(egui::RichText::new(format!("{label} hash")).weak());
        draw_hash_link(ui, set.hash, format_hash_hex_and_decimal(set.hash));
        ui.end_row();
    }
}

fn draw_hash_item_sockets(ui: &mut egui::Ui, catalog: &Catalog, item: &crate::catalog::ItemDef) {
    let socket_count = item.sockets.len().max(item.default_plugs.len());
    if socket_count == 0 {
        return;
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Sockets ({socket_count})"))
        .id_salt(("hash_item_sockets", item.hash))
        .default_open(socket_count <= 4)
        .show(ui, |ui| {
            egui::Grid::new(("hash_item_socket_rows", item.hash))
                .num_columns(6)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Index");
                    ui.strong("Socket");
                    ui.strong("Socket type");
                    ui.strong("Hash");
                    ui.strong("Name");
                    ui.strong("Plugs");
                    ui.end_row();
                    for socket_index in 0..socket_count {
                        let socket = item.sockets.get(socket_index);
                        let default_hash = item
                            .default_plugs
                            .get(socket_index)
                            .and_then(Option::as_deref)
                            .and_then(parse_hash_hex);
                        ui.monospace(socket_index.to_string());
                        ui.label(socket.map_or("<not present>".into(), |socket| {
                            socket.display_label(socket_index)
                        }));
                        ui.monospace(
                            socket.map_or_else(
                                || "-".into(),
                                |socket| socket.socket_type.to_string(),
                            ),
                        );
                        if let Some(default_hash) = default_hash {
                            draw_hash_link(ui, default_hash, format_hash_hex(default_hash));
                            item_definition_name_cell(ui, catalog, default_hash, 220.0);
                        } else {
                            ui.label(egui::RichText::new("-").weak());
                            table_cell(ui, 220.0, egui::RichText::new("-").weak());
                        }
                        ui.monospace(
                            socket
                                .map_or(0, |socket| catalog.socket_options(socket).len())
                                .to_string(),
                        );
                        ui.end_row();
                    }
                });

            for (socket_index, socket) in item.sockets.iter().enumerate() {
                let options = catalog.socket_options(socket);
                if options.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(format!(
                    "Socket {socket_index} plugs ({})",
                    options.len()
                ))
                .id_salt(("hash_item_socket_plugs", item.hash, socket_index))
                .show(ui, |ui| {
                    draw_hash_item_rows(
                        ui,
                        catalog,
                        ("socket_plugs", item.hash, socket_index),
                        options.iter().copied(),
                    );
                });
            }
        });
}

fn draw_hash_item_abilities(ui: &mut egui::Ui, item: &crate::catalog::ItemDef) {
    let abilities = &item.abilities;
    let choice_count = abilities.movement.len()
        + abilities.grenade.len()
        + abilities.super_ability.len()
        + abilities.melee.len()
        + abilities.class_ability.len()
        + abilities
            .attunements
            .iter()
            .map(|attunement| {
                attunement.super_abilities.len()
                    + attunement.perks.len()
                    + usize::from(attunement.melee.entry != 0)
            })
            .sum::<usize>();
    if choice_count == 0 {
        return;
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Abilities ({choice_count})"))
        .id_salt(("hash_item_abilities", item.hash))
        .show(ui, |ui| {
            egui::Grid::new(("hash_item_ability_rows", item.hash))
                .num_columns(3)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Ability");
                    ui.strong("Hash");
                    ui.strong("Name");
                    ui.end_row();
                    for (label, choices) in [
                        ("Movement", abilities.movement.as_slice()),
                        ("Grenade", abilities.grenade.as_slice()),
                        ("Super ability", abilities.super_ability.as_slice()),
                        ("Melee", abilities.melee.as_slice()),
                        ("Class ability", abilities.class_ability.as_slice()),
                    ] {
                        for choice in choices {
                            draw_hash_ability_row(ui, label, choice);
                        }
                    }
                    for attunement in &abilities.attunements {
                        for choice in &attunement.super_abilities {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Super ability", attunement.name),
                                choice,
                            );
                        }
                        if attunement.melee.entry != 0 {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Melee", attunement.name),
                                &attunement.melee,
                            );
                        }
                        for choice in &attunement.perks {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Perk", attunement.name),
                                choice,
                            );
                        }
                    }
                });
        });
}

fn draw_hash_ability_row(ui: &mut egui::Ui, label: &str, choice: &crate::catalog::AbilityChoice) {
    ui.label(label);
    draw_hash_link(ui, choice.entry, format_hash_hex(choice.entry));
    ui.label(metadata_text(&choice.name));
    ui.end_row();
}

fn draw_hash_inventory_bucket(
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
    hash_metadata_section(ui, "Inventory bucket", false, |ui| {
        egui::Grid::new(("hash_inventory_bucket", hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(
                    ui,
                    "Definition hash",
                    format_hash_hex_and_decimal(hash),
                    true,
                );
                hash_detail_field(ui, "Items", items.len().to_string(), true);
                hash_detail_field(
                    ui,
                    "Native buckets",
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

fn draw_hash_item_rows(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: (&'static str, u64, usize),
    hashes: impl IntoIterator<Item = u64>,
) {
    let hashes = hashes.into_iter().collect::<Vec<_>>();
    let hash_width = 126.0;
    let name_width = 240.0;
    let type_width = 180.0;
    ui.horizontal(|ui| {
        table_cell(ui, hash_width, egui::RichText::new("Hash").strong());
        table_cell(ui, name_width, egui::RichText::new("Name").strong());
        table_cell(ui, type_width, egui::RichText::new("Type").strong());
    });
    egui::ScrollArea::vertical()
        .id_salt(("hash_item_rows", id))
        .auto_shrink([false, false])
        .max_height(240.0)
        .show_rows(ui, TABLE_CELL_HEIGHT, hashes.len(), |ui, range| {
            egui::Grid::new(("hash_item_row_grid", id))
                .num_columns(3)
                .striped(true)
                .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                .show(ui, |ui| {
                    for row in range {
                        let hash = hashes[row];
                        draw_hash_hex_cell(ui, hash_width, Some(hash));
                        table_cell(
                            ui,
                            name_width,
                            catalog.package_item_name(hash).unwrap_or("<not present>"),
                        );
                        table_cell(
                            ui,
                            type_width,
                            catalog
                                .package_item_type_name(hash)
                                .unwrap_or("<not present>"),
                        );
                        ui.end_row();
                    }
                });
        });
}
