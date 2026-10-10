//! An Ability Property's key picker defines a tuning, the program carries it and rekeys its
//! actions when the tuning changes. The captures, when `SUNDIAL_TEST_ARTIFACTS` is set, are
//! the modal as a reader meets it and the action once its key is the tuning.
use super::*;
use sundial::package_authoring::sandbox_perk::program::PropertyOperation;
use sundial::package_authoring::{
    ability_bank::{GRENADE_SLOT, StockParameter, slot_parameters},
    sandbox_perk::program::{AbilityTuning, Action, Program, Trigger},
};

/// Extra Grenade Charge, the stock key the effect starts with.
const EXTRA_GRENADE_CHARGE: u32 = 0xBDA0_ACD6;
/// The native blast radius scalar selected for the tuning workflow.
const BLAST_RADIUS_SCALAR: u32 = 0x99B1_D826;
const SIZE: egui::Vec2 = egui::vec2(1320.0, 900.0);

fn setup() -> (egui::Context, Workbench) {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let mut recipe = PerkRecipe::new();
    recipe.name = "Bigger Grenades".into();
    let mut effect = program::new_effect(1178);
    let program = effect.program.as_mut().unwrap();
    program.name = "Grenade Property".into();
    program.trigger = Trigger::Always;
    program.actions.push(Action::AbilityProperty {
        target: GRENADE_SLOT,
        key: EXTRA_GRENADE_CHARGE,
        option: PropertyOperation::Apply,
    });
    recipe.effects = vec![effect];
    (
        ctx,
        Workbench {
            initialized: true,
            open: true,
            drafts_writable: true,
            page: Page::Effects,
            documents: vec![Document::new(recipe, None)],
            ..Default::default()
        },
    )
}

fn render(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(ctx, workbench, true, size, vec![]);
    }
    output
}

fn click(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2, pos: egui::Pos2) {
    for events in crate::test_support::driver::tap(pos) {
        frame(ctx, workbench, true, size, events);
    }
}

/// Every text on screen with the clip rectangle it is drawn under.
fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
                shape.clip_rect,
            )),
            _ => None,
        })
        .collect()
}

/// Clicks the text `name`, which must be on screen exactly once and inside its clip, since a
/// row scrolled out of view is drawn but takes no click.
fn click_name(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2, name: &str) {
    let output = render(ctx, workbench, size);
    let found = texts(&output)
        .into_iter()
        .filter(|(text, _, _)| text == name)
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "{name} should be on screen once: {found:?}");
    let (_, rect, clip) = &found[0];
    assert!(
        clip.contains(rect.center()),
        "{name} at {rect:?} is clipped to {clip:?}"
    );
    click(ctx, workbench, size, rect.center());
}

fn open_program(workbench: &Workbench) -> &Program {
    workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap()
}

fn stock() -> &'static StockParameter {
    slot_parameters(GRENADE_SLOT)
        .iter()
        .find(|parameter| parameter.hash == BLAST_RADIUS_SCALAR)
        .expect("every grenade bank lists the blast radius scalar")
}

#[test]
fn the_key_picker_defines_a_tuning_and_an_edit_rekeys_the_action() {
    let (ctx, mut workbench) = setup();
    let added = define_a_tuning(&ctx, &mut workbench);
    let edited = edit_the_tuning(&ctx, &mut workbench, &added);
    choose_a_stock_key_again(&ctx, &mut workbench, &edited);
}

/// The key reads by its stock name, its list ends with the row that defines a tuning, and
/// the program defines the tuning at the stock change with the action applying its key.
fn define_a_tuning(ctx: &egui::Context, workbench: &mut Workbench) -> AbilityTuning {
    let size = SIZE;
    let stock = stock();
    click_name(ctx, workbench, size, "Extra Grenade Charge");
    click_name(ctx, workbench, size, "Add Property…");
    let output = render(ctx, workbench, size);
    assert!(label(&output, "New Property").is_some(), "the modal opens");
    if label(&output, "Blast Radius Scalar").is_none() {
        let current = texts(&output)
            .into_iter()
            .find(|(text, _, _)| {
                slot_parameters(GRENADE_SLOT).iter().any(|parameter| {
                    sundial::package_authoring::ability_bank::parameter_label(parameter.hash)
                        .map_or_else(
                            || format!("Parameter 0x{:08X}", parameter.hash),
                            str::to_owned,
                        )
                        == *text
                })
            })
            .expect("current parameter control");
        click(ctx, workbench, size, current.1.center());
        click_name(ctx, workbench, size, "Blast Radius Scalar");
    }
    capture::write(ctx, &output, "ability-tuning-editor");
    click_name(ctx, workbench, size, "Add Property");
    let output = render(ctx, workbench, size);
    capture::write(ctx, &output, "ability-tuning-applied");
    let added = AbilityTuning::new(GRENADE_SLOT, BLAST_RADIUS_SCALAR, stock.applied, stock.add);
    let program = open_program(workbench);
    assert_eq!(program.ability_tunings, vec![added.clone()]);
    let applied = program.ability_properties().unwrap();
    assert!(applied.contains(&(GRENADE_SLOT, added.key)), "{applied:?}");
    assert!(!applied.iter().any(|(_, key)| *key == EXTRA_GRENADE_CHARGE));
    program.validate().unwrap();
    let reading = program::native::tunings::label(&added);
    let output = render(ctx, workbench, size);
    assert!(
        label(&output, &reading).is_some(),
        "the key reads as {reading}"
    );
    added
}

/// Editing the tuning's change gives it a new key, and the action follows it.
fn edit_the_tuning(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    added: &AbilityTuning,
) -> AbilityTuning {
    let size = SIZE;
    let stock = stock();
    // The card offers the edit beside the tuning's reading, without opening the list.
    click_name(ctx, workbench, size, "Edit Property…");
    let output = render(ctx, workbench, size);
    assert!(
        label(&output, "Edit Property").is_some(),
        "the modal opens for the tuning"
    );
    click_name(ctx, workbench, size, if stock.add { "Add" } else { "Set" });
    click_name(ctx, workbench, size, if stock.add { "Set" } else { "Add" });
    click_name(ctx, workbench, size, "Save Property");
    let edited = AbilityTuning::new(GRENADE_SLOT, BLAST_RADIUS_SCALAR, stock.applied, !stock.add);
    assert_ne!(edited.key, added.key);
    let program = open_program(workbench);
    assert_eq!(program.ability_tunings, vec![edited.clone()]);
    let applied = program.ability_properties().unwrap();
    assert!(applied.contains(&(GRENADE_SLOT, edited.key)), "{applied:?}");
    assert!(!applied.iter().any(|(_, key)| *key == added.key));
    program.validate().unwrap();
    edited
}

/// Choosing a stock key again leaves the tuning behind, and the program drops it.
fn choose_a_stock_key_again(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    edited: &AbilityTuning,
) {
    let size = SIZE;
    click_name(
        ctx,
        workbench,
        size,
        &program::native::tunings::label(edited),
    );
    click_name(ctx, workbench, size, "Extra Grenade Charge");
    let program = open_program(workbench);
    assert!(
        program.ability_tunings.is_empty(),
        "{:?}",
        program.ability_tunings
    );
    assert!(
        program
            .ability_properties()
            .unwrap()
            .contains(&(GRENADE_SLOT, EXTRA_GRENADE_CHARGE))
    );
}
