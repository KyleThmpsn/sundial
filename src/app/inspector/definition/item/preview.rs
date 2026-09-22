//! Read-only appearance assembled from the inspected definition and its saved plugs.
use super::ItemInspection;
use crate::{
    catalog::{ItemArtArrangement, ItemPackageMetadata, ItemRenderOverride},
    hash::parse_hash_hex,
    ui::model_preview::{self, Appearance},
};
use eframe::egui;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn draw(ui: &mut egui::Ui, content: &ItemInspection<'_>, details: bool) {
    let Some(base) = content.matches.item_package_metadata else {
        model_preview::inspected_unavailable(ui);
        if details {
            ui.label("No model is available for this definition.");
        }
        return;
    };
    let saved = content
        .source_context
        .and_then(|context| context.plugs.as_ref());
    let defaults = content
        .matches
        .item
        .map_or(&[][..], |item| item.default_plugs.as_slice());
    let plugs: Vec<_> = plug_hashes(saved, defaults)
        .into_iter()
        .filter_map(|hash| {
            content
                .catalog
                .item_package_metadata(hash)
                .map(|metadata| (hash, metadata))
        })
        .collect();
    let ornament = selected_ornament(
        plugs
            .iter()
            .map(|(hash, metadata)| (*hash, *metadata, content.catalog.is_weapon_ornament(*hash))),
    );
    let geometry = ornament.map_or(base, |(_, metadata)| metadata);
    let rows: Vec<_> = valid_art(geometry).collect();
    if rows.is_empty() {
        model_preview::inspected_unavailable(ui);
        if details {
            ui.label("No model is available for this definition.");
        }
        return;
    }
    if details {
        ui.label(if saved.is_some_and(Value::is_array) {
            "Saved Appearance"
        } else {
            "Catalog Appearance"
        });
        if saved.is_some_and(Value::is_array) {
            ui.weak("Uses the plugs captured when this inspector was opened.");
        }
        if let Some((hash, _)) = ornament {
            ui.label(format!(
                "Ornament: {}",
                content.catalog.package_item_name(hash).unwrap_or("Unnamed")
            ));
        }
        for (hash, metadata) in &plugs {
            if !content.catalog.is_weapon_ornament(*hash)
                && metadata
                    .translation_dye_rows
                    .iter()
                    .any(|rows| !rows.is_empty())
            {
                ui.label(format!(
                    "Colors: {}",
                    content
                        .catalog
                        .package_item_name(*hash)
                        .unwrap_or("Unnamed")
                ));
            }
        }
        if base.weapon_inventory_slot.is_none() && !content.catalog.is_weapon_ornament(content.hash)
        {
            ui.weak("Non-weapon models use base textures. Shader colors are currently supported for weapons.");
        }
    }
    let colors = if geometry.translation_dye_rows.iter().all(Vec::is_empty) {
        base
    } else {
        geometry
    };
    let variant_id = ui.id().with(("preview-art-variant", content.hash));
    let mut index = ui
        .data(|data| data.get_temp::<usize>(variant_id))
        .filter(|&index| index < rows.len())
        .unwrap_or_else(|| {
            rows.iter()
                .position(|row| row.character_class == -1)
                .unwrap_or(0)
        });
    if details && rows.len() > 1 {
        egui::ComboBox::from_id_salt("preview-model-variant")
            .selected_text(variant_label(rows[index]))
            .show_ui(ui, |ui| {
                for (i, row) in rows.iter().enumerate() {
                    ui.selectable_value(&mut index, i, variant_label(row));
                }
            });
        ui.data_mut(|data| data.insert_temp(variant_id, index));
    }
    let dyes = effective_dyes(
        &colors.translation_dye_rows,
        plugs.iter().map(|(_, metadata)| *metadata),
    );
    model_preview::inspected_weapon(
        ui,
        &content.catalog.install_path().join("packages"),
        Appearance {
            arrangement: rows[index].arrangement,
            dyes,
        },
        content.resolved_name.as_deref().unwrap_or("Model Preview"),
        content.catalog.inspection_access(),
    );
}

fn valid_art(metadata: &ItemPackageMetadata) -> impl Iterator<Item = &ItemArtArrangement> {
    metadata
        .art_arrangements
        .iter()
        .filter(|row| row.arrangement != u16::MAX)
}

