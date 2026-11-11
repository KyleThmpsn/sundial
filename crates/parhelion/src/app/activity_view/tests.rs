//! The activity editor's overview of a converted map, read from the newest finished conversion of
//! the map recipe `PARHELION_OVERVIEW_RECIPE` names in Parhelion's data folder. The editor's frame
//! and `overview.json`, which names the conversion and what the frame drew from it, land in
//! `captures` under `SUNDIAL_TEST_ARTIFACTS` when it is set.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::test_support::driver::{label, texts};
use std::time::Instant;

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.draw_activity_editor(&ctx);
        },
    );
    capture::record(&output);
    output
}

#[test]
#[ignore = "requires PARHELION_OVERVIEW_RECIPE, a map recipe with a finished conversion"]
fn a_converted_map_shows_its_overview() {
    let recipe = PathBuf::from(
        std::env::var_os("PARHELION_OVERVIEW_RECIPE").expect("PARHELION_OVERVIEW_RECIPE"),
    );
    let mut app = PackageAuthoringApp {
        packages: PathBuf::new(),
        ..PackageAuthoringApp::default()
    };
    app.open_activity(&recipe);
    assert!(app.activity_editor.is_some(), "The recipe did not open");
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    let started = Instant::now();
    loop {
        let output = frame(&ctx, &mut app, vec![]);
        let editor = app.activity_editor.as_ref().unwrap();
        if editor.overview_texture.is_some() {
            break;
        }
        if let Some(Err(error)) = &editor.overview {
            panic!("The overview could not be read: {error}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "The overview did not load: {:?}",
            texts(&output)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // The overview sits below the window's default height. The fields scroll to the end, where
    // the caption shows, then back until the heading shows whole as well.
    let output = frame(&ctx, &mut app, vec![]);
    let at = label(&output, "Title").expect("Missing Title").center();
    let scroll = |app: &mut PackageAuthoringApp, delta: f32| {
        frame(
            &ctx,
            app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
                egui::Event::PointerGone,
            ],
        );
        for _ in 0..4 {
            frame(&ctx, app, vec![]);
        }
        frame(&ctx, app, vec![])
    };
    scroll(&mut app, -5000.0);
    for _ in 0..100 {
        let output = scroll(&mut app, 10.0);
        if texts(&output).iter().any(|(text, _)| text == "Overview") {
            break;
        }
    }
    let output = scroll(&mut app, 10.0);
    let editor = app.activity_editor.as_ref().unwrap();
    let Some(Ok(Some(overview))) = &editor.overview else {
        panic!("The map has no finished conversion");
    };

    // The conversion as written, read without the editor's parser.
    let conversion: serde_json::Value =
        serde_json::from_slice(&std::fs::read(overview.folder.join("conversion.json")).unwrap())
            .unwrap();
    let mut spawns = conversion["spawns"].as_array().unwrap().clone();
    for area in conversion["areas"].as_array().into_iter().flatten().skip(1) {
        spawns.extend(area["spawns"].as_array().unwrap().iter().cloned());
    }
    let plan = &conversion["plan"];
    let image = image::open(overview.folder.join(plan["image"].as_str().unwrap()))
        .unwrap()
        .to_rgba8();
    assert_eq!(
        [image.width(), image.height()],
        [
            plan["width"].as_u64().unwrap() as u32,
            plan["height"].as_u64().unwrap() as u32
        ]
    );
    let covered = image.pixels().filter(|pixel| pixel[3] == 255).count();
    let water = image
        .pixels()
        .filter(|pixel| pixel[3] == 255 && pixel[2] == 255)
        .count();
    assert!(covered > 0, "The plan holds no surface");
    let texture = editor.overview_texture.as_ref().unwrap();
    assert_eq!(
        texture.size(),
        [image.width() as usize, image.height() as usize]
    );

    let labels = texts(&output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    assert!(
        labels.iter().any(|label| label == "Overview"),
        "Missing Overview in {labels:?}"
    );
    let caption = match spawns.len() {
        1 => "1 spawn point".to_owned(),
        count => format!("{count} spawn points"),
    };
    assert!(
        labels.iter().any(|label| label == &caption),
        "Missing {caption:?} in {labels:?}"
    );

    if let Some(directory) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        let directory = PathBuf::from(directory).join("captures");
        std::fs::create_dir_all(&directory).unwrap();
        capture::write(&ctx, &output, "activity-overview");
        let digest = crate::artifact::digest_file(&overview.folder.join("plan.png")).unwrap();
        std::fs::write(
            directory.join("overview.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "recipe": recipe,
                "conversion": overview.folder,
                "plan": plan,
                "plan_sha256": digest.sha256,
                "cells_covered": covered,
                "cells_water": water,
                "spawns": spawns.len(),
                "caption": caption,
                "texture": texture.size(),
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
