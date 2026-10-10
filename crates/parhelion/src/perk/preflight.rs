//! Runtime constraints that keep a perk from working as authored, separate from compiler support.
use super::{PerkRecipe, SANDBOX_PERK_CAPACITY};
use crate::WeaponSandboxPerkRuntimeRecipe;
use serde::Serialize;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{self, Program},
};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub code: &'static str,
    pub effect: Option<usize>,
    pub group: Option<usize>,
    pub blocking: bool,
    pub message: String,
}

/// What keeps a perk from working as authored. Each issue is a known failure, never a doubt the
/// catalog cannot settle.
pub(crate) fn check(recipe: &PerkRecipe) -> Vec<Diagnostic> {
    let mut issues = Vec::new();
    if let Err(message) = recipe.validate() {
        issues.push(Diagnostic {
            code: "structure",
            effect: None,
            group: None,
            blocking: true,
            message,
        });
    }
    for (index, effect) in recipe.effects.iter().enumerate() {
        let mut add = |code, group, blocking, message| {
            issues.push(Diagnostic {
                code,
                effect: Some(index),
                group,
                blocking,
                message,
            })
        };
        if index >= SANDBOX_PERK_CAPACITY {
            add(
                "inactive_effect",
                None,
                false,
                format!(
                    "This effect is inactive. The runtime reads only the first {SANDBOX_PERK_CAPACITY} effects. Move it earlier, remove an effect, or consolidate compatible actions."
                ),
            );
        }
        let Some(program) = &effect.program else {
            continue;
        };
        let decoded = match decoded(program) {
            Ok(decoded) => decoded,
            Err(message) => {
                add("structure", None, true, message);
                continue;
            }
        };
        inspect_behaviors(&decoded, &mut add);
    }
    issues
}

fn inspect_behaviors(
    decoded: &action::DecodedAction,
    add: &mut impl FnMut(&'static str, Option<usize>, bool, String),
) {
    for (group, behavior) in decoded.groups.iter().enumerate() {
        let event = !behavior.activation.is_empty()
            && !behavior
                .activation
                .iter()
                .any(|node| matches!(node.kind, 0 | 1 | 14 | 16));
        // Always and After a Delay start a behavior with no event, so nothing is the event's.
        let no_event = behavior
            .activation
            .iter()
            .all(|node| matches!(node.kind, 0 | 1));
        if event && behavior.removal.is_empty() {
            add("no_ending", Some(group), false,
                "This event starts a behavior with no ending. It cannot start again until removed. Set a duration or an immediate ending.".into());
        }
        // An event-started extra behavior never fired in game, and no stock perk has one.
        if group > 0 && event {
            add("secondary_event", Some(group), false,
                "Only an effect's first behavior starts on an event. Move this behavior to its own effect, within the four-effect budget.".into());
        }
        for node in &behavior.effects {
            if node.kind == 8 && node.native.get(3).is_some_and(|state| *state > 2) {
                add("invalid_state", Some(group), true,
                    "Component Value Adjustment skips state selectors above 2. Choose Any, Inactive, or Active.".into());
            }
            if matches!(node.kind, 1 | 2 | 4)
                && node
                    .native
                    .get(2)
                    .is_some_and(|target| matches!(target, 2 | 3))
                && no_event
            {
                add("event_target", Some(group), false,
                    "This action targets the event's object, but its trigger has no event. Choose the host or the owning player.".into());
            }
        }
    }
}

pub(crate) fn decoded(program: &Program) -> Result<action::DecodedAction, String> {
    action::decode(&program::native_draft(program)?.graph.emit()?)
}

/// Only merge actions sharing a deterministic lifetime. Independent events and state stay
/// separate. A native program must first prove a lossless conversion into that same shape.
fn mergeable(effect: &WeaponSandboxPerkRuntimeRecipe) -> Result<Program, String> {
    if effect.activation.is_some()
        || !effect.runtime_values.is_empty()
        || !effect.action_float_values.is_empty()
        || !effect.projectiles.is_empty()
    {
        return Err(
            "This effect has private runtime edits that must keep their own effect.".into(),
        );
    }
    let original = effect
        .program
        .as_ref()
        .ok_or("Convert the stock effect before consolidating it.")?;
    if !original.ability_tunings.is_empty()
        || !original.ability_inputs.is_empty()
        || !original.native_asset_patches.is_empty()
        || !original.imported_assets.is_empty()
        || original.assets().any(|asset| {
            *asset
                != program::Asset {
                    graph: asset.graph,
                    ..Default::default()
                }
        })
    {
        return Err(
            "This effect has component or resource edits that must keep their own effect.".into(),
        );
    }
    let program = if original.native.is_some() {
        let payload = program::native_draft(original)?.graph.emit()?;
        let decoded = action::decode(&payload)?;
        let recovered = program::decompile::decompile(&decoded, &original.name, |tag| tag)
            .map_err(|e| e.to_string())?;
        let compiled = program::native_draft(&recovered)?.graph.emit()?;
        if !program::decompile::native_fidelity(&payload, &compiled)?.is_empty() {
            return Err(
                "This native configuration cannot be consolidated without changing its data."
                    .into(),
            );
        }
        recovered
    } else {
        original.clone()
    };
    if program.policy.is_some()
        || !program.auxiliary.is_empty()
        || !program.additional_groups.is_empty()
        || program.chance_permyriad != 10_000
    {
        return Err(
            "Consolidation requires one behavior, the default policy, and a deterministic trigger."
                .into(),
        );
    }
    let decoded = decoded(&program)?;
    if decoded
        .conditions()
        .iter()
        .any(|node| matches!(node.kind, 26 | 31 | 35))
        || decoded
            .effects()
            .any(|node| !matches!(node.kind, 5 | 7 | 8 | 10 | 11 | 14..=16 | 20 | 24 | 33 | 35))
    {
        return Err("These actions or conditions share runtime state or have unverified consolidation behavior. Keep them in separate effects.".into());
    }
    Ok(program)
}

pub(crate) fn consolidate(
    recipe: &mut PerkRecipe,
    target: usize,
    source: usize,
) -> Result<(), String> {
    if target == source {
        return Err("Choose two different effects.".into());
    }
    let effects = &recipe.effects;
    let mut left = mergeable(
        effects
            .get(target)
            .ok_or("The destination effect no longer exists.")?,
    )?;
    let mut right = mergeable(
        effects
            .get(source)
            .ok_or("The source effect no longer exists.")?,
    )?;
    let left_actions = std::mem::take(&mut left.actions);
    let right_actions = std::mem::take(&mut right.actions);
    right.name.clone_from(&left.name);
    if left != right {
        return Err("These effects have different triggers, endings, or cooldowns. Keep their lifetimes separate.".into());
    }
    left.actions = left_actions;
    left.actions.extend(right_actions);
    left.validate()?;
    let mut changed = recipe.clone();
    let old = changed.effects[source].source_perk_index;
    let kept = changed.effects[target].source_perk_index;
    changed.effects[target].program = Some(left);
    changed.effects.remove(source);
    for provenance in &mut changed.sources {
        if provenance.effect == old {
            provenance.effect = kept;
        }
    }
    changed.validate()?;
    *recipe = changed;
    Ok(())
}
