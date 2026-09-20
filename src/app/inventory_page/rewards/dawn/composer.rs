use super::*;
use crate::app::inventory_page::{definitions, presentation};
use crate::{app::item_editor, catalog::Catalog, persistence::dawn_account::supports_currency};

pub(super) fn draw(ui: &mut egui::Ui, catalog: &Catalog, draft: &mut RewardDraft) -> bool {
    let width = currency_width(ui);
    let mut add = false;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.label("Currency");
            picker(ui, catalog, draft, width);
        });
        ui.vertical(|ui| {
            ui.set_width(QUANTITY_WIDTH);
            let label = ui.label("Quantity");
            let maximum = super::super::maximum_quantity(catalog, draft.definition_hash);
            draft.quantity = draft.quantity.clamp(1, maximum);
            ui.add_sized(
                [QUANTITY_WIDTH, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut draft.quantity).range(1..=maximum),
            )
            .labelled_by(label.id);
        });
        ui.vertical(|ui| {
            ui.set_width(ACTION_WIDTH);
            ui.label("");
            let valid = draft
                .definition_hash
                .and_then(|hash| catalog.inventory_definition(u64::from(hash)))
                .is_some_and(|definition| supports_currency(definition.metadata));
            add = ui
                .add_enabled_ui(valid, |ui| {
                    ui.add_sized(
                        [ACTION_WIDTH, ui.spacing().interact_size.y],
                        egui::Button::new("Add to Queue"),
                    )
                })
                .inner
                .on_hover_text(
                    "Save to apply. Dawn can reduce or discard rewards at the currency cap.",
                )
                .on_disabled_hover_text("Choose a currency first.")
                .clicked();
        });
    });
    add
}

fn picker(ui: &mut egui::Ui, catalog: &Catalog, draft: &mut RewardDraft, width: f32) {
    let name = draft
        .definition_hash
        .and_then(|hash| catalog.inventory_definition(u64::from(hash)))
        .map_or("Choose Currency", |definition| definition.name);
    let anchor = ui.add_sized(
        [width, ui.spacing().interact_size.y],
        egui::Button::new(name).truncate(),
    );
    let action = item_editor::draw_definition_picker_with_open_request(
        ui,
        catalog,
        "dawn-reward-currency",
        &mut draft.query,
        presentation::picker_height(),
        (Some(&anchor), anchor.clicked()),
        |query| item_editor::DefinitionPickerChoices {
            definitions: definitions::profile_bucket_definition_choices(
                catalog
                    .profile_item_candidates(query)
                    .filter(|definition| supports_currency(definition.metadata)),
            ),
            existing_inventory: Vec::new(),
            clear: None,
            random_item_builder_hash: None,
            empty_message: "No supported currencies found".into(),
        },
    );
    if let Some(item_editor::ItemEditorAction::SetDefinition { hash }) = action {
        draft.definition_hash = u32::try_from(hash).ok();
    }
}
