use super::*;

pub(super) fn draw_sockets_page(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    let Some(item) = content.matches.item else {
        ui.weak("No socket definition is available for this item.");
        return;
    };
    if let Some(context) = content.source_context {
        ui.label(&context.source);
        if let Some(plugs) = &context.plugs {
            ui.horizontal_wrapped(|ui| {
                ui.label(super::super::instance::plug_source(plugs));
                crate::ui_help::info(ui, "Opening-time account snapshot, not live equipped state. Empty and missing entries are not assumed to use the catalog default.");
            });
        } else {
            ui.weak("Saved plug values were not available at this entry point.");
        }
    } else {
        ui.weak("Catalog defaults and options only. Open an owned item to see its saved plugs.");
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

pub(super) fn draw_item_material_requirement_set_link(
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
                        .num_columns(if plugs.is_some() { 5 } else { 4 })
                        .spacing([12.0, 3.0])
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
                                    ui.weak("-");
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

pub(super) fn draw_hash_item_socket_sources(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &crate::catalog::ItemDef,
    socket_index: usize,
) {
    let socket = &item.sockets[socket_index];
    let options = catalog.socket_options(socket);
    if socket.sources.is_empty() {
        ui.weak("No decoded option sources.");
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
            if ordered.is_empty() { ui.weak("No ordered member list was retained. Showing the normalized pool."); }
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

pub(super) fn draw_hash_item_socket_source_summary(
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
                        "Sundial retained every member it could decode safely. One or more package references were invalid.",
                    );
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

pub(super) fn draw_hash_ability_row(
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
