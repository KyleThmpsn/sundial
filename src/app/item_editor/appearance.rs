//! Composes cosmetic candidates without changing the item being edited.
use super::*;
use crate::{
    catalog::ItemPackageMetadata,
    ui::model_preview::{self, Appearance, Loadout},
};
use std::collections::BTreeMap;

pub(crate) fn loadout(catalog: &Catalog, hash: u64) -> Option<Loadout> {
    let metadata = catalog.item_package_metadata(hash)?;
    metadata.weapon_inventory_slot?;
    Some(Loadout {
        arrangement: arrangement(metadata)?,
        dyes: dye_rows(metadata),
        plugs: catalog
            .item(hash)?
            .default_plugs
            .iter()
            .map(|hash| {
                hash.as_deref()
                    .and_then(crate::hash::parse_hash_hex)
                    .and_then(|hash| u32::try_from(hash).ok())
            })
            .collect(),
    })
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
        if catalog.is_weapon_ornament(u64::from(*hash))
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
        dyes: compose_dyes(&base, plugs),
    }
}

fn compose_dyes(
    base: &[Vec<(i8, u16)>; 3],
    plugs: impl Iterator<Item = [Vec<(i8, u16)>; 3]>,
) -> Vec<(i8, u16)> {
    let mut dyes = BTreeMap::new();
    let mut insert = |rows: &[(i8, u16)]| {
        for &(key, value) in rows {
            if key >= 0 && value != u16::MAX {
                dyes.insert(key, value);
            }
        }
    };
    insert(&base[1]);
    insert(&base[0]);
    for rows in plugs {
        for stage in [1, 0, 2] {
            insert(&rows[stage]);
        }
    }
    insert(&base[2]);
    dyes.into_iter().collect()
}

fn visual_plug(catalog: &Catalog, hash: u64) -> bool {
    catalog.is_weapon_ornament(hash)
        || catalog.item_package_metadata(hash).is_some_and(|metadata| {
            metadata
                .translation_dye_rows
                .iter()
                .any(|rows| !rows.is_empty())
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
    model_preview::chooser::show(
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
                && ui.button("Reset to Native Default").clicked()
            {
                return Some(Some(ItemEditorAction::SetPlug {
                    socket_index: snapshot.socket_index,
                    hash: default.value(),
                }));
            }
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
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ItemRenderOverride;

    #[test]
    fn shader_preview_replaces_one_socket_without_mutating_other_plugs_or_locked_dyes() {
        let shader = |value| ItemPackageMetadata {
            translation_dye_rows: [
                vec![
                    ItemRenderOverride {
                        stage: 0,
                        key: 4,
                        value,
                    },
                    ItemRenderOverride {
                        stage: 0,
                        key: 6,
                        value,
                    },
                ],
                vec![],
                vec![],
            ],
            ..Default::default()
        };
        let catalog = Catalog::for_test(vec![], [(10, shader(100)), (20, shader(200))].into());
        let current = Loadout {
            arrangement: 42,
            dyes: [vec![], vec![(4, 1), (5, 2)], vec![(6, 3)]],
            plugs: vec![Some(999), Some(10)],
        };
        let mut candidate = current.clone();
        candidate.plugs[1] = Some(20);
        assert_eq!(
            resolve(&catalog, &candidate),
            Appearance {
                arrangement: 42,
                dyes: vec![(4, 200), (5, 2), (6, 3)]
            }
        );
        candidate.plugs[1] = None;
        assert_eq!(
            resolve(&catalog, &candidate).dyes,
            vec![(4, 1), (5, 2), (6, 3)]
        );
        assert_eq!(current.plugs, vec![Some(999), Some(10)]);
        assert_eq!(
            resolve(&catalog, &current).dyes,
            vec![(4, 100), (5, 2), (6, 3)]
        );
    }
}
