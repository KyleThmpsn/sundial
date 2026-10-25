//! Follow actual staged vehicle flow and secondary references for stock visual leakage.
use super::*;
use std::collections::BTreeSet;

pub(super) fn verify(staged: &mut Reader, stock: &Reader, runtime: &Value) -> Result<Value> {
    let root = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Vehicle runtime entity")?,
        16,
    )?;
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut flows = Vec::new();
    while let Some(tag) = pending.pop() {
        if !seen.insert(tag) {
            continue;
        }
        let entity = staged.tag(tag, Some(0x80809c0f))?;
        for row in entity.array(16, 12, None)? {
            let owner = entity.u32(row)?;
            let payload = staged.tag(owner, Some(0x80809c36))?;
            let definition = payload.pointer(24)?;
            match payload.u32(definition - 4)? {
                0x80803ac9 => {
                    let mut properties = vec![definition + 0x80];
                    properties.extend(payload.array(
                        definition + 0x240,
                        0x1c0,
                        Some(0x80803acf),
                    )?);
                    for property in properties {
                        let child = payload.u32(property + 0x70)?;
                        if ![0, u32::MAX].contains(&child) {
                            pending.push(child);
                        }
                    }
                }
                0x808084e9 => {
                    let mut systems = Vec::new();
                    for event in payload.array(definition + 0x168, 24, Some(0x808093e6))? {
                        let node = payload.pointer(event + 16)?;
                        let class = payload.u32(node - 4)?;
                        ensure!(class != 0x80806f49, "Vehicle retained a donor light event");
                        if class != 0x80806cc6 {
                            continue;
                        }
                        for row in payload.array(node + 0x40, 24, Some(0x80806cc8))? {
                            let system = payload.u32(row + 0x10)?;
                            if [0, u32::MAX].contains(&system) {
                                continue;
                            }
                            ensure!(
                                stock
                                    .manager
                                    .get_entry(tiger_pkg::TagHash(system))
                                    .is_none(),
                                "Vehicle flow still selects a stock particle system {system:08X}"
                            );
                            let particle = staged.tag(system, Some(0x80806e28))?;
                            let definition = particle.u32(0)?;
                            ensure!(
                                stock
                                    .manager
                                    .get_entry(tiger_pkg::TagHash(definition))
                                    .is_none(),
                                "Vehicle flow still selects a stock particle definition"
                            );
                            staged.tag(definition, Some(0x80806e2c))?;
                            systems.push(format!("{system:08X}"));
                        }
                    }
                    flows.push(json!({"entity":format!("{tag:08X}"),"owner":format!("{owner:08X}"),"private_systems":systems}));
                }
                _ => {}
            }
        }
    }
    ensure!(!flows.is_empty(), "No required vehicle flow was inspected");
    Ok(
        json!({"root":format!("{root:08X}"),"entities":seen,"flows":flows,"gameplay_verified":false}),
    )
}
