//! Source turn rate on a native target query and steering implementation.
use super::*;
use crate::tiger::entity::links::Object;

fn unbind(p: &mut Payload, definition: usize) -> Result<Vec<Object>> {
    let objects = program::unbind(p, definition)?;
    ensure!(
        objects.len() == 1,
        "Tracking expression input contract differs"
    );
    Ok(objects)
}

fn rates(behavior: &Value) -> Result<[f32; 2]> {
    let rates = behavior["guided_angular_velocity"]
        .as_array()
        .context("Source projectile turn rates")?;
    ensure!(rates.len() == 2, "Source turn-rate bounds differ");
    let rates = rates
        .iter()
        .map(|v| v.as_f64().map(|n| n as f32).context("Source turn rate"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        rates[0].is_finite() && rates[1].is_finite() && rates[0] >= 0. && rates[0] <= rates[1],
        "Invalid source turn-rate bounds"
    );
    Ok([rates[0], rates[1]])
}

pub(super) fn enabled(behavior: &Value) -> Result<bool> {
    Ok(rates(behavior)?[1] > 0.)
}

pub(super) fn policy(behavior: &Value) -> Result<Value> {
    let bounds = rates(behavior)?;
    let at_rest = behavior["guided_angular_velocity_at_rest"]
        .as_f64()
        .context("Source at-rest turn rate")? as f32;
    ensure!(
        at_rest.is_finite() && at_rest >= 0.,
        "Invalid source at-rest turn rate"
    );
    Ok(json!({"rate_bounds_radians_per_second":bounds,
        "sampling":"One uniform sample per launch when the bounds differ",
        "at_rest_radians_per_second":at_rest,
        "ramp_distance_source_units":behavior["acceleration_range"],
        "ramp_inverse_width":behavior["acceleration_inverse_width"],
        "ramp_input":"Integrated commanded speed, with source velocity multiplier scaling",
        "constant_rate_family":bounds[0] == bounds[1] && bounds[0] == at_rest,
        "limits":["Private guided carriers bind a stable native launch seed sample and source-speed distance fraction. Seed sequences, target selection, contact continuation, live-component relaunch and trajectories remain unverified."]}))
}

pub(super) fn prepare(
    owners: &mut BTreeMap<u32, Payload>,
    behavior: &Value,
) -> Result<BTreeSet<Object>> {
    let rates = rates(behavior)?;
    let mut removed = BTreeSet::new();
    for p in owners.values_mut() {
        let d = p.pointer(24)?;
        match assembly::Assembly::class(p)? {
            0x8080377d => {
                let angular = p.pointer(d + 0x160)?;
                ensure!(
                    p.u32(angular - 4)? == 0x80803773,
                    "Steering has no supported angular-rate implementation"
                );
                let expression = angular + 0x18;
                removed.extend(unbind(p, expression)?);
                program::constant(p, expression + 0x10, rates[0].to_degrees())?;
                for field in [0x58, 0xb0, 0x108, 0x170] {
                    removed.extend(program::unbind(p, d + field)?);
                    program::constant(p, d + field + 0x10, 1.)?;
                }
            }
            0x80803769 => {
                let expression = d + 0x58 + 0x18;
                // Retain the native range-dependent acquisition curve at its
                // ordinary launch setting, independent of the grenade throw mode.
                let code = p
                    .array(expression + 0x10, 1, Some(0x80800009))?
                    .into_iter()
                    .map(|at| p.u8(at))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    code == [
                        0x34, 0, 0x3c, 1, 0x34, 1, 3, 2, 0x34, 2, 0x3c, 1, 0x34, 3, 3, 2, 0x3c, 0,
                        0x22, 0, 0x34, 4, 3, 0x23, 0x23, 0x22, 0, 0x34, 5, 0x0f, 0x23, 0x22, 0,
                        0x10, 0x3e, 0
                    ],
                    "Native target acquisition expression differs"
                );
                let mut values = p
                    .array(expression + 0x20, 16, Some(0x80800090))?
                    .into_iter()
                    .map(|at| p.bytes::<16>(at))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(values.len() == 6, "Native acquisition constants differ");
                values.push([0; 16]);
                let mut revised = code;
                revised[2..4].copy_from_slice(&[0x34, 6]);
                revised[10..12].copy_from_slice(&[0x34, 6]);
                crate::presentation::append(&mut p.0, expression + 0x10, 0x80800009, &revised, 1)?;
                crate::presentation::append(
                    &mut p.0,
                    expression + 0x20,
                    0x80800090,
                    &values.concat(),
                    16,
                )?;
                put(&mut p.0, expression + 0x30, &1u64.to_le_bytes())?;
                removed.extend(unbind(p, expression)?);
            }
            _ => {}
        }
    }
    ensure!(
        removed.len() >= 2,
        "Guided projectile input replacement differs"
    );
    Ok(removed)
}

