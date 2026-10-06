//! Headless UI contract/layout checks. These do not open a window or mutate installed content.
use super::*;
use crate::test_support::driver::painted_text as text;

mod accessibility;
mod added_sockets;
mod authoring_safety;
mod branding;
mod build_check;
mod build_selection;
#[cfg(feature = "d2-model-importer")]
mod importer;
mod library;
mod operation_lock;
mod ornaments;
mod part_rows;
mod preferences;
mod recipe_editing;
mod runtime_editing;
mod tour;
mod weapon_layout;

fn field(kind: WeaponRuntimeValueKind, value: WeaponRuntimeValue) -> WeaponRuntimeField {
    crate::test_support::runtime_field("Runtime test field", kind, value)
}

fn render(width: f32, draw: impl FnMut(&mut egui::Ui)) -> (egui::FullOutput, f32) {
    render_with_capture(width, None, draw)
}

fn render_with_capture(
    width: f32,
    capture_name: Option<&str>,
    mut draw: impl FnMut(&mut egui::Ui),
) -> (egui::FullOutput, f32) {
    let ctx = egui::Context::default();
    let mut overflow = 0.0_f32;
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    let right = ui.max_rect().right();
                    draw(ui);
                    overflow = (ui.min_rect().right() - right).max(0.0);
                });
            },
        );
        if capture_name.is_some() {
            custom_perks::workbench::tests::capture::record(&output);
        }
    }
    if let Some(name) = capture_name {
        custom_perks::workbench::tests::capture::write(&ctx, &output, name);
    }
    (output, overflow)
}

fn text_origin(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    text_origins(output, label)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("Missing rendered label: {label}"))
}

fn text_origins(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn find(shape: &egui::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Text(value) if value.galley.job.text == label => positions.push(value.pos),
            egui::Shape::Vec(values) => {
                for value in values {
                    find(value, label, positions);
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        find(&shape.shape, label, &mut positions);
    }
    positions
}

fn assert_private_window_survives_tab_changes(app: &mut PackageAuthoringApp) {
    let before = app.recipe.clone();
    app.perk_request = Some(custom_perks::workbench::Request::EditChoice {
        socket: 0,
        choice: 0,
    });
    for page in WorkbenchPage::ALL {
        app.workbench_page = page;
        let (output, _) = render(1320.0, |ui| app.draw_perk_workbench(ui.ctx()));
        assert!(
            text(&output).contains("Custom Perk Workbench"),
            "window missing on {page:?}"
        );
        assert!(text(&output).contains("Apply to Weapon"));
        // The weapon's own sockets sit beside the editor under this header on every page.
        assert!(text(&output).contains("Current Weapon Perks"));
        assert!(app.perk_workbench.open);
        assert_eq!(
            app.recipe, before,
            "opening a private window must be read-only"
        );
    }
    app.perk_request = None;
    app.workbench_page = WorkbenchPage::Weapon;
}
