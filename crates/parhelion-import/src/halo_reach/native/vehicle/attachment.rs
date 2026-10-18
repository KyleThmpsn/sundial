//! Driver IK targets follow source hand and foot attachments on the extended rig.
use super::{Scene, assembly::Assembly};
use crate::{halo_reach::native::seat, presentation::put, tiger::markers};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn name(text: &str) -> u32 {
    text.bytes().fold(0x811c9dc5u32, |hash, byte| {
        hash.wrapping_mul(0x01000193) ^ u32::from(byte)
    })
}

pub(super) fn build(assembly: &mut Assembly, scene: &Scene, bones: &Value) -> Result<Value> {
    let owners = assembly
        .payloads
        .iter()
        .filter_map(|(&owner, payload)| {
            payload
                .pointer(16)
                .ok()
                .and_then(|at| payload.u32(at - 4).ok())
                .filter(|&class| class == markers::NATIVE_COMPONENT)
                .map(|_| owner)
        })
        .collect::<Vec<_>>();
    ensure!(owners.len() <= 1, "Vehicle driver marker set is ambiguous");
    let Some(owner) = owners.first() else {
        return Ok(json!({"targets":[]}));
    };
    let payload = assembly.payloads.get_mut(owner).unwrap();
    let rows = markers::read_native(payload)?;
    let model = &scene.models[0].model;
    let mut targets = Vec::new();
    for (source, native) in [
        ("left_hand", "left_hand_ik"),
        ("right_hand", "right_hand_ik"),
        ("left_foot", "left_foot_ik"),
        ("right_foot", "right_foot_ik"),
    ] {
        let selected = rows
            .iter()
            .filter(|row| row.name == name(native))
            .collect::<Vec<_>>();
        if selected.is_empty() || !model.markers.iter().any(|marker| marker.name == source) {
            continue;
        }
        let marker = seat::selected_marker(model, source)?;
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Driver IK requires an appended source skeleton")?,
        )?;
        let bone = marker.bone.map(|index| first + index).unwrap_or(0);
        let pose = seat::marker(model, source)?;
        let (position, orientation) = seat::frame(bones, bone, pose)?;
        for row in selected {
            // The first word selects a body group, not a skeletal binding mode.
            // Preserve that visibility selector and replace only the bone frame.
            ensure!(
                usize::try_from(row.binding[1])? < first
                    && payload.bytes::<8>(row.row + 8)? == [0; 8],
                "Driver IK marker is outside the original native skeleton"
            );
            put(
                &mut payload.0,
                row.row + 4,
                &u32::try_from(bone)?.to_le_bytes(),
            )?;
            for (index, value) in orientation
                .into_iter()
                .chain(position)
                .chain([1.])
                .enumerate()
            {
                put(
                    &mut payload.0,
                    row.row + 16 + index * 4,
                    &value.to_le_bytes(),
                )?;
            }
            targets.push(
                json!({"source":source,"native":native,"name":format!("{:08X}",row.name),
                "row":row.row,"previous_binding":row.binding,"bone":bone,
                "position":position,"orientation":orientation,"source_position":pose.0}),
            );
        }
    }
    Ok(
        json!({"owner":format!("{owner:08X}"),"targets":targets,"gameplay_verified":false,
        "limits":["The native rider pose and IK solver remain in use. Only supported source hand and foot targets are translated."]}),
    )
}
