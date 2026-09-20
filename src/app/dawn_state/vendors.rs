use super::*;
use crate::{
    catalog::Catalog,
    persistence::dawn_account::{ActivityState, VendorProgress, VendorUnlock},
};

// Dawn state/vendors/progress.h. Vendor index, installed progression index, character scope,
// and points per package. These are runtime joins, not guessed display names.
pub(in crate::app) const FACTIONS: &[(u16, u16, bool, i32)] = &[
    (11, 58, true, 2000),
    (16, 62, true, 2000),
    (18, 49, true, 2000),
    (20, 50, false, 2000),
    (21, 54, false, 2000),
    (22, 55, true, 3000),
    (24, 60, false, 2000),
    (392, 46, false, 2000),
    (467, 74, false, 5000),
    (5, 51, true, 2750),
    (6, 52, true, 2750),
    (10, 53, true, 2750),
    (14, 59, true, 2750),
    (66, 63, true, 2750),
    (125, 64, true, 2750),
];

/// The Dawn vendor backed by a progression definition: vendor index, whether progress is kept
/// per character, and reputation points per package.
pub(in crate::app) fn vendor_for_progression(definition_index: u16) -> Option<(u16, bool, i32)> {
    FACTIONS
        .iter()
        .find(|faction| faction.1 == definition_index)
        .map(|faction| (faction.0, faction.2, faction.3))
}

fn name(catalog: &Catalog, vendor: u16) -> String {
    FACTIONS
        .iter()
        .find(|f| f.0 == vendor)
        .and_then(|f| {
            catalog
                .progression_definitions()
                .iter()
                .find(|p| p.definition_index == f.1)
        })
        .and_then(|p| {
            p.factions
                .iter()
                .find(|f| !f.name.is_empty())
                .map(|f| f.name.as_str())
                .or_else(|| (!p.name.is_empty()).then_some(p.name.as_str()))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Vendor {vendor}"))
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    state: &mut ActivityState,
    account: &str,
    character: &str,
    campaigns: &mut u8,
) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("Add Vendor", |ui| {
            for &(vendor, _, personal, _) in FACTIONS {
                let owner = if personal { character } else { account };
                let rows = state
                    .vendors
                    .iter()
                    .filter(|r| r.owner.eq_ignore_ascii_case(owner))
                    .collect::<Vec<_>>();
                let free = (0..16).find(|p| !rows.iter().any(|r| r.position == *p));
                if ui
                    .add_enabled(
                        free.is_some()
                            && !owner.is_empty()
                            && !rows.iter().any(|r| r.vendor == vendor),
                        egui::Button::new(name(catalog, vendor)),
                    )
                    .clicked()
                {
                    state.vendors.push(VendorProgress {
                        owner: owner.into(),
                        position: free.unwrap(),
                        vendor,
                        points: 0,
                        rewards: 0,
                    });
                    ui.close_menu();
                }
            }
        });
    });
    ui.add_space(6.0);
    for (owner, label) in [(account, "Account"), (character, "Character")] {
        let mut remove = None;
        ui.strong(label);
        egui::Grid::new(("vendor-progress", owner))
            .striped(true)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for title in ["Vendor", "Reputation", "Packages Claimed", "Available", ""] {
                    ui.strong(title);
                }
                ui.end_row();
                for row in state
                    .vendors
                    .iter_mut()
                    .filter(|r| r.owner.eq_ignore_ascii_case(owner))
                {
                    ui.label(name(catalog, row.vendor))
                        .on_hover_text(format!("Vendor index {}", row.vendor));
                    ui.push_id((row.position, "points"), |ui| {
                        ui.add(egui::DragValue::new(&mut row.points).range(0..=i32::MAX))
                    });
                    ui.push_id((row.position, "rewards"), |ui| {
                        ui.add(egui::DragValue::new(&mut row.rewards).range(0..=i32::MAX))
                    })
                    .inner
                    .on_hover_text("Lifetime packages already claimed, not pending rewards.");
                    if let Some(f) = FACTIONS.iter().find(|f| f.0 == row.vendor) {
                        ui.label((row.points / f.3 - row.rewards).max(0).to_string());
                    } else {
                        ui.weak("Unknown");
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(row.position);
                    }
                    ui.end_row();
                }
            });
        if let Some(position) = remove {
            state
                .vendors
                .retain(|r| !r.owner.eq_ignore_ascii_case(owner) || r.position != position);
        }
        ui.add_space(10.0);
    }
    egui::CollapsingHeader::new("Campaign Selections").show(ui,|ui| {
        for (bit,hash) in [0xBEB63647_u32,0x6CBEA754,0x65683247].into_iter().enumerate() {
            let mut selected=*campaigns & (1<<bit)!=0;
            let label=catalog.display_name(u64::from(hash)).map(str::to_owned).unwrap_or_else(||format!("Campaign {}",bit+1));
            if ui.checkbox(&mut selected,label).on_hover_text("Controls the vendor's selection flag only. Does not grant the campaign quest or complete it.").changed() {
                if selected {*campaigns|=1<<bit;} else {*campaigns&=!(1<<bit);}
            }
        }
    });
    egui::CollapsingHeader::new("Vendor Unlocks")
        .show(ui, |ui| unlocks(ui, catalog, state, account, character));
}

