//! Runtime constraints and destination uncertainty, separate from compiler support.
use super::{PerkRecipe, SANDBOX_PERK_CAPACITY};
use crate::{ItemKind, WeaponSandboxPerkRuntimeRecipe};
use serde::Serialize;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{self, Program, Trigger},
};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub code: &'static str,
    pub effect: Option<usize>,
    pub group: Option<usize>,
    pub blocking: bool,
    pub message: String,
}

pub(crate) fn check(recipe: &PerkRecipe, destination: ItemKind) -> Vec<Diagnostic> {
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
    let mut overrides = std::collections::BTreeMap::new();
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
        if !destination.is_weapon()
            && (matches!(
                program.trigger,
                Trigger::Drawn | Trigger::WeaponKill | Trigger::PrecisionKill
            ) || decoded
                .conditions()
                .iter()
                .any(|node| matches!(node.kind, 13 | 16..=19)))
        {
            add(
                "weapon_context",
                None,
                false,
                format!(
                    "This behavior expects a weapon event. Its event routing on {} has not been verified.",
                    destination.label()
                ),
            );
        }
        inspect_behaviors(&decoded, index, &mut overrides, &mut add);
    }
    issues
}

type Overrides = std::collections::BTreeMap<(u8, u8, u8), (usize, Vec<u8>)>;

fn override_key(kind: u8, bytes: &[u8]) -> (u8, u8, u8) {
    // Override Host Key has separate target and interface selectors. Other overrides in
    // this check address the host or potentially overlapping ability slots. In particular,
    // Set Host Mode and Two Ability State Overrides store replacement values at +2.
    match kind {
        35 => (
            kind,
            bytes.get(2).copied().unwrap_or_default(),
            bytes.get(3).copied().unwrap_or_default(),
        ),
        _ => (kind, 0, 0),
    }
}

fn inspect_behaviors(
    decoded: &action::DecodedAction,
    index: usize,
    overrides: &mut Overrides,
    add: &mut impl FnMut(&'static str, Option<usize>, bool, String),
) {
    for (group, behavior) in decoded.groups.iter().enumerate() {
        let event = !behavior.activation.is_empty()
            && !behavior
                .activation
                .iter()
                .any(|node| matches!(node.kind, 0 | 1 | 14 | 16));
        if event && behavior.removal.is_empty() {
            add("no_ending", Some(group), false,
                "This event starts a behavior with no ending. It cannot start again until removed. Set a duration or an immediate ending.".into());
        }
        if group > 0 && event {
            add("secondary_event", Some(group), false,
                "An event-started secondary behavior has not fired in the verified runtime. Move this behavior to its own effect and keep it within the four-effect budget.".into());
        }
        for node in &behavior.effects {
            if node.kind == 8 && node.native.get(3).is_some_and(|state| *state > 2) {
                add("invalid_state", Some(group), true,
                    "Component Value Adjustment skips state selectors above 2. Choose Any, Inactive, or Active.".into());
            }
            if matches!(node.kind, 7 | 8 | 10 | 11 | 24..=26 | 35 | 48 | 53) {
                add(
                    "required_component",
                    Some(group),
                    false,
                    format!(
                        "{} requires its selected runtime component. The catalog does not prove that this destination supplies it.",
                        node.name()
                    ),
                );
            }
            if matches!(node.kind, 1 | 2 | 4)
                && node
                    .native
                    .get(2)
                    .is_some_and(|target| matches!(target, 2 | 3))
                && !event
            {
                add("event_target", Some(group), false,
                    "This action selects an event object, but the activation has no verified event object. Test the target or choose the host or owning player.".into());
            }
            if node.kind == 1 && node.native.get(2).is_some_and(|target| *target > 3) {
                add("unknown_target", Some(group), false,
                    "The selected entity target has no mapped meaning. Its native value is preserved.".into());
            }
            if matches!(node.kind, 6 | 18 | 22 | 25 | 26 | 28 | 29 | 35) {
                let key = override_key(node.kind, &node.native);
                if let Some(previous) = overrides.insert(key, (index, node.native.clone()))
                    && previous.1 != node.native
                {
                    add(
                        "overlapping_override",
                        Some(group),
                        false,
                        format!(
                            "{} may override the same component as effect {}. If both behaviors run together, check the targets, resulting value, and cleanup order in gameplay.",
                            node.name(),
                            previous.0 + 1
                        ),
                    );
                }
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
