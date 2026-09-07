use super::*;
use crate::app::inspector::DefinitionInspectionContext;
use crate::app::item_editor::displayed_item_power;
use tiger_pkg::TagHash;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum ItemPage {
    #[default]
    Overview,
    Sockets,
    Runtime,
    Technical,
    Related,
}

impl ItemPage {
    pub(super) const ALL: [Self; 5] = [
        Self::Overview,
        Self::Sockets,
        Self::Runtime,
        Self::Technical,
        Self::Related,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Sockets => "Sockets",
            Self::Runtime => "Runtime",
            Self::Technical => "Technical",
            Self::Related => "Related Records",
        }
    }
}

pub(super) struct ItemInspection<'a> {
    pub(super) catalog: &'a Catalog,
    pub(super) hash: u64,
    pub(super) resolved_name: &'a Option<String>,
    pub(super) matches: &'a CatalogHashMatches<'a>,
    pub(super) source_context: Option<&'a DefinitionInspectionContext>,
}

pub(super) fn draw_hash_item_matches(
    ui: &mut egui::Ui,
    content: ItemInspection<'_>,
    runtime: &mut super::runtime::RuntimeInspectionState,
    page: ItemPage,
) {
    let matches = content.matches;
    if matches.item_package_metadata.is_some() || matches.item.is_some() {
        ui.add_space(8.0);
        draw_hash_item_identity_summary(
            ui,
            content.catalog,
            content.hash,
            content.resolved_name,
            matches.item,
            matches.item_package_metadata,
            matches.inventory_metadata,
        );
        match page {
            ItemPage::Overview => draw_overview(ui, &content),
            ItemPage::Sockets => draw_sockets_page(ui, &content),
            ItemPage::Runtime => draw_runtime_page(ui, &content, runtime),
            ItemPage::Technical => draw_technical_page(ui, &content, runtime),
            ItemPage::Related => {}
        }
    } else {
        if let Some(context) = content.source_context {
            draw_hash_item_source_comparison(
                ui,
                content.catalog,
                content.hash,
                matches.item,
                context,
            );
        }
        draw_hash_item_stat_matches(ui, content.catalog, matches);
        if let Some(metadata) = matches.inventory_metadata {
            hash_metadata_section(ui, "Inventory Metadata", false, |ui| {
                draw_hash_inventory_placement_summary(ui, metadata);
                draw_hash_inventory_capacity_summary(ui, metadata);
            });
        }
        if !matches.bucket_items.is_empty() {
            draw_hash_inventory_bucket(ui, content.catalog, content.hash, &matches.bucket_items);
        }
    }
}

fn draw_overview(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
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
        super::item_details::draw_item_traits(ui, content.catalog, content.hash, metadata);
    }
    if let Some(item) = content.matches.item {
        draw_hash_item_abilities(ui, content.catalog, item);
    }
}

fn draw_overview_source(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
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

fn draw_overview_stats(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if let Some(metadata) = content.matches.item_package_metadata {
        draw_hash_item_investment_stats(ui, content.catalog, content.hash, metadata);
    }
}

fn draw_sockets_page(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    let Some(item) = content.matches.item else {
        ui.weak("No socket definition is available for this item.");
        return;
    };
    if let Some(context) = content.source_context {
        ui.label(&context.source);
        if let Some(plugs) = &context.plugs {
            ui.horizontal_wrapped(|ui| {
                ui.label(super::instance::plug_source(plugs));
                crate::ui_help::info(ui, "Opening-time account snapshot, not live equipped state. Empty and missing entries are not assumed to use the catalog default.");
            });
        } else {
            ui.weak("Saved plug values were not available at this entry point.");
        }
    } else {
        ui.weak(
            "Catalog defaults and options only. Open an owned item to compare its saved plugs.",
        );
    }
    draw_hash_item_sockets(
        ui,
        content.catalog,
        item,
        content
            .source_context
            .and_then(|context| context.plugs.as_ref()),
    );
    if item.sockets.is_empty() && item.default_plugs.is_empty() {
        ui.weak("No sockets or default plugs are decoded for this item.");
    }
}

fn draw_runtime_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        ui.weak("Package metadata is unavailable; runtime references cannot be resolved.");
        return;
    };
    super::runtime::draw_item_runtime(ui, content.catalog, content.hash, metadata, runtime);
    if metadata
        .weapon_pattern_index
        .is_none_or(|index| index == u16::MAX)
        && metadata.sandbox_perks.is_empty()
    {
        ui.weak("No weapon pattern or sandbox-perk references are decoded for this item.");
    }
}

