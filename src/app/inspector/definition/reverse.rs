//! What contains, carries or offers the inspected hash: presentation node contents, item traits,
//! plug categories, and the sockets and triumphs that offer or grant an item.

use super::item_details::{Cell, draw_hash_item_rows, draw_table, item_name};
use super::progression::{node_name, record_name, record_status};
use super::*;
use crate::app::inspector::look;

/// Kind label for hashes only the reverse lookups recognise, or None.
pub(super) fn reverse_kind(catalog: &Catalog, hash: u64) -> Option<&'static str> {
    if catalog.presentation_node(hash).is_some() {
        Some("Presentation Node")
    } else if catalog.item_trait(hash).is_some() {
        Some("Item Trait")
    } else if !catalog.plugs_in_category(hash).is_empty() {
        Some("Plug Category")
    } else {
        None
    }
}

/// How many reverse relationships a hash has.
pub(super) fn reverse_count(catalog: &Catalog, hash: u64) -> usize {
    let node = catalog.presentation_node(hash).map_or(0, |node| {
        let children = catalog.presentation_node_children(hash);
        children.nodes.len()
            + children.collectibles.len()
            + children.records.len()
            + node.parents.len()
    });
    let trait_items = catalog
        .item_trait(hash)
        .map_or(0, |(index, _)| catalog.items_with_trait(index).len());
    node + trait_items
        + catalog.plugs_in_category(hash).len()
        + catalog.plug_offers(hash).len()
        + catalog.records_rewarding_item(hash).len()
}

/// Sections for presentation nodes, item traits and plug categories, and for items the sockets
/// that offer them and the triumphs that grant them. Draws nothing when none apply.
pub(super) fn draw_reverse_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    snapshot: Option<&CollectionStateSnapshot>,
) {
    draw_presentation_node(ui, catalog, hash, snapshot);
    draw_item_trait(ui, catalog, hash);
    draw_plug_category(ui, catalog, hash);
    draw_plug_offers(ui, catalog, hash);
    draw_record_rewards(ui, catalog, hash);
}

fn draw_presentation_node(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    snapshot: Option<&CollectionStateSnapshot>,
) {
    let Some(node) = catalog.presentation_node(hash) else {
        return;
    };
    let children = catalog.presentation_node_children(hash);
    let show_state =
        snapshot.is_some() && !(children.collectibles.is_empty() && children.records.is_empty());
    let mut rows = Vec::with_capacity(
        children.nodes.len() + children.collectibles.len() + children.records.len(),
    );
    for child in &children.nodes {
        let mut row = vec![
            Cell::muted("Node"),
            Cell::link_unless(child.hash, hash, node_name(catalog, child.hash)),
            Cell::muted(""),
        ];
        if show_state {
            row.push(Cell::muted(""));
        }
        rows.push(row);
    }
    for collectible in &children.collectibles {
        let mut row = vec![
            Cell::muted("Collectible"),
            Cell::link_unless(
                collectible.hash,
                hash,
                collectible_item_name(catalog, collectible),
            ),
            Cell::muted(collectible.type_name.trim()),
        ];
        if show_state && let Some(snapshot) = snapshot {
            row.push(Cell::text(
                crate::app::collections_page::collectible_state(collectible, snapshot, catalog).0,
            ));
        }
        rows.push(row);
    }
    for record in &children.records {
        let mut row = vec![
            Cell::muted("Record"),
            Cell::link_unless(record.hash, hash, record_name(catalog, record)),
            Cell::muted(""),
        ];
        if show_state && let Some(snapshot) = snapshot {
            row.push(Cell::text(record_status(record, catalog, snapshot).label));
        }
        rows.push(row);
    }
    let headings: &[&str] = if show_state {
        &["Kind", "Name", "Type", "State"]
    } else {
        &["Kind", "Name", "Type"]
    };
    look::section(
        ui,
        ("reverse_node_contents", hash),
        "Contents",
        Some(rows.len()),
        true,
        |ui| {
            if rows.is_empty() {
                look::empty_state(ui, "No Contents");
            } else {
                draw_table(
                    ui,
                    catalog,
                    ("reverse_node_contents", hash),
                    headings,
                    &rows,
                );
            }
        },
    );
    if !node.parents.is_empty() {
        look::section(
            ui,
            ("reverse_node_parents", hash),
            "Parent Nodes",
            Some(node.parents.len()),
            true,
            |ui| {
                for parent in &node.parents {
                    if *parent == hash {
                        continue;
                    }
                    draw_named_catalog_hash_link(ui, catalog, *parent, node_name(catalog, *parent));
                }
            },
        );
    }
}

