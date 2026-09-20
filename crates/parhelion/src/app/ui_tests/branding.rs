use super::*;
use crate::branding::Branding;

#[test]
fn runtime_branding_survives_recipe_and_catalog_resets() {
    for branding in [Branding::Dawn, Branding::Sunrise] {
        let mut app = PackageAuthoringApp::default();
        app.presentation_editor.set_branding(branding);
        let original = app.recipe.clone();
        for reset in [
            PackageAuthoringApp::clear_presentation_picker_queries,
            PackageAuthoringApp::clear_dependent_picker_queries,
            PackageAuthoringApp::drop_loaded_catalog,
        ] {
            reset(&mut app);
            assert_eq!(app.presentation_editor.branding(), branding);
            let (output, overflow) = render(480.0, |ui| {
                app.presentation_editor
                    .draw_corner(ui, &mut app.recipe.overrides);
                app.presentation_editor
                    .draw_badge(ui, &mut app.recipe.overrides, &[]);
            });
            let labels = text(&output);
            let runtime = if branding == Branding::Dawn {
                "Dawn"
            } else {
                "Sunrise"
            };
            assert!(
                labels.contains(&format!("Use {runtime} Watermark")),
                "{labels}"
            );
            assert!(
                labels.contains(&format!("Include in {} Badge", branding.name())),
                "{labels}"
            );
            assert!(overflow <= 1.0, "{overflow}");
            assert_eq!(app.recipe, original);
        }
    }
}

#[test]
fn reset_uploads_the_active_runtime_watermark_not_the_default_runtime() {
    let mut app = PackageAuthoringApp::default();
    for branding in [Branding::Dawn, Branding::Sunrise, Branding::Dawn] {
        app.presentation_editor.set_branding(branding);
        app.drop_loaded_catalog();
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.presentation_editor
                    .draw_corner(ui, &mut app.recipe.overrides);
            });
        });
        let expected = branding.watermark().unwrap();
        let expected = egui::ColorImage::from_rgba_unmultiplied(
            [expected.width() as usize, expected.height() as usize],
            expected.as_raw(),
        );
        assert!(
            output.textures_delta.set.iter().any(|(_, delta)| {
                matches!(&delta.image, egui::ImageData::Color(image) if image.as_ref() == &expected)
            }),
            "{branding:?} watermark pixels were not uploaded"
        );
    }
}