fn draw_technical_page(
    ui: &mut egui::Ui,
    content: &ItemInspection<'_>,
    runtime: &mut super::runtime::RuntimeInspectionState,
) {
    let Some(metadata) = content.matches.item_package_metadata else {
        ui.weak("Package metadata is unavailable for this item.");
        return;
    };
    super::item_details::draw_classification_details(ui, content.hash, metadata);
    draw_hash_item_package_metadata(
        ui,
        content.catalog,
        content.hash,
        metadata,
        content.matches.item_material_requirement_set_indices,
    );
    super::item_details::draw_structural_details(ui, content.catalog, content.hash, metadata);
    super::runtime::draw_dye_colors(ui, metadata, runtime);
    super::item_details::draw_stat_group(ui, content.catalog, content.hash);
    if let Some(inventory) = content.matches.inventory_metadata {
        draw_hash_inventory_placement_summary(ui, inventory);
        draw_hash_inventory_capacity_summary(ui, inventory);
    }
}

fn draw_hash_item_source_comparison(
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
            ui.label(egui::RichText::new(&context.source).strong());
            ui.weak(
                "Snapshot captured when opened; reopen the item after editing its account data.",
            );
            ui.add_space(6.0);
            egui::Grid::new(("hash_item_instance", hash))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    if let Some(level) = context.authored_level {
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
                            super::instance::plug_source(plugs),
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

fn draw_hash_item_identity_summary(
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

fn draw_inline_item_hash(ui: &mut egui::Ui, drew: &mut bool, catalog: &Catalog, hash: u64) {
    if *drew {
        ui.label(egui::RichText::new("·").weak());
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

fn draw_inline_item_fact(
    ui: &mut egui::Ui,
    drew: &mut bool,
    label: &str,
    value: impl Into<String>,
    monospace: bool,
) {
    if *drew {
        ui.label(egui::RichText::new("·").weak());
    }
    ui.label(metadata_label_text(ui, label));
    let value = egui::RichText::new(value.into()).strong();
    ui.label(if monospace { value.monospace() } else { value });
    *drew = true;
}

fn draw_inline_item_fact_with_tooltip(
    ui: &mut egui::Ui,
    drew: &mut bool,
    label: &str,
    value: impl Into<String>,
    monospace: bool,
    tooltip: impl Into<String>,
) {
    if *drew {
        ui.label(egui::RichText::new("·").weak());
    }
    ui.label(metadata_label_text(ui, label));
    let value = egui::RichText::new(value.into()).strong();
    ui.label(if monospace { value.monospace() } else { value })
        .on_hover_text(tooltip.into());
    *drew = true;
}

fn draw_inline_item_definition_fact(
    ui: &mut egui::Ui,
    drew: &mut bool,
    catalog: &Catalog,
    label: &str,
    value: impl Into<String>,
    hash: u64,
) {
    if *drew {
        ui.label(egui::RichText::new("·").weak());
    }
    ui.label(metadata_label_text(ui, label));
    draw_named_catalog_hash_link(ui, catalog, hash, value);
    *drew = true;
}

fn draw_hash_item_package_metadata(
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

fn draw_hash_item_package_primary_fields(
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

fn draw_hash_item_package_reference_fields(
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

fn draw_hash_inventory_placement_summary(ui: &mut egui::Ui, metadata: &InventoryMetadata) {
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

fn draw_hash_inventory_capacity_summary(ui: &mut egui::Ui, metadata: &InventoryMetadata) {
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
                            ui.label(egui::RichText::new("-").weak());
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

fn draw_hash_item_stat_matches(
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

fn draw_item_material_requirement_set_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &str,
    index: Option<u16>,
) {
    let Some(index) = index else {
        return;
    };
    hash_detail_field(ui, &format!("{label} Index"), index.to_string(), true);
    if let Some(set) = catalog.material_requirement_set(usize::from(index)) {
        ui.label(metadata_label_text(ui, format!("{label} Hash")));
        draw_catalog_hash_link(ui, catalog, set.hash, format_hash_hex_and_decimal(set.hash));
        ui.end_row();
    }
}

fn draw_hash_item_sockets(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    plugs: Option<&serde_json::Value>,
) {
    let socket_count = item.sockets.len().max(item.default_plugs.len()).max(
        plugs
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len),
    );
    if socket_count == 0 {
        return;
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Sockets ({socket_count})"))
        .id_salt(("hash_item_sockets", item.hash))
        .default_open(true)
        .show(ui, |ui| {
            let selection_id = egui::Id::new(("hash_item_socket_source_selection", item.hash));
            let mut clicked_socket = None;
            let current_socket = ui.data_mut(|data| data.get_temp::<usize>(selection_id));
            egui::ScrollArea::horizontal()
                .id_salt("socket-table-columns")
                .show(ui, |ui| {
                    egui::Grid::new(("hash_item_socket_rows", item.hash))
                        .num_columns(if plugs.is_some() { 6 } else { 4 })
                        .spacing([12.0, 3.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Socket");
                            ui.strong("Default Plug");
                            if plugs.is_some() {
                                ui.strong("Saved Plug");
                                ui.strong("Comparison");
                            }
                            ui.strong("Plug Choices");
                            ui.strong("Option Sets");
                            ui.end_row();
                            for socket_index in 0..socket_count {
                                let socket = item.sockets.get(socket_index);
                                let default_hash = item
                                    .default_plugs
                                    .get(socket_index)
                                    .and_then(Option::as_deref)
                                    .and_then(parse_hash_hex);
                                let socket_label = socket.map_or_else(
                                    || format!("{} · No Decoded Socket", socket_index + 1),
                                    |socket| socket.display_label(socket_index),
                                );
                                let response = ui.add_enabled(
                                    socket.is_some_and(|socket| {
                                        !catalog.socket_options(socket).is_empty()
                                            || !socket.sources.is_empty()
                                            || default_hash.is_some()
                                    }),
                                    egui::SelectableLabel::new(
                                        current_socket == Some(socket_index),
                                        socket_label,
                                    ),
                                );
                                if response.clicked() {
                                    clicked_socket = Some(socket_index);
                                }
                                if let Some(socket) = socket {
                                    response.on_hover_text(format!(
                                        "Socket type {} · pool {}",
                                        socket.socket_type, socket.pool
                                    ));
                                }
                                if let Some(default_hash) = default_hash {
                                    let name = catalog
                                        .package_item_name(default_hash)
                                        .or_else(|| catalog.display_name(default_hash))
                                        .unwrap_or("Name not resolved");
                                    draw_named_catalog_hash_link(ui, catalog, default_hash, name);
                                } else {
                                    ui.label(egui::RichText::new("-").weak());
                                }
                                if let Some(plugs) = plugs {
                                    super::instance::draw_saved_plug(
                                        ui,
                                        catalog,
                                        plugs,
                                        socket_index,
                                        default_hash,
                                    );
                                }
                                ui.monospace(
                                    socket
                                        .map_or(0, |socket| catalog.socket_options(socket).len())
                                        .to_string(),
                                );
                                let source_count = socket.map_or(0, |socket| socket.sources.len());
                                ui.monospace(source_count.to_string());
                                ui.end_row();
                            }
                        });
                });

            let detail_socket_indices = item
                .sockets
                .iter()
                .enumerate()
                .filter_map(|(socket_index, socket)| {
                    let has_default = item
                        .default_plugs
                        .get(socket_index)
                        .and_then(Option::as_deref)
                        .and_then(parse_hash_hex)
                        .is_some();
                    (!catalog.socket_options(socket).is_empty()
                        || !socket.sources.is_empty()
                        || has_default)
                        .then_some(socket_index)
                })
                .collect::<Vec<_>>();
            let Some(first_socket_index) = detail_socket_indices.first().copied() else {
                return;
            };
            let mut selected_socket_index = clicked_socket
                .or(current_socket)
                .filter(|selected| detail_socket_indices.contains(selected))
                .unwrap_or(first_socket_index);

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.strong("Socket Details");
                egui::ComboBox::from_id_salt(("hash_item_socket_source_picker", item.hash))
                    .selected_text(
                        item.sockets[selected_socket_index].display_label(selected_socket_index),
                    )
                    .width(ui.available_width().clamp(180.0, 320.0))
                    .show_ui(ui, |ui| {
                        for socket_index in &detail_socket_indices {
                            let socket = &item.sockets[*socket_index];
                            ui.selectable_value(
                                &mut selected_socket_index,
                                *socket_index,
                                format!(
                                    "{} · {} Option Set{}",
                                    socket.display_label(*socket_index),
                                    socket.sources.len(),
                                    if socket.sources.len() == 1 { "" } else { "s" }
                                ),
                            );
                        }
                    });
            });
            ui.data_mut(|data| data.insert_temp(selection_id, selected_socket_index));

            draw_hash_item_socket_sources(ui, catalog, item, selected_socket_index);
        });
}

fn draw_hash_item_socket_sources(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    socket_index: usize,
) {
    let socket = &item.sockets[socket_index];
    let options = catalog.socket_options(socket);
    if socket.sources.is_empty() {
        ui.label(egui::RichText::new("No decoded option sources.").weak());
    } else {
        draw_hash_item_socket_source_summary(ui, catalog, item, socket_index);
    }

    for (source_index, source) in socket.sources.iter().enumerate() {
        let source_options = catalog.socket_source_options(source);
        if source_options.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(format!(
            "{} Members ({})",
            source.label(),
            source_options.len()
        ))
        .id_salt((
            "hash_item_socket_source_members",
            item.hash,
            socket_index,
            source_index,
        ))
        .default_open(false)
        .show(ui, |ui| {
            let source_slot = socket_index.saturating_mul(64).saturating_add(source_index);
            let ordered = &source.ordered_members;
            let order_id = ui.id().with("authored_order");
            let mut authored_order = ui.data_mut(|data| data.get_temp::<bool>(order_id).unwrap_or(!ordered.is_empty()));
            ui.add_enabled(!ordered.is_empty(), egui::Checkbox::new(&mut authored_order, "Stored Package Order"))
                .on_hover_text("Preserves the native member order and duplicates. When off, shows the normalized picker pool.");
            ui.data_mut(|data| data.insert_temp(order_id, authored_order));
            if ordered.is_empty() { ui.weak("No ordered member list was retained; showing the normalized pool."); }
            draw_hash_item_rows(
                ui,
                catalog,
                ("socket_source_members", item.hash, source_slot),
                if authored_order && !ordered.is_empty() { ordered.as_slice() } else { source_options }.iter().copied(),
            );
        });
    }

    if !options.is_empty() {
        egui::CollapsingHeader::new(format!("Combined Plug Options ({})", options.len()))
            .id_salt(("hash_item_socket_combined", item.hash, socket_index))
            .default_open(false)
            .show(ui, |ui| {
                draw_hash_item_rows(
                    ui,
                    catalog,
                    ("socket_combined", item.hash, socket_index),
                    options.iter().copied(),
                );
            });
    }
}

fn draw_hash_item_socket_source_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    socket_index: usize,
) {
    let socket = &item.sockets[socket_index];
    egui::Grid::new(("hash_item_socket_source_rows", item.hash, socket_index))
        .num_columns(4)
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Option Set");
            ui.strong("Plug Count");
            ui.strong("Data Source");
            ui.strong("Catalog Pool ID").on_hover_text(
                "Interned catalog pool ID. The source label contains the package record index or hash.",
            );
            ui.end_row();
            for source in &socket.sources {
                let source_options = catalog.socket_source_options(source);
                ui.label(source.label());
                ui.monospace(source_options.len().to_string());
                if source.valid {
                    ui.label(source.origin_label());
                } else {
                    let decode_status = if source_options.is_empty() {
                        "unavailable"
                    } else {
                        "partial"
                    };
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} · {decode_status}", source.origin_label()),
                    )
                    .on_hover_text(
                        "Sundial retained every member it could decode safely; one or more package references were invalid.",
                    );
                }
                ui.monospace(source.pool.to_string());
                ui.end_row();
            }
        });
}

fn draw_hash_item_abilities(ui: &mut egui::Ui, catalog: &Catalog, item: &crate::catalog::ItemDef) {
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
                    ui.strong("Name");
                    ui.strong("Hash");
                    ui.end_row();
                    for (label, choices) in [
                        ("Movement", abilities.movement.as_slice()),
                        ("Grenade", abilities.grenade.as_slice()),
                        ("Super Ability", abilities.super_ability.as_slice()),
                        ("Melee", abilities.melee.as_slice()),
                        ("Class Ability", abilities.class_ability.as_slice()),
                    ] {
                        for choice in choices {
                            draw_hash_ability_row(ui, catalog, label, choice);
                        }
                    }
                    for attunement in &abilities.attunements {
                        for choice in &attunement.super_abilities {
                            draw_hash_ability_row(
                                ui,
                                catalog,
                                &format!("{} · Super Ability", attunement.name),
                                choice,
                            );
                        }
                        if attunement.melee.entry != 0 {
                            draw_hash_ability_row(
                                ui,
                                catalog,
                                &format!("{} · Melee", attunement.name),
                                &attunement.melee,
                            );
                        }
                        for choice in &attunement.perks {
                            draw_hash_ability_row(
                                ui,
                                catalog,
                                &format!("{} · Perk", attunement.name),
                                choice,
                            );
                        }
                    }
                });
        });
}

fn draw_hash_ability_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &str,
    choice: &crate::catalog::AbilityChoice,
) {
    ui.label(label);
    draw_named_catalog_hash_link(ui, catalog, choice.entry, metadata_text(&choice.name));
    draw_catalog_hash_link(ui, catalog, choice.entry, format_hash_hex(choice.entry));
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

fn draw_hash_item_rows(
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
