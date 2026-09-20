//! In-game verification checklists derived from an authored perk.
use super::*;
use sundial::package_authoring::sandbox_perk::program::{KeyCatalog, Trigger};

pub(super) fn render(
    recipe: &PerkRecipe,
    stock_names: &BTreeMap<u16, String>,
    keys: Option<&KeyCatalog>,
    asset_labels: &BTreeMap<u32, String>,
) -> String {
    let name = if recipe.name.trim().is_empty() {
        "Untitled Perk"
    } else {
        recipe.name.trim()
    };
    let mut lines = vec![
        format!("{name} In-Game Test Plan"),
        String::new(),
        "Setup".into(),
        "- [ ] Install a weapon carrying this private perk and reacquire it from Collections.".into(),
        "- [ ] Record the weapon's damage type, ammunition, magazine and relevant baseline behavior.".into(),
    ];
    if !recipe.description.trim().is_empty() {
        lines.push(format!(
            "- [ ] Expected perk text: {}",
            recipe.description.trim()
        ));
    }

    for (index, effect) in recipe.effects.iter().enumerate() {
        lines.push(String::new());
        let effect_name = effect.program.as_ref().map_or_else(
            || {
                stock_names
                    .get(&effect.source_perk_index)
                    .cloned()
                    .unwrap_or_else(|| format!("Stock Effect {}", effect.source_perk_index))
            },
            |program| program.name.clone(),
        );
        lines.push(format!("Effect {}: {effect_name}", index + 1));
        if let Some(program) = &effect.program {
            lines.push(format!(
                "- [ ] Trigger: {}",
                trigger_instruction(program.trigger)
            ));
            lines.push(format!(
                "- [ ] Expected behavior: {}",
                guidance::summary_with_assets(program, keys, Some(asset_labels))
            ));
            add_lifetime_checks(&mut lines, program);
            if let Some(hint) = program.authoring_hint() {
                lines.push(format!("- [ ] Risk check: {hint}"));
            }
        } else {
            let trigger = effect.activation.map_or_else(
                || "Use the stock effect's normal activation condition.".to_owned(),
                |activation| format!("Activate with {}.", activation.label()),
            );
            lines.push(format!("- [ ] Trigger: {trigger}"));
            lines.push(
                "- [ ] Confirm the copied stock behavior and every edited value or projectile."
                    .into(),
            );
        }
    }

    lines.extend([
        String::new(),
        "Isolation and Persistence".into(),
        "- [ ] Confirm the corresponding stock perk on an unmodified weapon is unchanged.".into(),
        "- [ ] Holster and redraw the weapon. Confirm retained state resets or persists as authored.".into(),
        "- [ ] Swap weapons and swap back. Confirm no effect leaks to the other weapon.".into(),
        "- [ ] Die and respawn. Confirm counters, spawned entities and overrides return to the intended state.".into(),
        "- [ ] Return to orbit, load another activity and reacquire the weapon from Collections.".into(),
        "- [ ] Relaunch the game and confirm the weapon, perk text and behavior still match this recipe.".into(),
    ]);
    lines.join("\n")
}

fn trigger_instruction(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Always => "Equip the perk and observe its always-active behavior.",
        Trigger::Equipped => "Equip the weapon, then unequip it to test cleanup.",
        Trigger::Drawn => "Draw the weapon, then holster it to test cleanup.",
        Trigger::WeaponKill => "Get a kill with this weapon.",
        Trigger::PrecisionKill => "Get a precision kill with this weapon.",
        Trigger::MeleeKill => "Get a melee kill while this perk is equipped.",
        Trigger::GrenadeKill => "Get a grenade kill while this perk is equipped.",
        Trigger::AnyKill => "Get a credited kill from more than one damage source.",
        Trigger::Native => "Satisfy the native trigger shown in the Workbench reading.",
    }
}

fn add_lifetime_checks(
    lines: &mut Vec<String>,
    program: &sundial::package_authoring::sandbox_perk::program::Program,
) {
    if program.duration_ms != 0 {
        lines.push(format!(
            "- [ ] Duration: confirm the effect ends after about {} seconds.",
            seconds(program.duration_ms)
        ));
    } else if program.native_removal.is_some()
        || program.removal_key.is_some()
        || !program.alternative_removals.is_empty()
    {
        lines.push(
            "- [ ] End condition: satisfy each ending path shown in the Workbench and confirm cleanup.".into(),
        );
    }
    if program.cooldown_ms != 0 {
        let purpose = if program.trigger == Trigger::Always {
            "repeat interval"
        } else {
            "cooldown"
        };
        lines.push(format!(
            "- [ ] Rearm: confirm the {purpose} is about {} seconds and cannot fire early.",
            seconds(program.cooldown_ms)
        ));
    } else if program.native_rearm.is_some() || !program.alternative_rearms.is_empty() {
        lines.push(
            "- [ ] Rearm: satisfy each Ready Again path shown in the Workbench and confirm the effect can activate again.".into(),
        );
    }
    if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        lines.push(format!(
            "- [ ] Chance: repeat the trigger enough times to sanity check the authored {}% activation rate.",
            f32::from(program.chance_permyriad) / 100.0
        ));
    }
}

fn seconds(milliseconds: u32) -> String {
    let seconds = milliseconds as f32 / 1000.0;
    if seconds.fract() == 0.0 {
        format!("{seconds:.0}")
    } else {
        format!("{seconds:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation;
    use sundial::package_authoring::sandbox_perk::program::{Action, Program};

    #[test]
    fn authored_program_plan_names_trigger_lifetime_cooldown_and_isolation() {
        let mut recipe = PerkRecipe::new();
        recipe.name = "Smoke Perk".into();
        recipe.description = "Does an unusual thing.".into();
        let mut program = Program {
            name: "Precision Window".into(),
            trigger: Trigger::PrecisionKill,
            duration_ms: 2_500,
            cooldown_ms: 5_000,
            chance_permyriad: 5_000,
            ..Program::default()
        };
        program.actions.push(Action::add_rounds(2));
        recipe.effects.push(WeaponSandboxPerkRuntimeRecipe {
            program: Some(program),
            ..PerkRecipe::effect(421)
        });

        let plan = render(&recipe, &BTreeMap::new(), None, &BTreeMap::new());
        for expected in [
            "Smoke Perk In-Game Test Plan",
            "Get a precision kill",
            "2.50 seconds",
            "5 seconds",
            "50% activation rate",
            "stock perk on an unmodified weapon is unchanged",
            "no effect leaks to the other weapon",
            "Relaunch the game",
        ] {
            assert!(plan.contains(expected), "missing {expected:?} in {plan}");
        }
    }

    #[test]
    fn stock_effect_plan_uses_its_name_and_activation_override() {
        let mut recipe = PerkRecipe::new();
        let mut effect = PerkRecipe::effect(77);
        effect.activation = Some(PerkActivation::GrenadeKill);
        recipe.effects.push(effect);
        let names = BTreeMap::from([(77, "Borrowed Behavior".into())]);

        let plan = render(&recipe, &names, None, &BTreeMap::new());
        assert!(plan.contains("Borrowed Behavior"));
        assert!(plan.contains("Grenade Kill"));
    }
}
