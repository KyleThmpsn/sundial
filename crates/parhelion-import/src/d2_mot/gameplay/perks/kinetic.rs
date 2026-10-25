//! Explicit compatibility policy for sustained-hit controllers and finite shockwave graphs.
use super::{Assignment, assigned, controller, lower, translate};
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub struct Source {
    pub assignment: Assignment,
    pub hits: f32,
    pub timeout: f32,
    pub cooldown_ms: u32,
    pub visual: u32,
    pub pulses: u8,
    pub interval: f32,
    pub report: Value,
}

fn family(p: &Payload, row: usize, wanted: u32) -> Result<bool> {
    ensure!(
        p.u64(row + 16)? == 0x100,
        "unsupported source counter matcher"
    );
    let mut matches = false;
    for condition in p.array(row + 24, 16, Some(0x808091B7))? {
        let at = p.pointer(condition + 8)?;
        match p.u32(at - 4)? {
            0x808042CB => {
                // Positive category labels select a family. Exclusions and numeric frame
                // predicates remain in the receipt and use the conservative family maximum.
                for field in [at, at + 16] {
                    for label in p.array(field, 32, Some(0x80809787))? {
                        matches |= p.u32(label)? == wanted;
                    }
                }
            }
            0x8080B88A => {}
            class => anyhow::bail!("unsupported source counter predicate {class:08X}"),
        }
    }
    Ok(matches)
}

struct Closure {
    seen: BTreeSet<u32>,
    visual: BTreeSet<u32>,
    intervals: BTreeSet<u32>,
    repeats: BTreeSet<u8>,
}

impl Closure {
    fn visit(&mut self, r: &mut Reader, tag: u32) -> Result<()> {
        if !self.seen.insert(tag) {
            return Ok(());
        }
        ensure!(
            self.seen.len() <= 32,
            "source attachment closure is too large"
        );
        let graph = r.tag(tag, Some(0x80809AD8))?;
        let mut children = BTreeSet::new();
        let mut visual = false;
        for row in graph.array(8, 12, Some(0x80809ACD))? {
            let owner = r.tag(graph.u32(row)?, Some(0x80809B06))?;
            let d = owner.pointer(24)?;
            if owner.u32(d - 4)? != 0x80808179 {
                continue;
            }
            for row in owner.array(d + 0x1C8, 24, Some(0x808091F1))? {
                let at = owner.pointer(row + 16)?;
                if owner.u32(at - 4)? == 0x808091E5 && owner.u8(at + 0x1D)? > 1 {
                    ensure!(owner.u8(at + 0x1E)? == 0, "source pulse loop is indefinite");
                    self.repeats.insert(owner.u8(at + 0x1D)?);
                }
            }
            for row in owner.array(d + 0x1D8, 24, Some(0x808091F1))? {
                let at = owner.pointer(row + 16)?;
                match owner.u32(at - 4)? {
                    0x80808485 => {
                        children.insert(r.ref64(&owner, at + 0x70)?);
                    }
                    0x808067B9 => {
                        visual = true;
                    }
                    0x808091D7 if owner.f32(at + 12)? > 0.0 && owner.f32(at + 12)? < 1.0 => {
                        ensure!(
                            owner.u32(at + 12)? == owner.u32(at + 16)?,
                            "randomized source pulse delay"
                        );
                        self.intervals.insert(owner.u32(at + 12)?);
                    }
                    _ => {}
                }
            }
        }
        if visual {
            self.visual.insert(tag);
        }
        for child in children {
            self.visit(r, child)?;
        }
        Ok(())
    }
}

