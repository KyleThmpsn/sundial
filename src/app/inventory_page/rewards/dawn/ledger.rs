use super::*;
#[cfg(test)]
mod tests;
use crate::{
    catalog::Catalog,
    persistence::dawn_account::{
        DawnAccountDocument, EDITOR_MISSION, RewardDebt, supports_currency,
    },
};

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: &DawnAccountDocument,
    edit: &mut Option<Edit>,
) {
    let pending: Vec<_> = document
        .reward_debts()
        .iter()
        .filter(|row| !row.delivered)
        .collect();
    let finished: Vec<_> = document
        .reward_debts()
        .iter()
        .filter(|row| row.delivered)
        .collect();
    let width = currency_width(ui);
    ui.horizontal(|ui| {
        ui.strong("Pending");
        ui.weak(pending.len().to_string());
    });
    ui.separator();
    if pending.is_empty() {
        ui.weak("No pending rewards");
    } else {
        egui::Grid::new("dawn-pending-rewards")
            .num_columns(3)
            .striped(true)
            .spacing([GAP, 6.0])
            .show(ui, |ui| {
                for debt in pending {
                    ui.push_id(debt.id, |ui| currency(ui, catalog, debt, width));
                    let editable =
                        debt.credited == 0 && debt.account_soid == document.primary_soid().get();
                    quantity(ui, catalog, debt, editable, edit);
                    ui.push_id((debt.id, "cancel"), |ui| {
                        if ui
                            .add_enabled_ui(editable, |ui| {
                                ui.add_sized(
                                    [ACTION_WIDTH, ui.spacing().interact_size.y],
                                    egui::Button::new("Cancel"),
                                )
                            })
                            .inner
                            .on_hover_text("Cancel this reward without credit. Save to apply.")
                            .clicked()
                        {
                            *edit = Some(Edit::Remove(debt.id));
                        }
                    });
                    ui.end_row();
                }
            });
    }
    if !finished.is_empty() {
        ui.add_space(16.0);
        egui::CollapsingHeader::new(format!("Delivery History ({})", finished.len()))
            .id_salt("dawn-reward-history")
            .show(ui, |ui| history(ui, catalog, &finished));
    }
}

fn currency(ui: &mut egui::Ui, catalog: &Catalog, debt: &RewardDebt, width: f32) {
    let name = catalog
        .names
        .get(&u64::from(debt.definition_hash))
        .cloned()
        .unwrap_or_else(|| format!("0x{:08X}", debt.definition_hash));
    let origin = if debt.mission_hash == EDITOR_MISSION {
        "Sundial".into()
    } else {
        format!("Mission 0x{:08X}", debt.mission_hash)
    };
    let height = ui.spacing().interact_size.y;
    let texture = catalog.icon_texture(ui.ctx(), u64::from(debt.definition_hash));
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, height));
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(texture) = texture {
                ui.add(
                    egui::Image::new((texture.id(), egui::vec2(height, height)))
                        .bg_fill(crate::app::ui::package_icon_backdrop(ui)),
                );
            } else {
                ui.allocate_exact_size(egui::vec2(height, height), egui::Sense::hover());
            }
            ui.add(egui::Label::new(&name).truncate())
        },
    )
    .inner
    .on_hover_text(format!(
        "{name}\nSource: {origin}\nReward #{}\nCharacter: {:016X}",
        debt.id, debt.character_soid
    ));
}

fn quantity(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    debt: &RewardDebt,
    editable: bool,
    edit: &mut Option<Edit>,
) {
    let metadata = catalog
        .inventory_definition(u64::from(debt.definition_hash))
        .map(|definition| *definition.metadata)
        .filter(supports_currency);
    ui.push_id((debt.id, "quantity"), |ui| {
        if editable && let Some(metadata) = metadata {
            let mut quantity = debt.quantity;
            if ui
                .add_sized(
                    [QUANTITY_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut quantity).range(
                        1..=metadata
                            .max_stack_size
                            .unwrap_or(1)
                            .max(debt.quantity as u32),
                    ),
                )
                .changed()
            {
                *edit = Some(Edit::Quantity(debt.id, quantity));
            }
        } else {
            ui.add_sized(
                [QUANTITY_WIDTH, ui.spacing().interact_size.y],
                egui::Label::new(debt.quantity.to_string()),
            );
        }
    });
}

fn history(ui: &mut egui::Ui, catalog: &Catalog, debts: &[&RewardDebt]) {
    let width = (ui.available_width() - 2.0 * QUANTITY_WIDTH - ACTION_WIDTH - 3.0 * GAP).max(100.0);
    egui::Grid::new("dawn-finished-rewards").num_columns(4).striped(true).spacing([GAP, 6.0]).show(ui, |ui| {
        for (heading, cell_width, align) in [("Currency", width, egui::Align::Min), ("Queued", QUANTITY_WIDTH, egui::Align::Max), ("Received", QUANTITY_WIDTH, egui::Align::Max), ("Result", ACTION_WIDTH, egui::Align::Min)] {
            text_cell(ui, egui::RichText::new(heading).strong(), cell_width, align);
        }
        ui.end_row();
        for debt in debts.iter().rev() {
            currency(ui, catalog, debt, width);
            text_cell(ui, debt.quantity.to_string(), QUANTITY_WIDTH, egui::Align::Max);
            text_cell(ui, debt.credited.to_string(), QUANTITY_WIDTH, egui::Align::Max);
            let status = if debt.credited == debt.quantity { "Delivered" } else if debt.credited == 0 { "No Credit" } else { "Partial" };
            text_cell(ui, status, ACTION_WIDTH, egui::Align::Min)
                .on_hover_text("No Credit means the reward was cancelled or the currency was capped. Finished rewards cannot be edited.");
            ui.end_row();
        }
    });
}

fn text_cell(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    width: f32,
    align: egui::Align,
) -> egui::Response {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(align),
        |ui| {
            ui.set_min_size(size);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}
