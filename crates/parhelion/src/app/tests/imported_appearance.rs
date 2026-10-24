//! Reopened weapons present the imported sources that replace their native template.
use super::*;
use crate::test_support::driver;
use parhelion_import::GraphReference;
use std::{fs, time::Instant};

fn saved_source(name: &str) -> (tempfile::TempDir, String) {
    let folder = tempfile::tempdir().unwrap();
    let mut recipe = WeaponRecipe::new_weapon("parhelion.source-view").unwrap();
    recipe.set_donor(0x3001, "Native Template");
    recipe.name = "Authored Weapon".into();
    let item = recipe.identity.item_hash.parse_u32().unwrap();
    fs::write(folder.path().join("parent.bin"), [0u8; 24]).unwrap();
    fs::write(
        folder.path().join("asset-graph.json"),
        serde_json::to_vec(&serde_json::json!({
            "kind":"weapon", "item_hash":item, "source_item":0xABCD1234u32,
            "source_name":name, "source_type":"Auto Rifle", "parent":"parent",
            "nodes":[{"symbol":"parent", "file":"parent.bin", "patches":[]}],
            "dyes":[{"channel":4, "manifest":1, "parent":"parent"}],
            "animation":{"first_person_status":"linked", "first_person":{"files":{}, "clips":[]}}
        }))
        .unwrap(),
    )
    .unwrap();
    recipe.overrides.imported_graph = Some(GraphReference::new(folder.path(), item).unwrap());
    let saved = recipe.to_json_pretty().unwrap();
    (folder, saved)
}

fn frame(ctx: &egui::Context, app: &mut PackageAuthoringApp, appearance: bool) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 1000.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if appearance {
                    app.draw_appearance_workspace(ui);
                } else {
                    app.draw_core_recipe_editor(ui);
                }
            });
        },
    );
    capture::record(&output);
    output
}

