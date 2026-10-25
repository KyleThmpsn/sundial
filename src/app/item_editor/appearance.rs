//! Composes cosmetic candidates without changing the item being edited.
use super::*;
use crate::dyes::{compose as compose_dyes, gear_type};
use crate::{
    catalog::ItemPackageMetadata,
    ui::model_preview::{self, Appearance, Loadout},
};

pub(crate) fn loadout(catalog: &Catalog, hash: u64) -> Option<Loadout> {
    catalog.item_package_metadata(hash)?.weapon_inventory_slot?;
    item_loadout(catalog, hash)
}

fn item_loadout(catalog: &Catalog, hash: u64) -> Option<Loadout> {
    let metadata = catalog.item_package_metadata(hash)?;
    Some(Loadout {
        arrangement: arrangement(metadata)?,
        dyes: dye_rows(metadata),
        plugs: catalog
            .item(hash)
            .map_or(&[][..], |item| item.default_plugs.as_slice())
            .iter()
            .map(|hash| {
                hash.as_deref()
                    .and_then(crate::hash::parse_hash_hex)
                    .and_then(|hash| u32::try_from(hash).ok())
            })
            .collect(),
    })
}

/// Resolve an account item's opening-time appearance without changing its saved plugs.
pub(crate) fn saved(
    catalog: &Catalog,
    hash: u64,
    plugs: Option<&serde_json::Value>,
) -> Option<Appearance> {
    let mut loadout = item_loadout(catalog, hash)?;
    match plugs {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Array(plugs)) => {
            loadout.plugs = plugs
                .iter()
                .map(|plug| {
                    plug.as_u64()
                        .or_else(|| plug.as_str().and_then(crate::hash::parse_hash_hex))
                        .and_then(|hash| u32::try_from(hash).ok())
                        .filter(|&hash| hash != 0 && hash != u32::MAX)
                })
                .collect();
        }
        Some(_) => loadout.plugs.clear(),
    }
    Some(resolve(catalog, &loadout))
}

fn arrangement(metadata: &ItemPackageMetadata) -> Option<u16> {
    metadata
        .art_arrangements
        .iter()
        .filter(|row| row.arrangement != u16::MAX)
        .min_by_key(|row| row.character_class != -1)
        .map(|row| row.arrangement)
}

fn dye_rows(metadata: &ItemPackageMetadata) -> [Vec<(i8, u16)>; 3] {
    std::array::from_fn(|stage| {
        metadata.translation_dye_rows[stage]
            .iter()
            .map(|row| (row.key, row.value))
            .collect()
    })
}

pub(crate) fn resolve(catalog: &Catalog, loadout: &Loadout) -> Appearance {
    let mut arrangement = loadout.arrangement;
    let mut base = loadout.dyes.clone();
    for hash in loadout.plugs.iter().flatten() {
        if is_ornament(catalog, u64::from(*hash))
            && let Some(metadata) = catalog.item_package_metadata(u64::from(*hash))
            && let Some(model) = self::arrangement(metadata)
        {
            arrangement = model;
            if metadata
                .translation_dye_rows
                .iter()
                .any(|rows| !rows.is_empty())
            {
                base = dye_rows(metadata);
            }
        }
    }
    let plugs = loadout
        .plugs
        .iter()
        .flatten()
        .filter_map(|hash| catalog.item_package_metadata(u64::from(*hash)))
        .map(dye_rows);
    Appearance {
        arrangement,
        dye_textures: Vec::new(),
        dyes: compose_dyes(&base, plugs),
    }
}

/// An item wearing a shader, for previewing the shader on it. The item's own dyes name its gear
/// type, and only the shader's dyes for that gear type apply, as in the game.
pub(crate) fn with_shader(
    catalog: &Catalog,
    hash: u64,
    shader: &[Vec<(i8, u16)>; 3],
) -> Option<Appearance> {
    let metadata = catalog.item_package_metadata(hash)?;
    let base = dye_rows(metadata);
    let gear_types: Vec<_> = base
        .iter()
        .flatten()
        .filter_map(|&(key, _)| gear_type(key))
        .collect();
    let shader = shader.clone().map(|rows| {
        rows.into_iter()
            .filter(|&(key, _)| gear_type(key).is_some_and(|first| gear_types.contains(&first)))
            .collect()
    });
    Some(Appearance {
        arrangement: arrangement(metadata)?,
        dye_textures: Vec::new(),
        dyes: compose_dyes(&base, std::iter::once(shader)),
    })
}

fn visual_plug(catalog: &Catalog, hash: u64) -> bool {
    is_ornament(catalog, hash)
        || catalog.item_package_metadata(hash).is_some_and(|metadata| {
            metadata
                .translation_dye_rows
                .iter()
                .any(|rows| !rows.is_empty())
        })
}

