//! Real Gameplay input, recipe reload and donor lifecycle, written before the controls.
//! Failures: controls hidden by Technical, edits lost on save, stale async donor defaults,
//! edits accumulating while rendering, inaccessible numeric fields, and reset retaining edits.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::test_support::driver::{accessible, control, label, tap};
use std::fs;

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                let donor = app.current_donor();
                app.draw_gameplay_workspace(ui, donor.as_ref());
            });
        },
    );
    capture::record(&output);
    output
}

fn ready(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = frame(ctx, app, Vec::new());
        if accessible(&output, "Pellets per Shot").is_some() {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "Barrel controls never loaded: {}",
            text(&output)
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, name: &str) {
    let output = ready(ctx, app);
    let rect = control(&output, name)
        .or_else(|| label(&output, name))
        .unwrap_or_else(|| panic!("missing {name}"));
    for events in tap(rect.center()) {
        frame(ctx, app, events);
    }
}

fn number(ctx: &egui::Context, app: &mut PackageAuthoringApp, name: &str, value: &str) {
    click(ctx, app, name);
    click(ctx, app, name);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(
        ctx,
        app,
        vec![
            key(egui::Key::A, egui::Modifiers::COMMAND),
            egui::Event::Text(value.into()),
            key(egui::Key::Enter, egui::Modifiers::NONE),
        ],
    );
    frame(ctx, app, Vec::new());
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn gameplay_barrel_controls_edit_reload_change_donor_and_reset() {
    let packages = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    let named = |name: &str| {
        donors
            .iter()
            .find(|donor| donor.name.replace('\u{2019}', "'") == name)
            .unwrap()
            .clone()
    };
    let thorn = named("Thorn");
    let shotgun = named("Felwinter's Lie");
    let mut app = PackageAuthoringApp {
        packages,
        catalog: Some(catalog),
        donor_summaries: donors.clone(),
        show_experimental_options: false,
        recipe: WeaponRecipe::new_weapon_for_donor("parhelion.barrel-ui", thorn.hash, &thorn.name)
            .unwrap(),
        ..Default::default()
    };
    app.library_state.refresh_donors(&donors);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let output = ready(&ctx, &mut app);
    assert!(!text(&output).contains("Technical"));
    assert!(app.recipe.overrides.barrel.is_none());
    number(&ctx, &mut app, "Pellets per Shot", "8");
    number(&ctx, &mut app, "Spread", "50");
    let edits = app
        .recipe
        .overrides
        .barrel
        .as_ref()
        .expect("saved controls");
    assert_eq!(edits.pellets, Some(8));
    assert_eq!(edits.spread_scale_bits, Some(0.5_f32.to_bits()));
    click(&ctx, &mut app, "Custom");
    number(&ctx, &mut app, "Ring 1 Inner Radius", "50");
    number(&ctx, &mut app, "Ring 1 Rotation", "90");
    number(&ctx, &mut app, "Ring 1 Randomness", "0");
    let rings = app
        .recipe
        .overrides
        .barrel
        .as_ref()
        .unwrap()
        .rings
        .as_ref()
        .unwrap();
    assert_eq!(f32::from_bits(rings[0].inner_radius_bits), 0.5);
    assert!((f32::from_bits(rings[0].rotation_bits) - std::f32::consts::FRAC_PI_2).abs() < 0.00001);
    assert_eq!(f32::from_bits(rings[0].randomness_bits), 0.0);
    let output_dir = crate::test_support::artifact_dir("barrel-controls-ui");
    fs::create_dir_all(output_dir.parent().unwrap()).unwrap();
    fs::create_dir(&output_dir).expect("fresh artifacts");
    for (name, source) in [
        ("barrel_controls.rs", include_str!("barrel_controls.rs")),
        ("barrel.rs", include_str!("../runtime_view/barrel.rs")),
        (
            "controls.rs",
            include_str!("../runtime_view/barrel/controls.rs"),
        ),
    ] {
        fs::write(output_dir.join(name), source).unwrap();
    }
    let path = output_dir.join("recipe.json");
    app.recipe.save_json(&path).unwrap();
    let saved = app.recipe.clone();
    app.recipe = WeaponRecipe::load_json(&path).unwrap();
    capture::write(&ctx, &ready(&ctx, &mut app), "barrel-controls-custom");
    assert_eq!(app.recipe, saved);
    click(&ctx, &mut app, "Reset Barrel");
    assert!(app.recipe.overrides.barrel.is_none());
    app.recipe.set_component_splice(
        sundial::package_authoring::entity::WEAPON_BARREL_COMPONENT_KEY,
        Some(WeaponDonorReference {
            item_hash: shotgun.hash.into(),
            expected_name: Some(shotgun.name),
        }),
    );
    ready(&ctx, &mut app);
    number(&ctx, &mut app, "Pellets per Shot", "6");
    click(&ctx, &mut app, "Custom");
    assert_eq!(
        app.recipe
            .overrides
            .barrel
            .as_ref()
            .unwrap()
            .rings
            .as_ref()
            .unwrap()
            .iter()
            .map(|ring| ring.pellets)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    capture::write(&ctx, &ready(&ctx, &mut app), "barrel-controls-donor");
    app.recipe = saved;
    ready(&ctx, &mut app);
    assert_eq!(
        app.recipe.overrides.barrel.as_ref().unwrap().pellets,
        Some(8)
    );
    click(&ctx, &mut app, "Reset Barrel");
    let reset = output_dir.join("reset-recipe.json");
    app.recipe.save_json(&reset).unwrap();
    assert!(
        WeaponRecipe::load_json(reset)
            .unwrap()
            .overrides
            .barrel
            .is_none()
    );
    capture::write(&ctx, &ready(&ctx, &mut app), "barrel-controls-reset");
    let executable = std::env::current_exe().unwrap();
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    fs::write(output_dir.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "repeat_command": "cargo test --release -p parhelion gameplay_barrel_controls_edit_reload_change_donor_and_reset -- --ignored --nocapture",
        "native_build": "86657.20.08.23.1800.d2_rc___release", "reset_recipe": app.recipe,
        "packages": app.packages, "revision": String::from_utf8_lossy(&revision.stdout).trim(),
        "executable": executable, "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
        "limits": "Rendered Gameplay controls and saved recipes. No gameplay claim."
    })).unwrap()).unwrap();
}
