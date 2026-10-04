//! Checked modern state routing. Source kinds are not native condition enums.
use anyhow::{Result, ensure};
use serde::Serialize;

use crate::d2_mot::payload::Payload;

#[derive(Debug, Serialize)]
pub struct Controller {
    pub states: Vec<State>,
    pub auxiliary: Vec<Auxiliary>,
}

/// The auxiliary record class whose layout is decoded. Other classes stay source records.
pub const STATE_AUXILIARY: u32 = 0x8080_B843;

#[derive(Debug, Serialize)]
pub struct Auxiliary {
    pub offset: usize,
    pub class: u32,
    pub key: Option<u32>,
    pub unknown_integer: Option<u32>,
    pub state_offsets: Vec<usize>,
    pub state_keys: Vec<u32>,
    pub source_record: String,
}

#[derive(Debug, Serialize)]
pub struct State {
    pub offset: usize,
    pub key: u32,
    pub event_mask: u64,
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Serialize)]
pub struct Transition {
    pub offset: usize,
    pub key: u32,
    pub destination: usize,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Serialize)]
pub struct Condition {
    pub offset: usize,
    pub class: u32,
    pub kind: u8,
    pub event_mask: u64,
    pub children: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record_complete: Option<bool>,
}

fn rows(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
    let count = p.u64(at)?;
    ensure!(count <= 256, "controller list exceeds limit at {at:X}");
    if count == 0 {
        ensure!(
            p.u64(at + 8)? == 0,
            "empty controller list has a pointer at {at:X}"
        );
        return Ok(Vec::new());
    }
    let header = p.pointer(at + 8)?;
    ensure!(header >= 4, "controller array lacks a marker at {at:X}");
    ensure!(
        p.u32(header - 4)? == 0x80809FB8,
        "controller array marker differs at {at:X}"
    );
    ensure!(
        p.u64(header + 8)? == u64::from(class),
        "controller array class differs at {at:X}"
    );
    p.array(at, stride, Some(class))
}

fn pointed(p: &Payload, at: usize) -> Result<usize> {
    ensure!(p.u64(at)? != 0, "null controller pointer at {at:X}");
    let target = p.pointer(at)?;
    ensure!(target >= 4, "controller record lacks a class at {at:X}");
    Ok(target)
}

fn condition(p: &Payload, at: usize, ancestors: &mut Vec<usize>) -> Result<Condition> {
    ensure!(
        ancestors.len() < 32 && !ancestors.contains(&at),
        "cyclic or excessively deep condition tree at {at:X}"
    );
    let class = p.u32(at - 4)?;
    let kind = p.u8(at + 5)?;
    ensure!(class >> 16 == 0x8080, "invalid condition class at {at:X}");
    ensure!(kind < 64, "condition exceeds event mask width at {at:X}");
    p.bytes::<8>(at)?;
    let source_record = if class == 0x8080BDCF {
        ensure!(
            kind == 49,
            "condition class 8080BDCF has wrong dispatch at {at:X}"
        );
        // The checked common prefix ends before a neighboring typed object in
        // some controllers. The exact condition size is not yet established.
        Some(hex::encode(p.bytes::<40>(at)?))
    } else {
        None
    };
    ancestors.push(at);
    let mut children = Vec::new();
    let mut nested = 0;
    match class {
        0x80803115 => {
            ensure!(
                kind == 27,
                "counter class has wrong dispatch kind at {at:X}"
            );
            for row in rows(p, at + 8, 184, 0x80803117)? {
                let child = condition(p, pointed(p, row)?, ancestors)?;
                nested |= child.event_mask;
                children.push(child);
            }
            let stored = p.u64(at + 24)?;
            ensure!(
                stored == if children.is_empty() { 1 } else { nested },
                "counter child mask differs at {at:X}"
            );
            // Empty counters retain the unconditional event contribution as well.
            nested |= stored;
        }
        0x808030C1 => {
            ensure!(
                kind == 32,
                "subgroups class has wrong dispatch kind at {at:X}"
            );
            for row in rows(p, at + 8, 32, 0x808030C3)? {
                let mut subgroup = 0;
                for reference in rows(p, row + 8, 8, 0x808037B9)? {
                    let child = condition(p, pointed(p, reference)?, ancestors)?;
                    subgroup |= child.event_mask;
                    children.push(child);
                }
                ensure!(
                    p.u64(row + 24)? == subgroup,
                    "subgroup child mask differs at {row:X}"
                );
                nested |= subgroup;
            }
        }
        0x8080305F => {
            ensure!(
                kind == 36,
                "nested predicate class has wrong dispatch kind at {at:X}"
            );
            if p.u64(at + 64)? != 0 {
                let child = condition(p, pointed(p, at + 64)?, ancestors)?;
                nested |= child.event_mask;
                children.push(child);
            }
        }
        _ => {}
    }
    ancestors.pop();
    Ok(Condition {
        offset: at,
        class,
        kind,
        event_mask: (1u64 << kind) | nested,
        children,
        source_record_complete: source_record.as_ref().map(|_| false),
        source_record,
    })
}