pub(in crate::app) fn is_ornament(catalog: &Catalog, hash: u64) -> bool {
    catalog
        .plug_type_name(hash)
        .or_else(|| catalog.package_item_type_name(hash))
        .is_some_and(|name| {
            let name = name.trim().to_ascii_lowercase();
            name == "ornament" || name.ends_with(" ornament")
        })
}

pub(super) fn supported(catalog: &Catalog, snapshot: &PlugPickerSnapshot) -> bool {
    let label = snapshot.socket_label.to_lowercase();
    snapshot.preview.is_some()
        && (label.contains("shader")
            || label.contains("ornament")
            || snapshot
                .current_hash
                .is_some_and(|hash| visual_plug(catalog, hash))
            || snapshot
                .native_default
                .and_then(NativePlugDefault::value)
                .is_some_and(|hash| visual_plug(catalog, hash))
            || (!snapshot.choices.is_empty()
                && snapshot
                    .choices
                    .iter()
                    .all(|choice| visual_plug(catalog, choice.hash))))
}

pub(super) fn browser(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    opened: bool,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    footer: impl FnOnce(&mut egui::Ui) -> bool,
) -> Option<ItemEditorAction> {
    let loadout = snapshot.preview.as_ref()?;
    // A scope change keeps the chooser open, so it is returned after the window rather than
    // picked from it.
    let mut scope = None;
    let picked = model_preview::chooser::show(
        ui,
        id,
        &format!("Choose {}", snapshot.socket_label),
        opened,
        |ui, opened| {
            if footer(ui) {
                return Some(None);
            }
            if let Some(default) = snapshot
                .native_default
                .filter(|default| default.value() != snapshot.current_hash)
                && ui.button("Reset to Default").clicked()
            {
                return Some(Some(ItemEditorAction::SetPlug {
                    socket_index: snapshot.socket_index,
                    hash: default.value(),
                }));
            }
            scope = super::plug_picker::draw_plug_scope_selector(ui, snapshot);
            let response = ui.add(
                egui::TextEdit::singleline(query)
                    .hint_text("Search Shaders or Ornaments")
                    .desired_width(ui.available_width()),
            );
            if opened {
                response.request_focus();
            }
            let search = CatalogSearchQuery::new(query);
            let mut choices: Vec<_> = snapshot
                .choices
                .iter()
                .filter(|choice| {
                    search.matches(catalog, choice.hash, &[&choice.label, &choice.type_name])
                })
                .collect();
            choices.sort_by_key(|choice| choice.label.to_lowercase());
            let mut keys: Vec<_> = choices.iter().map(|choice| choice.hash).collect();
            if query.trim().is_empty() {
                if let Some(hash) = snapshot.current_hash.filter(|hash| !keys.contains(hash)) {
                    keys.insert(0, hash);
                }
                keys.insert(0, 0);
            }
            if opened {
                ui.data_mut(|data| {
                    data.insert_temp(
                        ui.make_persistent_id("inspected-choice"),
                        snapshot.current_hash.unwrap_or(0),
                    )
                });
            }
            crate::ui::catalog::BrowserList {
                keys: &keys,
                height: (ui.available_height() - 30.0).max(180.0),
                reset: response.changed(),
                row_height: 48.0,
                select: None,
            }
            .draw_with_actions(
                ui,
                |ui, index, selected| {
                    let hash = keys[index];
                    let name = if hash == 0 {
                        "None".to_owned()
                    } else {
                        catalog.plug_label(hash, false)
                    };
                    draw_catalog_picker_row(
                        ui,
                        catalog,
                        CatalogPickerRow {
                            hash,
                            primary: &name,
                            primary_max_rows: 1,
                            secondary: None,
                            icon_size: 28.0,
                            row_height: 48.0,
                            selected,
                        },
                    )
                },
                |ui, index| {
                    let hash = keys[index];
                    let mut candidate = loadout.clone();
                    candidate
                        .plugs
                        .resize(candidate.plugs.len().max(snapshot.socket_index + 1), None);
                    candidate.plugs[snapshot.socket_index] =
                        u32::try_from(hash).ok().filter(|hash| *hash != 0);
                    let name = if hash == 0 {
                        "No Plug".to_owned()
                    } else {
                        catalog.plug_label(hash, false)
                    };
                    let chosen =
                        ui.button("Apply Choice")
                            .clicked()
                            .then_some(ItemEditorAction::SetPlug {
                                socket_index: snapshot.socket_index,
                                hash: (hash != 0).then_some(hash),
                            });
                    let access = snapshot
                        .preview_guarded
                        .then(|| catalog.inspection_access());
                    model_preview::chooser::preview_with_access(
                        ui,
                        &catalog.install_path().join("packages"),
                        resolve(catalog, &candidate),
                        &name,
                        access,
                    );
                    chosen.map(Some)
                },
            )
        },
    )
    .flatten();
    scope
        .map(|mode| ItemEditorAction::SetPlugSelectionMode { mode })
        .or(picked)
}
