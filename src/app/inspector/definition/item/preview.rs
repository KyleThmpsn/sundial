//! Read-only appearance assembled from the inspected definition and its saved plugs.
use super::super::item_details::item_name;
use super::ItemInspection;
use crate::app::inspector::look;
use crate::app::item_editor::appearance::is_ornament;
use crate::{
    catalog::{ItemArtArrangement, ItemPackageMetadata, ItemRenderOverride},
    hash::parse_hash_hex,
    ui::model_preview::{self, Appearance},
};
use eframe::egui;
use serde_json::Value;

/// The inspected item's model: its saved or default plugs, the ornament among them and the art
/// rows of whichever geometry is shown, with the dyes that apply.
struct Resolved<'a> {
    saved: Option<&'a Value>,
    plugs: Vec<(u64, &'a ItemPackageMetadata)>,
    ornament: Option<(u64, &'a ItemPackageMetadata)>,
    rows: Vec<&'a ItemArtArrangement>,
    dyes: Vec<(i8, u16)>,
}

fn resolve<'a>(content: &ItemInspection<'a>) -> Option<Resolved<'a>> {
    let base = content.matches.item_package_metadata?;
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
            .map(|(hash, metadata)| (*hash, *metadata, is_ornament(content.catalog, *hash))),
    );
    let geometry = ornament.map_or(base, |(_, metadata)| metadata);
    let rows: Vec<_> = valid_art(geometry).collect();
    if rows.is_empty() {
        return None;
    }
    let colors = if geometry.translation_dye_rows.iter().all(Vec::is_empty) {
        base
    } else {
        geometry
    };
    let dyes = effective_dyes(
        &colors.translation_dye_rows,
        plugs.iter().map(|(_, metadata)| *metadata),
    );
    Some(Resolved {
        saved,
        plugs,
        ornament,
        rows,
        dyes,
    })
}

/// Whether the inspected item has a model to show.
pub(super) fn available(content: &ItemInspection<'_>) -> bool {
    resolve(content).is_some()
}

/// The model row shown, the default-class one unless the Preview page chose another.
fn variant_id(ui: &egui::Ui, hash: u64) -> egui::Id {
    egui::Id::new(("preview-art-variant", hash, ui.ctx().viewport_id()))
}

fn variant(ui: &egui::Ui, hash: u64, rows: &[&ItemArtArrangement]) -> usize {
    ui.data(|data| data.get_temp::<usize>(variant_id(ui, hash)))
        .filter(|&index| index < rows.len())
        .unwrap_or_else(|| {
            rows.iter()
                .position(|row| row.character_class == -1)
                .unwrap_or(0)
        })
}

/// The Overview's model beside its details, as Parhelion's editor shows it: turning on drag, with
/// a corner button that opens the full viewer. Returns `false` when the item has no model.
pub(super) fn inline(ui: &mut egui::Ui, content: &ItemInspection<'_>, size: egui::Vec2) -> bool {
    let Some(resolved) = resolve(content) else {
        return false;
    };
    let index = variant(ui, content.hash, &resolved.rows);
    let appearance = Appearance {
        arrangement: resolved.rows[index].arrangement,
        dye_textures: Vec::new(),
        dyes: resolved.dyes,
    };
    let packages = content.catalog.install_path().join("packages");
    let id = egui::Id::new(("inspector-model", content.hash));
    model_preview::still::placeholder(ui.ctx(), id, None);
    let response =
        model_preview::still::show(ui, id, &packages, Some(appearance.clone()), &[], size);
    model_preview::pop_out(
        ui,
        id,
        response.rect,
        &packages,
        (
            appearance,
            content.resolved_name.as_deref().unwrap_or("Model Preview"),
        ),
    );
    true
}

pub(super) fn draw(ui: &mut egui::Ui, content: &ItemInspection<'_>, details: bool, button: bool) {
    let Some(Resolved {
        saved,
        plugs,
        ornament,
        rows,
        dyes,
    }) = resolve(content)
    else {
        if button {
            model_preview::inspected_unavailable(ui);
        }
        if details {
            look::empty_state(ui, "No Model");
        }
        return;
    };
    if details {
        let catalog = content.catalog;
        look::properties(ui, ("preview-appearance", content.hash), |p| {
            p.text(
                "Appearance",
                if saved.is_some_and(Value::is_array) {
                    "Saved Plugs"
                } else {
                    "Catalog Defaults"
                },
            );
            if let Some((hash, _)) = ornament {
                p.link("Ornament", catalog, hash, item_name(catalog, hash));
            }
            for (hash, metadata) in &plugs {
                if !is_ornament(catalog, *hash)
                    && metadata
                        .translation_dye_rows
                        .iter()
                        .any(|rows| !rows.is_empty())
                {
                    p.link("Colors", catalog, *hash, item_name(catalog, *hash));
                }
            }
        });
    }
    let mut index = variant(ui, content.hash, &rows);
    if details && rows.len() > 1 {
        look::properties(ui, ("preview-model-variant", content.hash), |p| {
            p.custom("Model", |ui| {
                egui::ComboBox::from_id_salt("preview-model-variant")
                    .selected_text(variant_label(rows[index]))
                    .show_ui(ui, |ui| {
                        for (i, row) in rows.iter().enumerate() {
                            ui.selectable_value(&mut index, i, variant_label(row));
                        }
                    });
            });
        });
        let variant_id = variant_id(ui, content.hash);
        ui.data_mut(|data| data.insert_temp(variant_id, index));
    }
    if button {
        model_preview::inspected_weapon(
            ui,
            &content.catalog.install_path().join("packages"),
            Appearance {
                arrangement: rows[index].arrangement,
                dye_textures: Vec::new(),
                dyes,
            },
            content.resolved_name.as_deref().unwrap_or("Model Preview"),
            content.catalog.inspection_access(),
        );
    }
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
    let rows = |stages: &[Vec<ItemRenderOverride>; 3]| {
        std::array::from_fn(|stage| {
            stages[stage]
                .iter()
                .map(|row| (row.key, row.value))
                .collect()
        })
    };
    crate::dyes::compose(
        &rows(base),
        plugs.map(|plug| rows(&plug.translation_dye_rows)),
    )
}