/// Read a modern B835 controller, checking every destination and compiled state mask.
/// Unknown leaf kinds remain source records. This does not establish native support.
pub fn read(p: &Payload) -> Result<Controller> {
    ensure!(
        p.u64(0)? == p.0.len() as u64,
        "controller declared size mismatch"
    );
    p.bytes::<80>(0)?;
    let offsets = rows(p, 40, 136, 0x8080B83F)?;
    let mut states = Vec::new();
    for &at in &offsets {
        let mut transitions = Vec::new();
        let mut event_mask = 0;
        for row in rows(p, at + 32, 48, 0x8080B83D)? {
            let destination = pointed(p, row + 8)?;
            ensure!(
                offsets.contains(&destination),
                "transition destination is not a controller state at {row:X}"
            );
            let mut conditions = Vec::new();
            for reference in rows(p, row + 16, 8, 0x808037B9)? {
                let node = condition(p, pointed(p, reference)?, &mut Vec::new())?;
                event_mask |= node.event_mask;
                conditions.push(node);
            }
            transitions.push(Transition {
                offset: row,
                key: p.u32(row)?,
                destination,
                conditions,
            });
        }
        ensure!(
            p.u64(at + 64)? == event_mask,
            "state event mask differs at {at:X}"
        );
        states.push(State {
            offset: at,
            key: p.u32(at)?,
            event_mask,
            transitions,
        });
    }
    let mut auxiliary = Vec::new();
    for row in rows(p, 24, 8, 0x8080B840)? {
        let at = pointed(p, row)?;
        let class = p.u32(at - 4)?;
        ensure!(
            class >> 16 == 0x8080,
            "invalid controller auxiliary class at {at:X}"
        );
        if class != STATE_AUXILIARY {
            // An auxiliary of another class keeps its class and its common eight-byte prefix.
            // Its size and references are not established, so nothing else is read from it.
            auxiliary.push(Auxiliary {
                offset: at,
                class,
                key: None,
                unknown_integer: None,
                state_offsets: Vec::new(),
                state_keys: Vec::new(),
                source_record: hex::encode(p.bytes::<8>(at)?),
            });
            continue;
        }
        let source_record = hex::encode(p.bytes::<56>(at)?);
        let mut state_offsets = Vec::new();
        let mut state_keys = Vec::new();
        for reference in rows(p, at + 8, 8, 0x8080B848)? {
            let state = pointed(p, reference)?;
            ensure!(
                offsets.contains(&state),
                "controller auxiliary points outside its states at {reference:X}"
            );
            state_offsets.push(state);
            state_keys.push(p.u32(state)?);
        }
        auxiliary.push(Auxiliary {
            offset: at,
            class,
            key: Some(p.u32(at + 24)?),
            unknown_integer: Some(p.u32(at + 28)?),
            state_offsets,
            state_keys,
            source_record,
        });
    }
    Ok(Controller { states, auxiliary })
}