pub fn extract(r: &mut Reader, plug: u32, weapon_family: &str) -> Result<Source> {
    let mut assignments = assigned(r, plug)?;
    ensure!(
        assignments.len() == 1,
        "sustained-hit import needs one complete controller"
    );
    let assignment = assignments.remove(0);
    let p = &assignment.controller;
    let decoded = controller::read(p)?;
    ensure!(
        decoded.states.len() == 3 && decoded.auxiliary.is_empty(),
        "unsupported sustained-hit controller states or auxiliary data"
    );
    ensure!(
        p.u64(8)? == 0
            && p.u64(16)? == 0
            && p.u32(64)? == 0x811C9DC5
            && p.bytes::<4>(68)? == [0, 3, 1, 0]
            && p.u32(72)? == 0,
        "unsupported sustained-hit controller metadata"
    );
    for state in &decoded.states {
        translate::neutral_state(p, state)?;
    }
    let [idle, active, cooldown] = decoded.states.as_slice() else {
        unreachable!()
    };
    ensure!(
        idle.transitions[0].destination == active.offset
            && active.transitions[0].destination == cooldown.offset
            && cooldown.transitions[0].destination == idle.offset,
        "unsupported sustained-hit state cycle"
    );
    ensure!(
        translate::state_effects(p, idle)?.is_empty()
            && translate::state_effects(p, cooldown)?.is_empty(),
        "inactive sustained-hit states have effects"
    );
    let condition = &idle.transitions[0].conditions;
    ensure!(
        condition.len() == 1 && condition[0].class == 0x8080B7B3 && condition[0].kind == 4,
        "unsupported sustained-hit trigger"
    );
    let at = condition[0].offset;
    ensure!(
        p.bytes::<8>(at)? == [0, 0, 128, 63, 255, 4, 1, 0]
            && p.u32(at + 0x80)? == 1
            && p.u8(at + 0x98)? == 0
            && p.u8(at + 0x99)? == 1
            && p.u8(at + 0xA2)? == 1,
        "source trigger is not an owned kinetic damage condition"
    );
    let count = p.pointer(at + 0x158)?;
    ensure!(
        p.u32(count - 4)? == 0x8080B7B5,
        "source trigger lacks a conditional counter"
    );
    let wanted = weapon_family
        .to_lowercase()
        .bytes()
        .fold(0x811C9DC5u32, |h, b| {
            h.wrapping_mul(16777619) ^ u32::from(b)
        });
    let mut candidates = Vec::new();
    for row in p.array(count, 40, Some(0x8080B7B7))? {
        let hits = p.f32(row)?;
        let timeout = p.f32(row + 8)?;
        ensure!(
            hits.is_finite()
                && (1.0..=100.0).contains(&hits)
                && hits.fract() == 0.0
                && p.u32(row)? == p.u32(row + 4)?
                && timeout.is_finite()
                && (0.0..=60.0).contains(&timeout)
                && p.u32(row + 12)? == 0,
            "unsupported source counter limits"
        );
        if family(p, row, wanted)? {
            candidates.push((hits, timeout));
        }
    }
    let (hits, timeout) = candidates
        .iter()
        .copied()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .context("source perk has no counter for this weapon family")?;
    let removal = &active.transitions[0].conditions;
    ensure!(
        removal.len() == 1 && removal[0].class == 0x808030C0 && removal[0].kind == 0,
        "source pulse activation is not instantaneous"
    );
    let rearm = &cooldown.transitions[0].conditions;
    ensure!(rearm.len() == 1, "source cooldown needs one timer");
    lower::timer_condition(p, rearm[0].offset)?;
    let seconds = p.f32(rearm[0].offset + 8)?;
    ensure!(
        seconds > 0.0 && seconds <= 3600.0 && (seconds * 1000.0).fract() == 0.0,
        "source cooldown is not representable"
    );
    let effects = translate::state_effects(p, active)?;
    ensure!(
        effects.len() == 1
            && p.u32(effects[0] - 4)? == 0x8080B7C6
            && p.bytes::<4>(effects[0])? == [1, 1, 3, 0],
        "unsupported sustained-hit attachment"
    );
    let attachment = r.ref64(p, effects[0] + 16)?;
    let mut closure = Closure {
        seen: BTreeSet::new(),
        visual: BTreeSet::new(),
        intervals: BTreeSet::new(),
        repeats: BTreeSet::new(),
    };
    closure.visit(r, attachment)?;
    ensure!(
        closure.visual.len() == 1 && closure.intervals.len() == 1 && closure.repeats.len() == 1,
        "source pulse graph does not have one supported visual, interval and repetition profile"
    );
    let pulses = *closure.repeats.first().context("pulse repetition")?;
    ensure!(
        (2..=8).contains(&pulses),
        "source pulse repetition exceeds compatibility limit"
    );
    let interval = f32::from_bits(*closure.intervals.first().context("pulse interval")?);
    let visual = *closure.visual.first().context("source pulse visual")?;
    let report = json!({"attachment":attachment,"visual":visual,"family":weapon_family,
        "counter_candidates":candidates,"selected_hits":hits,"timeout_seconds":timeout,
        "pulse_count":pulses,"pulse_interval_seconds":interval,
        "differences":["Frame predicates use the highest source threshold for the chosen weapon family.",
            "Native direct kinetic hit filtering replaces modern target category predicates.",
            "Shockwaves stay at the triggering hit position instead of following the target.",
            "Pulse damage and radius use a private native compatibility profile.",
            "The source repetition byte is interpreted as a finite pulse count. Its modern consumer is unverified.",
            "The source attachment status record is not reproduced."],"gameplay_verified":false});
    Ok(Source {
        assignment,
        hits,
        timeout,
        cooldown_ms: (seconds * 1000.0) as u32,
        visual,
        pulses,
        interval,
        report,
    })
}
