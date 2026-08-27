//! Shared labels, schema guidance, picker sizing, and visual presentation helpers.

use eframe::egui;

use super::{
    super::{
        ITEM_PICKER_MAX_HEIGHT, ITEM_PICKER_MIN_HEIGHT, SLOTS,
        inventory::{DismantleGearClass, DismantleRarity, SchemaMode},
        item_editor::PickerHeight,
    },
    model::{
        TRANSFER_DESTINATION_ROW_HEIGHT, TRANSFER_DESTINATION_ROW_SPACING,
        TRANSFER_FOOTER_CHROME_HEIGHT, TRANSFER_PICKER_MIN_LIST_HEIGHT,
    },
};

pub(super) fn equipment_target_for_bucket(
    bucket_hash: u64,
) -> Option<(&'static str, &'static str)> {
    SLOTS
        .iter()
        .find_map(|(slot, label, bucket)| (*bucket == bucket_hash).then_some((*slot, *label)))
}

pub(super) fn equipped_header_fill(ui: &egui::Ui) -> egui::Color32 {
    let base = ui.visuals().panel_fill;
    let accent = egui::Color32::from_rgb(255, 210, 72);
    blend_color(base, accent, 0.16)
}

pub(super) fn blend_color(
    base: egui::Color32,
    accent: egui::Color32,
    amount: f32,
) -> egui::Color32 {
    let mix = |base: u8, accent: u8| {
        (f32::from(base) + (f32::from(accent) - f32::from(base)) * amount).round() as u8
    };
    egui::Color32::from_rgba_premultiplied(
        mix(base.r(), accent.r()),
        mix(base.g(), accent.g()),
        mix(base.b(), accent.b()),
        base.a(),
    )
}

pub(super) fn picker_height() -> PickerHeight {
    PickerHeight {
        min: ITEM_PICKER_MIN_HEIGHT,
        max: ITEM_PICKER_MAX_HEIGHT,
    }
}

pub(super) fn picker_height_with_transfer_destinations(destination_count: usize) -> PickerHeight {
    if destination_count == 0 {
        return picker_height();
    }
    let destination_count = u16::try_from(destination_count).unwrap_or(u16::MAX);
    let footer_height = TRANSFER_FOOTER_CHROME_HEIGHT
        + TRANSFER_DESTINATION_ROW_HEIGHT * f32::from(destination_count)
        + TRANSFER_DESTINATION_ROW_SPACING * f32::from(destination_count.saturating_sub(1));
    let min = (ITEM_PICKER_MIN_HEIGHT - footer_height).max(TRANSFER_PICKER_MIN_LIST_HEIGHT);
    PickerHeight {
        min,
        max: (ITEM_PICKER_MAX_HEIGHT - footer_height).max(min),
    }
}

pub(super) fn dismantle_rarity_label(rarity: DismantleRarity) -> &'static str {
    match rarity {
        DismantleRarity::Common => "Common",
        DismantleRarity::Uncommon => "Uncommon",
        DismantleRarity::Rare => "Rare",
        DismantleRarity::Legendary => "Legendary",
        DismantleRarity::Exotic => "Exotic",
    }
}

pub(super) fn dismantle_rarity_summary(rarities: &[DismantleRarity]) -> String {
    match rarities {
        [] => "Any rarity".to_owned(),
        [rarity] => dismantle_rarity_label(*rarity).to_owned(),
        rarities => format!("{} rarities", rarities.len()),
    }
}

pub(super) fn dismantle_class_label(gear_class: Option<DismantleGearClass>) -> &'static str {
    match gear_class {
        None => "Any gear",
        Some(DismantleGearClass::Weapon) => "Weapon",
        Some(DismantleGearClass::Armor) => "Armor",
        Some(DismantleGearClass::Both) => "Weapon + armor",
    }
}

pub(super) fn dismantle_masterwork_label(masterworked: Option<bool>) -> &'static str {
    match masterworked {
        None => "Any state",
        Some(true) => "Masterworked",
        Some(false) => "Not masterworked",
    }
}

#[derive(Clone, Copy)]
pub(super) enum InventoryPageKind {
    Profile,
    Character,
}

pub(super) fn draw_schema_notice(ui: &mut egui::Ui, mode: SchemaMode, page: InventoryPageKind) {
    match mode {
        SchemaMode::MissingOrInvalid => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The settings schema is missing or invalid. Existing items are shown read-only.",
            );
        }
        SchemaMode::Unsupported(version) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Schema {version} predates the schemas supported by this Sundial release. Existing items are shown read-only."
                ),
            );
        }
        SchemaMode::PreInventory(version) if matches!(page, InventoryPageKind::Character) => {
            ui.label(
                egui::RichText::new(format!(
                    "Schema {version} supports profile inventory and equipped loadouts, but stored character inventory requires schema 6. Stored rows are never created or rewritten here."
                ))
                .weak(),
            );
        }
        SchemaMode::PreInventory(_) | SchemaMode::Inventory(_) => {}
        SchemaMode::Future(version) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Schema {version} is newer than this Sundial release. Known item fields remain editable; unrecognized fields are preserved."
                ),
            );
        }
    }

    let editable = !mode.is_read_only()
        && match page {
            InventoryPageKind::Profile => mode.can_mutate_profile_items(),
            InventoryPageKind::Character => {
                mode.can_mutate_character_inventory() || mode.can_mutate_equipment()
            }
        };
    if !editable {
        ui.label(
            egui::RichText::new(
                "Guided controls are disabled; All settings (JSON) remains available for inspection.",
            )
            .weak(),
        );
    }
}

pub(super) fn draw_section_error(ui: &mut egui::Ui, error: &str) {
    ui.colored_label(ui.visuals().error_fg_color, error);
    ui.label(
        egui::RichText::new(
            "This section was left untouched. Repair it in All settings (JSON) before using guided controls.",
        )
        .weak(),
    );
}

pub(super) fn draw_inventory_source_error(ui: &mut egui::Ui, source: &str, error: &str) {
    ui.colored_label(
        ui.visuals().error_fg_color,
        format!("{source} could not be read: {error}"),
    );
    ui.label(
        egui::RichText::new(
            "The other inventory source remains visible, but additions are disabled until this is repaired in All settings (JSON).",
        )
        .weak(),
    );
}

pub(super) fn draw_unresolved_bucket_warning(ui: &mut egui::Ui) {
    ui.colored_label(
        ui.visuals().warn_fg_color,
        "At least one existing definition has no known bucket. Unknown rows are conservatively counted against each candidate bucket, so some additions or cross-bucket replacements may be hidden.",
    );
}
