//! Custom name, description and native classification controls.
use super::*;

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    donor_hash: u32,
    socket_type: Option<u16>,
    variant: &mut WeaponSocketPlugVariantRecipe,
) {
    ui.push_id(
        (
            "private-plug-classification",
            variant.socket_index,
            variant.choice_index,
        ),
        |ui| {
            draw_contents(ui, catalog, donor_hash, socket_type, variant);
        },
    );
}

fn draw_contents(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    donor_hash: u32,
    socket_type: Option<u16>,
    variant: &mut WeaponSocketPlugVariantRecipe,
) {
    let mut name = variant.name.clone().unwrap_or_default();
    ui.label("Custom Perk Name");
    if named_control(
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Keep source name")
                .desired_width(ui.available_width()),
        ),
        "Custom Perk Name",
    )
    .changed()
    {
        variant.name = (!name.trim().is_empty()).then_some(name);
    }
    let mut description = variant.description.clone().unwrap_or_default();
    ui.label("Description");
    if named_control(
        ui.add(
            egui::TextEdit::multiline(&mut description)
                .hint_text("Keep source description")
                .desired_rows(2)
                .desired_width(ui.available_width()),
        ),
        "Custom Perk Description",
    )
    .changed()
    {
        variant.description = (!description.trim().is_empty()).then_some(description);
    }
    ui.label("Perk Classification").on_hover_text(
            "Copies this stock plug's native category and item-type text into the custom copy. Perks, runtime, stats, name and icon remain from your custom perk. The destination socket type is a separate setting.",
        );
    let selected = variant
        .classification_donor_hash
        .as_ref()
        .and_then(|hash| hash.parse_u32().ok());
    let label = variant.classification_donor_hash.as_ref().map_or_else(
        || "Original Classification".to_owned(),
        |hash| {
            selected.map_or_else(
                || format!("Invalid source: {hash}"),
                |hash| catalog.plug_label(hash, false),
            )
        },
    );
    let query_id = ui.id().with("query");
    let mut query = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(query_id).unwrap_or_default());
    let result = catalog.draw_supported_plug_choice_picker(
        ui,
        donor_hash,
        usize::from(variant.socket_index),
        socket_type,
        usize::from(variant.choice_index),
        selected,
        &mut query,
        PlugChoicePickerButton {
            text: &label,
            icon_hash: selected,
            tooltip: None,
            width: ui.available_width().max(1.0) as u16,
        },
        PlugSelectionMode::AnyPlug,
    );
    ui.ctx().data_mut(|data| data.insert_temp(query_id, query));
    match result {
        Ok(Some(choice)) => variant.classification_donor_hash = choice.hash.map(HexHash::new),
        Ok(None) => {}
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    ui.label("Changes the perk's category and type label, not its behavior or socket role.");
    if variant.classification_donor_hash.is_some()
        && ui.button("Restore Original Classification").clicked()
    {
        variant.classification_donor_hash = None;
    }
}
