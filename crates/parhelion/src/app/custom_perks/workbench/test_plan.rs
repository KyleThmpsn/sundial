//! In-game verification checklists derived from an authored perk.
use super::*;
use crate::ItemKind;
use sundial::package_authoring::sandbox_perk::program::{KeyCatalog, Trigger};

/// The item the checklist installs, as it reads mid-sentence.
const fn item_phrase(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon => "a weapon",
        ItemKind::Armor => "an armor piece",
        ItemKind::Sparrow => "a Sparrow",
        ItemKind::Ship => "a ship",
        ItemKind::GhostShell => "a Ghost Shell",
        ItemKind::Shader => "a shader",
        ItemKind::Subclass => "a subclass",
    }
}

/// `kind` is the open recipe's, since the perk goes wherever the reader applies it.
pub(super) fn render(
    recipe: &PerkRecipe,
    kind: ItemKind,
    stock_names: &BTreeMap<u16, String>,
    keys: Option<&KeyCatalog>,
    asset_labels: &BTreeMap<u32, String>,
) -> String {
    let name = if recipe.name.trim().is_empty() {
        "Untitled Perk"
    } else {
        recipe.name.trim()
    };
    let item = item_phrase(kind);
    let noun = kind.noun();
    let mut lines = vec![
        format!("{name} In-Game Test Plan"),
        String::new(),
        "Setup".into(),
        format!(
            "- [ ] Install {item} carrying this private perk and reacquire it from Collections."
        ),
        if kind.is_weapon() {
            "- [ ] Record the weapon's damage type, ammunition, magazine and relevant baseline behavior.".into()
        } else {
            format!("- [ ] Record the {noun}'s stats and relevant baseline behavior.")
        },
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
        // An effect authored as native lists leaves the guided trigger and timers at their
        // defaults, so its checks come from the lists themselves.
        let native = effect.program.as_ref().and_then(|program| {
            let bytes = program.native.as_ref()?.graph.emit().ok()?;
            sundial::package_authoring::sandbox_perk::action::decode(&bytes).ok()
        });
        if let Some(decoded) = native {
            add_native_checks(
                &mut lines,
                &guidance::native_summary(&decoded, Some(asset_labels)),
            );
        } else if let Some(program) = &effect.program {
            lines.push(format!(
                "- [ ] Trigger: {}",
                trigger_instruction(program.trigger, kind)
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

    lines.push(String::new());
    lines.push("Isolation and Persistence".into());
    if kind.is_weapon() {
        lines.extend([
            "- [ ] Confirm the corresponding stock perk on an unmodified weapon is unchanged.".into(),
            "- [ ] Holster and redraw the weapon. Confirm retained state resets or persists as authored.".into(),
            "- [ ] Swap weapons and swap back. Confirm no effect leaks to the other weapon.".into(),
        ]);
    } else {
        lines.extend([
            "- [ ] Confirm the corresponding stock perk on unmodified gear is unchanged.".into(),
            format!("- [ ] Unequip and re-equip the {noun}. Confirm retained state resets or persists as authored."),
        ]);
    }
    lines.extend([
        "- [ ] Die and respawn. Confirm counters, spawned entities and overrides return to the intended state.".into(),
        format!("- [ ] Return to orbit, load another activity and reacquire the {noun} from Collections."),
        format!("- [ ] Relaunch the game and confirm the {noun}, perk text and behavior still match this recipe."),
    ]);
    lines.join("\n")
}

fn trigger_instruction(trigger: Trigger, kind: ItemKind) -> String {
    let weapon_only = matches!(
        trigger,
        Trigger::Drawn | Trigger::WeaponKill | Trigger::PrecisionKill
    );
    if weapon_only && !kind.is_weapon() {
        return format!(
            "This trigger belongs to weapons. Confirm whether it fires from {}.",
            item_phrase(kind)
        );
    }
    match trigger {
        Trigger::Always => "Equip the perk and observe its always-active behavior.".into(),
        Trigger::Equipped => {
            format!(
                "Equip the {}, then unequip it to test cleanup.",
                kind.noun()
            )
        }
        Trigger::Drawn => "Draw the weapon, then holster it to test cleanup.".into(),
        Trigger::WeaponKill => "Get a kill with this weapon.".into(),
        Trigger::PrecisionKill => "Get a precision kill with this weapon.".into(),
        Trigger::MeleeKill => "Get a melee kill while this perk is equipped.".into(),
        Trigger::GrenadeKill => "Get a grenade kill while this perk is equipped.".into(),
        Trigger::AnyKill => "Get a credited kill from more than one damage source.".into(),
        Trigger::Native => "Satisfy the native trigger shown in the Workbench reading.".into(),
    }
}

/// Each behavior's trigger, what it does, and how it ends and fires again, as its lists read.
fn add_native_checks(
    lines: &mut Vec<String>,
    summary: &sundial::package_authoring::sandbox_perk::action::ActionSummary,
) {
    lines.push(format!("- [ ] Expected behavior: {}", summary.render()));
    let named = summary.groups.len() > 1;
    for group in &summary.groups {
        let prefix = if named {
            format!("{}: ", group.label)
        } else {
            String::new()
        };
        if let Some(trigger) = group.activation.first() {
            lines.push(format!("- [ ] {prefix}Trigger: {}.", trigger.text));
        }
        let cooldown = lone(&group.rearm).filter(|text| text.starts_with("After "));
        match lone(&group.removal) {
            Some("At once") if cooldown.is_none() => lines.push(format!(
                "- [ ] {prefix}Repeat: trigger it twice in quick succession. It fires both times."
            )),
            Some("At once") => {}
            Some(text) if text.starts_with("After ") => lines.push(format!(
                "- [ ] {prefix}Duration: confirm the effect ends {}.",
                text.to_lowercase()
            )),
            _ if group.removal.is_empty() => {}
            _ => lines.push(format!(
                "- [ ] {prefix}End condition: satisfy each Ends When path and confirm cleanup."
            )),
        }
        match cooldown {
            Some(text) => lines.push(format!(
                "- [ ] {prefix}Cooldown: confirm it fires again only {}.",
                text.to_lowercase()
            )),
            None if !group.rearm.is_empty() => lines.push(format!(
                "- [ ] {prefix}Reactivation: satisfy each reactivation condition and confirm it fires again."
            )),
            None => {}
        }
    }
}

/// The text of a list's only condition. A lone timer reads "After 5 s", and a lone Always
/// ending "At once".
fn lone(list: &[sundial::package_authoring::sandbox_perk::action::SummaryLine]) -> Option<&str> {
    match list {
        [line] if line.depth == 0 => Some(line.text.as_str()),
        _ => None,
    }
}

fn add_lifetime_checks(
    lines: &mut Vec<String>,
    program: &sundial::package_authoring::sandbox_perk::program::Program,
) {
    if program.duration_ms != 0 {
        lines.push(format!(
            "- [ ] Duration: confirm the effect ends after about {}.",
            seconds(program.duration_ms)
        ));
    } else if program.native_removal.is_some()
        || program.removal_key.is_some()
        || !program.alternative_removals.is_empty()
    {
        lines.push(
            "- [ ] End condition: satisfy each end condition on the card and confirm cleanup."
                .into(),
        );
    }
    if program.cooldown_ms != 0 {
        let purpose = if program.trigger == Trigger::Always {
            "repeat interval"
        } else {
            "cooldown"
        };
        lines.push(format!(
            "- [ ] Reactivation: confirm the {purpose} is about {} and cannot fire early.",
            seconds(program.cooldown_ms)
        ));
    } else if program.native_rearm.is_some() || !program.alternative_rearms.is_empty() {
        lines.push(
            "- [ ] Reactivation: satisfy each reactivation condition on the card and confirm the effect can start again.".into(),
        );
    }
    if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        lines.push(format!(
            "- [ ] Chance: repeat the trigger enough times to sanity check the authored {}% activation rate.",
            f32::from(program.chance_permyriad) / 100.0
        ));
    }
}

/// "1 second", "5 seconds" or "2.5 seconds".
fn seconds(milliseconds: u32) -> String {
    let seconds = milliseconds as f32 / 1000.0;
    let text = format!("{seconds:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "1" {
        "1 second".to_owned()
    } else {
        format!("{text} seconds")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation;

    #[test]
    fn stock_effect_plan_uses_its_name_and_activation_override() {
        let mut recipe = PerkRecipe::new();
        let mut effect = PerkRecipe::effect(77);
        effect.activation = Some(PerkActivation::GrenadeKill);
        recipe.effects.push(effect);
        let names = BTreeMap::from([(77, "Borrowed Behavior".into())]);

        let plan = render(&recipe, ItemKind::Weapon, &names, None, &BTreeMap::new());
        assert!(plan.contains("Borrowed Behavior"));
        assert!(plan.contains("Grenade Kill"));
    }
}
