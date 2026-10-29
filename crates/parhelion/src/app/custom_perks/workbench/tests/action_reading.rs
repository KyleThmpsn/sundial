//! Persisted private effects must explain their own rows without adopting or changing them.
use super::*;
use crate::test_support::driver::painted_text;
use sundial::package_authoring::sandbox_perk::program::Program;

fn draw(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    program: &mut Program,
    width: f32,
) -> egui::FullOutput {
    let labels = BTreeMap::new();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 1500.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                canvas::draw_effect(
                    ui,
                    canvas::Canvas {
                        name: "Dynamic Modifier Example",
                        backend: canvas::Backend::Program {
                            program,
                            stock: None,
                            description: None,
                            labels: &labels,
                            editing: Some(canvas::Editing { workbench }),
                        },
                        header: None,
                        footer: None,
                        trigger_command: None,
                    },
                    super::super::cards::Card::new("action-reading", 419, 0, 1),
                );
            });
        },
    );
    capture::record(&output);
    output
}

#[test]
fn private_actions_explain_persisted_changes_without_losing_their_resources() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    let fixture: Program =
        serde_json::from_str(include_str!("action_reading/program.json")).unwrap();
    let mut recipe = PerkRecipe::new();
    recipe.name = "Action Readings".into();
    let mut effect = program::new_effect(419);
    effect.program = Some(fixture.clone());
    recipe.effects.push(effect);
    let entry = library.save(&recipe, None).unwrap();
    let mut reopened = Library::read(&entry.path).unwrap().recipe;
    let program = reopened.effects[0].program.as_mut().unwrap();
    let ctx = egui::Context::default();
    let mut workbench = Workbench::offline();
    let mut evidence = Vec::new();
    for (width, name) in [
        (1100.0, "perk-action-changes"),
        (520.0, "perk-action-changes-narrow"),
    ] {
        let mut output = draw(&ctx, &mut workbench, program, width);
        for _ in 0..2 {
            output = draw(&ctx, &mut workbench, program, width);
        }
        let text = painted_text(&output);
        for expected in [
            "Accuracy 5 and Accuracy 6: ×4 at full weight",
            "Damage: ×0.5 at full weight",
            "Time Between Shots and Time Between Shots 2: -0.66667 s at full weight",
            "Charge Time: -0.65933 s at full weight",
            "Projectile Speed and Projectile Speed 2: ×4",
            "Strength follows the effect's counter.",
            "5. Hold to Fire",
        ] {
            assert!(text.contains(expected), "Missing {expected:?}:\n{text}");
        }
        capture::write(&ctx, &output, name);
        evidence.push(serde_json::json!({"width":width,"text":text}));
    }
    assert_eq!(
        *program, fixture,
        "reading must retain the complete program"
    );
    let card = super::super::cards::Card::new("action-reading", 419, 0, 1);
    card.set_expanded(&ctx, false);
    let output = draw(&ctx, &mut workbench, program, 1100.0);
    let text = painted_text(&output);
    assert!(
        text.contains("Accuracy 5"),
        "folded card must identify its changes"
    );
    // A folded card names its first changes and counts the rest.
    assert!(
        text.contains("Always: Scale Accuracy 5 and Accuracy 6, Scale Damage, 3 More"),
        "folded card must account for every change:\n{text}"
    );
    card.set_expanded(&ctx, true);
    let saved = library.save(&reopened, Some(&entry.baseline)).unwrap();
    let persisted = Library::read(&saved.path).unwrap().recipe;
    assert_eq!(persisted.effects[0].program.as_ref(), Some(&fixture));

    // Neither a different instance class nor an unrecognized input gains a guessed meaning.
    let mut unknown = fixture.clone();
    unknown.native_asset_patches[0].patches[0].bytes[4..8]
        .copy_from_slice(&0x8080_3AFE_u32.to_le_bytes());
    unknown.native_asset_patches[3].patches[0].bytes[0x4C] = 200;
    unknown.native_asset_patches[3].patches[0].bytes[0x4A..0x4C]
        .copy_from_slice(&123_u16.to_le_bytes());
    let output = draw(&ctx, &mut workbench, &mut unknown, 1100.0);
    let text = painted_text(&output);
    assert!(text.contains("Component 200 / Input 123"));
    assert!(text.contains("Additional private resource edits"));
    capture::write(&ctx, &output, "perk-action-changes-unknown");
    if let Some(directory) = crate::test_support::artifacts("action-readings") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::copy(&saved.path, directory.join("saved-perk.json")).unwrap();
        std::fs::write(
            directory.join("readback.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "frames":evidence,"unknown":text,"program_preserved":true,
                "limits":"Headless authoring display and persistence. No gameplay verification."
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