fn await_text(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    appearance: bool,
    expected: &str,
) -> egui::FullOutput {
    let started = Instant::now();
    loop {
        let output = frame(ctx, app, appearance);
        let shown = text(&output);
        if shown.contains(expected) {
            return output;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "{shown}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn reopened_weapon_sources_follow_imports_and_report_invalid_pins() {
    // Weapon recipes retain their pinned source folder when saved. Keep those inputs alive
    // through the reopen and selection workflow, as an ordinary saved import requires.
    let (_first_assets, first) = saved_source("Imported Auto Rifle");
    let (_second_assets, second) = saved_source("Another Imported Model");
    let mut app = PackageAuthoringApp {
        recipe: WeaponRecipe::from_json_str(&first).unwrap(),
        ..Default::default()
    };
    app.importer.enabled = false;
    assert!(app.catalog.is_none());
    assert!(
        !app.importer.enabled,
        "viewing a saved import does not require the importer browser"
    );
    let ctx = egui::Context::default();
    let weapon = await_text(&ctx, &mut app, false, "Imported Auto Rifle");
    let labels = text(&weapon);
    assert!(
        labels.contains("Native Template"),
        "the gameplay base stays visible"
    );
    assert!(
        labels.contains("0xABCD1234"),
        "the source identity must survive saved-recipe reload"
    );
    capture::write(&ctx, &weapon, "imported-weapon-source");

    let appearance = await_text(&ctx, &mut app, true, "Imported Auto Rifle");
    let labels = text(&appearance);
    assert!(labels.contains("Model") && labels.contains("Animations"));
    assert!(!labels.contains("Follow Native Template"));
    assert!(!labels.contains("Donor Preview"));
    capture::write(&ctx, &appearance, "imported-weapon-appearance");

    app.recipe = WeaponRecipe::from_json_str(&second).unwrap();
    let switched = await_text(&ctx, &mut app, true, "Another Imported Model");
    assert!(!text(&switched).contains("Imported Auto Rifle"));
    capture::write(&ctx, &switched, "imported-weapon-switched");

    app.recipe.overrides.imported_graph.as_mut().unwrap().sha256 = "damaged".into();
    let damaged = await_text(&ctx, &mut app, false, "Imported assets changed");
    assert!(!text(&damaged).contains("Another Imported Model"));
    capture::write(&ctx, &damaged, "imported-weapon-damaged");
    crate::test_support::artifact(
        "imported-weapon-sources.json",
        &serde_json::json!({
            "first_recipe":serde_json::from_str::<serde_json::Value>(&first).unwrap(),
            "second_recipe":serde_json::from_str::<serde_json::Value>(&second).unwrap(),
            "weapon":driver::painted_text(&weapon), "appearance":driver::painted_text(&appearance),
            "switched":driver::painted_text(&switched), "damaged":driver::painted_text(&damaged),
            "limits":"Synthetic metadata and UI workflow. No native model or gameplay acceptance."
        }),
    );
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, PARHELION_PREVIEW_RECIPES and fresh SUNDIAL_TEST_ARTIFACTS"]
fn imported_weapon_models_reopen_without_a_donor_catalog() {
    use sha2::{Digest, Sha256};
    let cases = PathBuf::from(std::env::var_os("PARHELION_PREVIEW_RECIPES").expect("recipe list"));
    let paths: Vec<PathBuf> = serde_json::from_slice(&fs::read(&cases).unwrap()).unwrap();
    assert!(
        paths.len() >= 2,
        "selection changes require two source models"
    );
    let artifacts = crate::test_support::artifact_dir("imported-weapon-models");
    fs::create_dir(&artifacts).expect("fresh artifacts");
    let mut app = PackageAuthoringApp {
        packages: crate::test_support::stock_packages(),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    sundial::ui::model_preview::set_options(
        &ctx,
        sundial::ui::model_preview::Options {
            play_animations: false,
            show_fps: false,
        },
    );
    let mut receipts = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        app.recipe = WeaponRecipe::load_json(path).unwrap();
        assert!(app.recipe.kind.is_weapon());
        let graph = app
            .recipe
            .overrides
            .imported_graph
            .as_ref()
            .expect("imported weapon")
            .clone();
        let saved = app.recipe.to_json_pretty().unwrap();
        fs::write(artifacts.join(format!("{index}.parhelion.json")), &saved).unwrap();
        app.recipe = WeaponRecipe::from_json_str(&saved).unwrap();
        assert!(app.catalog.is_none());
        let started = Instant::now();
        loop {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(520.0, 440.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.draw_weapon_preview(ui, None));
                },
            );
            capture::record(&output);
            let model = output
                .textures_delta
                .set
                .iter()
                .filter(|(id, _)| **id != egui::TextureId::Managed(0))
                .flat_map(|(_, deltas)| deltas.iter())
                .filter(|delta| delta.pos.is_none())
                .map(|delta| {
                    let egui::ImageData::Color(image) = &delta.image;
                    image
                })
                .find(|image| image.width() >= 200 && image.height() >= 200);
            if !text(&output).contains("Loading Model")
                && let Some(model) = model
            {
                let pixels = model
                    .pixels
                    .iter()
                    .flat_map(|p| p.to_array())
                    .collect::<Vec<_>>();
                let image =
                    image::RgbaImage::from_raw(model.width() as u32, model.height() as u32, pixels)
                        .unwrap();
                let image_path = artifacts.join(format!("{index}-model.png"));
                image.save(&image_path).unwrap();
                capture::write(&ctx, &output, &format!("imported-weapon-model-{index}"));
                receipts.push(serde_json::json!({
                    "recipe":path, "recipe_sha256":hex::encode(Sha256::digest(saved.as_bytes())),
                    "graph_sha256":graph.sha256, "image":image_path,
                    "image_sha256":hex::encode(Sha256::digest(fs::read(&image_path).unwrap())),
                }));
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(180),
                "No imported model: {}",
                text(&output)
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    app.recipe.overrides.imported_graph.as_mut().unwrap().sha256 = "damaged".into();
    let started = Instant::now();
    loop {
        let output = frame(&ctx, &mut app, false);
        if text(&output).contains("Imported assets changed") {
            capture::write(&ctx, &output, "imported-weapon-model-error");
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "{}",
            text(&output)
        );
        thread::sleep(Duration::from_millis(50));
    }
    let executable = std::env::current_exe().unwrap();
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases, "packages":app.packages, "recipes":receipts,
        "executable":executable,
        "executable_sha256":hex::encode(Sha256::digest(fs::read(&executable).unwrap())),
        "limits":"Unbuilt imported model capture and selection lifecycle. No render comparison or gameplay acceptance."
    })).unwrap()).unwrap();
}
