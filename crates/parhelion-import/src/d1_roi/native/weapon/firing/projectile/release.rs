//! A remote detonation provider no component reads, translated as a canister drop on trigger
//! release while the projectile flies on.
//!
//! Shadowkeep's remote detonation provider, class `8080378A` with key `2F4A6E81` (the FNV-1 hash
//! of `remote_detonation`) at definition `+0x98`, publishes the name at `+0x90`, `CD7422C6` in
//! stock. Remote grenades read it through their bank as a held value: their movement bounces at
//! or above 1 and detonates on the first contact below it. Rise of Iron's Dragon's Breath rocket
//! keeps such a provider while neither its bank nor its carrier's declares the name, and its
//! talent drops a napalm canister on release. Shadowkeep creates that canister only in the
//! final-impact graph's flow, at one kind-15 creation event under a gate on a receiver channel.
//!
//! A private copy of that flow joins the projectile as a component of its own. Its controls are
//! rebuilt as a gate on the provider's value below 1, started after the provider's arming hold,
//! holding a copy of the impact gate, holding the stock creation event, so the canister is created
//! straight below the projectile's own locator while the projectile flies on. One of the copy's inputs reads the provider's name, which the private
//! bank declares as an incoming getter fed by the provider. The other inputs keep their names and
//! read the private bank by name. The impact flow's other events stay unreachable, and the flow no
//! longer requests its owner's retirement when its root completes.
use super::*;
use crate::d1_roi::native::component::Component;
use crate::tiger::sequence::{self, Kind, Node, Timing};

const PROVIDER: u32 = 0x8080_378A;
const KEY: u32 = 0x2F4A_6E81;
const FLOW: u32 = 0x8080_84E9;
const CREATE: u32 = 0x8080_8881;
const CONDITION: u32 = 0x8080_93ED;
/// Remote grenade gates treat the provider as held at or above 1.
const HELD: f32 = 1.;
/// The release gate starts this long after the provider's arming hold, the first value of its
/// block at definition `+0x9C`, 0.3 seconds in stock. Until a held trigger arms, the provider reads
/// below 1, so a gate started earlier dropped the canister at the muzzle on every shot. A gate
/// reads a level and keeps no latch, so a tap still drops the canister when the gate starts.
const MARGIN: f32 = 0.1;
/// The canister is created this far straight below the rocket, in world space, Z up. At the
/// rocket's own locator the rocket's next sweep begins inside the new canister and takes the
/// count-limited ricochet response, which bounced the rocket. The rocket and canister movements
/// hold 0.06 and 0.07 at definition `+0x7C`.
const DROP: f32 = 0.5;
/// Node flag that stores the no-expiry sentinel in place of a sampled duration, tested with
/// `0x08` at `58753E`. Without it a gate with zero duration expires the frame it activates, and
/// the root completes before the trigger is released. Every examined stock range gate holds it,
/// among them the remote detonation flows `80FDBF75` and `80FDBF76`.
const PERSISTENT: u32 = 0x10;

pub(super) struct Plan {
    /// The provider's published name.
    pub name: u32,
    pub provider: u32,
    /// The provider's scalar getter slot, its third slot at definition `+0x78`.
    pub getter: usize,
    graph: u32,
    flow: u32,
    event: usize,
    /// The impact gate's input ordinal and range.
    gate: (u32, [f32; 2]),
    /// Each flow input's definition offset and name, with the release input renamed.
    pub inputs: Vec<(usize, u32)>,
    release: usize,
    /// The provider's arming hold in seconds.
    hold: f32,
}

