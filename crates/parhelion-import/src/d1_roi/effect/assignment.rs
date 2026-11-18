//! A delayed native marker owns the hit-cancellation eligibility of a temporary team assignment.
use super::*;
use crate::tiger::{instance, sequence};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Assignment {
    pub graph: u32,
    pub owner: u32,
    pub key: u32,
    pub delay: f32,
    pub duration: f32,
}

const SEQUENCE: u32 = 0x80FE6F9B;
const SEQUENCE_GRAPH: u32 = 0x80FE6F99;
const HOST: u32 = 0x8157808D;
const HOST_GRAPH: u32 = 0x80BC830F;

pub(super) fn write(
    native: &mut Reader,
    scope: &component::Scope,
    plan: &Assignment,
    ordinal: usize,
) -> Result<(Component, Vec<Pending>)> {
    ensure!(
        plan.delay.is_finite()
            && plan.duration.is_finite()
            && plan.delay > 0.0
            && plan.delay < plan.duration
            && ![0, u32::MAX, 0x811C9DC5].contains(&plan.key),
        "Assignment marker timing or identity differs"
    );
    let template = (*native.tag(SEQUENCE, Some(0x80809C36))?).clone();
    let entity = (*native.tag(SEQUENCE_GRAPH, Some(0x80809C0F))?).clone();
    let allocation_tag = template.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809BBB))?).clone();
    let mut payload = template.clone();
    let definition = payload.pointer(24)?;
    ensure!(
        payload.u64(definition + 0x188)? == 0
            && payload.u64(definition + 0x2B8)? == 0
            && payload.u64(definition + 0x178)? == 0
            && payload.u64(definition + 0x1B0)? == 1,
        "Assignment sequence template has other inputs, outputs or locators"
    );
    let child_symbol = format!("{}-assignment-marker", scope.symbol);
    let root = sequence::Node {
        name: plan.key,
        timing: sequence::Timing::default(),
        kind: sequence::Kind::Parallel(vec![sequence::Node {
            name: plan.key,
            timing: sequence::Timing {
                delay: [plan.delay, 0.0],
                duration: [plan.duration - plan.delay, 0.0],
                // Native mode-zero creation removes its attached graph on event retirement.
                flags: 0x18,
            },
            kind: sequence::Kind::Event(0x80808881),
        }]),
    };
    let written = sequence::write(
        &mut payload,
        &mut allocation,
        &root,
        std::slice::from_ref(&template),
    )?;
    let event = written.events[&plan.key];
    let graph_field = sequence::spawn::write(
        &mut payload,
        event,
        &sequence::spawn::Parameters {
            locator: 0,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 2],
            speed: [0.0; 2],
            cone: 0.0,
            angular_min: [0.0; 3],
            angular_max: [0.0; 3],
        },
    )?;
    payload.0[event + 0x72] = 0;
    // Start once with the graph. The source status owns the parent graph's retirement.
    payload.0[definition + 0x2E0] = 9;
    payload.0[definition + 0x2E1..definition + 0x2E4].fill(0);
    let id = scope
        .base
        .checked_add(u32::try_from(ordinal)?)
        .context("Assignment owner identity overflow")?;
    retarget(&mut payload, SEQUENCE, id)?;
    let mut patches = vec![json!({"offset":graph_field,"symbol":child_symbol})];
    let end = payload.0.len();
    instance::seal_span(&mut payload, &mut patches, end)?;
    let patches = patches
        .into_iter()
        .map(|patch| {
            Ok((
                usize::try_from(
                    patch["offset"]
                        .as_u64()
                        .context("Assignment patch offset")?,
                )?,
                patch["symbol"]
                    .as_str()
                    .context("Assignment patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let size = payload.0.len() as u64;
    put(&mut payload.0, 0, &size.to_le_bytes())?;
    let child_id = scope
        .base
        .checked_add(0x80)
        .context("Assignment marker identity overflow")?;
    let additional = marker(native, &child_symbol, child_id, plan)?;
    Ok((
        Component {
            id,
            ordinal,
            family: Family::Sequence,
            symbol: format!("{}-assignment-sequence", scope.symbol),
            template_tag: SEQUENCE,
            template,
            entity,
            payload,
            patches,
            allocation: Some((allocation_tag, allocation)),
            evidence: json!({"assignment_marker":plan,"ai_tuning":"native"}),
        },
        additional,
    ))
}

fn marker(native: &mut Reader, symbol: &str, id: u32, plan: &Assignment) -> Result<Vec<Pending>> {
    let template = (*native.tag(HOST, Some(0x80809C36))?).clone();
    let entity = (*native.tag(HOST_GRAPH, Some(0x80809C0F))?).clone();
    let mut payload = template.clone();
    retarget(&mut payload, HOST, id)?;
    let mut duration = [0; 20];
    duration[0] = 5;
    duration[4..8].copy_from_slice(&plan.key.to_le_bytes());
    duration[8..12].copy_from_slice(&1u32.to_le_bytes());
    let seconds = (plan.duration - plan.delay)
        .to_le_bytes()
        .repeat(4)
        .try_into()
        .unwrap();
    let timer = payload.bytes::<8>(payload.pointer(24)? + 0x48 + 0x120)?;
    owner::host(
        &mut payload,
        &pattern::Host {
            duration,
            timer,
            lifetime: pattern::Lifetime::Constant(seconds),
            lifetime_input: None,
        },
    )?;
    let links = links::Graph::read(&entity, false)?;
    let mut connections = Vec::new();
    for row in links.connections {
        let Some(consumer) = row.consumer.object else {
            continue;
        };
        if consumer.owner != HOST {
            continue;
        }
        ensure!(
            row.provider.object.is_none(),
            "Marker host has a required stock provider"
        );
        connections.push(links::Connection {
            consumer: endpoint(id, consumer.class, usize::try_from(consumer.offset)?)?,
            provider: row.provider,
            channel: row.channel,
            flags: row.flags,
        });
    }
    let rows = links::Graph {
        components: vec![id],
        connections,
        named_connections: Vec::new(),
    }
    .native_rows(
        &[id],
        &BTreeMap::from([(id, payload.clone())]),
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let mut graph = compiled::emit_authored(
        &entity,
        &[compiled::Component {
            owner: id,
            payload: &payload,
            template: &template,
            entity: &entity,
        }],
        &rows,
        &BTreeMap::new(),
    )?;
    // This local marker has no network owner or replication registration.
    put(&mut graph.0, 0x88, &u32::MAX.to_le_bytes())?;
    let owner_symbol = format!("{symbol}-duration");
    let mut owner_patches = words(&payload, id, &owner_symbol)?;
    let allocation_tag = template.u32(0x44)?;
    let allocation_symbol = format!("{symbol}-allocation");
    owner_patches.push((0x44, allocation_symbol.clone()));
    Ok(vec![
        Pending {
            symbol: symbol.to_owned(),
            template: HOST_GRAPH,
            patches: words(&graph, id, &owner_symbol)?,
            payload: graph,
        },
        Pending {
            symbol: allocation_symbol,
            template: allocation_tag,
            payload: (*native.tag(allocation_tag, Some(0x80809BBB))?).clone(),
            patches: Vec::new(),
        },
        Pending {
            symbol: owner_symbol,
            template: HOST,
            payload,
            patches: owner_patches,
        },
    ])
}
