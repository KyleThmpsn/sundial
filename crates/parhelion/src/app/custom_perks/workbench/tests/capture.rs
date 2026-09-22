pub(crate) fn write(ctx: &egui::Context, output: &egui::FullOutput, name: &str) {
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
    let meshes = ctx.tessellate(output.shapes.clone(), output.pixels_per_point).into_iter().filter_map(|primitive| {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { return None };
        let clip = primitive.clip_rect;
        Some(serde_json::json!({"clip":[clip.min.x,clip.min.y,clip.max.x,clip.max.y],"indices":mesh.indices,
            "vertices":mesh.vertices.iter().map(|v| serde_json::json!([v.pos.x,v.pos.y,v.uv.x,v.uv.y,v.color.to_array()])).collect::<Vec<_>>()}))
    }).collect::<Vec<_>>();
    let size = ctx.input(|input| input.screen_rect().size());
    std::fs::write(directory.join(format!("{name}.json")), serde_json::to_vec(&serde_json::json!({"width":size.x,"height":size.y,"atlas_size":atlas.size,"meshes":meshes})).unwrap()).unwrap();
}
