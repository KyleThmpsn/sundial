//! Preview the same model and color sources used by the recipe compiler.
use super::*;
#[cfg(test)]
use sundial::investment::WeaponDyeReference;
use sundial::ui::model_preview::{self, Loadout};

pub(in crate::app) fn loadout(
    catalog: &InvestmentCatalog,
    recipe: &WeaponRecipe,
) -> Option<Loadout> {
    let donor_hash = recipe.donor.item_hash.parse_u32().ok()?;
    let geometry_hash = recipe
        .presentation_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
        .unwrap_or(donor_hash);
    let color_hash = recipe
        .render_gear_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
        .unwrap_or(geometry_hash);
    let geometry = catalog.preview_loadout(geometry_hash)?;
    let colors = catalog.preview_loadout(color_hash)?;
    let donor = catalog.preview_loadout(donor_hash)?;
    let arrangement = match &recipe.overrides.art_arrangements {
        Some(rows) => {
            rows.iter()
                .filter(|row| row.arrangement != u16::MAX)
                .min_by_key(|row| row.character_class != -1)?
                .arrangement
        }
        None => geometry.arrangement,
    };
    let dyes = recipe
        .overrides
        .render_dye_rows
        .as_ref()
        .map(|rows| {
            std::array::from_fn(|stage| {
                rows[stage]
                    .iter()
                    .map(|row| (row.channel_index, row.dye_reference_index))
                    .collect()
            })
        })
        .unwrap_or(colors.dyes);
    let plugs = (0..donor.plugs.len().max(recipe.overrides.socket_columns.len()))
        .map(|index| {
            match recipe
                .overrides
                .socket_columns
                .get(index)
                .and_then(Option::as_ref)
            {
                Some(column) if column.socket_type == Some(u16::MAX) => None,
                Some(column) => column
                    .choices
                    .first()
                    .and_then(|hash| hash.parse_u32().ok()),
                None => donor.plugs.get(index).copied().flatten(),
            }
        })
        .collect();
    Some(Loadout {
        arrangement,
        dyes,
        plugs,
    })
}

impl PackageAuthoringApp {
    pub(in crate::app) fn follow_appearance_preview(&self, ctx: &egui::Context) {
        if !model_preview::weapon_is_open(ctx) {
            return;
        }
        let Some(catalog) = &self.catalog else {
            return;
        };
        let Some(loadout) = loadout(catalog, &self.recipe) else {
            return;
        };
        let geometry = self.current_geometry_donor();
        model_preview::follow_weapon(
            ctx,
            &self.packages,
            catalog.preview_appearance(&loadout),
            geometry
                .as_ref()
                .map_or("Current Appearance", |g| g.summary.name.as_str()),
        );
    }

    pub(in crate::app) fn draw_appearance_preview(&self, ui: &mut egui::Ui) {
        let Some(catalog) = &self.catalog else {
            return;
        };
        let Some(loadout) = loadout(catalog, &self.recipe) else {
            ui.label("This appearance has no model row.");
            return;
        };
        #[cfg(feature = "d2-model-importer")]
        let imported = self.recipe.overrides.imported_graph.is_some();
        #[cfg(not(feature = "d2-model-importer"))]
        let imported = false;
        ui.heading(if imported {
            "Native Donor Preview"
        } else {
            "Weapon Preview"
        });
        if imported {
            ui.weak("This shows the conversion donor. The imported model is applied when packages are built.");
        }
        let plug_names: Vec<_> = loadout
            .plugs
            .iter()
            .flatten()
            .filter(|hash| {
                catalog
                    .item_render_dye_rows(**hash)
                    .iter()
                    .any(|rows| !rows.is_empty())
            })
            .map(|hash| catalog.plug_label(*hash, false))
            .collect();
        if !plug_names.is_empty() {
            ui.label(format!("Socket Colors: {}", plug_names.join(", ")));
        }
        let geometry = self.current_geometry_donor();
        model_preview::weapon(
            ui,
            &self.packages,
            catalog.preview_appearance(&loadout),
            geometry.as_ref().map_or("Current Appearance", |geometry| {
                geometry.summary.name.as_str()
            }),
        );
    }
}

#[cfg(test)]
fn effective_dyes<'a>(
    rows: &[Vec<WeaponDyeReference>; 3],
    plugs: impl Iterator<Item = &'a [Vec<WeaponDyeReference>; 3]>,
) -> Vec<(i8, u16)> {
    let mut result = BTreeMap::new();
    let mut insert = |rows: &[WeaponDyeReference]| {
        for row in rows {
            if row.channel_index >= 0 && row.dye_reference_index != u16::MAX {
                result.insert(row.channel_index, row.dye_reference_index);
            }
        }
    };
    insert(&rows[1]);
    insert(&rows[0]);
    for plug in plugs {
        insert(&plug[1]);
        insert(&plug[0]);
        insert(&plug[2]);
    }
    insert(&rows[2]);
    result.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_replaces_defaults_but_keeps_locked_channels() {
        let row = |channel_index, dye_reference_index| WeaponDyeReference {
            channel_index,
            dye_reference_index,
        };
        let base = [
            vec![row(4, 11)],
            vec![row(4, 10), row(5, 20), row(6, 30)],
            vec![row(6, 31)],
        ];
        let shader = [
            vec![row(4, 40), row(5, 50), row(6, 60), row(-1, 90)],
            vec![],
            vec![],
        ];
        assert_eq!(
            effective_dyes(&base, [&shader].into_iter()),
            vec![(4, 40), (5, 50), (6, 31)]
        );
        assert_eq!(
            effective_dyes(&base, std::iter::empty()),
            vec![(4, 11), (5, 20), (6, 31)]
        );
    }
}
