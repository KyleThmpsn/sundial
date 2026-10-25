//! Source damage pools and native conditions shared by render and hit geometry.
mod effect;
pub(super) mod region;
mod section;

use super::assembly::Assembly;
use crate::{
    halo_reach::{Scene, cache::Cache, damage::Geometry, resource::Pages},
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c9dc5u32, |value, byte| {
        value.wrapping_mul(16777619) ^ u32::from(byte)
    })
}

pub(super) fn definition(
    payload: &Payload,
    instance_class: u32,
    definition_class: u32,
) -> Result<(usize, usize)> {
    let instance = payload.pointer(16)?;
    let definition = usize::try_from(payload.u64(instance + 8)?)?;
    ensure!(
        payload.u32(instance - 4)? == instance_class
            && payload.u32(instance + 4)? == definition_class,
        "Vehicle damage component has another native implementation"
    );
    payload.bytes::<16>(definition)?;
    Ok((instance, definition))
}

pub(super) fn array(
    payload: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    count: usize,
) -> Result<Vec<usize>> {
    let size = stride
        .checked_mul(count)
        .context("Vehicle damage array size overflow")?;
    append(
        &mut payload.0,
        field,
        u64::from(class),
        &vec![0; size],
        stride,
    )?;
    payload.array(field, stride, Some(class))
}

fn reciprocal(
    payload: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    instance_class: u32,
    definition_class: u32,
    parent: usize,
) -> Result<()> {
    for (at, class, target) in [
        (instance, definition_class, definition),
        (definition, instance_class, instance),
    ] {
        put(&mut payload.0, at, &owner.to_le_bytes())?;
        put(&mut payload.0, at + 4, &class.to_le_bytes())?;
        put(
            &mut payload.0,
            at + 8,
            &u64::try_from(target)?.to_le_bytes(),
        )?;
    }
    let offset = i64::try_from(parent)? - i64::try_from(instance + 16)?;
    put(&mut payload.0, instance + 16, &offset.to_le_bytes())
}

pub(super) fn allocation(
    payload: &mut Payload,
    names: &[u32],
    class: u32,
    count: usize,
) -> Result<()> {
    ensure!(!names.is_empty(), "Vehicle damage allocation path is empty");
    let mut field = 32;
    for (depth, &name) in names.iter().enumerate() {
        let rows = payload.array(field, 40, Some(0x80808852))?;
        let matches = rows
            .into_iter()
            .filter(|&row| payload.u32(row).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Vehicle allocation field {name:08X} is absent or ambiguous"
        );
        let row = matches[0];
        if depth + 1 == names.len() {
            ensure!(
                payload.u32(row + 16)? == class,
                "Vehicle damage allocation class differs"
            );
            return put(
                &mut payload.0,
                row + 20,
                &u32::try_from(count)?.to_le_bytes(),
            );
        }
        ensure!(
            payload.u32(row + 16)? == u32::MAX,
            "Vehicle nested allocation is not a container"
        );
        let target = if payload.u64(row + 8)? == 0 {
            row
        } else {
            let target = payload.pointer(row + 8)?;
            ensure!(
                payload.u32(target - 4)? == 0x80808852,
                "Vehicle indirect allocation marker differs"
            );
            target
        };
        field = target + 24;
    }
    unreachable!()
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
    cache: &Cache,
    pages: &mut Pages,
) -> Result<Value> {
    let Some(geometry) = &scene.damage else {
        return Ok(Value::Null);
    };
    let owner = assembly.owner(0x80804bee)?;
    let mut effects = effect::build(cache, pages, native, graph, assembly, scene, bones)?;
    let (allocation_tag, mut layout) = assembly.allocation(native, graph, owner)?;
    let payload = assembly.payloads.get_mut(&owner).unwrap();
    let (instance, definition) = definition(payload, 0x80804bee, 0x80804b8a)?;
    let regions = region::health(payload, owner, instance, definition, geometry)?;
    let sections = section::build(
        payload,
        owner,
        instance,
        definition,
        geometry,
        &scene.object.gameplay["damage"],
        &mut effects,
    )?;
    allocation(
        &mut layout,
        &[0x7a9059b4],
        0x80804b98,
        geometry.regions.len(),
    )?;
    for (name, class) in [(0xccd237b9, 0x80804bab), (0x1d69dbff, 0x80804c5e)] {
        allocation(&mut layout, &[name], class, sections.count)?;
    }
    allocation(
        &mut layout,
        &[0xa53ceaa0, 0x30ea7e62],
        0x80804c5c,
        sections.count,
    )?;
    allocation(&mut layout, &[0xa005fa37], 0x80804b91, sections.outputs)?;
    allocation(&mut layout, &[0x5cf76067], 0x8080949e, effects.slot_count())?;
    let end = payload.0.len();
    for (offset, symbol) in effects.resources {
        assembly.resource(owner, offset, symbol)?;
    }
    assembly.runtime_end(owner, end)?;
    assembly.set_allocation(graph, owner, allocation_tag, &layout)?;
    let render = region::render(native, graph, assembly, geometry)?;
    Ok(
        json!({"source":scene.object.gameplay["damage"]["model"], "owner":format!("{owner:08X}"),
        "regions":regions,"sections":sections.report,"render":render,"effects":effects.report,"gameplay_verified":false}),
    )
}
