use super::*;

pub(super) fn draw_hash_material_requirements(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    requirements: &[MaterialRequirementDef],
) {
    if requirements.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Material requirements ({})", requirements.len()))
        .id_salt((id, "material_requirements"))
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new(("hash_material_requirements", id))
                .num_columns(7)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Index");
                    ui.strong("Hash");
                    ui.strong("Name");
                    ui.strong("Quantity");
                    ui.strong("Delete").on_hover_text("Delete on action");
                    ui.strong("Omit").on_hover_text("Omit from requirements");
                    ui.strong("Condition");
                    ui.end_row();
                    for requirement in requirements {
                        ui.monospace(requirement.item_definition_index.to_string());
                        draw_hash_link(
                            ui,
                            requirement.item_hash,
                            format_hash_hex(requirement.item_hash),
                        );
                        item_definition_name_cell(ui, catalog, requirement.item_hash, 190.0);
                        ui.monospace(requirement.quantity.to_string());
                        ui.label(yes_no(requirement.delete_on_action));
                        ui.label(yes_no(requirement.omit_from_requirements));
                        ui.monospace(format!("0x{:04X}", requirement.condition));
                        ui.end_row();
                    }
                });
        });
}

pub(super) fn draw_hash_material_requirement_set(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    set: &crate::catalog::MaterialRequirementSetDef,
    inspected_hash: u64,
) {
    metadata_subsection(
        ui,
        &format!("Material requirement set #{}", set.index),
        |ui| {
            egui::Grid::new(("hash_material_requirement_set", set.index))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    hash_detail_field(
                        ui,
                        "Matched as",
                        if set.hash == inspected_hash {
                            "Material requirement set hash"
                        } else {
                            "Item definition hash"
                        },
                        false,
                    );
                    hash_detail_field(
                        ui,
                        "Material requirement set index",
                        set.index.to_string(),
                        true,
                    );
                    hash_hex_and_decimal_field(ui, "Material requirement set hash", set.hash);
                });
            draw_hash_material_requirements(
                ui,
                catalog,
                egui::Id::new(("hash_material_requirement_set_detail", set.index, set.hash)),
                &set.requirements,
            );
        },
    );
}