/// The release a carrier's provider supplies, when no component reads it and the final-impact
/// graph creates one object under a receiver gate.
pub(super) fn plan(
    native: &mut Reader,
    carrier: &graph::Graph,
    declared: &[u32],
) -> Result<Option<Plan>> {
    let Some(provider) = carrier.owners.get(&graph::Role::Provider) else {
        return Ok(None);
    };
    let p = &provider.payload;
    let d = provider.definition;
    if p.u32(d - 4)? != PROVIDER || p.u32(d + 0x98)? != KEY {
        return Ok(None);
    }
    let name = p.u32(d + 0x90)?;
    if declared.contains(&name) {
        return Ok(None);
    }
    ensure!(
        p.pointer(d + 0x78)? == d,
        "Remote detonation provider getter slot differs"
    );
    let hold = p.f32(d + 0x9C)?;
    ensure!(
        hold.is_finite() && (0.0..=10.0).contains(&hold),
        "Remote detonation provider arming hold is unqualified"
    );
    let movement = carrier.owner(graph::Role::Movement)?;
    let graph = movement.payload.u32(movement.definition + 0x180)?;
    if graph == u32::MAX {
        return Ok(None);
    }
    let entity = native.tag(graph, Some(0x8080_9C0F))?;
    let mut found = Vec::new();
    for row in entity.array(16, 12, Some(0x8080_9C04))? {
        let tag = entity.u32(row)?;
        let owner = native.tag(tag, Some(0x8080_9C36))?;
        let definition = owner.pointer(24)?;
        if owner.u32(definition - 4)? != FLOW {
            continue;
        }
        for row in owner.array(definition + 0x168, 24, Some(0x8080_93E6))? {
            let event = owner.pointer(row + 16)?;
            if owner.u32(event - 4)? == CREATE && owner.u32(event + 0x30)? == 15 {
                found.push((tag, event));
            }
        }
    }
    let [(flow, event)] = found[..] else {
        return Ok(None);
    };
    let owner = native.tag(flow, Some(0x8080_9C36))?;
    let definition = owner.pointer(24)?;
    // The creation event's parent is a control, domain 0, whose condition is its gate.
    let parent = owner.u32(event + 0x14)?;
    ensure!(
        parent & 0xFFFF == 0,
        "Impact creation event has no parent control"
    );
    let controls = owner.array(definition + 0x158, 24, Some(0x8080_93E6))?;
    let control = owner.pointer(
        *controls
            .get((parent >> 16) as usize)
            .context("Impact creation control")?
            + 16,
    )?;
    if owner.u64(control + 0x48)? == 0 {
        return Ok(None);
    }
    let condition = owner.pointer(control + 0x48)?;
    ensure!(
        owner.u32(condition - 4)? == CONDITION && owner.u32(condition)? == 0,
        "Impact creation gate is not a scalar range"
    );
    let gate = (
        owner.u32(condition + 12)?,
        [owner.f32(condition + 4)?, owner.f32(condition + 8)?],
    );
    let mut inputs = owner
        .array(definition + 0x188, 40, Some(0x8080_9789))?
        .into_iter()
        .map(|at| Ok((at, owner.u32(at + 32)?)))
        .collect::<Result<Vec<_>>>()?;
    let release = (0..inputs.len())
        .find(|&index| index as u32 != gate.0)
        .context("Impact flow has no input besides its gate")?;
    ensure!(
        (gate.0 as usize) < inputs.len(),
        "Impact creation gate input is undeclared"
    );
    inputs[release].1 = name;
    Ok(Some(Plan {
        name,
        provider: provider.tag,
        getter: d + 0x78,
        graph,
        flow,
        event,
        gate,
        inputs,
        release,
        hold,
    }))
}

pub(super) struct Flow {
    pub component: Component,
    /// The stock allocation the private copy extends.
    pub allocation: u32,
    pub report: Value,
}

pub(super) const ALLOCATION: &str = "projectile-release-allocation";

