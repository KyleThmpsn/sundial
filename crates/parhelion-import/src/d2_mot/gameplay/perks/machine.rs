//! A modern controller's states as Shadowkeep action programs.
//!
//! A modern controller is a state machine: each state holds effects and each transition holds
//! alternative conditions and a destination. A Shadowkeep action is the fixed cycle idle, active
//! and rearm, with one condition list for each step. The shapes below are the machines that
//! cycle expresses. Twin perks pair them the same way: the first state's transitions are the
//! Shadowkeep activation list, the effect state's transitions the removal list and a third
//! state's transitions the rearm list.
//!
//! A machine those shapes do not express becomes one program for each state that holds
//! effects. Each program marks its state in a named property of the weapon for as long as it
//! is active, and the others gate their conditions on that mark: see `decompose`. That form
//! is built from stock parts and has no twin behind it, so the import records it as an
//! inference.
//!
//! Every condition must translate, since a missing one changes when the perk runs. An effect
//! that does not translate is left out and recorded, as is a HUD report with no Shadowkeep
//! record. Nothing here establishes that Shadowkeep publishes the events these conditions wait
//! for.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sundial::{
    investment::discovery::labels::Registry,
    package_authoring::fnv1,
    package_authoring::sandbox_perk::program::{
        Action, HudStatus, ImportedAsset, NativeAssetPatch, NativeNode, NativeRecord, Policy,
        Program, Trigger,
    },
};

use super::{Assignment, controller, lower, modifier, translate};
use crate::d2_mot::gameplay::perks::table;
use crate::d2_mot::payload::Payload;

const EMPTY: u32 = 0x811C_9DC5;
/// The plain unconditional condition: probability one, literal, kind 0, no linked state.
const UNCONDITIONAL: [u8; 7] = [0, 0, 0x80, 0x3F, 0xFF, 0, 0];
/// The programs one plug carries: the sandbox perk entries Sunrise reads for an item or plug.
pub const PROGRAM_LIMIT: usize = 4;

/// The translated programs of one source plug and every source controller, effect or report
/// left out, each with its reason.
pub struct Translated {
    pub programs: Vec<Program>,
    pub omitted: Vec<Value>,
}