pub(super) fn bind(
    reader: &mut Reader,
    owners: &mut BTreeMap<u32, Payload>,
    graph: &mut crate::tiger::entity::links::Graph,
    allocations: &mut BTreeMap<u32, Payload>,
    behavior: &Value,
    removed: &BTreeSet<Object>,
) -> Result<()> {
    use crate::tiger::{channel::inputs, entity::links, instance};
    let one = |class| -> Result<u32> {
        let candidates = owners
            .iter()
            .filter(|(_, p)| assembly::Assembly::class(p).ok() == Some(class))
            .map(|(&tag, _)| tag)
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "Guidance component {class:08X} is absent or ambiguous"
        );
        Ok(candidates[0])
    };
    let movement = one(0x8080388f)?;
    let bank = one(0x80809790)?;
    let tracking = one(0x8080377d)?;
    let sources = [25, 15]
        .into_iter()
        .map(|method| inputs::provider(reader, movement, &owners[&movement], 0x80803b73, method))
        .collect::<Result<Vec<_>>>()?;
    let original = owners[&bank].clone();
    let allocation_tag = original.u32(0x44)?;
    let allocation = reader.tag(allocation_tag, Some(0x80809bbb))?;
    let declarations = sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            Ok(inputs::Input {
                name: if i == 0 { 0x7ed319a9 } else { 0x032643e4 },
                namespace: 0x811c9dc5,
                property: owners[&movement].u32(source.offset as usize + 24)?,
                value_type: 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let extension = inputs::extend(&original, &allocation, &declarations)?;
    let provider = graph
        .connections
        .iter()
        .chain(&graph.named_connections)
        .find(|row| {
            row.provider
                .object
                .is_some_and(|v| v.owner == bank && v.class == 0x808097c2)
        })
        .context("Guidance bank vector provider")?
        .provider
        .clone();
    for row in graph
        .connections
        .iter_mut()
        .chain(&mut graph.named_connections)
    {
        if row
            .provider
            .object
            .is_some_and(|v| v.owner == bank && v.class == 0x808097c2)
        {
            row.channel = extension.channel(row.channel)?;
        }
        if let Some(object) = row
            .consumer
            .object
            .as_mut()
            .filter(|v| v.owner == bank && v.class == 0x808097a8)
        {
            object.offset = *extension
                .relocated
                .get(&(object.offset as usize))
                .context("Guidance bank consumer outside declarations")?
                as u64;
        }
    }
    let endpoint = |owner, class, offset| -> Result<links::Endpoint> {
        Ok(links::Endpoint {
            namespace: 0x811c9dc5,
            selector: graph
                .components
                .iter()
                .position(|&tag| tag == owner)
                .context("Guidance component ordinal")? as u64,
            object: Some(Object {
                owner,
                class,
                offset,
            }),
        })
    };
    let mut additions = Vec::new();
    for (source, declaration) in sources.iter().zip(&extension.declarations) {
        additions.push(links::Connection {
            consumer: endpoint(bank, 0x808097a8, *declaration as u64)?,
            provider: endpoint(source.owner, source.class, source.offset)?,
            channel: 0,
            flags: 0,
        });
    }
    let p = owners.get_mut(&tracking).unwrap();
    let definition = p.pointer(24)?;
    let angular = p.pointer(definition + 0x160)? + 0x18;
    let source = usize::try_from(p.u64(angular + 8)?)?;
    // The detached original row retains its complete typed declaration and state.
    let template = removed
        .iter()
        .filter(|o| o.owner == tracking)
        .find(|o| {
            p.u64(o.offset as usize + 8)
                .ok()
                .and_then(|at| p.pointer(at as usize + 16).ok())
                == Some(source)
        })
        .context("Original angular input template")?;
    let declaration = p.bytes::<40>(template.offset as usize)?;
    let state = p.bytes::<96>(p.u64(template.offset as usize + 8)? as usize)?;
    ensure!(
        p.u64(template.offset as usize + 24)? == 0x808097c1,
        "Angular input is not a vector channel"
    );
    crate::presentation::append(
        &mut p.0,
        angular + 0x40,
        0x80809789,
        &[declaration, declaration].concat(),
        40,
    )?;
    crate::presentation::append(
        &mut p.0,
        source + 0x20,
        0x80809788,
        &[state, state].concat(),
        96,
    )?;
    let ds = p.array(angular + 0x40, 40, Some(0x80809789))?;
    let ss = p.array(source + 0x20, 96, Some(0x80809788))?;
    for (i, (&d, &s)) in ds.iter().zip(&ss).enumerate() {
        put(&mut p.0, d + 8, &(s as u64).to_le_bytes())?;
        put(&mut p.0, d + 32, &declarations[i].name.to_le_bytes())?;
        put(&mut p.0, s + 8, &(d as u64).to_le_bytes())?;
        put(
            &mut p.0,
            s + 16,
            &(source as i64 - s as i64 - 16).to_le_bytes(),
        )?;
        additions.push(links::Connection {
            consumer: endpoint(tracking, 0x80809789, d as u64)?,
            provider: provider.clone(),
            channel: (extension.first + i) as u32,
            flags: 0,
        });
    }
    let rates = rates(behavior)?;
    let at_rest = behavior["guided_angular_velocity_at_rest"]
        .as_f64()
        .context("At-rest guidance")? as f32;
    ensure!(
        at_rest.is_finite() && at_rest >= 0.,
        "Invalid at-rest guidance"
    );
    // lerp(atRest, lerp(lower, upper, stableSeed), distanceFraction).
    program::write(
        p,
        angular + 0x10,
        &[
            0x34, 0, 0x34, 1, 0x34, 2, 0x3c, 5, 0x10, 0x3c, 6, 0x10, 0x3e, 0,
        ],
        &[
            at_rest.to_degrees(),
            rates[0].to_degrees(),
            rates[1].to_degrees(),
        ],
        7,
    )?;
    let tag = p.u32(0x44)?;
    let mut allocation = (*reader.tag(tag, Some(0x80809bbb))?).clone();
    let child = |p: &Payload, field, name| -> Result<usize> {
        let rows = p
            .array(field, 40, Some(0x80808852))?
            .into_iter()
            .filter(|&r| p.u32(r).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(rows.len() == 1, "Angular allocation child differs");
        Ok(rows[0])
    };
    let optional = child(&allocation, 0x20, 0x1bdc6d6e)?;
    ensure!(
        allocation.u32(optional + 16)? == 0x80803772 && allocation.u32(optional + 20)? == 1,
        "Angular optional allocation differs"
    );
    let anonymous = child(&allocation, optional + 24, 0x811c9dc5)?;
    let indirect = child(&allocation, anonymous + 24, 0x437bcaa9)?;
    let target = allocation.pointer(indirect + 8)?;
    ensure!(
        allocation.u32(target - 4)? == 0x80808852,
        "Angular indirect allocation differs"
    );
    let leaf = child(&allocation, target + 24, 0x1d7c9056)?;
    ensure!(
        allocation.u32(leaf + 16)? == 0x80809788 && allocation.u32(leaf + 20)? == 1,
        "Angular input allocation differs"
    );
    put(&mut allocation.0, leaf + 20, &2u32.to_le_bytes())?;
    allocations.insert(tag, allocation);
    let end = p.0.len();
    instance::seal_span(p, &mut Vec::new(), end)?;
    let mut bank_payload = extension.payload;
    let end = bank_payload.0.len();
    instance::seal_span(&mut bank_payload, &mut Vec::new(), end)?;
    owners.insert(bank, bank_payload);
    allocations.insert(allocation_tag, extension.allocation);
    graph.connections.extend(additions);
    Ok(())
}
