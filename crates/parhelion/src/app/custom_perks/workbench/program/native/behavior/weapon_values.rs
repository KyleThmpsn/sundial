//! Replace Three Weapon Values, named by the stock perks that set its three floats.
use super::*;

/// Replace Three Weapon Values. What its three floats drive is not established, so its stock
/// settings are named by the perks that set them.
pub(super) const WEAPON_VALUES_CLASS: u32 = 0x8080_3E0B;

/// The three floats as the stock perks set them together, with those perks.
const WEAPON_VALUE_SETTINGS: [(&str, [f32; 3], &str); 3] = [
    (
        "Full Auto Perks",
        [0.35, 0.35, 0.35],
        "Set by 10 stock perks, such as Full Auto Trigger System, Rapid-Fire Frame and Thunderer.",
    ),
    (
        "MIDA Multi-Tool",
        [0.1, 0.1, 0.1],
        "Set by MIDA Multi-Tool and For the Empire.",
    ),
    (
        "Close the Gap",
        [0.1, 0.1, 0.01],
        "Set by Close the Gap and one other stock perk.",
    ),
];

/// A Stock Setting tile over the three values: which stock setting they hold, and a choice
/// that writes one.
pub(super) fn weapon_values(ui: &mut egui::Ui, block: &mut native::Block) -> Result<(), String> {
    let mut current = [0_u32; 3];
    for (slot, at) in [4_usize, 8, 12].into_iter().enumerate() {
        let bytes = block
            .bytes
            .get(at..at + 4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .ok_or("Missing weapon values.")?;
        current[slot] = u32::from_le_bytes(bytes);
    }
    let matched = WEAPON_VALUE_SETTINGS
        .iter()
        .position(|&(_, values, _)| values.map(f32::to_bits) == current);
    let reading = matched.map_or("Custom", |index| WEAPON_VALUE_SETTINGS[index].0);
    let mut picked = matched;
    // The three values themselves are tiles of their own under this one, so a setting no
    // stock perk uses reads Custom here and is typed there.
    crate::app::style::tiles(ui, |ui, width| {
        crate::app::style::tile(
            ui,
            width,
            "weapon-value-setting",
            "Stock Setting",
            "",
            false,
            |ui| {
                egui::ComboBox::from_id_salt("weapon-value-setting")
                    .width(ui.available_width())
                    .truncate()
                    .selected_text(reading)
                    .show_ui(ui, |ui| {
                        for (index, (name, _, hint)) in WEAPON_VALUE_SETTINGS.iter().enumerate() {
                            ui.selectable_value(&mut picked, Some(index), *name)
                                .on_hover_text(*hint);
                        }
                    });
                pickers::name_combo(ui, "weapon-value-setting", "Stock Setting");
            },
        );
    });
    if let Some(index) = picked.filter(|index| Some(*index) != matched) {
        let values = WEAPON_VALUE_SETTINGS[index].1;
        for (value, at) in values.into_iter().zip([4_usize, 8, 12]) {
            block.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}