/// Translate every controller assigned to a source plug. A controller that does not translate
/// is recorded in `omitted` and contributes no program. With `tags`, an attachment whose entity
/// only holds stat modifiers is translated too.
pub fn translate(
    assigned: &[Assignment],
    name: &str,
    labels: Option<&Registry>,
    mut tags: Option<&mut (dyn modifier::Tags + '_)>,
) -> Translated {
    let mut programs = Vec::new();
    let mut omitted = Vec::new();
    for assignment in assigned {
        match self::programs(&assignment.controller, name, labels, tags.as_deref_mut()) {
            Ok((translated, skipped)) => {
                programs.extend(translated);
                omitted.extend(skipped.into_iter().map(|mut row| {
                    row["controller"] = json!(format!("{:08X}", assignment.action_tag));
                    row
                }));
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

/// One test of a state mark: the property key and whether the state must be held.
#[derive(Clone, Copy)]
struct Gate {
    key: u32,
    on: bool,
}

/// One entry of a Shadowkeep condition list.
#[derive(Clone)]
enum Step {
    /// A source condition, by its offset.
    Condition(usize),
    /// A timer of this many seconds, standing for a state the source holds that long before
    /// an unconditional exit.
    Timer(f32),
    /// A source condition that only counts while every gate holds, or the gates alone.
    Gated(Vec<Gate>, Option<usize>),
    /// At once.
    Always,
}

/// One Shadowkeep cycle: each condition list, the effect offsets and the source state that
/// holds them.
#[derive(Default)]
struct Cycle {
    activation: Vec<Step>,
    state: usize,
    effects: Vec<usize>,
    removal: Vec<Step>,
    rearm: Vec<Step>,
    /// The property the program holds at 1 while it is active, for other programs to gate on.
    marker: Option<u32>,
    /// Only the effects that hold no state, for a program that fires again on re-entry.
    instant: bool,
}

struct State<'a> {
    source: &'a controller::State,
    effects: Vec<usize>,
    /// Destination state index of each transition.
    destinations: Vec<usize>,
    /// Seconds the state is held before its one unconditional exit is taken.
    held: Option<f32>,
}

impl State<'_> {
    fn conditions(&self) -> Vec<Step> {
        if let Some(seconds) = self.held {
            return vec![Step::Timer(seconds)];
        }
        self.source
            .transitions
            .iter()
            .flat_map(|transition| {
                transition
                    .conditions
                    .iter()
                    .map(|node| Step::Condition(node.offset))
            })
            .collect()
    }

    /// The conditions of the transitions that lead to `destination`.
    fn conditions_to(&self, destination: usize) -> Vec<Step> {
        if let Some(seconds) = self.held {
            return vec![Step::Timer(seconds)];
        }
        self.source
            .transitions
            .iter()
            .zip(&self.destinations)
            .filter(|(_, to)| **to == destination)
            .flat_map(|(transition, _)| {
                transition
                    .conditions
                    .iter()
                    .map(|node| Step::Condition(node.offset))
            })
            .collect()
    }

    fn leads_only_to(&self, destination: usize) -> bool {
        self.destinations.iter().all(|to| *to == destination)
    }
}

/// Whether every effect of `state` only sets a counter (`80803111`), which twin perks run among
/// the effects of their one Shadowkeep cycle (as `80803E2F`, kind 42).
fn sets_counters_only(p: &Payload, state: &State<'_>) -> bool {
    state
        .effects
        .iter()
        .all(|at| p.u32(at - 4).ok() == Some(0x8080_3111))
}

/// The cycles a controller's states form, or the reason they form none.
fn cycles(p: &Payload, states: &[State<'_>]) -> Result<Vec<Cycle>> {
    let idle = |state: &State<'_>| state.effects.is_empty();
    match states {
        // One state with no way out holds its effects for as long as the perk is applied.
        [only] if only.destinations.is_empty() => Ok(vec![Cycle {
            state: only.source.offset,
            effects: only.effects.clone(),
            ..Cycle::default()
        }]),
        // Idle to active and back. Without a way back the effects stay once started.
        [first, active]
            if idle(first)
                && !first.destinations.is_empty()
                && first.leads_only_to(1)
                && active.leads_only_to(0) =>
        {
            Ok(vec![Cycle {
                activation: first.conditions(),
                state: active.source.offset,
                effects: active.effects.clone(),
                removal: active.conditions(),
                rearm: Vec::new(),
                ..Cycle::default()
            }])
        }
        // Two effect states that alternate. The first is active from the start, ends on its
        // own transitions and starts again on the second state's. The second runs between.
        [first, second]
            if !idle(first)
                && !idle(second)
                && !first.destinations.is_empty()
                && !second.destinations.is_empty()
                && first.leads_only_to(1)
                && second.leads_only_to(0) =>
        {
            Ok(vec![
                Cycle {
                    activation: Vec::new(),
                    state: first.source.offset,
                    effects: first.effects.clone(),
                    removal: first.conditions(),
                    rearm: second.conditions(),
                    ..Cycle::default()
                },
                Cycle {
                    activation: first.conditions(),
                    state: second.source.offset,
                    effects: second.effects.clone(),
                    removal: second.conditions(),
                    rearm: Vec::new(),
                    ..Cycle::default()
                },
            ])
        }
        // An effect state that is active from the start and an idle state it rests in.
        [first, rest]
            if !idle(first)
                && idle(rest)
                && !first.destinations.is_empty()
                && !rest.destinations.is_empty()
                && first.leads_only_to(1)
                && rest.leads_only_to(0) =>
        {
            Ok(vec![Cycle {
                activation: Vec::new(),
                state: first.source.offset,
                effects: first.effects.clone(),
                removal: first.conditions(),
                rearm: rest.conditions(),
                ..Cycle::default()
            }])
        }
        // An effect state that is active from the start and an idle state with no way out.
        // Shadowkeep's always-active actions that end early have a removal list and no rearm
        // list, and stay ended.
        [first, rest]
            if !idle(first)
                && idle(rest)
                && !first.destinations.is_empty()
                && rest.destinations.is_empty()
                && first.leads_only_to(1) =>
        {
            Ok(vec![Cycle {
                activation: Vec::new(),
                state: first.source.offset,
                effects: first.effects.clone(),
                removal: first.conditions(),
                rearm: Vec::new(),
                ..Cycle::default()
            }])
        }
        // Idle, active and a rearm state that holds nothing.
        [first, active, rearm]
            if idle(first)
                && idle(rearm)
                && !first.destinations.is_empty()
                && !active.destinations.is_empty()
                && !rearm.destinations.is_empty()
                && first.leads_only_to(1)
                && active.leads_only_to(2)
                && rearm.leads_only_to(0) =>
        {
            Ok(vec![Cycle {
                activation: first.conditions(),
                state: active.source.offset,
                effects: active.effects.clone(),
                removal: active.conditions(),
                rearm: rearm.conditions(),
                ..Cycle::default()
            }])
        }
        // Idle, an active state that may return to itself, and a reset state that only sets
        // counters and returns to idle. Twin perks write such machines as one cycle (168 of 169
        // three-state twins, all 17 whose active state returns to itself). The return to itself
        // is dropped, as on `80CECED0`, where an Extend Timers effect of the active state
        // lengthens it instead. A counter reset runs first among the cycle's effects, as on
        // `80C3010B`, and the reset state's exit is the rearm list.
        [first, active, reset]
            if idle(first)
                && first.held.is_none()
                && active.held.is_none()
                && !first.destinations.is_empty()
                && first.leads_only_to(1)
                && !idle(active)
                && active.destinations.contains(&2)
                && active.destinations.iter().all(|to| matches!(to, 1 | 2))
                && !reset.destinations.is_empty()
                && reset.leads_only_to(0)
                && sets_counters_only(p, reset) =>
        {
            Ok(vec![Cycle {
                activation: first.conditions(),
                state: active.source.offset,
                effects: reset
                    .effects
                    .iter()
                    .chain(&active.effects)
                    .copied()
                    .collect(),
                removal: active.conditions_to(2),
                rearm: reset.conditions(),
                ..Cycle::default()
            }])
        }
        // An idle state that branches to effect states, each of which only returns to it. Each
        // branch becomes its own cycle. The source holds one branch at a time, while separate
        // Shadowkeep programs can overlap, which the caller records.
        [first, branches @ ..]
            if branches.len() >= 2
                && idle(first)
                && first.held.is_none()
                && !first.destinations.is_empty()
                && branches.iter().enumerate().all(|(index, branch)| {
                    !idle(branch)
                        && !branch.destinations.is_empty()
                        && branch.leads_only_to(0)
                        && first.destinations.contains(&(index + 1))
                }) =>
        {
            Ok(branches
                .iter()
                .enumerate()
                .map(|(index, branch)| Cycle {
                    activation: first.conditions_to(index + 1),
                    state: branch.source.offset,
                    effects: branch.effects.clone(),
                    removal: branch.conditions(),
                    rearm: Vec::new(),
                    ..Cycle::default()
                })
                .collect())
        }
        _ => bail!(
            "source states do not form a Shadowkeep cycle: {}",
            states
                .iter()
                .map(|state| format!(
                    "{}{:?}",
                    if idle(state) { "idle" } else { "effects" },
                    state.destinations
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

fn native(node: lower::Node) -> NativeNode {
    NativeNode {
        kind: node.kind,
        bytes: node.bytes,
    }
}

/// `nodes` with a second kill condition after each one that asks for a modern status Shadowkeep
/// holds under another key. The native checker compares one status definition key. The condition
/// as written passes on the status a translated perk attaches, which keeps the modern key, and
/// the added one passes on the stock status that plays its part. Conditions of a list are
/// alternatives, so either passes the step. A kill condition inside a gate is left as written.
fn with_stock_statuses(nodes: Vec<NativeNode>) -> Result<Vec<NativeNode>> {
    const KILL: u8 = 2;
    const STATE: usize = 0x144;
    let mut all = Vec::with_capacity(nodes.len());
    for node in nodes {
        let stock = match node.bytes.get(STATE..STATE + 4) {
            Some(key) if node.kind == KILL => table::status(u32::from_le_bytes(key.try_into()?))?,
            _ => None,
        };
        let twin = stock.map(|key| {
            let mut twin = node.clone();
            twin.bytes[STATE..STATE + 4].copy_from_slice(&key.to_le_bytes());
            twin
        });
        all.push(node);
        all.extend(twin);
    }
    // A source that already asks for the stock key beside the modern one needs no second copy.
    let mut kept: Vec<NativeNode> = Vec::with_capacity(all.len());
    for node in all {
        if node.kind != KILL || !kept.contains(&node) {
            kept.push(node);
        }
    }
    Ok(kept)
}

fn conditions(p: &Payload, steps: &[Step], labels: Option<&Registry>) -> Result<Vec<NativeNode>> {
    with_stock_statuses(steps_conditions(p, steps, labels)?)
}

fn steps_conditions(
    p: &Payload,
    steps: &[Step],
    labels: Option<&Registry>,
) -> Result<Vec<NativeNode>> {
    steps
        .iter()
        .map(|step| match step {
            Step::Condition(at) => lower::condition_with(p, *at, labels)
                .map(native)
                .with_context(|| format!("source condition at {at:X}")),
            Step::Timer(seconds) => {
                // The Shadowkeep timer: probability one, literal, kind 1 and its seconds.
                let mut bytes = vec![0, 0, 0x80, 0x3F, 0xFF, 1, 0, 0];
                bytes.extend(seconds.to_le_bytes());
                Ok(NativeNode { kind: 1, bytes })
            }
            Step::Gated(gates, inner) => {
                let node = inner
                    .map(|at| {
                        lower::condition_with(p, at, labels)
                            .with_context(|| format!("source condition at {at:X}"))
                    })
                    .transpose()?;
                // One predicate tests one key and holds one condition state. A single gate
                // wraps a condition that leaves that state alone. Any other step is a compound
                // condition of its gates and its condition, side by side: a form no twin shows
                // for a state machine, so when it passes in game is not established.
                if let [gate] = gates.as_slice()
                    && let Ok(gated) = lower::key_gate(gate.key, gate.on, node.as_ref())
                {
                    return Ok(native(gated));
                }
                let marks = gates
                    .iter()
                    .map(|gate| lower::key_gate(gate.key, gate.on, None))
                    .collect::<Result<Vec<_>>>()?;
                match (marks.len(), node) {
                    (0, node) => node
                        .map(native)
                        .context("a gated step has neither a gate nor a condition"),
                    (_, node) => lower::all_of(&marks, node.as_ref()).map(native),
                }
            }
            Step::Always => Ok(NativeNode {
                kind: 0,
                bytes: [UNCONDITIONAL.as_slice(), &[0]].concat(),
            }),
        })
        .collect()
}

/// The mark of state `index` of the controller `p`: a property key of the controller's own.
fn mark(p: &Payload, index: usize) -> Result<u32> {
    let identity = p.0.iter().fold(EMPTY, |hash, byte| {
        hash.wrapping_mul(16_777_619) ^ u32::from(*byte)
    });
    let key = fnv1(&format!("d2_mot/perk_state/{identity:08X}/{index}"));
    ensure!(
        !matches!(key, 0 | u32::MAX | EMPTY),
        "the state mark of state {index} is a reserved key"
    );
    Ok(key)
}

/// A machine the single cycle shapes do not express, as one program for each state.
///
/// A state with effects, and every idle state but one, gets a program that is active exactly
/// while the machine is in that state. The remaining idle state is the one in which no
/// program is active. A program whose state another program must test holds a named property
/// of the weapon at 1 while it is active, the way stock perks hold `A43A8C2E`, and the tests
/// are stock property predicates:
///
/// - Entering a state from a state with a program: the transition's condition inside a
///   predicate that the source state's mark is 1. From the idle state: inside predicates that
///   every other mark is 0.
/// - Leaving for a state with a program: the predicate that the destination's mark is 1, so
///   the destination starts before the source ends and the event is not lost between them.
///   Leaving for the idle state: the transition's own condition.
/// - A transition from a state to itself fires the state's effects that hold nothing again,
///   in a program that ends at once. Effects that hold state are not restarted.
///
/// A state the machine starts in is active from the start and takes its entries as its rearm
/// list. Nothing here has been run, and a plug carries four programs.
fn decompose(p: &Payload, states: &[State<'_>]) -> Result<Vec<Cycle>> {
    ensure!(
        states.iter().all(|state| state.held.is_none()),
        "a state of a machine beyond one cycle is held for a time"
    );
    let idle = |index: usize| states[index].effects.is_empty();
    // The idle state with no program: the first state when it is idle, else the first idle.
    let rest = (0..states.len()).find(|index| idle(*index));
    let programmed = (0..states.len())
        .filter(|index| Some(*index) != rest)
        .collect::<Vec<_>>();
    let alone = states.len() == 1;
    // What must hold for the machine to be in `from`, as seen by the program of `to`.
    let gates = |from: usize, to: usize| -> Result<Vec<Gate>> {
        if alone {
            return Ok(Vec::new());
        }
        if Some(from) == rest {
            return programmed
                .iter()
                .filter(|other| **other != to)
                .map(|other| {
                    Ok(Gate {
                        key: mark(p, *other)?,
                        on: false,
                    })
                })
                .collect();
        }
        Ok(vec![Gate {
            key: mark(p, from)?,
            on: true,
        }])
    };
    let mut marked = std::collections::BTreeSet::new();
    let mut cycles = Vec::new();
    for &state in &programmed {
        let mut entries = Vec::new();
        for (from, source) in states.iter().enumerate() {
            if from == state {
                continue;
            }
            for (transition, to) in source.source.transitions.iter().zip(&source.destinations) {
                if *to != state {
                    continue;
                }
                for condition in &transition.conditions {
                    let gates = gates(from, state)?;
                    marked.extend(gates.iter().map(|gate| gate.key));
                    let inner = (!unconditional(p, condition)?).then_some(condition.offset);
                    entries.push(match (gates.is_empty(), inner) {
                        (true, Some(at)) => Step::Condition(at),
                        (true, None) => bail!(
                            "state {state} is entered at once from the idle state with nothing to gate on"
                        ),
                        (false, inner) => Step::Gated(gates, inner),
                    });
                }
            }
        }
        let own = &states[state];
        let (exits, loops) = exits(p, own, state, rest, &mut marked)?;
        if state != 0 && entries.is_empty() {
            // Nothing leads here, so the state is never held.
            continue;
        }
        let (activation, rearm) = if state == 0 {
            (Vec::new(), entries)
        } else {
            (entries, Vec::new())
        };
        cycles.push(Cycle {
            activation,
            state: own.source.offset,
            effects: own.effects.clone(),
            removal: exits,
            rearm,
            marker: Some(mark(p, state)?),
            instant: false,
        });
        if !loops.is_empty() && !own.effects.is_empty() {
            let gates = gates(state, state)?;
            marked.extend(gates.iter().map(|gate| gate.key));
            cycles.push(Cycle {
                activation: loops
                    .into_iter()
                    .map(|at| {
                        if gates.is_empty() {
                            Step::Condition(at)
                        } else {
                            Step::Gated(gates.clone(), Some(at))
                        }
                    })
                    .collect(),
                // No report names a re-entry.
                state: 0,
                effects: own.effects.clone(),
                removal: vec![Step::Always],
                rearm: Vec::new(),
                marker: None,
                instant: true,
            });
        }
    }
    // Only a state another program tests needs its mark.
    for cycle in &mut cycles {
        cycle.marker = cycle.marker.filter(|key| marked.contains(key));
    }
    ensure!(!cycles.is_empty(), "source machine has no state to hold");
    Ok(cycles)
}

/// How the program of `state` ends, and the conditions of its transitions to itself. Leaving
/// for the idle state `rest` ends it on the transition's own condition. Leaving for a state with
/// a program ends it on that state's mark, which goes into `marked`.
fn exits(
    p: &Payload,
    own: &State<'_>,
    state: usize,
    rest: Option<usize>,
    marked: &mut std::collections::BTreeSet<u32>,
) -> Result<(Vec<Step>, Vec<usize>)> {
    let mut exits = Vec::new();
    let mut loops = Vec::new();
    let mut left_for = Vec::new();
    for (transition, to) in own.source.transitions.iter().zip(&own.destinations) {
        for condition in &transition.conditions {
            if *to == state {
                ensure!(
                    !unconditional(p, condition)?,
                    "state {state} returns to itself at once"
                );
                loops.push(condition.offset);
            } else if Some(*to) == rest {
                exits.push(if unconditional(p, condition)? {
                    Step::Always
                } else {
                    Step::Condition(condition.offset)
                });
            } else if !left_for.contains(to) {
                left_for.push(*to);
                let key = mark(p, *to)?;
                marked.insert(key);
                exits.push(Step::Gated(vec![Gate { key, on: true }], None));
            }
        }
    }
    Ok((exits, loops))
}

/// The first node of a list as its typed field and the rest as its alternatives.
fn split(mut nodes: Vec<NativeNode>) -> (Option<NativeNode>, Vec<NativeNode>) {
    if nodes.is_empty() {
        return (None, nodes);
    }
    let first = nodes.remove(0);
    (Some(first), nodes)
}

#[derive(Default)]
struct Effects {
    actions: Vec<Action>,
    patches: Vec<NativeAssetPatch>,
    groups: Vec<ImportedAsset>,
}

impl Effects {
    /// Re-entry fires only effects that do not hold state. Kept patches and assets name the
    /// action's index after that filtering, so their references stay with the same action.
    fn push(
        &mut self,
        action: Action,
        patch: Option<NativeAssetPatch>,
        group: Option<ImportedAsset>,
        instant: bool,
    ) {
        let held = match &action {
            Action::Native { node } => node.bytes.get(1).is_some_and(|byte| *byte != 0),
            Action::Spawn { .. } => false,
            _ => true,
        };
        if instant && held {
            return;
        }
        if let Some(mut patch) = patch {
            patch.action_index = self.actions.len();
            self.patches.push(patch);
        }
        if let Some(mut group) = group {
            group.action_index = self.actions.len();
            self.groups.push(group);
        }
        self.actions.push(action);
    }
}

fn program(
    p: &Payload,
    cycle: &Cycle,
    name: &str,
    labels: Option<&Registry>,
    mut tags: Option<&mut (dyn modifier::Tags + '_)>,
    omitted: &mut Vec<Value>,
) -> Result<Option<(Program, usize)>> {
    let (native_trigger, alternative_triggers) = split(conditions(p, &cycle.activation, labels)?);
    let (native_removal, alternative_removals) = split(conditions(p, &cycle.removal, labels)?);
    let (native_rearm, alternative_rearms) = split(conditions(p, &cycle.rearm, labels)?);
    let mut effects = Effects::default();
    // The compiler writes the action list last to first, and twin perks keep the source order
    // in the Shadowkeep list, so the source effects are taken in reverse.
    for at in cycle.effects.iter().rev() {
        let class = p.u32(at - 4)?;
        let lowered = match (class, tags.as_deref_mut()) {
            (0x8080_B7C6 | 0x8080_3130, Some(tags)) => {
                match modifier::attach(tags, p, *at, name) {
                    Ok(rows) => Ok(rows
                        .into_iter()
                        .map(|row| (row.action, Some(row.patch), None))
                        .collect::<Vec<_>>()),
                    // A re-entry fires nothing that is held, so it converts no entity.
                    Err(rows) if class == 0x8080_B7C6 && !cycle.instant => tags
                        .whole(p, *at)
                        .map(|whole| vec![(whole.action, None, Some(whole.asset))])
                        .map_err(|whole| anyhow::anyhow!("{rows:#}. Whole entity: {whole:#}")),
                    Err(rows) => Err(rows),
                }
            }
            // A spawn creates its entity once, so it also fires on a re-entry.
            (0x8080_3128, Some(tags)) => tags
                .whole(p, *at)
                .map(|whole| vec![(whole.action, None, Some(whole.asset))]),
            _ => lower::effect_with(p, *at, labels)
                .map(|node| vec![(Action::Native { node: native(node) }, None, None)]),
        };
        match lowered {
            Ok(rows) => {
                // A count that cannot be read records nothing. It never refuses the effect.
                let left_out = match (class, tags.as_deref_mut()) {
                    (0x8080_B7C6 | 0x8080_3130, Some(tags)) => {
                        modifier::left_out(tags, p, *at).unwrap_or(0)
                    }
                    _ => 0,
                };
                if left_out != 0 {
                    omitted.push(json!({
                        "effect_offset": at,
                        "approximation": format!(
                            "{left_out} modifier rows of the attached entity change an input Shadowkeep does not have and are left out"
                        ),
                    }));
                }
                // Creation values that follow the perk's own count are fixed at a full count.
                let fixed = match class {
                    0x8080_B7C6 => lower::attachment::bound(p, at + 48).unwrap_or(0),
                    0x8080_3128 => lower::attachment::bound(p, at + 0x80).unwrap_or(0),
                    _ => 0,
                };
                if fixed != 0 {
                    omitted.push(json!({
                        "effect_offset": at,
                        "approximation": format!(
                            "{fixed} creation values of the entity follow the perk's count and are fixed at its full count"
                        ),
                    }));
                }
                for (action, patch, group) in rows {
                    effects.push(action, patch, group, cycle.instant);
                }
            }
            Err(error) => omitted.push(json!({
                "effect_offset": at,
                "effect_class": format!("{class:08X}"),
                "source_kind": p.u8(*at)?,
                "reason": format!("{error:#}"),
            })),
        }
    }
    let count = effects.actions.len();
    if let Some(key) = cycle.marker {
        // The form of the stock effects that hold `A43A8C2E` on their own weapon.
        effects.actions.push(Action::Property {
            key,
            target: 0,
            operation_byte: 0,
            removal: 1,
            value_bits: 1.0_f32.to_bits(),
            restore_bits: 0,
            ability_mask: 0,
            input: 0,
            flag: 1,
        });
    }
    if effects.actions.is_empty() {
        return Ok(None);
    }
    let program = Program {
        name: name.to_owned(),
        trigger: if native_trigger.is_some() {
            Trigger::Native
        } else {
            Trigger::Always
        },
        duration_ms: 0,
        cooldown_ms: 0,
        actions: effects.actions,
        native_trigger,
        alternative_triggers,
        native_removal,
        alternative_removals,
        native_rearm,
        alternative_rearms,
        native_asset_patches: effects.patches,
        imported_assets: effects.groups,
        ..Program::default()
    };
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok(Some((program, count)))
}

/// The modern HUD report `record` as Shadowkeep's `80804085`, which shows a status while its
/// action holds a state. On 48 twin reports the first byte 0 is Shadowkeep's state mask 2 and
/// 1 is mask 4, the scalar pair and its input selector carry over, and the two display flags
/// follow the source's on 47. Shadowkeep's sort order is at most 5 where the source's runs to
/// 450, and equals it only where the source's is that small. The name key differs between
/// the builds, so the report takes a HUD status of the project's own under the perk's name.
pub fn report(
    p: &Payload,
    record: &controller::Auxiliary,
    name: &str,
    tags: Option<&mut (dyn modifier::Tags + '_)>,
) -> Result<NativeRecord> {
    let at = record.offset;
    ensure!(
        record.class == controller::STATE_AUXILIARY,
        "the HUD report class has no Shadowkeep record on this route"
    );
    let head = p.bytes::<8>(at)?;
    let selector = p.u32(at + 40)?;
    let flags = p.bytes::<8>(at + 56)?;
    ensure!(
        head[0] <= 1
            && head[1] == 0xFF
            && head[2..] == [0; 6]
            && matches!(selector, 0 | 255)
            && p.u32(at + 44)? == EMPTY
            && p.u64(at + 48)? == 0
            && flags[0] <= 1
            && flags[1] <= 1
            // The third flag has no Shadowkeep byte: ten twin reports set it and their
            // Shadowkeep records equal those of reports that leave it clear.
            && flags[2] <= 1
            && flags[3..] == [0; 5],
        "the HUD report has a count input, a grouping or unread flags"
    );
    let mut bytes = vec![0; 24];
    bytes[0] = if head[0] == 0 { 2 } else { 4 };
    bytes[1] = 0xFF;
    // The build writes the key of the report's own HUD status here.
    bytes[4..8].copy_from_slice(&p.bytes::<4>(at + 24)?);
    let order = p.u32(at + 28)?;
    if order <= 5 {
        bytes[8..12].copy_from_slice(&order.to_le_bytes());
    }
    bytes[12..20].copy_from_slice(&p.bytes::<8>(at + 32)?);
    bytes[20] = selector as u8;
    bytes[21] = flags[0];
    bytes[22] = flags[1];
    // The source key names the report's row in the source HUD table, which holds its icon.
    let key = p.u32(at + 24)?;
    Ok(NativeRecord {
        class: 0x8080_4085,
        bytes,
        hud_status: Some(HudStatus {
            name: name.to_owned(),
            icon: Some(modifier::BUFF_ICON),
            image: tags.and_then(|tags| tags.status_image(key)),
        }),
    })
}

/// The modern feedback record `record` as Shadowkeep's `80804087`, which creates an entity
/// while its action holds a state. On 56 twin records the first byte and the flag word carry
/// over as in the report, on 53 of them, and the entity is a stock Shadowkeep one. The record
/// names its entity by a 64-bit name, and [`table::feedback`] holds the Shadowkeep entity of
/// each name twins agree on. An entity no twin shows is left out.
fn feedback(p: &Payload, record: &controller::Auxiliary) -> Result<NativeRecord> {
    let at = record.offset;
    let head = p.bytes::<8>(at)?;
    let flag = p.u64(at + 24)?;
    ensure!(
        head[0] <= 1
            && head[1] == 0xFF
            && head[2..] == [0; 6]
            && flag <= 2
            && p.u32(at + 40)? == u32::MAX
            && p.u32(at + 44)? == 0,
        "the feedback record has unread flags or names its entity directly"
    );
    let name = p.u64(at + 48)?;
    let entity = table::feedback(name)?.with_context(|| {
        format!("the feedback entity {name:016X} has no Shadowkeep entity among twin perks")
    })?;
    let mut bytes = vec![0; 24];
    bytes[0] = if head[0] == 0 { 2 } else { 4 };
    bytes[1] = 0xFF;
    bytes[2] = flag as u8;
    bytes[16..20].copy_from_slice(&entity.to_le_bytes());
    Ok(NativeRecord {
        class: 0x8080_4087,
        bytes,
        hud_status: None,
    })
}

/// Whether the condition at `at` is the plain unconditional one.
fn unconditional(p: &Payload, node: &controller::Condition) -> Result<bool> {
    Ok(node.class == 0x8080_30C0 && p.bytes::<7>(node.offset)? == UNCONDITIONAL)
}

/// Drops the states that only start the machine. A first state that holds nothing, leaves at
/// once and is never entered again only starts the machine. Twin perks with such a state pair
/// the states after it with the Shadowkeep cycle.
fn drop_starters(p: &Payload, source: &mut Vec<&controller::State>) -> Result<()> {
    while source.len() > 1 {
        let first = source[0];
        let [transition] = first.transitions.as_slice() else {
            break;
        };
        let [condition] = transition.conditions.as_slice() else {
            break;
        };
        let entered_again = source.iter().any(|state| {
            state
                .transitions
                .iter()
                .any(|transition| transition.destination == first.offset)
        });
        if transition.destination != source[1].offset
            || entered_again
            || !unconditional(p, condition)?
            || !translate::state_effects(p, first)?.is_empty()
            || p.u64(first.offset + 24)? != 0
        {
            break;
        }
        source.remove(0);
    }
    Ok(())
}

/// Validate source states and resolve their transition destinations before planning cycles.
fn read_states<'a>(
    p: &Payload,
    source: Vec<&'a controller::State>,
    omitted: &mut Vec<Value>,
) -> Result<Vec<State<'a>>> {
    let offsets = source.iter().map(|state| state.offset).collect::<Vec<_>>();
    let mut states = Vec::new();
    for state in source {
        // The mask at `+60` is the timer-extension mask of the Shadowkeep root (`+A0`), equal
        // on every twin that sets it, and the compiler derives it from the effects.
        let at = state.offset;
        ensure!(
            p.u32(at + 4)? == 0
                && p.u64(at + 48)? == 0
                && p.u64(at + 56)? == 0
                && p.u64(at + 72)? == 0
                && p.u64(at + 80)? == 0
                && p.u64(at + 88)? == u64::MAX
                && p.bytes::<32>(at + 104)? == [0; 32],
            "unsupported source state metadata at {at:X}"
        );
        // The Float32 at `+18` is set on 92 twin states to thirteen values from 0.001 to 9.8
        // with no Shadowkeep field that follows it, so it is read past. No twin sets the one at
        // `+1C`. It is read here as the seconds a state is held, which only a state with one
        // unconditional exit can express: that exit becomes a timer.
        let rate = p.f32(at + 24)?;
        let seconds = p.f32(at + 28)?;
        ensure!(
            rate.is_finite() && rate >= 0.0 && seconds.is_finite() && seconds >= 0.0,
            "unsupported source state timing at {at:X}"
        );
        let mut timed = false;
        if let [transition] = state.transitions.as_slice() {
            timed = true;
            for condition in &transition.conditions {
                timed &= unconditional(p, condition)?;
            }
        }
        let held = if seconds == 0.0 {
            None
        } else if timed {
            omitted.push(json!({
                "inference": format!(
                    "a state held {seconds} seconds before its unconditional exit is written as a timer"
                ),
            }));
            Some(seconds)
        } else {
            // A state with conditional exits has no timer to carry the seconds, and no twin
            // shows what they delay. Its exits are written as they stand, so they can be
            // taken that much sooner than in the source.
            omitted.push(json!({
                "approximation": format!(
                    "the {seconds} seconds a state is held before its conditional exits are not kept"
                ),
            }));
            None
        };
        let mut destinations = Vec::new();
        for transition in &state.transitions {
            let at = transition.offset;
            ensure!(
                p.u32(at + 4)? == 0 && p.bytes::<16>(at + 32)? == [0; 16],
                "unsupported source transition metadata at {at:X}"
            );
            ensure!(
                !transition.conditions.is_empty(),
                "source transition at {at:X} has no condition"
            );
            destinations.push(
                offsets
                    .iter()
                    .position(|offset| *offset == transition.destination)
                    .context("source transition destination")?,
            );
        }
        states.push(State {
            source: state,
            effects: translate::state_effects(p, state)?,
            destinations,
            held,
        });
    }
    Ok(states)
}

/// Translate one controller into the programs its states form. Returns the programs and the
/// effects and reports left out. Fails when the states form no cycle, a condition does not
/// translate or no effect does.
pub fn programs(
    p: &Payload,
    name: &str,
    labels: Option<&Registry>,
    mut tags: Option<&mut (dyn modifier::Tags + '_)>,
) -> Result<(Vec<Program>, Vec<Value>)> {
    let decoded = controller::read(p)?;
    // Bytes +69 and +70 are the retained-state and timer reservations, which equal the
    // Shadowkeep root's on 708 of 807 twins and which the compiler counts again. Byte +18 is
    // 0 or 1 on every twin, with no Shadowkeep field that follows it.
    ensure!(
        p.u64(8)? == 0
            && p.u16(16)? == 0
            && p.u8(18)? <= 1
            && p.bytes::<5>(19)? == [0; 5]
            && p.u32(64)? != 0
            && p.u8(68)? <= 1
            && p.u8(71)? == 0
            && p.u32(72)? == 0,
        "unsupported source controller metadata"
    );
    let mut source = decoded.states.iter().collect::<Vec<_>>();
    let mut omitted = Vec::new();
    drop_starters(p, &mut source)?;
    let states = read_states(p, source, &mut omitted)?;
    let cycles = match cycles(p, &states) {
        Ok(cycles) => {
            if cycles.len() > 1 && states[0].effects.is_empty() {
                omitted.push(json!({
                    "approximation": "the source holds one branch at a time, while its Shadowkeep programs can overlap",
                }));
            }
            cycles
        }
        Err(shape) => {
            let cycles = decompose(p, &states).with_context(|| format!("{shape:#}"))?;
            ensure!(
                cycles.len() <= PROGRAM_LIMIT,
                "{shape:#}, and its states need {} programs where a plug carries {PROGRAM_LIMIT}",
                cycles.len()
            );
            omitted.push(json!({
                "inference": format!(
                    "{} source states are written as {} programs that mark their state in a named property and gate on each other's marks, a form with no twin that has not been run",
                    states.len(),
                    cycles.len()
                ),
            }));
            cycles
        }
    };
    // The controller key is the Shadowkeep root key: equal on 31 of the 34 twins that hold
    // one on both sides. Its role is unresolved in both builds.
    let key = p.u32(64)?;
    let mut programs = Vec::new();
    let mut reported = Vec::new();
    let mut translated = 0;
    for cycle in cycles {
        if let Some((mut program, effects)) =
            program(p, &cycle, name, labels, tags.as_deref_mut(), &mut omitted)?
        {
            translated += effects;
            // A HUD report shows while the state it names is held, so it joins the cycle
            // whose effects that state holds.
            for record in &decoded.auxiliary {
                if record.state_offsets != [cycle.state] || reported.contains(&record.offset) {
                    continue;
                }
                reported.push(record.offset);
                let translated = if record.class == controller::FEEDBACK_AUXILIARY {
                    feedback(p, record)
                } else {
                    report(p, record, name, tags.as_deref_mut())
                };
                match translated {
                    Ok(record) => program.auxiliary.push(record),
                    Err(error) => omitted.push(json!({
                        "report_class": format!("{:08X}", record.class),
                        "reason": format!("{error:#}"),
                    })),
                }
            }
            program.validate_structure().map_err(anyhow::Error::msg)?;
            if key != EMPTY {
                program.policy = Some(Policy {
                    selector: 0,
                    modifier: 0,
                    key,
                    configuration: None,
                });
                program.validate_structure().map_err(anyhow::Error::msg)?;
            }
            programs.push(program);
        }
    }
    for record in &decoded.auxiliary {
        if !reported.contains(&record.offset) {
            omitted.push(json!({
                "report_class": format!("{:08X}", record.class),
                "reason": if matches!(
                    record.class,
                    controller::STATE_AUXILIARY | controller::FEEDBACK_AUXILIARY
                ) {
                    "the HUD report names no state whose effects translate"
                } else {
                    "the HUD report class has no Shadowkeep record on this route"
                },
            }));
        }
    }
    ensure!(
        translated != 0,
        "source controller has no translated effects: {}",
        serde_json::to_string(&omitted)?
    );
    Ok((programs, omitted))
}
