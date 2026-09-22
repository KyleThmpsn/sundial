use super::*;

#[test]
fn picker_rows_emphasize_titles_without_changing_geometry() {
    for dark in [true, false] {
        let ctx = egui::Context::default();
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let family = egui::FontFamily::Name("test-medium".into());
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            family.clone(),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        ctx.style_mut(|style| {
            style.text_styles.insert(
                crate::ui_help::tooltip_title_style(),
                egui::FontId::new(20.0, family.clone()),
            );
        });
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = draw_picker_row(
                    ui,
                    None,
                    CatalogPickerRow {
                        hash: 1,
                        primary: "On Weapon Kill",
                        primary_max_rows: 1,
                        secondary: Some("Actions start on a kill with this weapon."),
                        icon_size: 0.0,
                        row_height: 48.0,
                        selected: false,
                    },
                );
                assert_eq!(response.rect.height(), 48.0);
            });
        });
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        let primary = texts
            .iter()
            .find(|text| text.galley.job.text == "On Weapon Kill")
            .unwrap();
        let secondary = texts
            .iter()
            .find(|text| text.galley.job.text.starts_with("Actions start"))
            .unwrap();
        assert_eq!(primary.galley.job.sections[0].format.font_id.family, family);
        let primary_color = primary.galley.job.sections[0].format.color;
        let secondary_color = secondary.galley.job.sections[0].format.color;
        assert_eq!(secondary_color, primary_color.gamma_multiply(0.75));
        assert_eq!(primary.pos.x, secondary.pos.x);
        assert!(primary.pos.y + primary.galley.size().y <= secondary.pos.y);
    }
}
