//! Content-keyed presentation shared by saved perks and attached socket choices.
use crate::perk::Icon;
use sundial::investment::{IconOverride, InvestmentCatalog};

#[derive(Clone, Default)]
struct Images(Vec<(egui::Id, egui::TextureHandle)>);

pub(crate) fn icon(
    ui: &egui::Ui,
    catalog: &InvestmentCatalog,
    icon: Option<&Icon>,
) -> Option<IconOverride> {
    let icon = icon?;
    let visible =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 48.0));
    if !ui.is_rect_visible(visible) {
        return Some(IconOverride::Pending);
    }
    Some(match icon {
        Icon::Texture { tag } => tag
            .parse_u32()
            .ok()
            .and_then(|tag| catalog.texture_icon(ui.ctx(), tag))
            .map_or(IconOverride::Pending, |texture| {
                IconOverride::Texture(texture.id())
            }),
        Icon::Image { image, .. } => IconOverride::Texture(image_texture(ui.ctx(), image).id()),
    })
}

fn image_texture(
    ctx: &egui::Context,
    image: &crate::icon_edit::ImportedIcon,
) -> egui::TextureHandle {
    let key = egui::Id::new(serde_json::to_vec(image).expect("Validated icon serializes"));
    let cache_id = egui::Id::new("private-perk-image-previews");
    let cached = ctx.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Images>(cache_id);
        let index = cache.0.iter().position(|(id, _)| *id == key)?;
        let entry = cache.0.remove(index);
        let texture = entry.1.clone();
        cache.0.push(entry);
        Some(texture)
    });
    cached.unwrap_or_else(|| {
        let pixels = crate::icon_edit::glyph::fit(&image.fit_to(96, 96));
        let texture = ctx.load_texture(
            "private-perk-image",
            egui::ColorImage::from_rgba_unmultiplied([96, 96], pixels.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        ctx.data_mut(|data| {
            let cache = data.get_temp_mut_or_default::<Images>(cache_id);
            if cache.0.len() == 128 {
                drop(cache.0.remove(0));
            }
            cache.0.push((key, texture.clone()));
        });
        texture
    })
}

#[cfg(test)]
mod tests;
