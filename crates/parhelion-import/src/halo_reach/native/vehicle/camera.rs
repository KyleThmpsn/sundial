//! Extend the private seat camera's native rail without changing its aim rotations.
use super::{assembly::Assembly, secondary::Bundle};
use crate::{
    presentation::{Graph, put},
    tiger::reader::Reader,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
) -> Result<Value> {
    let driver = assembly.owner(0x808042ad)?;
    let payload = &assembly.payloads[&driver];
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    ensure!(
        payload.u32(definition - 4)? == 0x80803d7d,
        "Driver camera owner differs"
    );
    let secondary = payload.u32(definition + 0x248)?;
    let mut bundle = Bundle::read(native, secondary)?;
    let mut rails = BTreeMap::new();
    let mut selected = Vec::new();
    let distance_scale = 1.5f32;
    for (&owner, payload) in &bundle.owners {
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        if payload.u32(definition - 4)? != 0x808084e9 {
            continue;
        }
        for row in payload.array(definition + 0x168, 24, Some(0x808093e6))? {
            let node = payload.pointer(row + 16)?;
            if payload.u32(node - 4)? != 0x808066a3 {
                continue;
            }
            let camera = node + 0x38;
            let instance = usize::try_from(payload.u64(node + 8)?)?;
            ensure!(
                payload.u32(node + 4)? == 0x808066a2
                    && payload.u32(instance + 4)? == 0x808066a3
                    && payload.u64(instance + 8)? == node as u64
                    && node + 0x278 <= payload.0.len(),
                "Seat camera event pair differs"
            );
            let references = payload.array(camera + 0xd8, 16, Some(0x808066fa))?;
            ensure!(!references.is_empty(), "Seat camera has no native rail");
            for reference in references {
                let tag = payload.u32(reference + 8)?;
                let symbol = format!("vehicle-camera-rail-{tag:08X}");
                if let std::collections::btree_map::Entry::Vacant(entry) = rails.entry(tag) {
                    let mut rail = (*native.tag(tag, Some(0x808066fb))?).clone();
                    let points = rail.array(8, 32, Some(0x808066fd))?;
                    ensure!(points.len() >= 4, "Seat camera rail has too few points");
                    for &point in &points {
                        let rotation = (0..4)
                            .map(|axis| rail.f32(point + 16 + axis * 4))
                            .collect::<Result<Vec<_>>>()?;
                        ensure!(
                            rotation.iter().all(|v| v.is_finite())
                                && (rotation.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.001,
                            "Seat camera rail orientation is invalid"
                        );
                        for axis in 0..3 {
                            let at = point + axis * 4;
                            let value = rail.f32(at)? * distance_scale;
                            ensure!(value.is_finite(), "Seat camera rail position is invalid");
                            put(&mut rail.0, at, &value.to_le_bytes())?;
                        }
                    }
                    graph.add(&symbol, tag, &rail.0, None, Vec::new())?;
                    entry.insert(points.len());
                }
                bundle
                    .resources
                    .entry(owner)
                    .or_default()
                    .insert(reference + 8, symbol);
                selected.push(json!({"owner":format!("{owner:08X}"),"event":node,
                    "camera":camera,"rail":format!("{tag:08X}")}));
            }
        }
    }
    ensure!(!selected.is_empty(), "Driver has no reachable seat camera");
    let symbol = bundle.emit(graph, "vehicle-driver")?;
    assembly.resource(driver, definition + 0x248, symbol)?;
    Ok(
        json!({"distance_scale":distance_scale,"selections":selected,
        "limits":["The private native camera rail is farther from its target."]}),
    )
}