fn selected_ornament<'a>(
    plugs: impl Iterator<Item = (u64, &'a ItemPackageMetadata, bool)>,
) -> Option<(u64, &'a ItemPackageMetadata)> {
    plugs
        .filter(|(_, metadata, ornament)| *ornament && valid_art(metadata).next().is_some())
        .map(|(hash, metadata, _)| (hash, metadata))
        .last()
}

fn variant_label(row: &ItemArtArrangement) -> String {
    let class = match row.character_class {
        -1 => "Default",
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Other",
    };
    format!("{class} · Model {}", row.arrangement)
}

fn plug_hashes(saved: Option<&Value>, defaults: &[Option<String>]) -> Vec<u64> {
    let valid = |hash: &u64| *hash > 0 && *hash <= u64::from(u32::MAX);
    match saved {
        None | Some(Value::Null) => defaults
            .iter()
            .flatten()
            .filter_map(|hash| parse_hash_hex(hash))
            .filter(valid)
            .collect(),
        Some(Value::Array(plugs)) => plugs
            .iter()
            .filter_map(|plug| {
                plug.as_u64()
                    .or_else(|| plug.as_str().and_then(parse_hash_hex))
            })
            .filter(valid)
            .collect(),
        _ => Vec::new(),
    }
}

fn effective_dyes<'a>(
    base: &[Vec<ItemRenderOverride>; 3],
    plugs: impl Iterator<Item = &'a ItemPackageMetadata>,
) -> Vec<(i8, u16)> {
    let mut result = BTreeMap::new();
    let mut insert = |rows: &[ItemRenderOverride]| {
        for row in rows {
            if row.key >= 0 && row.value != u16::MAX {
                result.insert(row.key, row.value);
            }
        }
    };
    insert(&base[1]);
    insert(&base[0]);
    for plug in plugs {
        for stage in [1, 0, 2] {
            insert(&plug.translation_dye_rows[stage]);
        }
    }
    insert(&base[2]);
    result.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ornament_with_a_usable_model_replaces_the_base_geometry() {
        let valid = ItemPackageMetadata {
            art_arrangements: vec![ItemArtArrangement {
                character_class: -1,
                arrangement: 42,
            }],
            ..Default::default()
        };
        let disabled = ItemPackageMetadata {
            art_arrangements: vec![ItemArtArrangement {
                character_class: -1,
                arrangement: u16::MAX,
            }],
            ..Default::default()
        };
        let rows = [(1, &valid, false), (2, &valid, true), (3, &disabled, true)];
        let chosen = selected_ornament(rows.into_iter()).unwrap();
        assert_eq!(
            (chosen.0, chosen.1.art_arrangements[0].arrangement),
            (2, 42)
        );
        assert!(
            selected_ornament([(1, &valid, false), (2, &disabled, true)].into_iter()).is_none()
        );
    }
    #[test]
    fn saved_plugs_keep_explicit_empty_and_missing_slots_separate_from_defaults() {
        let defaults = vec![Some("0x0000000A".into()), Some("0x0000000B".into())];
        assert_eq!(plug_hashes(None, &defaults), vec![10, 11]);
        assert_eq!(plug_hashes(Some(&Value::Null), &defaults), vec![10, 11]);
        assert!(plug_hashes(Some(&serde_json::json!([])), &defaults).is_empty());
        assert_eq!(
            plug_hashes(
                Some(&serde_json::json!([
                    null,
                    "0x0000000C",
                    13,
                    0,
                    -1,
                    4294967296_u64
                ])),
                &defaults
            ),
            vec![12, 13]
        );
        assert!(plug_hashes(Some(&serde_json::json!("invalid")), &defaults).is_empty());
    }

    #[test]
    fn shaders_override_custom_and_default_dyes_but_preserve_locked_colors() {
        let row = |key, value| ItemRenderOverride {
            stage: 0,
            key,
            value,
        };
        let base = [
            vec![row(4, 11)],
            vec![row(4, 10), row(5, 20), row(6, 30)],
            vec![row(6, 31)],
        ];
        let plug = ItemPackageMetadata {
            translation_dye_rows: [
                vec![
                    row(4, 40),
                    row(5, 50),
                    row(6, 60),
                    row(-1, 99),
                    row(7, u16::MAX),
                ],
                vec![],
                vec![],
            ],
            ..Default::default()
        };
        assert_eq!(
            effective_dyes(&base, [&plug].into_iter()),
            vec![(4, 40), (5, 50), (6, 31)]
        );
        assert_eq!(
            effective_dyes(&base, std::iter::empty()),
            vec![(4, 11), (5, 20), (6, 31)]
        );
    }
}
