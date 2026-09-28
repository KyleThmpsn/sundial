//! Native classifications and complete ordered metadata, separate from the item summary, and the
//! table helpers the definition pages share.

use super::matches::CatalogHashMatches;
use super::{TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, TABLE_ROW_GAP, UNNAMED, table_cell};
use crate::app::inspector::{draw_catalog_hash_link, draw_named_catalog_hash_link, look};
use eframe::egui;

use crate::{
    catalog::{Catalog, ItemDamageProfile, ItemPackageMetadata},
    hash::format_hash_hex,
};

/// Rows after which a table offers a filter.
const TABLE_FILTER_AFTER: usize = 12;
/// Rows after which a table lays out only the rows in view.
const TABLE_VIRTUAL_AFTER: usize = 40;
/// Rows a virtualised table shows at once.
const TABLE_VISIBLE_ROWS: usize = 18;

/// One cell of a [`draw_table`] row.
pub(super) enum Cell {
    /// A definition name that opens the definition.
    Link(u64, String),
    /// A hash that opens its definition, in muted monospace.
    Hash(u64),
    Text(String),
    Mono(String),
    Muted(String),
    /// Several definition names in one cell.
    Links(Vec<(u64, String)>),
}

impl Cell {
    /// A link, or plain text when the hash is absent or names the page being shown.
    pub(super) fn link_unless(hash: u64, current: u64, name: impl Into<String>) -> Self {
        let name = name.into();
        if hash == 0 || hash == u64::from(u32::MAX) || hash == current {
            Self::Text(name)
        } else {
            Self::Link(hash, name)
        }
    }

    pub(super) fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub(super) fn mono(text: impl ToString) -> Self {
        Self::Mono(text.to_string())
    }

    pub(super) fn muted(text: impl Into<String>) -> Self {
        Self::Muted(text.into())
    }

    fn matches(&self, query: &str) -> bool {
        let hash_matches = |hash: u64| format_hash_hex(hash).to_lowercase().contains(query);
        match self {
            Self::Link(hash, text) => text.to_lowercase().contains(query) || hash_matches(*hash),
            Self::Hash(hash) => hash_matches(*hash),
            Self::Text(text) | Self::Mono(text) | Self::Muted(text) => {
                text.to_lowercase().contains(query)
            }
            Self::Links(links) => links
                .iter()
                .any(|(hash, text)| text.to_lowercase().contains(query) || hash_matches(*hash)),
        }
    }

    fn draw(&self, ui: &mut egui::Ui, catalog: &Catalog) {
        match self {
            Self::Link(hash, name) => {
                draw_named_catalog_hash_link(ui, catalog, *hash, name.as_str());
            }
            Self::Hash(hash) => {
                draw_catalog_hash_link(ui, catalog, *hash, format_hash_hex(*hash));
            }
            Self::Text(text) => {
                ui.label(text.as_str());
            }
            Self::Mono(text) => {
                ui.label(egui::RichText::new(text.as_str()).monospace());
            }
            Self::Muted(text) => {
                ui.label(egui::RichText::new(text.as_str()).color(look::muted(ui)));
            }
            Self::Links(links) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for (hash, name) in links {
                        draw_named_catalog_hash_link(ui, catalog, *hash, name.as_str());
                    }
                });
            }
        }
    }

    fn draw_fixed(&self, ui: &mut egui::Ui, catalog: &Catalog, width: f32) {
        match self {
            Self::Text(text) => {
                table_cell(ui, width, text.as_str());
            }
            Self::Mono(text) => {
                table_cell(ui, width, egui::RichText::new(text.as_str()).monospace());
            }
            Self::Muted(text) => {
                let color = look::muted(ui);
                table_cell(ui, width, egui::RichText::new(text.as_str()).color(color));
            }
            Self::Link(..) | Self::Hash(_) | Self::Links(_) => {
                ui.allocate_ui_with_layout(
                    egui::vec2(width, TABLE_CELL_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        self.draw(ui, catalog);
                    },
                );
            }
        }
    }
}

/// A filter field whose text persists under `id`. Returns the lowercase query.
pub(super) fn filter_field(ui: &mut egui::Ui, id: egui::Id, hint: &str) -> String {
    let mut query = ui.data_mut(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .id(id.with("edit"))
                .hint_text(hint)
                .desired_width(260.0),
        );
        if !query.is_empty() && ui.small_button("Clear").clicked() {
            query.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(id, query.clone()));
    query.trim().to_lowercase()
}

