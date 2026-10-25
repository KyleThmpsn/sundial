use super::*;
use sundial::package_authoring::sandbox_perk::program::{
    Action, Asset, NativeProgram, Position, Program, Trigger,
};

mod checker;

fn setup() -> (egui::Context, Workbench) {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let mut recipe = PerkRecipe::new();
    recipe.name = "Effect Organization".into();
    let mut spawn = program::new_effect(1178);
    let program = spawn.program.as_mut().unwrap();
    program.name = "Spawn on Kill".into();
    program.trigger = Trigger::WeaponKill;
    program.actions.push(Action::Spawn {
        asset: Asset::default(),
        position: Position::Event,
    });
    let mut native = program::new_effect(1179);
    let program = native.program.as_mut().unwrap();
    program.name = "Additional Conditions".into();
    program.native = Some(NativeProgram::empty());
    recipe.effects = vec![spawn, native, PerkRecipe::effect(421)];
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
