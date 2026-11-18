//! Qualify the activating and cancelling actions together before adapting their target state.
use super::*;
use crate::d1_roi::effect::Assignment;

pub(super) struct Plan {
    cancel: u32,
    condition: usize,
    marker: Assignment,
}

struct Gate {
    action: u32,
    condition: usize,
    delay: f32,
    duration: f32,
    repeat: f32,
    ending: u32,
}

struct Graph {
    tuning: Option<u32>,
    key: u32,
    priority: i32,
}

fn nodes(p: &Payload, field: usize) -> Result<Vec<usize>> {
    p.array(field, 8, None)?
        .into_iter()
        .map(|row| p.pointer(row))
        .collect()
}

fn times(p: &Payload) -> Result<Option<(f32, f32)>> {
    let mut found = Vec::new();
    for at in nodes(p, 0x38)? {
        if p.u32(at - 4)? == 0x80801101 {
            ensure!(
                p.bytes::<4>(at)? == [38, 0, 0, 0],
                "Assignment effect header differs"
            );
            found.push((p.f32(at + 4)?, p.f32(at + 8)?));
        }
    }
    ensure!(
        found.len() <= 1,
        "Action contains multiple team assignments"
    );
    Ok(found.pop())
}

fn victim(p: &Payload) -> Result<u32> {
    let mut found = Vec::new();
    for at in nodes(p, 0x38)? {
        if p.u32(at - 4)? == 0x80802953 && p.u8(at + 2)? == 3 {
            ensure!(
                p.bytes::<3>(at)? == [0, 1, 3],
                "Assignment attachment lifecycle differs"
            );
            found.push(p.u32(at + 0x10)?);
        }
    }
    ensure!(found.len() == 1, "Assignment action needs one victim graph");
    Ok(found[0])
}

impl Gate {
    fn read(action: u32, p: &Payload) -> Result<Option<Self>> {
        let conditions = nodes(p, 0x20)?;
        let mut found = Vec::new();
        for &at in &conditions {
            if p.u32(at - 4)? == 0x8080307B && p.u8(at + 0x175)? != 0 {
                found.push(at);
            }
        }
        if found.is_empty() {
            return Ok(None);
        }
        ensure!(
            conditions.len() == 1 && found.len() == 1,
            "Assignment cancellation needs one primary hit condition"
        );
        let at = found[0];
        let delay = p.f32(at + 0x178)?;
        let duration = p.f32(at + 0x17C)?;
        let (seconds, repeat) = times(p)?.context("Assignment cancellation has no team effect")?;
        ensure!(
            p.u8(at + 5)? == 4
                && p.u8(at + 0x174)? == 2
                && p.bytes::<3>(at + 0x175)? == [1, 0, 0]
                && delay.is_finite()
                && duration.is_finite()
                && repeat.is_finite()
                && delay > 0.0
                && delay < duration
                && duration <= 3600.0
                && seconds == duration
                && (0.0..=delay).contains(&repeat),
            "Assignment cancellation timing or condition differs"
        );
        Ok(Some(Self {
            action,
            condition: at,
            delay,
            duration,
            repeat,
            ending: victim(p)?,
        }))
    }
}

fn activation(p: &Payload) -> Result<()> {
    let primary = nodes(p, 0x20)?;
    ensure!(
        primary.len() == 1,
        "Assignment activation needs one compound player condition"
    );
    let at = primary[0];
    ensure!(
        p.u32(at - 4)? == 0x80801100 && p.u8(at + 5)? == 36,
        "Assignment activation lacks its player predicate"
    );
    let hit = p.pointer(at + 0xD0)?;
    // Activation only admits hostile targets. Cancellation only admits friendly ones, so an
    // expired allegiance cannot be restarted by the marker's hit condition.
    ensure!(
        p.u32(hit - 4)? == 0x8080307B
            && p.u8(hit + 5)? == 4
            && p.u8(hit + 0x174)? == 1
            && p.u8(hit + 0x175)? == 0,
        "Assignment activation must retain its hostile-target hit filter"
    );
    Ok(())
}

