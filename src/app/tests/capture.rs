use eframe::egui;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    /// Whole images that earlier frames uploaded, such as package icons, so captures can draw them.
    static IMAGES: RefCell<HashMap<egui::TextureId, Arc<egui::ColorImage>>> = RefCell::default();
}

/// Keeps the images a frame uploaded. Each frame reports only the textures it changed, so a
/// test whose captures show icons calls this after every frame it runs.
pub(in crate::app) fn record(output: &egui::FullOutput) {
    IMAGES.with_borrow_mut(|images| {
        for (id, delta) in &output.textures_delta.set {
            if let (egui::ImageData::Color(image), None) = (&delta.image, delta.pos) {
                images.insert(*id, Arc::clone(image));
            }
        }
        for id in &output.textures_delta.free {
            images.remove(id);
        }
    });
}

pub(in crate::app) fn write(ctx: &egui::Context, output: &egui::FullOutput, name: &str) {
    let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let atlas = ctx.fonts(|fonts| fonts.image());
    let pixels = atlas
        .srgba_pixels(None)
        .flat_map(|pixel| pixel.to_array())
        .collect::<Vec<_>>();
    std::fs::write(directory.join(format!("{name}-atlas.rgba")), pixels).unwrap();
    // A mesh samples the font atlas, a recorded image by its place in `textures`, or nothing
    // when its image was uploaded before the test recorded frames.
    let mut textures = Vec::new();
    let meshes = ctx.tessellate(output.shapes.clone(), output.pixels_per_point).into_iter().filter_map(|primitive| {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { return None };
        let clip = primitive.clip_rect;
        let texture = texture_ref(&mut textures, mesh.texture_id);
        Some(serde_json::json!({"clip":[clip.min.x,clip.min.y,clip.max.x,clip.max.y],"texture":texture,"indices":mesh.indices,
            "vertices":mesh.vertices.iter().map(|v| serde_json::json!([v.pos.x,v.pos.y,v.uv.x,v.uv.y,v.color.to_array()])).collect::<Vec<_>>()}))
    }).collect::<Vec<_>>();
    let sizes = IMAGES.with_borrow(|images| {
        textures
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let image = &images[id];
                let pixels = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect::<Vec<_>>();
                std::fs::write(
                    directory.join(format!("{name}-texture-{index}.rgba")),
                    pixels,
                )
                .unwrap();
                image.size
            })
            .collect::<Vec<_>>()
    });
    let size = ctx.input(|input| input.screen_rect().size());
    std::fs::write(directory.join(format!("{name}.json")), serde_json::to_vec(&serde_json::json!({"width":size.x,"height":size.y,"atlas_size":atlas.size,"textures":sizes,"meshes":meshes})).unwrap()).unwrap();
}

fn texture_ref(textures: &mut Vec<egui::TextureId>, id: egui::TextureId) -> serde_json::Value {
    if id == egui::TextureId::default() {
        return "atlas".into();
    }
    if !IMAGES.with_borrow(|images| images.contains_key(&id)) {
        return "unrecorded".into();
    }
    let index = textures
        .iter()
        .position(|known| *known == id)
        .unwrap_or_else(|| {
            textures.push(id);
            textures.len() - 1
        });
    index.into()
}
