use super::item_details::{Cell, draw_table, item_name};
use super::progression::property_link;
use super::*;
use crate::app::inspector::look;

pub(super) fn draw_hash_material_requirements(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    requirements: &[MaterialRequirementDef],
) {
    if requirements.is_empty() {
        return;
    }
    look::section(
        ui,
        (id, "material_requirements"),
        "Material Requirements",
        Some(requirements.len()),
        false,
        |ui| draw_material_requirement_rows(ui, catalog, id, requirements),
    );
}

fn draw_material_requirement_rows(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    requirements: &[MaterialRequirementDef],
) {
    super::super::requests::request_owned_quantities(ui.ctx());
    let answer = super::super::requests::owned_quantities(ui.ctx());
    if answer.is_none() {
        // The app answers on its next pass. Make sure there is one.
        ui.ctx().request_repaint();
    }
    let owned = answer.flatten();
    egui::Grid::new(("hash_material_requirements", id))
        .num_columns(6)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Item");
            ui.strong("Quantity");
            ui.strong("Owned");
            ui.strong("Consumed");
            ui.strong("Omitted");
            ui.strong("Condition");
            ui.end_row();
            for requirement in requirements {
                draw_named_catalog_hash_link(
                    ui,
                    catalog,
                    requirement.item_hash,
                    item_name(catalog, requirement.item_hash),
                );
                ui.monospace(requirement.quantity.to_string());
                match owned.as_deref() {
                    Some(owned) => {
                        let held = owned.get(&requirement.item_hash).copied().unwrap_or(0);
                        let short = held < i64::from(requirement.quantity);
                        let text = egui::RichText::new(held.to_string()).monospace();
                        if short {
                            ui.label(text.color(ui.visuals().warn_fg_color))
                                .on_hover_text("Fewer than the requirement.");
                        } else {
                            ui.label(text);
                        }
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.label(yes_no(requirement.delete_on_action));
                ui.label(yes_no(requirement.omit_from_requirements));
                ui.monospace(format!("0x{:04X}", requirement.condition));
                ui.end_row();
            }
        });
}

pub(super) fn draw_hash_material_requirement_set(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    set: &crate::catalog::MaterialRequirementSetDef,
    inspected_hash: u64,
) {
    let own_page = set.hash == inspected_hash;
    if !own_page {
        look::subheading(ui, &format!("Material Requirement Set #{}", set.index));
    }
    look::properties(ui, ("hash_material_requirement_set", set.index), |p| {
        if !own_page {
            p.text("Matched As", "Required Item");
            property_link(
                p,
                "Set",
                catalog,
                set.hash,
                inspected_hash,
                format!("Set #{}", set.index),
            );
        }
        p.mono("Set Index", set.index.to_string());
    });
    let id = egui::Id::new(("hash_material_requirement_set_detail", set.index, set.hash));
    if own_page {
        look::section(
            ui,
            (id, "material_requirements"),
            "Material Requirements",
            Some(set.requirements.len()),
            true,
            |ui| {
                if set.requirements.is_empty() {
                    look::empty_state(ui, "No Requirements");
                } else {
                    draw_material_requirement_rows(ui, catalog, id, &set.requirements);
                }
            },
        );
        draw_material_set_users(ui, catalog, set);
    } else {
        draw_hash_material_requirements(ui, catalog, id, &set.requirements);
    }
}

/// Items whose insertion or enabled requirement is this set.
fn draw_material_set_users(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    set: &crate::catalog::MaterialRequirementSetDef,
) {
    let users = catalog.items_using_material_requirement_set(set.index);
    if users.is_empty() {
        return;
    }
    let rows = users
        .iter()
        .map(|(item_hash, usage)| {
            vec![
                Cell::link_unless(*item_hash, set.hash, item_name(catalog, *item_hash)),
                Cell::muted(
                    catalog
                        .package_item_type_name(*item_hash)
                        .unwrap_or_default(),
                ),
                Cell::text(format!("{usage:?}")),
            ]
        })
        .collect::<Vec<_>>();
    look::section(
        ui,
        ("hash_material_set_users", set.index),
        "Used by Items",
        Some(rows.len()),
        rows.len() <= HASH_RELATIONSHIP_AUTO_EXPAND_LIMIT,
        |ui| {
            draw_table(
                ui,
                catalog,
                ("hash_material_set_users", set.index),
                &["Item", "Type", "Requirement"],
                &rows,
            );
        },
    );
}