impl Graph {
    fn read(source: &Source, tag: u32) -> Result<Self> {
        ensure!(
            source.reference(tag)? == 0x80800734,
            "Assignment victim is not a graph"
        );
        let graph = source.read(tag)?;
        let mut statuses = Vec::new();
        let mut tuning = None;
        for (ordinal, row) in graph
            .array(0x20, 12, Some(0x80800715))?
            .into_iter()
            .enumerate()
        {
            let tag = graph.u32(row)?;
            let owner = source.read(tag)?;
            let definition = owner.pointer(24)?;
            match owner.u32(definition - 4)? {
                0x80802698 => statuses.push(pattern::effect::status(&graph, &owner, tag, ordinal)?),
                0x808010FB => {
                    ensure!(
                        tuning.is_none()
                            && owner.bytes::<8>(definition + 0x78)? == [2, 0, 0, 0, 0, 0, 0, 0],
                        "Assignment actor tuning shape differs"
                    );
                    tuning = Some(tag);
                }
                _ => {}
            }
        }
        ensure!(
            statuses.len() == 1,
            "Assignment victim needs one competing status"
        );
        let status = &statuses[0];
        ensure!(
            status.duration[..4] == [5, 0, 0, 0]
                && status.duration[8..12] == 1u32.to_le_bytes()
                && status.duration[16..] == [0; 4],
            "Assignment status competition policy differs"
        );
        Ok(Self {
            tuning,
            key: u32::from_le_bytes(status.duration[4..8].try_into()?),
            priority: i32::from_le_bytes(status.duration[12..16].try_into()?),
        })
    }
}

impl Plan {
    pub(super) fn read(source: &Source, perks: &[Value]) -> Result<Option<Self>> {
        let actions = perks
            .iter()
            .filter_map(|p| p["action"].as_u64())
            .map(|tag| {
                let tag = u32::try_from(tag)?;
                Ok((tag, source.read(tag)?))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut gates = Vec::new();
        for (&tag, payload) in &actions {
            if let Some(gate) = Gate::read(tag, payload)? {
                gates.push(gate)
            }
        }
        if gates.is_empty() {
            return Ok(None);
        }
        ensure!(
            gates.len() == 1,
            "Talent has multiple assignment cancellation gates"
        );
        let gate = gates.remove(0);
        let ending = Graph::read(source, gate.ending)?;
        ensure!(
            ending.tuning.is_none(),
            "Cancellation graph reapplies actor tuning"
        );
        let mut starts = Vec::new();
        for (&tag, payload) in &actions {
            if tag != gate.action && times(payload)? == Some((gate.duration, gate.repeat)) {
                activation(payload)?;
                starts.push(victim(payload)?);
            }
        }
        ensure!(
            starts.len() == 1,
            "Assignment cancellation needs one activating action"
        );
        let graph = starts[0];
        let active = Graph::read(source, graph)?;
        ensure!(
            active.key == ending.key
                && ending.priority > active.priority
                && ![0, u32::MAX, 0x811C9DC5].contains(&active.key),
            "Cancellation must retire the same active status"
        );
        let owner = active
            .tuning
            .context("Assignment activation has no qualified actor tuning owner")?;
        let key = sundial::package_authoring::fnv1_name_hash(&format!(
            "parhelion/d1/assignment/{:08X}",
            active.key
        ));
        ensure!(
            key != active.key,
            "Assignment marker collides with the active status"
        );
        Ok(Some(Self {
            cancel: gate.action,
            condition: gate.condition,
            marker: Assignment {
                graph,
                owner,
                key,
                delay: gate.delay,
                duration: gate.duration,
            },
        }))
    }

    pub(super) fn prepare(
        &self,
        source: &Source,
        native: &mut Reader,
        folder: &Path,
    ) -> Result<()> {
        pattern::closure::prepare_assignment(source, native, folder, &self.marker)?;
        Ok(())
    }
}

pub(super) fn lower(
    context: &mut StepContext<'_>,
    plan: Option<&Plan>,
    tag: u32,
    name: &str,
) -> Result<(Program, usize)> {
    let Some(plan) = plan.filter(|plan| plan.cancel == tag) else {
        return program(
            context.source,
            tag,
            name,
            Some(context.native),
            Some(context.folder),
        );
    };
    ensure!(
        context.source.reference(tag)? == 0x80802711,
        "Cancellation is not an action controller"
    );
    let mut normalized = context.source.read(tag)?;
    normalized.0[plan.condition + 0x175..plan.condition + 0x180].fill(0);
    let (mut program, omitted) = action_body(
        context.source,
        &normalized,
        name,
        Some(context.native),
        Some(context.folder),
        &mut vec![tag],
    )?;
    let hit = program
        .native_trigger
        .as_mut()
        .context("Cancellation lost its native hit condition")?;
    ensure!(
        hit.kind == 4 && program.alternative_triggers.is_empty(),
        "Cancellation trigger changed kind"
    );
    put(&mut hit.bytes, 0x9C, &plan.marker.key.to_le_bytes())?;
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok((program, omitted))
}
