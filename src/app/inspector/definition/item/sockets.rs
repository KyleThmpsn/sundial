use super::super::item_details::{Cell, draw_hash_item_rows, draw_table, item_name};
use super::*;
use crate::app::inspector::look;

pub(super) fn draw_sockets_page(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    let Some(item) = content.matches.item else {
        look::empty_state(ui, "No Sockets");
        return;
    };
    if let Some(context) = content.source_context {
        look::properties(ui, ("item_socket_source", item.hash), |p| {
            p.text("Opened From", context.source.as_str());
            if let Some(plugs) = &context.plugs {
                p.text("Saved Plugs", super::super::instance::plug_source(plugs));
            }
        });
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
        look::empty_state(ui, "No Sockets");
    }
}

/// Insertion and enabled material requirement sets as property rows.
pub(super) fn material_requirement_set_rows(
    p: &mut look::Properties<'_>,
    catalog: &Catalog,
    label: &str,
    index: Option<u16>,
) {
    let Some(index) = index else {
        return;
    };
    match catalog.material_requirement_set(usize::from(index)) {
        Some(set) => p.link(label, catalog, set.hash, format!("Set #{index}")),
        None => p.mono(label, format!("Set #{index}")),
    }
}

pub(super) fn draw_hash_item_sockets(
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

    look::section(
        ui,
        ("hash_item_sockets", item.hash),
        "Sockets",
        Some(socket_count),
        true,
        |ui| {
            let selection_id = egui::Id::new(("hash_item_socket_source_selection", item.hash));
            let mut clicked_socket = None;
            let current_socket = ui.data_mut(|data| data.get_temp::<usize>(selection_id));
            egui::ScrollArea::horizontal()
                .id_salt(("socket-table-columns", item.hash))
                .show(ui, |ui| {
                    egui::Grid::new(("hash_item_socket_rows", item.hash))
                        .num_columns(if plugs.is_some() { 5 } else { 4 })
                        .spacing([16.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Socket");
                            ui.strong("Default Plug");
                            if plugs.is_some() {
                                ui.strong("Saved Plug");
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
                                    let details = format!(
                                        "Socket type {} · pool {}",
                                        socket.socket_type, socket.pool
                                    );
                                    response
                                        .on_hover_text(details.clone())
                                        .on_disabled_hover_text(details);
                                }
                                if let Some(default_hash) = default_hash {
                                    draw_named_catalog_hash_link(
                                        ui,
                                        catalog,
                                        default_hash,
                                        item_name(catalog, default_hash),
                                    );
                                } else {
                                    ui.label("");
                                }
                                if let Some(plugs) = plugs {
                                    super::super::instance::draw_saved_plug(
                                        ui,
                                        catalog,
                                        plugs,
                                        socket_index,
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

            look::subheading(ui, "Socket Details");
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
            ui.data_mut(|data| data.insert_temp(selection_id, selected_socket_index));
            ui.add_space(4.0);

            draw_hash_item_socket_sources(ui, catalog, item, selected_socket_index);
        },
    );
}

pub(super) fn draw_hash_item_socket_sources(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    socket_index: usize,
) {
    let socket = &item.sockets[socket_index];
    let options = catalog.socket_options(socket);
    if socket.sources.is_empty() {
        look::empty_state(ui, "No Option Sources");
    } else {
        draw_hash_item_socket_source_summary(ui, catalog, item, socket_index);
    }

    for (source_index, source) in socket.sources.iter().enumerate() {
        let source_options = catalog.socket_source_options(source);
        if source_options.is_empty() {
            continue;
        }
        look::section(
            ui,
            (
                "hash_item_socket_source_members",
                item.hash,
                socket_index,
                source_index,
            ),
            &format!("{} Members", source.label()),
            Some(source_options.len()),
            false,
            |ui| {
                let source_slot = socket_index.saturating_mul(64).saturating_add(source_index);
                let ordered = &source.ordered_members;
                let order_id = egui::Id::new(("authored_order", item.hash, source_slot));
                let mut authored_order = ui.data_mut(|data| {
                    data.get_temp::<bool>(order_id)
                        .unwrap_or(!ordered.is_empty())
                });
                ui.add_enabled(
                    !ordered.is_empty(),
                    egui::Checkbox::new(&mut authored_order, "Stored Package Order"),
                );
                ui.data_mut(|data| data.insert_temp(order_id, authored_order));
                draw_hash_item_rows(
                    ui,
                    catalog,
                    egui::Id::new(("socket_source_members", item.hash, source_slot)),
                    if authored_order && !ordered.is_empty() {
                        ordered.as_slice()
                    } else {
                        source_options
                    }
                    .iter()
                    .copied(),
                    &[],
                );
            },
        );
    }

    if !options.is_empty() {
        look::section(
            ui,
            ("hash_item_socket_combined", item.hash, socket_index),
            "Combined Plug Options",
            Some(options.len()),
            false,
            |ui| {
                draw_hash_item_rows(
                    ui,
                    catalog,
                    egui::Id::new(("socket_combined", item.hash, socket_index)),
                    options.iter().copied(),
                    &[],
                );
            },
        );
    }
}

pub(super) fn draw_hash_item_socket_source_summary(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    socket_index: usize,
) {
    let socket = &item.sockets[socket_index];
    egui::Grid::new(("hash_item_socket_source_rows", item.hash, socket_index))
        .num_columns(4)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Option Set");
            ui.strong("Plug Count");
            ui.strong("Data Source");
            ui.strong("Catalog Pool ID");
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
                    .on_hover_text("Some package references are invalid.");
                }
                ui.monospace(source.pool.to_string());
                ui.end_row();
            }
        });
}

pub(super) fn draw_hash_item_abilities(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
) {
    let abilities = &item.abilities;
    let mut rows = Vec::new();
    for (label, choices) in [
        ("Movement", abilities.movement.as_slice()),
        ("Grenade", abilities.grenade.as_slice()),
        ("Super Ability", abilities.super_ability.as_slice()),
        ("Melee", abilities.melee.as_slice()),
        ("Class Ability", abilities.class_ability.as_slice()),
    ] {
        for choice in choices {
            rows.push(ability_row(label.to_owned(), choice));
        }
    }
    for attunement in &abilities.attunements {
        for choice in &attunement.super_abilities {
            rows.push(ability_row(
                format!("{} · Super Ability", attunement.name),
                choice,
            ));
        }
        if attunement.melee.entry != 0 {
            rows.push(ability_row(
                format!("{} · Melee", attunement.name),
                &attunement.melee,
            ));
        }
        for choice in &attunement.perks {
            rows.push(ability_row(format!("{} · Perk", attunement.name), choice));
        }
    }
    if rows.is_empty() {
        return;
    }
    look::section(
        ui,
        ("hash_item_abilities", item.hash),
        "Abilities",
        Some(rows.len()),
        rows.len() <= super::super::HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_item_abilities", item.hash),
                &["Ability", "Name"],
                &rows,
            );
        },
    );
}

/// `entry` is the ability's position in its list, not a definition hash, so the name is text.
fn ability_row(label: String, choice: &crate::catalog::AbilityChoice) -> Vec<Cell> {
    let name = choice.name.trim();
    vec![
        Cell::Text(label),
        Cell::Text(if name.is_empty() { UNNAMED } else { name }.to_owned()),
    ]
}
