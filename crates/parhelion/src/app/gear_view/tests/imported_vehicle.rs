//! Portable vehicle selection, editor responsiveness and validation lifecycle.
use super::*;
use sha2::{Digest, Sha256};

fn draw(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 1800.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_gear_editor(ui));
        },
    );
    capture::record(&output);
    output
}

fn labels(output: &egui::FullOutput) -> String {
    texts(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn wait_for(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    expected: &str,
) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = draw(ctx, app);
        if labels(&output).contains(expected) {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "Missing {expected}: {}",
            labels(&output)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, PARHELION_PREVIEW_RECIPES and fresh SUNDIAL_TEST_ARTIFACTS"]
#[allow(clippy::cognitive_complexity)]
fn imported_vehicles_reopen_and_keep_the_editor_responsive() {
    let cases = PathBuf::from(std::env::var_os("PARHELION_PREVIEW_RECIPES").expect("recipe list"));
    let paths: Vec<PathBuf> = serde_json::from_slice(&fs::read(&cases).unwrap()).unwrap();
    assert!(
        paths.len() >= 2,
        "configure different vehicle sources for selection changes"
    );
    let artifacts = crate::test_support::artifact_dir("imported-vehicle-preview");
    fs::create_dir(&artifacts).expect("fresh artifacts");
    let ctx = context();
    sundial::ui::model_preview::set_options(
        &ctx,
        sundial::ui::model_preview::Options {
            play_animations: false,
            show_fps: false,
            ..Default::default()
        },
    );
    let mut app = PackageAuthoringApp {
        packages: crate::test_support::stock_packages(),
        ..Default::default()
    };
    let mut receipts = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        app.recipe = WeaponRecipe::load_json(path).unwrap();
        assert_eq!(app.recipe.kind, ItemKind::Sparrow);
        let original = app
            .recipe
            .overrides
            .sparrow
            .clone()
            .expect("summon override");
        let entity = original
            .summon
            .entity()
            .unwrap()
            .expect("native runtime template");
        let graph = app
            .recipe
            .overrides
            .imported_graph
            .clone()
            .expect("imported appearance");
        graph
            .validate_vehicle(entity)
            .expect("configure compatible source vehicles");
        let saved = app.recipe.to_json_pretty().unwrap();
        fs::write(artifacts.join(format!("{index}.parhelion.json")), &saved).unwrap();
        app.recipe = WeaponRecipe::from_json_str(&saved).unwrap();
        assert!(
            app.catalog.is_none(),
            "source preview must work without a native catalog"
        );
        let start = Instant::now();
        let mut loading_ms = Vec::new();
        let mut model = None;
        loop {
            let frame_start = Instant::now();
            let output = draw(&ctx, &mut app);
            loading_ms.push(frame_start.elapsed().as_secs_f64() * 1000.0);
            let shown = labels(&output);
            assert!(
                !shown.contains("Imported assets changed")
                    && !shown.contains("different summoned vehicle"),
                "{shown}"
            );
            if !shown.contains("Loading Model") {
                for (id, deltas) in &output.textures_delta.set {
                    if *id == egui::TextureId::Managed(0) {
                        continue;
                    }
                    for delta in deltas {
                        let egui::ImageData::Color(image) = &delta.image;
                        if delta.pos.is_none() && image.width() >= 200 && image.height() >= 200 {
                            model = Some(image.clone());
                        }
                    }
                }
            }
            if model.is_some() && !shown.contains("Checking Vehicle Compatibility") {
                capture::write(&ctx, &output, &format!("imported-vehicle-{index}"));
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(180),
                "No source vehicle: {shown}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let image_path = artifacts.join(format!("{index}-model.png"));
        save_image(&model.unwrap(), &image_path);
        let mut idle_ms = Vec::new();
        for _ in 0..60 {
            let start = Instant::now();
            draw(&ctx, &mut app);
            idle_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let mut ordered = idle_ms.clone();
        ordered.sort_by(f64::total_cmp);
        let p95 = ordered[ordered.len() * 95 / 100];
        receipts.push(serde_json::json!({
            "recipe":path, "recipe_sha256":hex::encode(Sha256::digest(saved.as_bytes())),
            "graph_sha256":graph.sha256, "source_directory":graph.directory,
            "runtime_entity":format!("{entity:08X}"), "image":image_path,
            "image_sha256":hex::encode(Sha256::digest(fs::read(&image_path).unwrap())),
            "loading_frame_ms":loading_ms, "idle_frame_ms":idle_ms, "idle_p95_ms":p95,
        }));
        fs::write(
            artifacts.join("frames.json"),
            serde_json::to_vec_pretty(&receipts).unwrap(),
        )
        .unwrap();
        assert!(
            p95 < 100.0,
            "settled editor frames stalled: p95 {p95:.2} ms"
        );

        let wrong = if original.summon == crate::vehicle::Summon::Tank {
            crate::vehicle::Summon::HeavyPike
        } else {
            crate::vehicle::Summon::Tank
        };
        app.recipe.overrides.sparrow.as_mut().unwrap().summon = wrong;
        let mismatch = wait_for(&ctx, &mut app, "different summoned vehicle");
        capture::write(
            &ctx,
            &mismatch,
            &format!("imported-vehicle-{index}-mismatch"),
        );
        app.recipe.overrides.sparrow = Some(original);
        let start = Instant::now();
        loop {
            let output = draw(&ctx, &mut app);
            let shown = labels(&output);
            if !shown.contains("Checking Vehicle Compatibility")
                && !shown.contains("different summoned vehicle")
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(180),
                "restored summon retained a stale diagnostic"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    app.recipe.overrides.imported_graph.as_mut().unwrap().sha256 = "damaged".into();
    let damaged = wait_for(&ctx, &mut app, "Imported assets changed");
    capture::write(&ctx, &damaged, "imported-vehicle-damaged");
    let executable = std::env::current_exe().unwrap();
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases, "packages":app.packages, "recipes":receipts,
        "executable":executable,
        "executable_sha256":hex::encode(Sha256::digest(fs::read(&executable).unwrap())),
        "mismatch_and_recovery":true, "damaged_pin_reported":true,
        "limits":"Headless editor frame timing and source model captures. Visual review required. No native gameplay or live OpenGL measurement."
    })).unwrap()).unwrap();
}
