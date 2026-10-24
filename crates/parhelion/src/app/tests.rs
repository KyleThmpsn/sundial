//! The app's own tests: unit checks of its state, and headless UI contract and layout
//! checks, which open no window and change no installed content.
use super::*;
use crate::test_support::driver::painted_text as text;

mod accessibility;
#[cfg(feature = "d2-model-importer")]
mod activities;
mod added_sockets;
mod artwork;
mod authoring_safety;
mod barrel_controls;
mod build_selection;
#[cfg(feature = "d2-model-importer")]
mod imported_appearance;
mod library;
mod recipe_editing;
mod runtime_editing;

#[test]
fn exporting_the_open_recipe_updates_its_save_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.export-regression").unwrap();
    let path = library.save_new(&recipe).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        recipe: recipe.clone(),
        recipe_baseline: recipe,
        recipe_path: Some(path.clone()),
        ..Default::default()
    };
    app.recipe.name = "Exported draft".into();
    app.recipe_dirty = true;
    app.export_recipe_to(&path).unwrap();
    assert_eq!(app.recipe_baseline, app.recipe);
    assert!(!app.recipe_dirty);
    app.recipe.name = "Next edit".into();
    app.save_library_recipe();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap().name, "Next edit");
    let other = app
        .recipe_library
        .as_ref()
        .unwrap()
        .root()
        .join("duplicate.json");
    assert!(app.export_recipe_to(&other).is_err());
    assert!(!other.exists());
    let external = directory.path().join("export.json");
    app.export_recipe_to(&external).unwrap();
    assert_eq!(WeaponRecipe::load_json(&external).unwrap(), app.recipe);
}

#[test]
fn removing_a_socket_choice_keeps_native_metadata_aligned() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        choices: [1, 2, 3].into_iter().map(HexHash::new).collect(),
        choice_weight_bits: vec![10, 20, 30],
        choice_conditions: [1, 2, 3]
            .into_iter()
            .map(|operand| {
                vec![WeaponNumericInstructionRecipe {
                    opcode: 10,
                    operand,
                }]
            })
            .collect(),
        ..WeaponSocketColumnRecipe::default()
    })];

    set_recipe_socket_column(&mut recipe, 1, 0, &[1, 2, 3], vec![1, 3], Some(1));

    let column = recipe.overrides.socket_columns[0].as_ref().unwrap();
    assert_eq!(column.choices, vec![HexHash::new(1), HexHash::new(3)]);
    assert_eq!(column.choice_weight_bits, vec![10, 30]);
    assert_eq!(
        column
            .choice_conditions
            .iter()
            .map(|program| program[0].operand)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
}

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
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1600.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
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
