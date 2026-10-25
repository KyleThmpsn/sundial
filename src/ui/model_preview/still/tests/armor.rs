//! Configured package-to-inline-frame acceptance for complete cloth armor.
//! Source draw evidence supplies the required silhouette coverage. A load result,
//! hidden geometry or a stale texture from another selection cannot satisfy it.
use super::*;

#[test]
#[ignore = "Requires SUNDIAL_ARMOR_PREVIEW_CASES and fresh SUNDIAL_TEST_ARTIFACTS"]
fn configured_armor_keeps_its_robe_in_the_inline_preview() {
    use std::fs;
    assert!(
        !model_preview::gpu::available(),
        "Use the ordinary software UI path"
    );
    let cases = std::env::var_os("SUNDIAL_ARMOR_PREVIEW_CASES").expect("Armor cases");
    let output = crate::test_support::artifact_dir("armor-preview");
    assert!(!output.exists(), "Use fresh artifacts");
    let cases: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(cases).unwrap()).unwrap();
    assert!(!cases.is_empty(), "The required armor corpus is empty");
    fs::create_dir_all(&output).unwrap();
    let ctx = egui::Context::default();
    let id = egui::Id::new("complete-cloth-armor");
    let mut receipts = Vec::new();
    for (ordinal, case) in cases.iter().enumerate() {
        let packages = PathBuf::from(case["packages"].as_str().unwrap());
        let appearance = Appearance {
            arrangement: case["arrangement"].as_u64().unwrap().try_into().unwrap(),
            dyes: serde_json::from_value(case["dyes"].clone()).unwrap(),
            dye_textures: vec![],
        };
        let size: [usize; 2] = serde_json::from_value(case["size"].clone()).unwrap();
        let started = std::time::Instant::now();
        let image = loop {
            let frame = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        show(
                            ui,
                            id,
                            &packages,
                            Some(appearance.clone()),
                            &[],
                            egui::vec2(size[0] as f32, size[1] as f32),
                        );
                    });
                },
            );
            let state = ctx.data(|d| d.get_temp::<Arc<Mutex<Still>>>(id)).unwrap();
            let state = state.lock().unwrap();
            assert!(
                state.error.is_none(),
                "{:?}",
                state.error.as_ref().map(|v| &v.1)
            );
            let current = state.wanted == state.shown && state.model.is_some();
            let image = state
                .texture
                .as_ref()
                .filter(|_| current)
                .and_then(|texture| {
                    frame
                        .textures_delta
                        .set
                        .iter()
                        .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
                        .find_map(|(id, delta)| {
                            if *id == texture.id() {
                                let egui::ImageData::Color(image) = &delta.image;
                                Some(image.clone())
                            } else {
                                None
                            }
                        })
                });
            if let Some(image) = image {
                let model = state.model.as_ref().unwrap();
                if case["cloth"] == true {
                    assert!(model.has_cloth(), "{:?}", model.notices);
                }
                fs::write(
                    output.join(format!("armor-{ordinal}.glb")),
                    model_preview::export::glb(model, state.rendered_seconds.unwrap()).unwrap(),
                )
                .unwrap();
                receipts.push(serde_json::json!({"case":case,"notices":model.notices,"vertices":model.vertices.len(),"triangles":model.triangles.len(),"seconds":state.rendered_seconds}));
                break image;
            }
            drop(state);
            assert!(
                started.elapsed().as_secs() < 120,
                "The current inline armor frame did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(16));
        };
        let background = Scene::default().background;
        let covered = image
            .pixels
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let c = p.to_array();
                (c[..3]
                    .iter()
                    .zip(background)
                    .any(|(&a, b)| a.abs_diff(b) > 12))
                .then_some([i % image.width(), i / image.width()])
            })
            .collect::<Vec<_>>();
        assert!(!covered.is_empty(), "The armor frame is blank");
        let top = covered.iter().map(|p| p[1]).min().unwrap();
        let bottom = covered.iter().map(|p| p[1]).max().unwrap();
        let height = (bottom - top + 1) as f64 / image.height() as f64;
        let lower = covered
            .iter()
            .filter(|p| p[1] >= image.height() / 2)
            .count();
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        fs::write(
            output.join(format!("armor-{ordinal}.png")),
            model_preview::export::png(&rgba, image.width(), image.height()).unwrap(),
        )
        .unwrap();
        receipts.last_mut().unwrap()["coverage"] = serde_json::json!({"top":top,"bottom":bottom,"height_fraction":height,"lower_half_pixels":lower});
        fs::write(
            output.join("receipt.json"),
            serde_json::to_vec_pretty(&receipts).unwrap(),
        )
        .unwrap();
        assert!(
            height >= case["minimum_height_fraction"].as_f64().unwrap(),
            "Incomplete armor silhouette: {height}"
        );
        assert!(
            lower >= case["minimum_lower_pixels"].as_u64().unwrap() as usize,
            "The lower robe is missing: {lower} pixels"
        );
    }
}
