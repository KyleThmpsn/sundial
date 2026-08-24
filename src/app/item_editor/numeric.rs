use super::*;

const POWER_PER_LEVEL: i64 = 10;
const MINIMUM_POWERED_ITEM_POWER: i64 = 750;
const DEFAULT_MAXIMUM_POWERED_ITEM_POWER: i64 = 1060;
const MAXIMUM_STORED_ITEM_POWER: i64 = 21_474_836_470;

pub(crate) fn draw_level_and_quantity(
    ui: &mut egui::Ui,
    scope: impl Hash,
    fields: NumericItemFields,
) -> Vec<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let mut actions = Vec::new();
        if let Some(level) = fields.level {
            let mut power = displayed_item_power(level);
            let authored_power_max = item_power_input_max(fields.power_max);
            let input_power_max =
                effective_power_input_max(fields.power_max, fields.allow_power_above_cap);
            ui.label("Power");
            let response = ui.add(
                egui::DragValue::new(&mut power)
                    .speed(POWER_PER_LEVEL as f64)
                    .range(0..=input_power_max)
                    .clamp_existing_to_range(false),
            );
            if response.changed()
                && let Some(authored_level) = authored_item_level(power)
                && authored_level != level
            {
                actions.push(ItemEditorAction::SetLevel {
                    level: authored_level,
                });
            }
            let tooltip = if fields.allow_power_above_cap {
                format!(
                    "Experimental unrestricted Power is enabled. This item's package-defined cap is {authored_power_max}. Destiny may display the item at its cap, but the stored value may still affect overall character Power. Sunrise stores one-tenth of the entered value; edits snap down to a multiple of 10."
                )
            } else {
                format!(
                    "This item supports up to {authored_power_max} Power. Sunrise stores one-tenth of the in-game value; edits snap down to a multiple of 10."
                )
            };
            response.on_hover_text(tooltip);
        }
        if fields.level.is_some() && fields.quantity.is_some() {
            ui.add_space(8.0);
        }
        if let Some(mut quantity) = fields.quantity {
            let quantity_max = fields
                .quantity_max
                .unwrap_or_else(|| i64::from(i32::MAX))
                .max(1);
            ui.label("Quantity");
            if ui
                .add(egui::DragValue::new(&mut quantity).range(1..=quantity_max))
                .changed()
            {
                actions.push(ItemEditorAction::SetQuantity { quantity });
            }
        }
        actions
    })
    .inner
}

pub(crate) fn displayed_item_power(authored_level: i64) -> i64 {
    if authored_level <= 0 {
        0
    } else {
        authored_level
            .saturating_mul(POWER_PER_LEVEL)
            .max(MINIMUM_POWERED_ITEM_POWER)
    }
}

pub(crate) fn item_power_input_max(power_max: Option<i64>) -> i64 {
    power_max
        .unwrap_or(DEFAULT_MAXIMUM_POWERED_ITEM_POWER)
        .max(MINIMUM_POWERED_ITEM_POWER)
}

pub(crate) fn effective_power_input_max(
    power_max: Option<i64>,
    allow_power_above_cap: bool,
) -> i64 {
    if allow_power_above_cap {
        MAXIMUM_STORED_ITEM_POWER
    } else {
        item_power_input_max(power_max)
    }
}

pub(crate) fn new_inventory_item_level(native_bucket_id: u8, power_max: Option<i64>) -> i64 {
    if native_bucket_id <= 7 {
        item_power_input_max(power_max) / POWER_PER_LEVEL
    } else {
        0
    }
}

pub(crate) fn authored_item_level(display_power: i64) -> Option<i64> {
    if display_power < 0 {
        None
    } else if display_power == 0 {
        Some(0)
    } else {
        Some(display_power.max(MINIMUM_POWERED_ITEM_POWER) / POWER_PER_LEVEL)
    }
}
