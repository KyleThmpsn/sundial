use super::*;
use crate::app::inspector::look;
use crate::app::inspector::requests::{owned_quantities, request_owned_quantities};

pub(super) fn draw_hash_item_source_comparison(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    item: Option<&ItemDef>,
    context: &DefinitionInspectionContext,
) {
    let title = if context.instance_id.is_some() {
        "Selected Instance"
    } else {
        "Source Snapshot"
    };
    look::section(ui, ("item_source", hash), title, None, true, |ui| {
        ui.label(
            egui::RichText::new(format!("Opened from {}", context.source)).color(look::muted(ui)),
        );
        ui.add_space(4.0);
        look::properties(ui, ("item_instance", hash), |rows| {
            draw_instance_rows(rows, catalog, hash, item, context);
        });
    });
}

fn draw_instance_rows(
    rows: &mut look::Properties<'_>,
    catalog: &Catalog,
    hash: u64,
    item: Option<&ItemDef>,
    context: &DefinitionInspectionContext,
) {
    if catalog.item_has_power_stat(hash)
        && let Some(level) = context.authored_level
    {
        let cap = catalog.item_power_cap(hash).map_or_else(
            || "No catalog cap".into(),
            |value| format!("Catalog cap {value}"),
        );
        rows.text(
            "Power",
            format!(
                "{} · authored level {level} · {cap}",
                displayed_item_power(level)
            ),
        );
    }
    if let Some(instance_id) = &context.instance_id {
        rows.mono("Instance ID", instance_id.as_str());
    }
    if let Some(quantity) = context.quantity {
        rows.mono("Quantity", quantity.to_string());
    }
    if let Some(plugs) = &context.plugs {
        rows.text("Plug Source", super::super::instance::plug_source(plugs));
    }
    if let Some(plug_count) = context.plug_count {
        let sockets = item.map_or_else(
            || "Catalog sockets unavailable".into(),
            |item| format!("{} catalog sockets", item.sockets.len()),
        );
        rows.mono("Saved Plugs", format!("{plug_count} · {sockets}"));
    }
    if let Some(flags) = context.flags {
        rows.mono("Instance Flags", format!("0x{flags:02X}"));
    }
}

/// The card at the top of an item page, with the item's description under it.
pub(super) fn draw_header(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    let catalog = content.catalog;
    let header = look::Header {
        title: content.resolved_name.as_deref().unwrap_or(UNNAMED),
        kind: catalog.item_kind_label(content.hash),
        hash: content.hash,
        icon: content.hash,
        subtitle: catalog.package_item_type_name(content.hash),
        path: item_crumbs(catalog, content.hash),
    };
    look::header(ui, catalog, &header, |ui| draw_facts(ui, content));
    if let Some(description) = catalog
        .description(content.hash)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(crate::app::ui::destiny_text(ui, description).color(look::muted(ui)))
                .wrap(),
        );
    }
}

/// An item sits in Collections through its collectible.
fn item_crumbs(catalog: &Catalog, hash: u64) -> Vec<look::Crumb> {
    catalog
        .collectibles()
        .iter()
        .find(|collectible| collectible.item_hash == hash)
        .map_or_else(Vec::new, |collectible| {
            look::presentation_crumbs(catalog, collectible.hash)
        })
}

fn draw_facts(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if let Some(metadata) = content.matches.item_package_metadata {
        draw_package_facts(ui, metadata);
    }
    if let Some(item) = content.matches.item.filter(|item| item.class_type <= 2) {
        look::fact(ui, "Class", item_class_type_label(item.class_type));
    }
    draw_bucket_fact(ui, content);
    draw_owned_fact(ui, content);
}

fn draw_package_facts(ui: &mut egui::Ui, metadata: &ItemPackageMetadata) {
    if let Some(ammo) = metadata.weapon_ammo_type {
        look::fact(ui, "Ammo", ammo.label());
    }
    if metadata.rarity != ItemRarity::Unknown {
        look::fact(ui, "Rarity", metadata.rarity.label());
    }
    if let Some(damage_type) = metadata.damage_type {
        look::fact(ui, "Damage Type", damage_type.label());
    }
    if let Some(power_cap) = metadata.power_cap {
        look::fact(ui, "Power Cap", power_cap.to_string());
    }
}

fn draw_bucket_fact(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    let bucket_hash = content.matches.item.map_or(0, |item| item.bucket_hash);
    let label = content
        .matches
        .inventory_metadata
        .map(|metadata| metadata.bucket_label())
        .or_else(|| content.catalog.display_name(bucket_hash).map(str::to_owned));
    if let Some(label) = label {
        look::fact_link(ui, content.catalog, "Bucket", bucket_hash, label);
    }
}

/// What the loaded account holds, for items that can be held.
fn draw_owned_fact(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    if content.matches.inventory_metadata.is_none() {
        return;
    }
    request_owned_quantities(ui.ctx());
    match owned_quantities(ui.ctx()) {
        // The app answers on its next pass.
        None => ui.ctx().request_repaint(),
        Some(Some(quantities)) => {
            let owned = quantities.get(&content.hash).copied().unwrap_or(0);
            look::fact(ui, "Owned", owned.to_string());
        }
        Some(None) => {}
    }
}
