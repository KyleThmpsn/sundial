//! Lossless inspection of native rows alongside the intentionally bounded authoring view.
use super::state::InvestmentTable;
use super::*;

pub(super) fn hidden_count(document: &Value, table: InvestmentTable) -> usize {
    let kind = match table {
        InvestmentTable::FlagOverrides => 0,
        InvestmentTable::ValueOverrides => 1,
    };
    document["_native_progression"]["hidden_family_counts"][kind]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

struct Row {
    bank: &'static str,
    slot: usize,
    lane: i64,
    value: i64,
    meaning: &'static str,
    hash: Option<u64>,
    name: String,
}

const BANKS: [&str; 8] = [
    "Account Flags",
    "Profile Flags",
    "Character Flags",
    "Account Objectives",
    "Character Object Flags",
    "Character Objectives",
    "Account Progressions",
    "Character Progressions",
];

fn row(catalog: &Catalog, family: bool, bank: usize, slot: usize, lane: i64, value: i64) -> Row {
    let definition = if family {
        if bank == 0 {
            catalog.unlock_flag_definition(slot)
        } else {
            catalog.unlock_value_definition(slot)
        }
    } else {
        match bank {
            0 => catalog.unlock_flag_for_state(1, slot),
            1 => catalog.unlock_flag_for_state(2, slot),
            2 => catalog.unlock_flag_for_state(6, slot),
            3 => catalog.unlock_value_for_state(1, slot),
            4 => catalog.unlock_flag_for_state(3, slot),
            5 => catalog.unlock_value_for_state(2, slot),
            _ => None,
        }
        .map(|(_, definition)| definition)
    };
    let progression = (!family && bank >= 6)
        .then(|| catalog.progression_definition(slot))
        .flatten();
    let flag = if family {
        bank == 0
    } else {
        matches!(bank, 0 | 1 | 2 | 4)
    };
    let meaning = if flag {
        match value {
            2 => "Set",
            0 => "Clear",
            1 if family => "Logical Value 1",
            _ => "Not Set · Raw Byte",
        }
    } else if !family && bank >= 6 && lane != 0 {
        "Undecoded Lane"
    } else {
        "Saved Value"
    };
    let hash = definition
        .map(|definition| definition.hash)
        .or_else(|| progression.map(|definition| definition.hash));
    let name = definition
        .and_then(definition_name)
        .map(str::to_owned)
        .or_else(|| progression.and_then(progression_display_name))
        .or_else(|| hash.and_then(|hash| catalog.display_name(hash).map(str::to_owned)))
        .or_else(|| {
            definition.map(|definition| format!("References: {}", override_meaning(definition)))
        })
        .unwrap_or_else(|| "No Package Name".into());
    Row {
        bank: if family {
            if bank == 0 {
                "Flag Overrides"
            } else {
                "Value Overrides"
            }
        } else {
            BANKS[bank]
        },
        slot,
        lane,
        value,
        meaning,
        hash,
        name,
    }
}

fn rows(document: &Value, catalog: &Catalog, view: View) -> Vec<Row> {
    let family = matches!(view, View::Investment);
    let key = if family { "family" } else { "unlocks" };
    document["_native_progression"][key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let bank = usize::try_from(entry[0].as_u64()?).ok()?;
            if bank >= if family { 2 } else { 8 } {
                return None;
            }
            let slot = usize::try_from(entry[1].as_u64()?).ok()?;
            let lane = if family { 0 } else { entry[2].as_i64()? };
            let value = entry[if family { 2 } else { 3 }].as_i64()?;
            Some(row(catalog, family, bank, slot, lane, value))
        })
        .collect()
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    document: &Value,
    catalog: &Catalog,
    view: View,
    query: &mut String,
) {
    let Some(native) = document.get("_native_progression") else {
        return;
    };
    egui::CollapsingHeader::new("Native SQLite State").id_salt("native_progression").show(ui, |ui| {
        let character = native["character_slot"].as_i64().unwrap_or(0) + 1;
        ui.label(format!("Shared account state and Character {character}. Values reflect the current workspace, including unsaved edits."));
        ui.label("All stored rows are visible here, including zero values and preserved raw bytes. Missing native rows read as zero. Only flag value 2 is set. Claimable rewards also depend on their objective conditions.");
        ui.add(egui::TextEdit::singleline(query).hint_text("Search Bank, Slot, Value, Name, Or Hash").desired_width(ui.available_width()));
        let all = rows(document, catalog, view);
        let search = query.trim().to_lowercase();
        let filtered = all.iter().filter(|row| search.is_empty() || format!("{} {} {} {} {} {} {}", row.bank, row.slot, row.lane, row.value, row.meaning, row.name, row.hash.map_or_else(String::new, |hash| format!("{hash} {hash:08x}"))).to_lowercase().contains(&search)).collect::<Vec<_>>();
        ui.label(format!("{} / {} Stored Rows", filtered.len(), all.len()));
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::both().id_salt("native_progression_rows").max_height(240.0).auto_shrink([false, false]).show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
            egui::Grid::new("native_progression_grid").striped(true).show(ui, |ui| {
                for index in range {
                    let row = filtered[index];
                    ui.label(row.bank);
                    ui.monospace(if matches!(view, View::Investment) {
                        format!("Definition #{}", row.slot)
                    } else {
                        format!("Slot {} · Lane {}", row.slot, row.lane)
                    });
                    ui.monospace(row.value.to_string());
                    ui.label(row.meaning);
                    draw_hash_hex_cell(ui, 110.0, row.hash);
                    ui.label(&row.name);
                    ui.end_row();
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_objectives_accept_values_that_legacy_json_reserves() {
        let mut document =
            json!({"state":{"unlocks":{"character_objective_values":[[443,7],[502,9]]}}});
        assert!(parse(&document).is_err());
        document["_native_progression"] = json!({});
        assert!(parse(&document).is_ok());
        assert!(super::super::mutations::set_unlock_value(
            &mut document,
            "character_object_objective_values",
            443,
            8
        ));
        assert!(super::super::mutations::remove_unlock_value(
            &mut document,
            "character_object_objective_values",
            502
        ));
        assert_eq!(
            parse(&document).unwrap().unlocks.character_objective_values,
            vec![IndexedValue {
                index: 443,
                value: 8
            }]
        );
    }

    #[test]
    fn hidden_native_rows_count_against_authoring_capacity() {
        let mut document = json!({"_native_progression":{"hidden_family_counts":[100,0]}});
        assert!(!super::super::mutations::set_investment_override(
            &mut document,
            InvestmentTable::FlagOverrides,
            10,
            2
        ));
        assert!(super::super::mutations::set_investment_override(
            &mut document,
            InvestmentTable::ValueOverrides,
            10,
            42
        ));
    }

    #[test]
    fn raw_native_overrides_remain_visible_to_state_inspection() {
        let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
            vec![UnlockDefinition {
                name: Some("Raw Flag".into()),
                ..Default::default()
            }],
            Vec::new(),
            Vec::new(),
        );
        let document =
            json!({"_native_progression":{"family":[[0,0,255]],"hidden_family_counts":[1,0]}});
        let snapshot = collection_state_snapshot(&document).unwrap();
        assert_eq!(
            snapshot.flag_text(0, catalog.unlock_flag_definition(0).unwrap()),
            "Override 255"
        );
        assert_eq!(snapshot.evaluated_flag(0, &catalog), None);
        let visible = rows(&document, &catalog, View::Investment);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "Raw Flag");
        assert_eq!(visible[0].value, 255);
    }

    #[test]
    fn expanded_readers_preserve_documents_in_both_themes_and_sizes() {
        let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
            vec![UnlockDefinition {
                hash: 0x1234,
                name: Some("Unsaved Definition".into()),
                ..Default::default()
            }],
            Vec::new(),
            Vec::new(),
        );
        let document = json!({"_native_progression":{"character_slot":0,"family":[[0,0,255]],"unlocks":[[0,3,0,1],[5,443,0,7]]},"state":{"unlocks":{"character_objective_values":[[443,7]]}}});
        let before = document.clone();
        for size in [egui::vec2(640.0, 480.0), egui::vec2(1280.0, 800.0)] {
            for dark in [true, false] {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut state = UiState::default();
                for view in [View::Unlocks, View::Investment] {
                    let output = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO,size)), ..Default::default() }, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            for id in ["native_progression", "all_unlock_definitions"] {
                                egui::collapsing_header::CollapsingState::load_with_default_open(ctx, ui.make_persistent_id(id), true).store(ctx);
                            }
                            draw(ui, &document, &catalog, view, &mut state.native_query);
                            super::super::browser::draw(ui, &document, &catalog, &mut state);
                        });
                    });
                    assert!(!output.shapes.is_empty());
                    assert_eq!(document, before);
                }
            }
        }
    }
}
