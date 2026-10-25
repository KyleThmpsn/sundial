//! The Weapon tab's Magazine row against inspect counts read in game, written before the count.
//! Failures: reading the stat group's display curve instead of the components, rounding
//! where the inspect display truncates, leaving out a starting perk's Magazine modifier, dividing by
//! bullets per shot where the HUD mode row does not ask for it or not dividing where it does, a 0
//! that gives no warning, a readout that never loads, and a reading left from the previous weapon.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
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
                egui::vec2(700.0, 1800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                let donor = app.current_donor();
                app.draw_investment_stats_panel(ui, donor.as_ref());
            });
        },
    );
    capture::record(&output);
    output
}

/// The Magazine row's reading and whether a warning marks it, painted once the weapon has been
/// read. Until then the row reads the display curve.
fn readout(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> (egui::FullOutput, i64, bool) {
    let start = Instant::now();
    loop {
        frame(ctx, app, Vec::new());
        let donor = app.current_donor().expect("a weapon donor");
        if let Some(read) = app.barrel_reading(ctx, Some(&donor)) {
            let read = read.expect("the weapon reads");
            let magazine = read.expect("the weapon has a Barrel").magazine;
            magazine.expect("the Magazine count reads");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the weapon was never read"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let output = frame(ctx, app, Vec::new());
    let painted = text(&output);
    let lines = painted.lines().collect::<Vec<_>>();
    let at = lines
        .iter()
        .position(|line| *line == "Magazine")
        .unwrap_or_else(|| panic!("no Magazine row: {painted}"));
    // The row paints its name, the warning before it, then the reading and the raw value.
    let reading = lines[at + 1..]
        .iter()
        .position(|line| line.parse::<i64>().is_ok())
        .map(|offset| at + 1 + offset)
        .unwrap_or_else(|| panic!("no Magazine reading: {painted}"));
    let warned = lines[at + 1..reading].contains(&egui_phosphor::regular::WARNING);
    (output, lines[reading].parse().unwrap(), warned)
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn weapon_tab_magazine_readout_matches_inspect_counts() {
    // Item hash, the count the inspect display showed, and what each case separates.
    let cases: [(u32, i64, &str); 6] = [
        (
            2_014_642_399,
            43,
            "auto rifle, runtime logic type 2, HUD flag clear",
        ),
        (
            1_911_843_789,
            11,
            "single-bullet sidearm, capacity 11.5 truncates",
        ),
        (
            1_533_499_361,
            36,
            "burst sidearm, Adaptive Frame adds -18, no division",
        ),
        (
            2_208_405_142,
            4,
            "fusion rifle, HUD flag divides by 7 bullets",
        ),
        (
            1_508_896_098,
            1,
            "rocket launcher, HUD flag divides by 8 rockets",
        ),
        (
            417_164_956,
            0,
            "fusion rifle, 5.0015 over 7 bullets, row hidden",
        ),
    ];
    let packages = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    let mut app = PackageAuthoringApp {
        packages,
        catalog: Some(catalog),
        donor_summaries: donors.clone(),
        ..Default::default()
    };
    app.library_state.refresh_donors(&donors);
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let output_dir = crate::test_support::artifact_dir("magazine-readout");
    fs::create_dir_all(output_dir.parent().unwrap()).unwrap();
    fs::create_dir(&output_dir).expect("fresh artifacts");
    let mut readings = Vec::new();
    // One app for every weapon, so each reading also checks the previous one was replaced.
    for (hash, expected, case) in cases {
        let donor = donors
            .iter()
            .find(|donor| donor.hash == hash)
            .unwrap_or_else(|| panic!("no donor {hash}"))
            .clone();
        app.recipe =
            WeaponRecipe::new_weapon_for_donor("parhelion.magazine-readout", hash, &donor.name)
                .unwrap();
        let (output, shown, hidden) = readout(&ctx, &mut app);
        capture::write(&ctx, &output, &format!("magazine-readout-{hash}"));
        readings.push(serde_json::json!({
            "item_hash": hash, "name": donor.name, "case": case,
            "observed_in_game": expected, "readout": shown, "hidden_warning": hidden,
        }));
        assert_eq!(shown, expected, "{} ({case})", donor.name);
        assert_eq!(hidden, expected == 0, "{} warning ({case})", donor.name);
    }
    let executable = std::env::current_exe().unwrap();
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    fs::write(
        output_dir.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "repeat_command": "cargo test --release -p parhelion weapon_tab_magazine_readout_matches_inspect_counts -- --ignored --nocapture",
            "native_build": "86657.20.08.23.1800.d2_rc___release",
            "packages": app.packages,
            "revision": String::from_utf8_lossy(&revision.stdout).trim(),
            "executable": executable,
            "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
            "oracle": "Collections inspect counts read in game, recorded in the Almanac's inspect magazine section",
            "readings": readings,
            "limits": "Rendered stock weapons with their starting plugs. Authored edits and exotic perk weights are not compared with the game.",
        }))
        .unwrap(),
    )
    .unwrap();
}
