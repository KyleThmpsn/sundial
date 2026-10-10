//! Pointer resizing must reveal more activity and keep the selected window height.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::test_support::driver::{accessible, button_frames};

fn draw(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 1100.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.draw_build_status_window(ui),
    );
    capture::record(&output);
    output
}

fn settle(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    for _ in 0..3 {
        draw(ctx, app, Vec::new());
    }
    draw(ctx, app, Vec::new())
}

fn window(output: &egui::FullOutput) -> egui::Rect {
    accessible(output, "Build & Install").expect("the visible build window")
}

fn visible_events(output: &egui::FullOutput) -> usize {
    fn count(shape: &egui::Shape, clip: egui::Rect) -> usize {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("Resize Event ") => {
                usize::from(clip.contains_rect(text.galley.rect.translate(text.pos.to_vec2())))
            }
            egui::Shape::Vec(shapes) => shapes.iter().map(|shape| count(shape, clip)).sum(),
            _ => 0,
        }
    }
    output
        .shapes
        .iter()
        .map(|shape| count(&shape.shape, shape.clip_rect))
        .sum()
}

fn resize(ctx: &egui::Context, app: &mut PackageAuthoringApp, delta: f32) -> egui::FullOutput {
    let start = window(&settle(ctx, app)).center_bottom() - egui::vec2(0.0, 1.0);
    draw(ctx, app, vec![egui::Event::PointerMoved(start)]);
    let [press, _] = button_frames(start, egui::PointerButton::Primary);
    draw(ctx, app, press);
    for step in 1..=6 {
        draw(
            ctx,
            app,
            vec![egui::Event::PointerMoved(
                start + egui::vec2(0.0, delta * step as f32 / 6.0),
            )],
        );
    }
    let [_, release] = button_frames(start + egui::vec2(0.0, delta), egui::PointerButton::Primary);
    draw(ctx, app, release);
    settle(ctx, app)
}

#[test]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn progress_window_grows_activity_shrinks_and_reopens_at_the_chosen_size() {
    let mut measurements = Vec::new();
    for installing in [false, true] {
        let name = if installing { "install" } else { "build" };
        let mut app = PackageAuthoringApp {
            build_status_open: true,
            ..Default::default()
        };
        // Worker state is supplied directly. Drawing and dragging run through the real dialog,
        // without starting a package build or an installation.
        let activity = if installing {
            app.build_dialog_step = BuildDialogStep::Install;
            app.install_receiver = Some(mpsc::channel().1);
            &mut app.install_status.activity
        } else {
            app.build_receiver = Some(mpsc::channel().1);
            &mut app.build_activity
        };
        for event in 0..100 {
            activity.push(
                Duration::from_secs(event),
                format!("Resize Event {event:03}"),
            );
        }
        let recipe = app.recipe.clone();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let initial = settle(&ctx, &mut app);
        let initial_height = window(&initial).height();
        let initial_events = visible_events(&initial);
        assert!(initial_events > 0, "no activity was visible");
        capture::write(&ctx, &initial, &format!("{name}-initial"));

        let grown = resize(&ctx, &mut app, 180.0);
        capture::write(&ctx, &grown, &format!("{name}-tall"));
        assert!(window(&grown).height() > initial_height + 140.0);
        assert!(visible_events(&grown) > initial_events);
        if !installing {
            assert!(window(&grown).contains_rect(accessible(&grown, "Close").unwrap()));
        }

        let shrunk = resize(&ctx, &mut app, -260.0);
        capture::write(&ctx, &shrunk, &format!("{name}-short"));
        assert!(window(&shrunk).height() < window(&grown).height() - 200.0);
        assert!(visible_events(&shrunk) < visible_events(&grown));

        let regrown = resize(&ctx, &mut app, 160.0);
        assert!(window(&regrown).height() > window(&shrunk).height() + 120.0);
        app.build_status_open = false;
        draw(&ctx, &mut app, Vec::new());
        app.build_status_open = true;
        let reopened = settle(&ctx, &mut app);
        capture::write(&ctx, &reopened, &format!("{name}-reopened"));
        assert!((window(&reopened).height() - window(&regrown).height()).abs() < 2.0);
        assert_eq!(app.recipe, recipe);
        measurements.push(serde_json::json!({
            "phase": name,
            "heights": [initial_height, window(&grown).height(), window(&shrunk).height(), window(&reopened).height()],
            "visible_events": [initial_events, visible_events(&grown), visible_events(&shrunk), visible_events(&reopened)],
            "recipe_unchanged": true,
        }));
    }
    if let Some(directory) = crate::test_support::artifacts("build-window") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("resize.json"),
            serde_json::to_vec_pretty(&measurements).unwrap(),
        )
        .unwrap();
    }
}