fn draw_item_trait(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let Some((index, definition)) = catalog.item_trait(hash) else {
        return;
    };
    look::properties(ui, ("reverse_item_trait", hash), |p| {
        p.text("Description", definition.description.trim());
        p.mono("Trait Index", index.to_string());
    });
    let items = catalog.items_with_trait(index);
    look::section(
        ui,
        ("reverse_trait_items", hash),
        "Items with This Trait",
        Some(items.len()),
        true,
        |ui| {
            if items.is_empty() {
                look::empty_state(ui, "No Items");
            } else {
                draw_hash_item_rows(
                    ui,
                    catalog,
                    egui::Id::new(("reverse_trait_items", hash)),
                    items.iter().copied(),
                    &[],
                );
            }
        },
    );
}

fn draw_plug_category(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let plugs = catalog.plugs_in_category(hash);
    if plugs.is_empty() {
        return;
    }
    look::section(
        ui,
        ("reverse_category_plugs", hash),
        "Plugs in This Category",
        Some(plugs.len()),
        true,
        |ui| {
            draw_hash_item_rows(
                ui,
                catalog,
                egui::Id::new(("reverse_category_plugs", hash)),
                plugs.iter().copied(),
                &[],
            );
        },
    );
}

/// Items whose sockets default to or offer this plug.
fn draw_plug_offers(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let offers = catalog.plug_offers(hash);
    if offers.is_empty() {
        return;
    }
    let rows = offers
        .iter()
        .map(|offer| {
            let socket = catalog
                .item(offer.item_hash)
                .and_then(|item| item.sockets.get(offer.socket_index));
            vec![
                Cell::link_unless(offer.item_hash, hash, item_name(catalog, offer.item_hash)),
                Cell::muted(
                    catalog
                        .package_item_type_name(offer.item_hash)
                        .unwrap_or_default(),
                ),
                Cell::text(socket.map_or_else(
                    || format!("Socket {}", offer.socket_index + 1),
                    |socket| socket.display_label(offer.socket_index),
                )),
                Cell::muted(if offer.is_default { "Default" } else { "" }),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("reverse_plug_offers", hash),
        "Offered By",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("reverse_plug_offers", hash),
                &["Item", "Type", "Socket", ""],
                &rows,
            );
        },
    );
}

/// Records whose rewards or intervals grant this item.
fn draw_record_rewards(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    let rewards = catalog.records_rewarding_item(hash);
    if rewards.is_empty() {
        return;
    }
    let records = catalog.records().unwrap_or_default();
    let rows = rewards
        .iter()
        .map(|reward| {
            vec![
                records.get(reward.record_index).map_or_else(
                    || Cell::muted(format!("Record #{}", reward.record_index)),
                    |record| Cell::link_unless(record.hash, hash, record_name(catalog, record)),
                ),
                reward.interval.map_or_else(
                    || Cell::text("Completion"),
                    |interval| Cell::text(format!("Interval {}", interval + 1)),
                ),
                Cell::mono(reward.quantity),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("reverse_record_rewards", hash),
        "Triumph Rewards",
        Some(rows.len()),
        true,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("reverse_record_rewards", hash),
                &["Triumph", "Granted At", "Quantity"],
                &rows,
            );
        },
    );
}
