//! Actual shader-page input must change rendered geometry and recover the authored preview.
//! The surrounding workflow then switches gear and compares the built shader's native materials.
use super::*;

pub(super) fn check(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    opening: &egui::ColorImage,
) {
    let output = crate::test_support::artifact_dir("gear").join("zoom");
    fs::create_dir_all(&output).unwrap();
    save_image(opening, &output.join("opening.png"));
    // Native material and skeletal playback may advance between camera actions. Hold its clock
    // so exact restoration compares the same moment, without replacing the page or its renderer.
    sundial::ui::model_preview::pause_source(ctx, true);
    // The earlier dye edits also redraw Icon From Dyes asynchronously. Finish that authored
    // change before comparing recipe state across camera-only input.
    let page = settle_loaded(ctx, app, "Shader materials before camera input");
    let recipe = app.recipe.clone();
    let canvas = accessible(&page, "Model Preview").expect("the shader's model canvas");
    let preview_heading = rect_of(&page, "Preview");

    let enlarged = action(ctx, app, "Zoom In", |image| {
        coverage(image) > coverage(opening)
    });
    save_image(&enlarged, &output.join("zoom-in.png"));
    assert!(coverage(&enlarged) as f64 / coverage(opening) as f64 > 1.1);
    capture::write(ctx, &settle(ctx, app), "shader-preview-zoomed");

    let reduced = action(ctx, app, "Zoom Out", |image| {
        let relative = coverage(image) as f64 / coverage(opening) as f64;
        (0.95..=1.05).contains(&relative)
    });
    assert!(coverage(&reduced) < coverage(&enlarged));
    let baseline = reduced.as_ref();
    save_image(baseline, &output.join("fit.png"));
    let zoomed = action(ctx, app, "Zoom In", |image| {
        differing_pixels(image, &enlarged) == 0
    });
    // The software image rounds a fractional canvas to whole pixels. Move by whole rendered
    // pixels so the surface comparison measures translation without introducing resampling.
    let pan = egui::vec2(
        25.0 * canvas.width() / zoomed.width() as f32,
        15.0 * canvas.height() / zoomed.height() as f32,
    );
    let moved = canvas.center() + pan;
    let drag = vec![
        vec![
            egui::Event::PointerMoved(canvas.center()),
            egui::Event::PointerButton {
                pos: canvas.center(),
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![egui::Event::PointerMoved(moved)],
        vec![egui::Event::PointerButton {
            pos: moved,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    let panned = image_after(ctx, app, "Pan", drag, |image| {
        differing_pixels(image, &zoomed) > 100
    });
    save_image(&panned, &output.join("pan.png"));
    // A pan translates the existing surface instead of changing its viewing angle.
    let translated = translated_matches(&zoomed, &panned, [25, 15]);
    assert!(
        translated > 0.97,
        "pan changed the visible surface: {translated}"
    );
    action(ctx, app, "Fit", |image| {
        differing_pixels(image, baseline) == 0
    });

    let wheel = image_after(
        ctx,
        app,
        "Wheel",
        vec![vec![
            egui::Event::PointerMoved(canvas.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 180.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]],
        |image| coverage(image) > coverage(baseline),
    );
    save_image(&wheel, &output.join("wheel.png"));
    assert_eq!(rect_of(&settle(ctx, app), "Preview"), preview_heading);
    let restored = action(ctx, app, "Fit", |image| {
        differing_pixels(image, baseline) == 0
    });
    save_image(&restored, &output.join("restored.png"));
    assert_eq!(app.recipe, recipe, "camera input must not edit the recipe");
    sundial::ui::model_preview::pause_source(ctx, false);
    capture::write(ctx, &settle(ctx, app), "shader-preview-fit");
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "shader_recipe": recipe,
            "preview_item": app.donor_summaries[0].hash,
            "image_size": baseline.size,
            "fit_pixels": coverage(baseline),
            "opening_to_paused_fit_differing_pixels": differing_pixels(opening, baseline),
            "zoomed_pixels": coverage(&enlarged),
            "wheel_pixels": coverage(&wheel),
            "pan_translation_match": translated,
            "pan_input_points": [pan.x, pan.y],
            "restored_differing_pixels": differing_pixels(baseline, &restored),
            "recipe_unchanged": true,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn coverage(image: &egui::ColorImage) -> usize {
    let background = image.pixels[0];
    image
        .pixels
        .iter()
        .filter(|pixel| {
            pixel
                .to_array()
                .into_iter()
                .zip(background.to_array())
                .any(|(value, background)| value.abs_diff(background) > 4)
        })
        .count()
}

fn translated_matches(a: &egui::ColorImage, b: &egui::ColorImage, delta: [usize; 2]) -> f64 {
    let mut matched = 0;
    let mut compared = 0;
    for y in 0..a.height() - delta[1] {
        for x in 0..a.width() - delta[0] {
            let before = a.pixels[y * a.width() + x];
            if before == a.pixels[0] {
                continue;
            }
            let after = b.pixels[(y + delta[1]) * b.width() + x + delta[0]];
            compared += 1;
            if before
                .to_array()
                .into_iter()
                .zip(after.to_array())
                .all(|(a, b)| a.abs_diff(b) <= 2)
            {
                matched += 1;
            }
        }
    }
    assert!(compared > 100, "no model pixels to compare after panning");
    f64::from(matched) / f64::from(compared)
}

fn action(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    label: &str,
    matches: impl Fn(&egui::ColorImage) -> bool,
) -> Arc<egui::ColorImage> {
    let page = settle(ctx, app);
    let button = accessible(&page, label).unwrap_or_else(|| panic!("missing {label}"));
    image_after(
        ctx,
        app,
        label,
        crate::test_support::driver::tap(button.center()).to_vec(),
        matches,
    )
}

fn image_after(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    label: &str,
    events: Vec<Vec<egui::Event>>,
    matches: impl Fn(&egui::ColorImage) -> bool,
) -> Arc<egui::ColorImage> {
    eprintln!("Shader preview action: {label}");
    let mut latest = None;
    for event in events {
        if let Some(image) = preview_image(&frame(ctx, app, event)) {
            latest = Some(image);
        }
    }
    let start = Instant::now();
    loop {
        let output = frame(ctx, app, Vec::new());
        if let Some(image) = preview_image(&output) {
            latest = Some(image);
        }
        let scrolling = ctx.input(|input| input.smooth_scroll_delta.y.abs() > 0.01);
        if !scrolling
            && let Some(image) = &latest
            && matches(image)
        {
            return image.clone();
        }
        if start.elapsed() >= Duration::from_secs(90) {
            capture::write(ctx, &output, "shader-preview-camera-failed");
            if let Some(image) = latest {
                let directory = PathBuf::from(std::env::var_os("SUNDIAL_TEST_ARTIFACTS").unwrap());
                save_image(&image, &directory.join("gear/camera-failed.png"));
            }
            panic!(
                "{label} produced no matching image. Page text: {:?}",
                texts(&output)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