/// Rows of cells under strong headings. Long tables get a filter and lay out only the rows in
/// view.
pub(super) fn draw_table(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: impl std::hash::Hash,
    headings: &[&str],
    rows: &[Vec<Cell>],
) {
    let id = egui::Id::new(("inspector_table", id));
    let query = if rows.len() > TABLE_FILTER_AFTER {
        filter_field(ui, id.with("filter"), "Filter")
    } else {
        String::new()
    };
    let matching = rows
        .iter()
        .filter(|row| query.is_empty() || row.iter().any(|cell| cell.matches(&query)))
        .collect::<Vec<_>>();
    if !query.is_empty() {
        ui.label(
            egui::RichText::new(format!("{} of {}", matching.len(), rows.len()))
                .small()
                .color(look::muted(ui)),
        );
    }
    if matching.is_empty() {
        look::empty_state(ui, "No Matches");
        return;
    }
    if matching.len() <= TABLE_VIRTUAL_AFTER {
        egui::Grid::new(id.with("grid"))
            .num_columns(headings.len())
            .striped(true)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for heading in headings {
                    ui.label(egui::RichText::new(*heading).strong());
                }
                ui.end_row();
                for row in &matching {
                    for cell in row.iter() {
                        cell.draw(ui, catalog);
                    }
                    ui.end_row();
                }
            });
        return;
    }
    let columns = headings.len().max(1) as f32;
    let width = ((ui.available_width()
        - ui.spacing().scroll.bar_width
        - TABLE_COLUMN_GAP * (columns - 1.0))
        / columns)
        .max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for heading in headings {
            table_cell(ui, width, egui::RichText::new(*heading).strong());
        }
    });
    let table_height = (TABLE_CELL_HEIGHT + TABLE_ROW_GAP) * TABLE_VISIBLE_ROWS as f32;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(id.with("rows"))
            .min_scrolled_height(table_height)
            .max_height(table_height)
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, matching.len(), |ui, range| {
                egui::Grid::new(id.with("row_grid"))
                    .num_columns(headings.len())
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .start_row(range.start)
                    .show(ui, |ui| {
                        for row in &matching[range] {
                            for cell in row.iter() {
                                cell.draw_fixed(ui, catalog, width);
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

/// The name an item or plug hash resolves to.
pub(super) fn item_name(catalog: &Catalog, hash: u64) -> &str {
    catalog
        .package_item_name(hash)
        .or_else(|| catalog.display_name(hash))
        .unwrap_or(UNNAMED)
}

pub(super) fn draw_item_traits(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if metadata.trait_indices.is_empty() {
        return;
    }
    let rows = metadata
        .trait_indices
        .iter()
        .map(|&index| {
            let Some(definition) = catalog.trait_definitions().get(usize::from(index)) else {
                return vec![
                    Cell::muted("Unresolved"),
                    Cell::muted(""),
                    Cell::mono(index),
                ];
            };
            let name = if definition.name.trim().is_empty() {
                format_hash_hex(definition.hash)
            } else {
                definition.name.trim().to_owned()
            };
            vec![
                Cell::link_unless(definition.hash, hash, name),
                Cell::muted(definition.description.trim()),
                Cell::mono(index),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("item_traits", hash),
        "Item Traits",
        Some(rows.len()),
        rows.len() <= super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("item_traits", hash),
                &["Trait", "Description", "Index"],
                &rows,
            );
        },
    );
}

pub(super) fn draw_structural_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    if !metadata.power_cap_groups.is_empty() {
        let rows = metadata
            .power_cap_groups
            .iter()
            .enumerate()
            .map(|(row, group)| {
                vec![
                    Cell::mono(row + 1),
                    Cell::mono(group),
                    Cell::mono(version_cap_text(
                        catalog.power_cap_for_version_group(*group),
                    )),
                    catalog
                        .power_cap_definitions()
                        .get(usize::from(*group))
                        .map_or_else(
                            || Cell::muted("Unresolved"),
                            |definition| Cell::Hash(u64::from(definition.hash)),
                        ),
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("item_power_cap_versions", hash),
            "Power Cap Versions",
            Some(rows.len()),
            rows.len() <= super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("item_power_cap_versions", hash),
                    &["Row", "Cap Table Index", "Power Cap", "Definition"],
                    &rows,
                );
            },
        );
    }
    if !metadata.art_arrangements.is_empty() {
        let rows = metadata
            .art_arrangements
            .iter()
            .enumerate()
            .map(|(row, art)| {
                vec![
                    Cell::mono(row + 1),
                    Cell::text(art_class_text(art.character_class)),
                    if art.arrangement == u16::MAX {
                        Cell::muted("Unassigned")
                    } else {
                        Cell::mono(art.arrangement)
                    },
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("item_art_arrangements", hash),
            "Art Arrangements",
            Some(rows.len()),
            rows.len() <= super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("item_art_arrangements", hash),
                    &["Row", "Class", "Arrangement Index"],
                    &rows,
                );
            },
        );
    }
    let rows = dye_rows(metadata);
    if !rows.is_empty() {
        let count = rows.len();
        let rows = rows
            .into_iter()
            .map(|row| {
                let mut cells = row.into_iter();
                let mut next = || cells.next().unwrap_or_default();
                vec![
                    Cell::Text(next()),
                    Cell::Mono(next()),
                    Cell::Mono(next()),
                    Cell::Mono(next()),
                    Cell::Text(next()),
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("item_dye_references", hash),
            "Dye References",
            Some(count),
            count <= super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
            |ui| {
                draw_table(
                    ui,
                    catalog,
                    ("item_dye_references", hash),
                    &[
                        "Override Lane",
                        "Row",
                        "Channel Index",
                        "Dye Reference",
                        "State",
                    ],
                    &rows,
                );
            },
        );
    }
}

fn version_cap_text(cap: Option<u32>) -> String {
    cap.map_or_else(|| "No Fixed Cap".into(), |cap| cap.to_string())
}

pub(super) fn draw_classification_details(
    ui: &mut egui::Ui,
    hash: u64,
    metadata: &ItemPackageMetadata,
) {
    let damage_profile = damage_profile_text(metadata.damage_profile);
    if damage_profile.is_none()
        && metadata.weapon_translation_group.is_none()
        && metadata.weapon_inventory_slot.is_none()
        && metadata.weapon_ammo_type.is_none()
    {
        return;
    }
    look::section(
        ui,
        ("item_classification", hash),
        "Classification",
        None,
        true,
        |ui| {
            look::properties(ui, ("item_classification", hash), |p| {
                if let Some(profile) = damage_profile {
                    p.text("Damage Profile", profile);
                }
                if let Some(group) = metadata.weapon_translation_group {
                    p.mono(
                        "Animation Translation Group",
                        format!("{group} · 0x{group:08X}"),
                    );
                }
                if let Some(slot) = metadata.weapon_inventory_slot {
                    p.text(
                        "Slot",
                        match slot {
                            crate::catalog::ItemWeaponInventorySlot::Kinetic => "Kinetic Slot",
                            crate::catalog::ItemWeaponInventorySlot::Energy => "Energy Slot",
                            crate::catalog::ItemWeaponInventorySlot::Power => "Power Slot",
                        },
                    );
                }
                if let Some(ammo) = metadata.weapon_ammo_type {
                    p.text("Ammo Type", format!("{} Ammo", ammo.label()));
                }
            });
        },
    );
}

fn damage_profile_text(profile: ItemDamageProfile) -> Option<String> {
    Some(match profile {
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
        ItemDamageProfile::Variable => "Variable Damage Type".into(),
        ItemDamageProfile::Unknown => return None,
    })
}

/// A stat definition's name, or its index.
fn stat_name(catalog: &Catalog, definition_index: u16) -> String {
    catalog
        .item_stat_definition(definition_index)
        .map(|definition| definition.name.trim())
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("Stat #{definition_index}"), str::to_owned)
}

/// A link to a stat definition, or its name when the index does not resolve.
fn stat_link(ui: &mut egui::Ui, catalog: &Catalog, definition_index: u16) {
    let name = stat_name(catalog, definition_index);
    match catalog.item_stat_definition(definition_index) {
        Some(definition) => {
            draw_named_catalog_hash_link(ui, catalog, definition.hash, name);
        }
        None => {
            ui.label(name);
        }
    }
}

fn stat_mode_text(is_linear: bool, display_as_numeric: bool) -> String {
    format!(
        "{} · {}",
        if is_linear { "Linear" } else { "Interpolated" },
        if display_as_numeric {
            "Numeric Display"
        } else {
            "Bar Display"
        }
    )
}

pub(super) fn draw_stat_group(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let Some(metadata) = catalog.item_package_metadata(hash) else {
        return;
    };
    let group = catalog.item_stat_group(hash);
    if group.is_none() && metadata.stat_group_index.is_none() {
        return;
    }
    look::section(
        ui,
        ("item_stat_curves", hash),
        "Stat Display Curves",
        group.map(|group| group.scaled_stats.len()),
        false,
        |ui| {
            let Some(group) = group else {
                look::empty_state(ui, "Stat Group Unresolved");
                return;
            };
            look::properties(ui, ("item_stat_curves", hash), |p| {
                p.link(
                    "Stat Group",
                    catalog,
                    group.hash,
                    metadata.stat_group_index.map_or_else(
                        || format_hash_hex(group.hash),
                        |index| format!("Group {index}"),
                    ),
                );
                p.mono("Maximum Value", group.maximum_value.to_string());
            });
            if let Some(index) = metadata.stat_group_index {
                draw_item_hash_list(
                    ui,
                    catalog,
                    ("stat-group-users", hash),
                    "Items Using This Stat Group",
                    catalog.items_with_stat_group(index),
                );
            }
            ui.add_space(6.0);
            for (row, stat) in group.scaled_stats.iter().enumerate() {
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    egui::Id::new(("item_stat_curve", hash, row)),
                    false,
                )
                .show_header(ui, |ui| {
                    stat_link(ui, catalog, stat.definition_index);
                    ui.label(
                        egui::RichText::new(stat_mode_text(
                            stat.is_linear,
                            stat.display_as_numeric,
                        ))
                        .color(look::muted(ui)),
                    );
                })
                .body(|ui| {
                    if stat.display_interpolation.is_empty() {
                        look::empty_state(ui, "No Interpolation Points");
                        return;
                    }
                    let rows = stat
                        .display_interpolation
                        .iter()
                        .map(|point| {
                            vec![
                                Cell::mono(point.investment_value),
                                Cell::mono(point.display_value),
                            ]
                        })
                        .collect::<Vec<_>>();
                    draw_table(
                        ui,
                        catalog,
                        ("item_stat_curve_points", hash, row),
                        &["Investment Value", "Display Value"],
                        &rows,
                    );
                });
            }
        },
    );
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

/// A collapsible, filterable list of item links.
pub(super) fn draw_item_hash_list(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: impl std::hash::Hash,
    title: &str,
    hashes: &[u64],
) {
    if hashes.is_empty() {
        return;
    }
    let id = egui::Id::new(id);
    look::section(
        ui,
        id,
        title,
        Some(hashes.len()),
        hashes.len() <= super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_hash_item_rows(ui, catalog, id, hashes.iter().copied(), &[]);
        },
    );
}

/// An extra item-row column: its heading and the text for the row at a position in the list.
pub(super) type ItemRowValue<'a> = (&'static str, &'a dyn Fn(usize) -> String);

/// Filterable item rows. Only the rows in view are laid out.
pub(super) fn draw_hash_item_rows(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    hashes: impl IntoIterator<Item = u64>,
    values: &[ItemRowValue<'_>],
) {
    const ROW_WIDTH: f32 = 44.0;
    const HASH_WIDTH: f32 = 96.0;
    const VALUE_WIDTH: f32 = 110.0;
    let hashes = hashes.into_iter().collect::<Vec<_>>();
    let query = if hashes.len() > TABLE_FILTER_AFTER {
        filter_field(
            ui,
            egui::Id::new(("item_rows_filter", id)),
            "Filter by Name, Type or Hash",
        )
    } else {
        String::new()
    };
    let matching = hashes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, hash)| {
            query.is_empty()
                || format_hash_hex(*hash).to_lowercase().contains(&query)
                || item_name(catalog, *hash).to_lowercase().contains(&query)
                || catalog
                    .package_item_type_name(*hash)
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    if !query.is_empty() {
        ui.label(
            egui::RichText::new(format!("{} of {}", matching.len(), hashes.len()))
                .small()
                .color(look::muted(ui)),
        );
    }
    if matching.is_empty() {
        look::empty_state(ui, "No Matches");
        return;
    }
    let value_columns = values.len() as f32;
    let fixed_width = ROW_WIDTH
        + HASH_WIDTH
        + VALUE_WIDTH * value_columns
        + TABLE_COLUMN_GAP * (3.0 + value_columns);
    let flexible_width =
        (ui.available_width() - ui.spacing().scroll.bar_width - fixed_width).max(300.0);
    let name_width = flexible_width * 0.6;
    let type_width = flexible_width - name_width;
    let heading = |ui: &mut egui::Ui, width: f32, text: &str| {
        table_cell(ui, width, egui::RichText::new(text).strong());
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        heading(ui, ROW_WIDTH, "Row");
        heading(ui, name_width, "Name");
        heading(ui, type_width, "Type");
        heading(ui, HASH_WIDTH, "Hash");
        for &(title, _) in values {
            heading(ui, VALUE_WIDTH, title);
        }
    });
    let visible_rows = matching.len().min(TABLE_VISIBLE_ROWS);
    let table_height = (TABLE_CELL_HEIGHT + TABLE_ROW_GAP) * visible_rows as f32;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("hash_item_rows", id))
            .min_scrolled_height(table_height)
            .max_height(table_height)
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, matching.len(), |ui, range| {
                egui::Grid::new(("hash_item_row_grid", id))
                    .num_columns(4 + values.len())
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .start_row(range.start)
                    .show(ui, |ui| {
                        let muted = look::muted(ui);
                        for &(position, hash) in &matching[range] {
                            table_cell(
                                ui,
                                ROW_WIDTH,
                                egui::RichText::new((position + 1).to_string())
                                    .monospace()
                                    .color(muted),
                            );
                            Cell::Link(hash, item_name(catalog, hash).to_owned())
                                .draw_fixed(ui, catalog, name_width);
                            table_cell(
                                ui,
                                type_width,
                                catalog.package_item_type_name(hash).unwrap_or_default(),
                            );
                            table_cell(
                                ui,
                                HASH_WIDTH,
                                egui::RichText::new(format_hash_hex(hash))
                                    .monospace()
                                    .color(muted),
                            );
                            for &(_, value) in values {
                                table_cell(
                                    ui,
                                    VALUE_WIDTH,
                                    egui::RichText::new(value(position)).monospace(),
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

/// A stat group or power-cap row inspected by its own hash: what it holds and which items use it.
/// These rows have no view of their own on the Items pages, so this is how an index printed on
/// an item becomes navigable.
pub(super) fn draw_structure_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
) {
    if let Some((index, group)) = matches.item_stat_group {
        look::properties(ui, ("hash_stat_group", index), |p| {
            p.mono("Stat Group Index", index.to_string());
            p.mono("Maximum Value", group.maximum_value.to_string());
        });
        let rows = group
            .scaled_stats
            .iter()
            .map(|stat| {
                let name = stat_name(catalog, stat.definition_index);
                vec![
                    catalog
                        .item_stat_definition(stat.definition_index)
                        .map_or_else(
                            || Cell::Text(name.clone()),
                            |definition| Cell::Link(definition.hash, name.clone()),
                        ),
                    Cell::mono(stat.definition_index),
                    Cell::text(stat_mode_text(stat.is_linear, stat.display_as_numeric)),
                    Cell::mono(stat.display_interpolation.len()),
                ]
            })
            .collect::<Vec<_>>();
        look::section(
            ui,
            ("hash_stat_group_stats", index),
            "Scaled Stats",
            Some(rows.len()),
            true,
            |ui| {
                if rows.is_empty() {
                    look::empty_state(ui, "No Scaled Stats");
                } else {
                    draw_table(
                        ui,
                        catalog,
                        ("hash_stat_group_stats", index),
                        &["Stat", "Index", "Display", "Curve Points"],
                        &rows,
                    );
                }
            },
        );
        draw_item_hash_list(
            ui,
            catalog,
            ("stat-group-items", index),
            "Items Using This Stat Group",
            matches.stat_group_items,
        );
    }
    if let Some((index, definition)) = matches.power_cap_definition {
        look::properties(ui, ("hash_power_cap", index), |p| {
            p.mono("Cap Table Index", index.to_string());
            p.mono("Power Cap", version_cap_text(Some(definition.power_cap)));
        });
        draw_item_hash_list(
            ui,
            catalog,
            ("power-cap-items", index),
            "Items Versioned Under This Cap",
            matches.power_cap_items,
        );
    }
}
