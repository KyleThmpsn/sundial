//! Dense native patterns retain their coordinates and geometry in bounded GLB charts.
use super::*;

fn document(glb: &[u8]) -> serde_json::Value {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + length]).unwrap()
}

#[test]
fn dense_native_detail_exports_bounded_charts_with_explicit_sampling_limits() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let actual = model(10, false);
    let reference = model(10, true);
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let images = [&actual, &reference].map(|m| render::image(m, camera, [480, 240]));
    for (suffix, image) in [("actual", &images[0]), ("reference", &images[1])] {
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            out.join(format!("native-dense-detail-{suffix}.png")),
            export::png(&rgba, 480, 240).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(images[0].pixels, images[1].pixels);
    let glbs = [&actual, &reference].map(|m| export::glb(m, 0.0).unwrap());
    for (suffix, glb) in [("actual", &glbs[0]), ("reference", &glbs[1])] {
        std::fs::write(out.join(format!("native-dense-detail-{suffix}.glb")), glb).unwrap();
    }
    assert_eq!(embedded(&glbs[0]), embedded(&glbs[1]));
    let doc = document(&glbs[0]);
    let limits = &doc["asset"]["extras"]["detail_sampling"];
    let reductions = limits["reduced_triangles"].as_array().unwrap();
    assert_eq!(reductions.len(), 2);
    assert!(
        reductions
            .iter()
            .all(|v| v["source_pixels"].as_f64().unwrap() > v["stored_edge"].as_f64().unwrap())
    );
    let primitives = doc["meshes"][0]["primitives"].as_array().unwrap();
    let indices: u64 = primitives
        .iter()
        .map(|p| {
            doc["accessors"][p["indices"].as_u64().unwrap() as usize]["count"]
                .as_u64()
                .unwrap()
        })
        .sum();
    assert_eq!(indices, 18);
    let mut texels = 0;
    for image in doc["images"].as_array().unwrap() {
        let bytes = repack::bytes(
            &glbs[0],
            &doc,
            image["bufferView"].as_u64().unwrap() as usize,
        );
        let (shape, pixels) = repack::png_pixels(bytes);
        assert!(shape[0] <= 2048 && shape[1] <= 2048 && !pixels.is_empty());
        texels += shape[0] * shape[1];
    }
    assert!(texels <= 4 * 16 * 1024 * 1024);
    std::fs::write(out.join("native-dense-detail.json"), serde_json::to_vec_pretty(&json!({
        "secondary":actual.detail_uvs,"detail_sampling":limits,"exported_indices":indices,
        "actual_and_reference_pixels_agree":true,"embedded_images_agree":true,"embedded_texels":texels
    })).unwrap()).unwrap();
}
