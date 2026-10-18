//! Bounded translation of one constant weapon modifier attached for the equip lifecycle.
use super::{Assignment, controller, lower, translate};
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

pub struct Source {
    pub assignment: Assignment,
    pub activation: lower::Node,
    pub removal: lower::Node,
    pub entity_tag: u32,
    pub owner_tag: u32,
    pub owner: Arc<Payload>,
    pub modifier_offset: usize,
    pub path: String,
}

pub fn extract(reader: &mut Reader, plug: u32) -> Result<Source> {
    const EMPTY: u32 = 0x811C9DC5;
    let mut assigned = super::assigned(reader, plug)?;
    ensure!(
        assigned.len() == 1,
        "equipped modifier requires exactly one complete controller"
    );
    let assignment = assigned.pop().context("source controller")?;
    let p = &assignment.controller;
    ensure!(
        p.u64(8)? == 0
            && p.u64(16)? == 0
            && p.u32(64)? == EMPTY
            && p.bytes::<4>(68)? == [0, 2, 0, 0]
            && p.u32(72)? == 0,
        "unsupported equipped controller metadata"
    );
    let decoded = controller::read(p)?;
    ensure!(
        decoded.states.len() == 2 && decoded.auxiliary.is_empty(),
        "equipped controller has additional states or reports"
    );
    for state in &decoded.states {
        translate::neutral_state(p, state)?;
    }
    let idle = &decoded.states[0];
    let active = &decoded.states[1];
    let enter = &idle.transitions[0];
    let exit = &active.transitions[0];
    ensure!(
        idle.key != active.key
            && enter.destination == active.offset
            && exit.destination == idle.offset
            && enter.conditions.len() == 1
            && exit.conditions.len() == 1
            && enter.conditions[0].class == 0x808030B6
            && exit.conditions[0].class == 0x808030B2,
        "unsupported equipped controller lifecycle"
    );
    let activation = lower::equipped_condition(p, enter.conditions[0].offset)?;
    let removal = lower::equipped_condition(p, exit.conditions[0].offset)?;
    let effects = translate::state_effects(p, active)?;
    ensure!(
        translate::state_effects(p, idle)?.is_empty() && effects.len() == 1,
        "equipped controller has additional effects"
    );
    let at = effects[0];
    ensure!(
        p.u32(at - 4)? == 0x8080B7C6
            && p.bytes::<8>(at)? == [1, 1, 0, 0, 0, 0, 0, 0]
            && p.u32(at + 32)? == 0
            && p.u32(at + 36)? == EMPTY
            && p.u32(at + 40)? == 0
            && p.u32(at + 44)? == EMPTY
            && p.u64(at + 48)? == 0
            && p.u64(at + 56)? == 0,
        "unsupported equipped attachment target or cleanup"
    );
    let attachment = translate::attachment(reader, p, at)?;
    let graph = reader.tag(attachment.entity_tag, Some(0x80809AD8))?;
    ensure!(
        graph.bytes::<32>(24)? == [0; 32],
        "equipped modifier has external input connections"
    );
    ensure!(
        attachment.modifiers.len() == 1,
        "equipped modifier has additional numeric inputs"
    );
    let offset = attachment.modifiers[0];
    let owner = &attachment.owner;
    let instance = owner.pointer(16)?;
    // A constant attachment must start at unit weight with no external driver.
    ensure!(
        owner.f32(instance + 96)? == 1.0
            && owner.f32(instance + 100)? == 1.0
            && owner.bytes::<4>(instance + 104)? == [0, 0, 1, 1],
        "equipped modifier does not have a constant unit weight"
    );
    ensure!(
        owner.f32(offset + 16)?.is_finite() && owner.u8(offset + 20)? <= 1,
        "equipped modifier has an unsupported amount or operation"
    );
    lower::modifier_input(owner.u8(offset + 96)?, owner.u16(offset + 90)?)?;
    Ok(Source {
        assignment,
        activation,
        removal,
        entity_tag: attachment.entity_tag,
        owner_tag: attachment.owner_tag,
        owner: attachment.owner,
        modifier_offset: offset,
        path: attachment.path,
    })
}
