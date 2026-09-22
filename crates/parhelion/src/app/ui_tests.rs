//! Headless UI contract/layout checks. These do not open a window or mutate installed content.
use super::*;

mod accessibility;
mod added_sockets;
mod authoring_safety;
mod branding;
mod build_flow;
mod build_selection;
#[cfg(feature = "d2-model-importer")]
mod importer;
mod library;
mod operation_lock;
mod ornaments;
mod preferences;
mod recipe_editing;
mod reports;
mod runtime_editing;
mod runtime_layout;
mod socket_account_updates;
mod weapon_layout;

#[test]
fn runtime_badge_controls_follow_dawn_and_sunrise_without_rewriting_the_recipe() {
    use crate::branding::Branding;
    let mut editor = crate::presentation::ui::Editor::default();
    let mut draft = crate::WeaponRecipeOverrides::default();
    let original = draft.clone();
    for branding in [Branding::Dawn, Branding::Sunrise] {
        editor.set_branding(branding);
        let (output, overflow) = render(460.0, |ui| {
            editor.draw_badge(ui, &mut draft, &[]);
            editor.draw_corner(ui, &mut draft);
        });
        assert_eq!(overflow, 0.0);
        let labels = text(&output);
        assert!(labels.contains(&format!("Include in {} Badge", branding.name())));
        assert_eq!(draft, original);
    }
}

fn field(kind: WeaponRuntimeValueKind, value: WeaponRuntimeValue) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: 0xB176_70ED,
            resource_index: 0,
            root: sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind::ComponentDefinition,
            root_schema: 0x8080_388F,
            path: Vec::new(),
            type_handle: 0x8080_2F16,
            value_offset: 0x48,
            byte_size: kind.byte_size(),
        },
        owner_offset: 0x100,
        name: "Runtime test field".into(),
        path_label: "Component / Runtime test field".into(),
        kind, value,
        source: WeaponRuntimeFieldSource::GeneratedSchema,
        generated_kind: None,
        name_inferred: false,
    }
}

fn render(width: f32, mut draw: impl FnMut(&mut egui::Ui)) -> (egui::FullOutput, f32) {
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
    }
    (output, overflow)
}

fn text(output: &egui::FullOutput) -> String {
    fn append(shape: &egui::Shape, result: &mut String) {
        match shape {
            egui::Shape::Text(value) => {
                result.push_str(&value.galley.job.text);
                result.push('\n');
            }
            egui::Shape::Vec(values) => {
                for value in values {
                    append(value, result);
                }
            }
            _ => {}
        }
    }
    let mut result = String::new();
    for shape in &output.shapes {
        append(&shape.shape, &mut result);
    }
    result
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