/// The private release flow at component `index`, with its private allocation published.
pub(super) fn component(assembly: &mut Assembly<'_>, plan: &Plan, index: usize) -> Result<Flow> {
    let mut flow = assembly.component(plan.flow, plan.graph, "projectile-release", index)?;
    // The envelope supplies the creation event's default under the private identity.
    let stock = flow.payload.clone();
    for &(at, name) in &plan.inputs {
        put(&mut flow.payload.0, at + 32, &name.to_le_bytes())?;
    }
    let allocation_tag = flow.payload.u32(0x44)?;
    let mut allocation = (*assembly.native.tag(allocation_tag, Some(0x8080_9BBB))?).clone();
    let hash = crate::tiger::audio::bank::hash;
    let create = hash("projectile/release/create");
    let arming = plan.hold + MARGIN;
    let root = Node {
        name: hash("projectile/release/root"),
        timing: Timing::default(),
        kind: Kind::Parallel(vec![Node {
            name: hash("projectile/release/gate"),
            timing: Timing {
                delay: [arming; 2],
                flags: PERSISTENT,
                ..Timing::default()
            },
            kind: Kind::Range {
                input: u32::try_from(plan.release)?,
                bounds: [-f32::MAX, HELD],
                children: vec![Node {
                    name: hash("projectile/release/receiver"),
                    timing: Timing {
                        flags: PERSISTENT,
                        ..Timing::default()
                    },
                    kind: Kind::Range {
                        input: plan.gate.0,
                        bounds: plan.gate.1,
                        children: vec![Node {
                            name: create,
                            timing: Timing::default(),
                            kind: Kind::Event(CREATE),
                        }],
                    },
                }],
            },
        }]),
    };
    let written = sequence::write(
        &mut flow.payload,
        &mut allocation,
        &root,
        std::slice::from_ref(&stock),
    )?;
    sequence::adopt(&mut flow.payload, written.events[&create], plan.event)?;
    // Creation handler `0040C0A0` adds offset `+0x40` to the locator's position, in world space
    // when byte `+0x71` is set (`0040C427..0040C446`) and rotated by the locator otherwise. The
    // impact event holds no offset, since there the rocket ends at the canister.
    ensure!(
        flow.payload.bytes::<16>(plan.event + 0x40)? == [0; 16]
            && flow.payload.u8(plan.event + 0x71)? == 0,
        "Impact creation event already holds an offset"
    );
    let mut offset = [0; 16];
    offset[8..12].copy_from_slice(&(-DROP).to_le_bytes());
    put(&mut flow.payload.0, plan.event + 0x40, &offset)?;
    flow.payload.0[plan.event + 0x71] = 1;
    // Definition `+0x2E0` bit 1 makes root completion request the owner's retirement, which
    // would end the projectile.
    let definition = flow.payload.pointer(24)?;
    let flags = flow.payload.u8(definition + 0x2E0)?;
    flow.payload.0[definition + 0x2E0] = flags & !2;
    assembly.node(ALLOCATION, allocation_tag, &allocation.0, None, &[])?;
    flow.patch(0x44, ALLOCATION)?;
    // The rebuilt controls are runtime records past the stock schema, which the client's
    // instance copy leaves out without a definition view after them.
    flow.seal()?;
    Ok(Flow {
        component: flow,
        allocation: allocation_tag,
        report: json!({"provider":format!("{:08X}",plan.provider),"name":format!("{:08X}",plan.name),
            "impact_graph":format!("{:08X}",plan.graph),"impact_flow":format!("{:08X}",plan.flow),
            "release_input":plan.release,"gate_input":plan.gate.0,"gate":plan.gate.1,
            "held_at_or_above":HELD,"provider_arming_hold":plan.hold,"arming_seconds":arming,
            "gate_flags":PERSISTENT,"flags_before":flags,"drop_below_metres":DROP,"world_offset":true,
            "limits":["The final-impact graph still creates its own canister on impact.",
                "The release gate repeats, so a second release during flight creates another canister.",
                "A tap released before the arming hold drops the canister when the gate starts.",
                "That the provider follows the held trigger after its arming hold is inferred from remote grenade gates and the stock block."],
            "gameplay_verified":false}),
    })
}
