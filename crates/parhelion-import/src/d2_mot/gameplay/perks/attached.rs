//! Complete two-state kill controllers with independently lived attachments.
use super::{Assignment, assigned, controller, lower, translate};
use crate::d2_mot::{entity::sequence, payload::Payload, reader::Reader};
use anyhow::{Result, ensure};

pub struct Source {
    pub assignment: Assignment,
    /// Conditions in a transition are alternatives, not an implicit conjunction.
    pub activation: Vec<usize>,
    /// Keep the Float32 timer. Some source delays are not exact milliseconds.
    pub removal: lower::Node,
    pub effects: Vec<Effect>,
}

pub enum Effect {
    Attach(lower::attachment::Attachment),
    Spawn(lower::spawn::Spawn),
}

/// A hit cycle whose stateless modifier is present in every source state.
/// The enclosing native compiler must retain that modifier for the host's whole
/// lifetime and preserve the separate hit-triggered attachment cycle.
pub struct Hosted {
    pub activation: lower::Hit,
    pub removal: lower::Node,
    pub modifiers: Vec<lower::Node>,
    pub attachments: Vec<lower::attachment::Attachment>,
}

fn metadata(p: &Payload, states: &[controller::State], retained: u8) -> Result<()> {
    ensure!(
        p.u64(8)? == 0
            && p.u64(16)? == 0
            && p.u32(64)? == 0x811C9DC5
            && p.bytes::<4>(68)? == [0, 2, retained, 0]
            && p.u32(72)? == 0,
        "unsupported attachment controller metadata"
    );
    for state in states {
        // Modern 100F0E0 selects a transition using its condition array at +10
        // and follows its destination pointer at +08. The name at +00 does not
        // participate in that dispatch. This closed two-state route rejects
        // auxiliary records and other transition metadata before lowering.
        translate::state_metadata(p, state)?;
    }
    Ok(())
}

fn timer_exit(p: &Payload, removal: &controller::Condition) -> Result<lower::Node> {
    ensure!(
        removal.class == 0x80803060
            && removal.children.is_empty()
            && p.f32(removal.offset)? == 1.0
            && p.u8(removal.offset + 6)? == 0
            && p.f32(removal.offset + 8)?.is_finite()
            && p.f32(removal.offset + 8)? > 0.0,
        "attachment exit is not an unconditional positive timer"
    );
    lower::timer_condition(p, removal.offset)
}

