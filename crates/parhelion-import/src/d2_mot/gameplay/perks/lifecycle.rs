//! Controllers whose effects last as long as the weapon holds the perk: active from equip to
//! unequip, or for the whole time the perk is applied. Their supported effects become native
//! actions under the matching typed trigger, which compiles to the same native lifecycle
//! conditions, kind 14 and 15 for equip and unequip.
use super::{Assignment, controller, lower, translate};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use sundial::package_authoring::sandbox_perk::program::{Program, Trigger};

/// The translated programs of one source plug and every source controller or effect left out,
/// each with its reason.
pub struct Translated {
    pub programs: Vec<Program>,
    pub omitted: Vec<Value>,
}

/// Translate every controller assigned to a source plug that has a supported lifetime and at
/// least one supported effect. The rest are recorded in `omitted` rather than approximated.
pub fn translate(assigned: &[Assignment], name: &str) -> Translated {
    let mut programs = Vec::new();
    let mut omitted = Vec::new();
    for assignment in assigned {
        match program(assignment, name) {
            Ok((program, skipped)) => {
                programs.push(program);
                omitted.extend(skipped);
            }
            Err(error) => omitted.push(json!({
                "controller": format!("{:08X}", assignment.action_tag),
                "perk": format!("{:08X}", assignment.perk_hash),
                "reason": format!("{error:#}"),
            })),
        }
    }
    Translated { programs, omitted }
}

fn program(assignment: &Assignment, name: &str) -> Result<(Program, Vec<Value>)> {
    const EMPTY: u32 = 0x811C_9DC5;
    let p = &assignment.controller;
    ensure!(
        p.u64(8)? == 0
            && p.u64(16)? == 0
            && p.u32(64)? == EMPTY
            && p.bytes::<4>(68)? == [0, 2, 0, 0]
            && p.u32(72)? == 0,
        "unsupported source controller metadata"
    );
    let decoded = controller::read(p)?;
    ensure!(
        decoded.auxiliary.is_empty(),
        "source controller HUD reports need their own translation"
    );
    let (trigger, active) = match decoded.states.as_slice() {
        [idle, active] => {
            for state in [idle, active] {
                translate::neutral_state(p, state)?;
            }
            let enter = &idle.transitions[0];
            let exit = &active.transitions[0];
            ensure!(
                idle.key != active.key
                    && enter.destination == active.offset
                    && exit.destination == idle.offset
                    && enter.conditions.len() == 1
                    && exit.conditions.len() == 1
                    && enter.conditions[0].class == 0x8080_30B6
                    && exit.conditions[0].class == 0x8080_30B2,
                "source controller is not an equip and unequip lifecycle"
            );
            // Both must be the plain lifecycle events the typed trigger compiles.
            lower::equipped_condition(p, enter.conditions[0].offset)?;
            lower::equipped_condition(p, exit.conditions[0].offset)?;
            ensure!(
                translate::state_effects(p, idle)?.is_empty(),
                "source idle state has effects"
            );
            (Trigger::Equipped, active)
        }
        [active] => {
            translate::state_fields(p, active)?;
            ensure!(
                active.transitions.is_empty(),
                "a single source state with transitions is not always active"
            );
            (Trigger::Always, active)
        }
        states => bail!("source controller has {} states", states.len()),
    };
    let mut actions = Vec::new();
    let mut omitted = Vec::new();
    for at in translate::state_effects(p, active)? {
        let class = p.u32(at - 4)?;
        let lowered = match class {
            0x8080_2093 => lower::named_property(p, at),
            _ => Err(anyhow::anyhow!(
                "source effect class {class:08X} has no translation here"
            )),
        };
        match lowered {
            Ok(action) => actions.push(action),
            Err(error) => omitted.push(json!({
                "controller": format!("{:08X}", assignment.action_tag),
                "effect_offset": at,
                "effect_class": format!("{class:08X}"),
                "source_kind": p.u8(at)?,
                "reason": format!("{error:#}"),
            })),
        }
    }
    ensure!(
        !actions.is_empty(),
        "source controller has no translated effects: {}",
        serde_json::to_string(&omitted)?
    );
    let program = Program {
        name: name.to_owned(),
        trigger,
        duration_ms: 0,
        cooldown_ms: 0,
        actions,
        ..Program::default()
    };
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok((program, omitted))
}
