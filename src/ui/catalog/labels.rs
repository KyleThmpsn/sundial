//! Selection over the active label registry. Callers own all native record edits.
use crate::investment::discovery::labels::{self, Registry};
use eframe::egui;

pub fn select_many(
    ui: &mut egui::Ui,
    registry: Option<&Registry>,
    observed: &[(u32, &str, u32)],
    current: &mut Vec<u32>,
) -> bool {
    if let Some(chosen) = choices(ui, registry, observed, current, true) {
        *current = chosen;
        true
    } else {
        false
    }
}

pub fn select_one(ui: &mut egui::Ui, registry: Option<&Registry>, current: &mut u32) -> bool {
    if let Some(chosen) = choices(ui, registry, &[], &[*current], false) {
        *current = chosen[0];
        true
    } else {
        false
    }
}

pub fn title(hash: u32) -> String {
    labels::name(hash).map_or_else(|| format!("0x{hash:08X}"), str::to_owned)
}

fn choices(
    ui: &mut egui::Ui,
    registry: Option<&Registry>,
    observed: &[(u32, &str, u32)],
    current: &[u32],
    multiple: bool,
) -> Option<Vec<u32>> {
    let state = ui.make_persistent_id("label-search");
    let (mut query, mut show_all) = ui
        .data(|data| data.get_temp::<(String, bool)>(state))
        .unwrap_or_default();
    ui.set_width(340.0_f32.min(ui.ctx().screen_rect().width() - 40.0));
    let mut hashes = Vec::new();
    ui.horizontal(|ui| {
        let search = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Search Labels")
                .desired_width((ui.available_width() - 150.0).max(80.0)),
        );
        search.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search Labels")
        });
        ui.checkbox(&mut show_all, "Show All")
            .on_hover_text("Include registered labels whose names are still unknown.");
        hashes = candidates(registry, observed, current, &query, show_all);
        ui.weak(hashes.len().to_string())
            .on_hover_text("Matching labels");
    });
    ui.data_mut(|data| data.insert_temp(state, (query.clone(), show_all)));
    let mut selected = current.to_vec();
    egui::ScrollArea::vertical()
        .id_salt("label-results")
        .max_height(260.0)
        .show(ui, |ui| {
            for hash in hashes {
                let entry = registry.and_then(|registry| registry.get(hash));
                let label = title(hash);
                let response = if multiple {
                    let mut on = selected.contains(&hash);
                    let response = ui.checkbox(&mut on, &label);
                    if response.changed() {
                        if on {
                            selected.push(hash);
                        } else {
                            selected.retain(|candidate| *candidate != hash);
                        }
                    }
                    response
                } else {
                    let response = ui.selectable_label(selected.contains(&hash), &label);
                    if response.clicked() {
                        selected = vec![hash];
                        ui.close_menu();
                    }
                    response
                };
                response.on_hover_ui(|ui| {
                    ui.set_max_width(340.0);
                    ui.label(format!("{label} · 0x{hash:08X}"));
                    if let Some((_, _, uses)) = observed.iter().find(|(key, _, _)| *key == hash) {
                        ui.label(format!("Used at this field by {uses} stock perk records."));
                    }
                    match (registry, entry) {
                        (Some(registry), Some(entry)) if entry.group => {
                            let members = registry.members(hash).map(|entry| entry.title()).collect::<Vec<_>>();
                            ui.label(format!("Group Members: {}", members.join(", ")));
                            ui.label("The filter applies to the group's member labels.");
                        }
                        (Some(_), Some(_)) => {
                            ui.label("Registered label. Whether it can match depends on this field's event or object.");
                        }
                        (Some(_), None) => {
                            ui.label("Not in the active registry. Remove or replace this value before building.");
                        }
                        (None, _) => {
                            ui.label("The active label registry is not available yet.");
                        }
                    }
                });
            }
        });
    (selected != current).then_some(selected)
}

fn candidates(
    registry: Option<&Registry>,
    observed: &[(u32, &str, u32)],
    current: &[u32],
    query: &str,
    show_all: bool,
) -> Vec<u32> {
    let mut hashes = registry.map_or_else(
        || {
            observed
                .iter()
                .map(|(hash, _, _)| *hash)
                .collect::<Vec<_>>()
        },
        |registry| registry.entries().iter().map(|entry| entry.hash).collect(),
    );
    // Unknown current values stay removable, even with Show All off.
    hashes.extend_from_slice(current);
    hashes.sort_unstable();
    hashes.dedup();
    let query = query.trim().to_lowercase();
    hashes.retain(|hash| {
        (show_all || current.contains(hash) || labels::name(*hash).is_some())
            && query.split_whitespace().all(|word| {
                format!("{} 0x{hash:08X}", title(*hash))
                    .to_lowercase()
                    .contains(word)
            })
    });
    hashes.sort_by_cached_key(|hash| {
        (
            !current.contains(hash),
            !observed.iter().any(|(candidate, _, _)| candidate == hash),
            title(*hash),
        )
    });
    hashes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_uses_active_registry_and_keeps_unknown_current_values_removable() {
        let registry =
            Registry::read(&crate::package_runtime::labels::fixture::registry()).unwrap();
        let observed = [(0x12345678, "stale label", 1)];
        let hashes = candidates(Some(&registry), &observed, &[0x87654321], "", false);
        assert!(!hashes.contains(&0x12345678));
        assert!(hashes.contains(&0x87654321));
        assert!(!hashes.contains(&0xEC6A8FC5));
        assert!(candidates(Some(&registry), &[], &[], "EC6A8FC5", true).contains(&0xEC6A8FC5));
        assert_eq!(
            candidates(Some(&registry), &[], &[], "precision", false),
            vec![0x962EA19B]
        );
    }
}