pub fn extract(reader: &mut Reader, plug: u32) -> Result<Source> {
    let mut assignments = assigned(reader, plug)?;
    ensure!(
        assignments.len() == 1,
        "attachment import requires one complete controller"
    );
    let assignment = assignments.remove(0);
    let p = &assignment.controller;
    let parsed = controller::read(p)?;
    ensure!(
        parsed.auxiliary.is_empty(),
        "attachment controller auxiliary records require translation"
    );
    let [idle, active] = parsed.states.as_slice() else {
        anyhow::bail!("attachment controller requires two states");
    };
    metadata(p, &parsed.states, 1)?;
    ensure!(
        idle.transitions[0].destination == active.offset
            && active.transitions[0].destination == idle.offset
            && translate::state_effects(p, idle)?.is_empty(),
        "attachment controller is not an idle and active cycle"
    );
    let activation = idle.transitions[0]
        .conditions
        .iter()
        .map(|condition| {
            ensure!(
                condition.class == 0x8080B7B2 && condition.children.is_empty(),
                "attachment activation is not a bounded kill predicate"
            );
            lower::kill_condition(p, condition.offset)?;
            Ok(condition.offset)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !activation.is_empty(),
        "attachment controller has no activation predicate"
    );
    let [removal] = active.transitions[0].conditions.as_slice() else {
        anyhow::bail!("attachment controller requires one timer exit");
    };
    let removal = timer_exit(p, removal)?;
    let effects = translate::state_effects(p, active)?
        .into_iter()
        .map(|offset| {
            Ok(match p.u32(offset - 4)? {
                0x8080B7C6 => Effect::Attach(lower::attachment::read(reader, p, offset)?),
                0x80803128 => Effect::Spawn(lower::spawn::read(reader, p, offset)?),
                class => {
                    anyhow::bail!("attachment controller effect {class:08X} requires translation")
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(!effects.is_empty(), "attachment controller has no effects");
    Ok(Source {
        assignment,
        activation,
        removal,
        effects,
    })
}

fn hit(
    p: &Payload,
    activation: &controller::Condition,
    selectors: &sequence::Selectors,
) -> Result<lower::Hit> {
    let activation = lower::hit_condition(
        p,
        activation.offset,
        &selectors.resources,
        &selectors.groups,
        &selectors.resolver,
    )?;
    ensure!(
        activation
            .selector
            .references
            .iter()
            .all(|reference| reference.target.is_some())
            && activation
                .selector
                .named_conditions
                .iter()
                .all(|name| selectors.accepts_name(name)),
        "hosted hit predicate has unbound resources or named inputs"
    );
    Ok(activation)
}

pub fn hosted(reader: &Reader, p: &Payload, selectors: &sequence::Selectors) -> Result<Hosted> {
    let parsed = controller::read(p)?;
    ensure!(
        parsed.auxiliary.is_empty(),
        "hosted controller auxiliary records require translation"
    );
    let [idle, active] = parsed.states.as_slice() else {
        anyhow::bail!("hosted attachment controller requires two states");
    };
    metadata(p, &parsed.states, 0)?;
    ensure!(
        idle.transitions[0].destination == active.offset
            && active.transitions[0].destination == idle.offset,
        "hosted attachment controller is not an idle and active cycle"
    );
    let [activation] = idle.transitions[0].conditions.as_slice() else {
        anyhow::bail!("hosted attachment controller requires one hit predicate");
    };
    let activation = hit(p, activation, selectors)?;
    let [removal] = active.transitions[0].conditions.as_slice() else {
        anyhow::bail!("hosted attachment controller requires one unconditional exit");
    };
    ensure!(
        removal.class == 0x808030C0 && p.bytes::<7>(removal.offset)? == [0, 0, 128, 63, 255, 0, 0],
        "hosted attachment exit is not unconditional"
    );
    let removal = lower::Node {
        class: 0x80803E03,
        kind: 0,
        bytes: p.bytes::<8>(removal.offset)?.to_vec(),
    };
    let (modifiers, attachments) = persistent(p, idle, active, selectors)?;
    let attachments = attachments
        .into_iter()
        .map(|at| lower::attachment::read(reader, p, at))
        .collect::<Result<Vec<_>>>()?;
    Ok(Hosted {
        activation,
        removal,
        modifiers,
        attachments,
    })
}

fn persistent(
    p: &Payload,
    idle: &controller::State,
    active: &controller::State,
    selectors: &sequence::Selectors,
) -> Result<(Vec<lower::Node>, Vec<usize>)> {
    let modifiers = translate::state_effects(p, idle)?
        .into_iter()
        .map(|at| lower::event_modifier_with_labels(p, at, &selectors.resources, &selectors.groups))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !modifiers.is_empty(),
        "hosted idle state has no stateless modifier"
    );
    let active_effects = translate::state_effects(p, active)?;
    let prefix = active_effects
        .len()
        .checked_sub(modifiers.len())
        .filter(|&count| count != 0)
        .ok_or_else(|| {
            anyhow::anyhow!("hosted active state lacks its attachments or persistent modifiers")
        })?;
    for (&at, expected) in active_effects[prefix..].iter().zip(&modifiers) {
        let actual =
            lower::event_modifier_with_labels(p, at, &selectors.resources, &selectors.groups)?;
        ensure!(
            actual.class == expected.class
                && actual.kind == expected.kind
                && actual.bytes == expected.bytes,
            "hosted damage modifier differs between source states"
        );
    }
    Ok((modifiers, active_effects[..prefix].to_vec()))
}
