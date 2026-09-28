//! Icon From Dyes: the shader's icon drawn in the stock style from its weapon dyes, with the page's
//! unbuilt edits, redrawn only when what it shows changes.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::*;
use crate::icon_edit::ImportedIcon;
use crate::shader_icon::{IconSurface, TRIANGLES, classic};

/// The dye and gear type of each channel's weapon row, whose dyes the stock icons show, or of the
/// armor row when the shader has no weapon row.
pub(super) fn icon_dyes(rows: &DyeRows) -> Option<[(u16, GearType); 3]> {
    let mut dyes = [(0, GearType::Weapon); 3];
    for (dye, channel) in dyes.iter_mut().zip(DyeChannel::ALL) {
        *dye = [GearType::Weapon, GearType::Armor]
            .into_iter()
            .find_map(|gear| Some((dye_for(rows, gear.key(channel))?, gear)))?;
    }
    Some(dyes)
}

impl PackageAuthoringApp {
    /// Keeps the icon's imported image drawn from the dyes as the page shows them. Waits until
    /// every dye, texture and iridescence ramp it uses has loaded. The page loads the dyes.
    pub(super) fn draw_icon_from_dyes(&mut self, ctx: &egui::Context, rows: &DyeRows) {
        let Some(dyes) = icon_dyes(rows) else {
            return;
        };
        let edits = &self.recipe.overrides.dye_edits;
        let texture_edits = &self.recipe.overrides.dye_texture_edits;
        let materials = &self.dye_materials;
        let mut key = DefaultHasher::new();
        dyes.hash(&mut key);
        let mut drawings = Vec::with_capacity(TRIANGLES.len());
        for (channel, variant) in TRIANGLES {
            let (dye, gear) = dyes[channel];
            let (channel, surface) = (DyeChannel::ALL[channel], DyeSurface::ALL[variant]);
            let edit = surface_edit(edits, gear, channel, surface);
            let textures = texture_edit(texture_edits, gear, channel);
            let Some(drawing) =
                materials.drawing(dye, (edit, textures), surface, &self.iridescence)
            else {
                return;
            };
            (edit, textures, drawing.ramp.is_some()).hash(&mut key);
            drawings.push(drawing);
        }
        let key = key.finish();
        let icon = &mut self.recipe.overrides.icon_edit.imported_image;
        if let Some((drawn, image)) = &self.dye_materials.drawn
            && *drawn == key
        {
            if icon.as_ref() != Some(image) {
                *icon = Some(image.clone());
                ctx.request_repaint();
            }
            return;
        }
        let surfaces: Vec<IconSurface<'_>> = drawings
            .iter()
            .map(|drawing| drawing.surface(materials))
            .collect();
        let Ok(surfaces) = <[IconSurface<'_>; 4]>::try_from(surfaces) else {
            return;
        };
        let Ok(image) = ImportedIcon::from_drawn(classic(&surfaces)) else {
            return;
        };
        *icon = Some(image.clone());
        self.dye_materials.drawn = Some((key, image));
        // The icon picker above the dyes drew this frame with the old icon.
        ctx.request_repaint();
    }
}
