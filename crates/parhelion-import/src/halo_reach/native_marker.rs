//! Approximate source attachment roles in native object-space marker sets.
use super::{model, rig};
use crate::{
    presentation::{Graph, put},
    tiger::{entity, markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811c9dc5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}
fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub(super) fn build(
    native: &mut Reader,
    g: &mut Graph,
    model: &model::Model,
    entity_bytes: &mut [u8],
    entity_patches: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let original_entity = Payload(entity_bytes.to_vec());
    let mut sets = Vec::new();
    for row in original_entity.array(16, 12, None)? {
        let tag = original_entity.u32(row)?;
        // The render owner has already become a graph relocation.
        if tag == u32::MAX {
            continue;
        }
        let owner = native.tag(tag, Some(0x80809c36))?;
        let header = owner.pointer(16)?;
        if header >= 4 && owner.u32(header - 4)? == markers::NATIVE_COMPONENT {
            sets.push((tag, owner));
        }
    }
    ensure!(
        sets.len() <= 1,
        "Native render entity has ambiguous marker sets"
    );
    let Some((tag, owner)) = sets.pop() else {
        return Ok(vec![json!({"status":"no_native_marker_set"})]);
    };
    let world = rig::world(&model.bones)?;
    let mut converted = Vec::new();
    let mut report = Vec::new();
    for marker in &model.markers {
        if let Some(region) = marker.region {
            let region = model.regions.get(region).context("Marker region")?;
            if let Some(permutation) = marker.permutation {
                let permutation = region
                    .permutations
                    .get(permutation)
                    .context("Marker permutation")?;
                if model.selected.get(&region.name) != Some(&permutation.name) {
                    continue;
                }
            }
        }
        let mut position = marker.translation;
        let mut orientation = marker.rotation;
        if let Some(bone) = marker.bone {
            position = rig::point(&world[bone], position, true);
            let mut index = Some(bone);
            while let Some(i) = index {
                orientation = product(model.bones[i].rotation, orientation);
                index = model.bones[i].parent;
            }
        }
        let role = match marker.name.as_str() {
            "primary_trigger" => Some("fire"),
            "right_hand" => Some("grip"),
            "left_hand" => Some("support"),
            "primary_ejection" => Some("eject"),
            _ => None,
        };
        for name in std::iter::once(marker.name.as_str()).chain(role) {
            converted.push(markers::Marker {
                name: hash(name),
                binding: [0, 1],
                position: [position[0], position[1], position[2], 1.],
                orientation: rig::quaternion(orientation)?,
                row: 0,
            });
        }
        report.push(json!({"source":marker.name,"native_role":role,"position":position,"orientation":orientation,"binding":"object_space"}));
    }
    let (mut rewritten, _) = markers::rewrite(&owner, &converted)?;
    let mut patches = Vec::new();
    for at in (0..rewritten.0.len().saturating_sub(3)).step_by(4) {
        if rewritten.u32(at)? == tag {
            ensure!(
                rewritten.u32(at + 4)? & 0xffff0000 == 0x80800000
                    && rewritten.u64(at + 8)? < rewritten.0.len() as u64,
                "Untyped marker owner self-reference"
            );
            put(&mut rewritten.0, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":at,"symbol":"markers"}));
        }
    }
    g.add("markers", tag, &rewritten.0, None, patches)?;
    for at in entity::owner_slots(&original_entity, &owner, tag)? {
        put(entity_bytes, at, &u32::MAX.to_le_bytes())?;
        entity_patches.push(json!({"offset":at,"symbol":"markers"}));
    }
    entity::reject_stale_owner(&Payload(entity_bytes.to_vec()), tag)?;
    Ok(report)
}