fn unlocks(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    state: &mut ActivityState,
    account: &str,
    character: &str,
) {
    let id = ui.make_persistent_id("vendor-unlock-scope");
    let mut personal = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut personal, false, "Account");
        ui.selectable_value(&mut personal, true, "Character");
    });
    ui.data_mut(|d| d.insert_temp(id, personal));
    let owner = if personal { character } else { account };
    for kind in 0..2 {
        let definitions = if kind == 0 {
            catalog.unlock_flag_definitions()
        } else {
            catalog.unlock_value_definitions()
        };
        let label = if kind == 0 { "Flags" } else { "Values" };
        egui::CollapsingHeader::new(label)
            .id_salt((owner, kind))
            .show(ui, |ui| {
                ui.menu_button(
                    format!("Add {}", if kind == 0 { "Flag" } else { "Value" }),
                    |ui| {
                        let search_id = ui.make_persistent_id((owner, kind, "search"));
                        let mut query = ui
                            .data(|d| d.get_temp::<String>(search_id))
                            .unwrap_or_default();
                        ui.add(
                            egui::TextEdit::singleline(&mut query)
                                .hint_text("Search by name or index…")
                                .desired_width(280.0),
                        );
                        ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
                        let query = query.trim().to_lowercase();
                        let existing = state
                            .unlocks
                            .iter()
                            .filter(|r| r.owner.eq_ignore_ascii_case(owner) && r.kind == kind)
                            .map(|r| r.slot)
                            .collect::<std::collections::BTreeSet<_>>();
                        let choices = definitions
                            .iter()
                            .enumerate()
                            .take(65536)
                            .filter(|(index, definition)| {
                                !existing.contains(&(*index as u16))
                                    && (query.is_empty()
                                        || index.to_string().contains(&query)
                                        || definition
                                            .name
                                            .as_deref()
                                            .unwrap_or_default()
                                            .to_lowercase()
                                            .contains(&query))
                            })
                            .collect::<Vec<_>>();
                        if existing.len() >= 2048 {
                            ui.weak("This bank is full.");
                            return;
                        }
                        if choices.is_empty() {
                            ui.weak("No matching unlocks.");
                        }
                        egui::ScrollArea::vertical().max_height(260.0).show_rows(
                            ui,
                            ui.spacing().interact_size.y,
                            choices.len(),
                            |ui, range| {
                                for (index, definition) in range.map(|row| choices[row]) {
                                    if ui
                                        .button(format!(
                                            "{index}: {}",
                                            definition.name.as_deref().unwrap_or("Unnamed")
                                        ))
                                        .clicked()
                                    {
                                        state.unlocks.push(VendorUnlock {
                                            owner: owner.into(),
                                            kind,
                                            position: existing.len() as i32,
                                            slot: index as u16,
                                            value: if kind == 0 { 1 } else { 0 },
                                        });
                                        ui.close_menu();
                                    }
                                }
                            },
                        );
                    },
                );
                let mut remove = None;
                egui::Grid::new(("vendor-unlocks", owner, kind))
                    .striped(true)
                    .show(ui, |ui| {
                        for row in state
                            .unlocks
                            .iter_mut()
                            .filter(|r| r.owner.eq_ignore_ascii_case(owner) && r.kind == kind)
                        {
                            let name = definitions
                                .get(usize::from(row.slot))
                                .and_then(|d| d.name.as_deref())
                                .filter(|s| !s.is_empty())
                                .unwrap_or("Unnamed");
                            ui.label(format!("{}: {name}", row.slot));
                            if kind == 0 {
                                egui::ComboBox::from_id_salt((owner, kind, row.slot, "value"))
                                    .selected_text(match row.value {
                                        0 => "False (0)",
                                        1 => "False",
                                        2 => "True",
                                        _ => "Unknown",
                                    })
                                    .show_ui(ui, |ui| {
                                        for (value, label) in [(1, "False"), (2, "True")] {
                                            ui.selectable_value(&mut row.value, value, label);
                                        }
                                    });
                            } else {
                                ui.push_id(row.slot, |ui| {
                                    ui.add(egui::DragValue::new(&mut row.value))
                                });
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(row.slot);
                            }
                            ui.end_row();
                        }
                    });
                if let Some(slot) = remove {
                    state.unlocks.retain(|r| {
                        !r.owner.eq_ignore_ascii_case(owner) || r.kind != kind || r.slot != slot
                    });
                    let mut rows = state
                        .unlocks
                        .iter_mut()
                        .filter(|r| r.owner.eq_ignore_ascii_case(owner) && r.kind == kind)
                        .collect::<Vec<_>>();
                    rows.sort_by_key(|r| r.position);
                    for (position, row) in rows.into_iter().enumerate() {
                        row.position = position as i32;
                    }
                }
            });
    }
}
