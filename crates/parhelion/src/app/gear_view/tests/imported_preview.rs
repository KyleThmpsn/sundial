//! Reopened source gear must draw its imported appearance before a build or catalog lookup.
//! PARHELION_PREVIEW_RECIPES is a JSON array of portable recipe paths. The configured install
//! contains the same recipes' built items as an independent native appearance oracle.
use super::*;
use sha2::{Digest, Sha256};
use sundial::ui::model_preview::{Options, set_options};

fn preview_frame(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(440.0, 440.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_gear_preview(ui, None));
        },
    )
}

fn paused_context() -> egui::Context {
    let ctx = context();
    set_options(
        &ctx,
        Options {
            play_animations: false,
            show_fps: false,
        },
    );
    ctx
}

fn installed_image(
    packages: &Path,
    appearance: Appearance,
    size: [usize; 2],
) -> Arc<egui::ColorImage> {
    let ctx = paused_context();
    let size = egui::vec2(size[0] as f32, size[1] as f32);
    await_preview(
        || {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        size + egui::vec2(64.0, 64.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        still::show(
                            ui,
                            egui::Id::new("installed"),
                            packages,
                            Some(appearance.clone()),
                            &[],
                            size,
                        );
                    });
                },
            )
        },
        "Installed appearance",
    )
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL, PARHELION_PREVIEW_RECIPES and fresh SUNDIAL_TEST_ARTIFACTS"]
fn reopened_imports_draw_source_models_and_report_damaged_assets() {
    let install = crate::test_support::install();
    let packages = install.join("packages");
    let cases = PathBuf::from(std::env::var_os("PARHELION_PREVIEW_RECIPES").expect("recipe cases"));
    let paths: Vec<PathBuf> = serde_json::from_slice(&fs::read(&cases).unwrap()).unwrap();
    assert!(
        paths.len() >= 2,
        "selection changes require two different imported items"
    );
    let artifacts = crate::test_support::artifact_dir("imported-gear-preview");
    fs::create_dir(&artifacts).expect("artifact folder must be fresh");
    let catalog = crate::test_support::catalog(&install).unwrap();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        ..Default::default()
    };
    let ctx = paused_context();
    let mut readback = Vec::new();
    let mut previous: Option<Arc<egui::ColorImage>> = None;
    for (index, path) in paths.iter().enumerate() {
        let bytes = fs::read(path).unwrap();
        fs::write(artifacts.join(format!("{index}.parhelion.json")), &bytes).unwrap();
        app.recipe = WeaponRecipe::from_json_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(
            app.recipe.kind,
            ItemKind::Armor,
            "configure imported armor recipes"
        );
        let graph = app
            .recipe
            .overrides
            .imported_graph
            .as_ref()
            .expect("imported recipe")
            .clone();
        let item = app.recipe.identity.item_hash.parse_u32().unwrap();
        let donor = app.recipe.donor.item_hash.parse_u32().unwrap();
        assert_ne!(item, donor);
        assert!(
            app.catalog.is_none(),
            "unbuilt source preview must not need an item catalog"
        );
        let shown = await_preview(|| preview_frame(&ctx, &mut app), "Reopened source gear");
        let appearance = |hash| {
            catalog
                .shader_preview_appearance(hash, &[vec![], vec![], vec![]])
                .expect("configured item must have an installed appearance")
        };
        let installed = appearance(item);
        let expected = installed_image(&packages, installed.clone(), shown.size);
        let native = installed_image(&packages, appearance(donor), shown.size);
        save_image(&shown, &artifacts.join(format!("{index}-unbuilt.png")));
        save_image(&expected, &artifacts.join(format!("{index}-installed.png")));
        save_image(&native, &artifacts.join(format!("{index}-donor.png")));
        assert_eq!(
            differing_pixels(&shown, &expected),
            0,
            "source preview must match emitted geometry and dyes"
        );
        let donor_difference = differing_pixels(&shown, &native);
        assert!(
            donor_difference > 0,
            "the donor is an independent negative control"
        );
        if let Some(previous) = previous {
            assert!(
                differing_pixels(&shown, &previous) > 0,
                "selection must replace the model"
            );
        }
        readback.push(serde_json::json!({
            "recipe":path,"recipe_sha256":hex::encode(Sha256::digest(&bytes)),
            "graph_sha256":graph.sha256,"item":format!("{item:08X}"),
            "donor":format!("{donor:08X}"),"donor_differing_pixels":donor_difference,
            "installed_differing_pixels":0,
            "installed_arrangement":installed.arrangement,"installed_dyes":installed.dyes,
        }));
        previous = Some(shown);
    }
    app.recipe.overrides.imported_graph.as_mut().unwrap().sha256 = "damaged".into();
    let start = Instant::now();
    loop {
        let output = preview_frame(&ctx, &mut app);
        if texts(&output)
            .iter()
            .any(|(text, _)| text.contains("Imported assets changed"))
        {
            capture::record(&output);
            fs::write(
                artifacts.join("damaged-import.txt"),
                texts(&output)
                    .into_iter()
                    .map(|(text, _)| text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "damaged import must report an error"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let executable = std::env::current_exe().unwrap();
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "packages":packages,"cases":cases,"recipes":readback,
        "executable":executable,"executable_sha256":hex::encode(Sha256::digest(fs::read(&executable).unwrap())),
        "revision":String::from_utf8_lossy(&std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap().stdout).trim(),
        "damaged_import_reported":true,"limits":"Preview lighting does not establish in-game brightness."
    })).unwrap()).unwrap();
}
