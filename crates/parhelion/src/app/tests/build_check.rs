//! The pre-build check's modal, rendered headlessly: the left-out items named, the three
//! choices offered, and a capture for review when `SUNDIAL_TEST_ARTIFACTS` is set.
use super::*;

#[test]
fn the_build_check_modal_names_the_left_out_items_and_offers_three_choices() {
    let mut app = PackageAuthoringApp::default();
    app.build_check.missing = Some(vec![
        crate::app::build_check::Missing {
            hash: 0x4CE3_CE93,
            label: "Test 02 Reload on Kill".to_owned(),
            recipe: PathBuf::from("test-02.parhelion.json"),
        },
        crate::app::build_check::Missing {
            hash: 0x7BF7_46CB,
            label: "Prismatic Hunter".to_owned(),
            recipe: PathBuf::from("prismatic-hunter.parhelion.json"),
        },
    ]);
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Dark);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 640.0),
        )),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run_ui(input(), |ui| {
            egui::CentralPanel::default().show(ui, workbench_style);
            app.draw_build_check(ui);
        });
    }
    let shown = text(&output);
    for expected in [
        "Installed Items Not in This Build",
        "Test 02 Reload on Kill",
        "Prismatic Hunter",
        "Add to Build",
        "Build Without Them",
        "Cancel",
    ] {
        assert!(
            shown.contains(expected),
            "missing {expected:?} in {shown:?}"
        );
    }
    assert!(
        app.build_check.missing.is_some(),
        "drawing the modal must not answer it"
    );
    crate::app::custom_perks::workbench::tests::capture::write(&ctx, &output, "build-check");
}

/// A modal sits in the middle of the screen and cannot be scrolled or moved, so a list taller than
/// the screen once pushed its heading and buttons off both edges. The list scrolls inside the
/// modal instead, starting at its first item, and every choice stays on screen.
#[test]
fn a_long_build_check_list_scrolls_and_keeps_its_buttons_on_screen() {
    let mut app = PackageAuthoringApp::default();
    app.build_check.missing = Some(
        (0..60_u32)
            .map(|index| crate::app::build_check::Missing {
                hash: 0x1000_0000 + index,
                label: format!("Test Item {index:02}"),
                recipe: PathBuf::from(format!("test-{index:02}.parhelion.json")),
            })
            .collect(),
    );
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Dark);
    let screen = egui::vec2(900.0, 640.0);
    let mut output = egui::FullOutput::default();
    // A modal takes its size from the frame before, so it settles on the third.
    for _ in 0..3 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, workbench_style);
                app.draw_build_check(ui);
            },
        );
    }
    for label in [
        "Installed Items Not in This Build",
        "Test Item 00",
        "Add to Build",
        "Build Without Them",
        "Cancel",
    ] {
        let origin = text_origin(&output, label);
        assert!(
            origin.y >= 0.0 && origin.y + 20.0 <= screen.y,
            "{label} at {origin:?} is off the {screen:?} screen"
        );
    }
    crate::app::custom_perks::workbench::tests::capture::write(&ctx, &output, "build-check-long");
}
