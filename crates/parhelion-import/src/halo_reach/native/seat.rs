//! Source driver attachments translated into the carrier's retained bone frames.
use super::*;
use crate::tiger::entity;

fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn marker(model: &model::Model, name: &str) -> Result<([f32; 3], [f32; 4])> {
    let markers = model
        .markers
        .iter()
        .filter(|marker| {
            marker.name == name && marker.region.is_none() && marker.permutation.is_none()
        })
        .collect::<Vec<_>>();
    ensure!(
        markers.len() == 1,
        "Source driver marker {name} is absent or ambiguous"
    );
    let marker = markers[0];
    let mut position = marker.translation;
    let mut orientation = marker.rotation;
    if let Some(bone) = marker.bone {
        let world = rig::world(&model.bones)?;
        position = rig::point(
            world.get(bone).context("Source driver marker bone")?,
            position,
            true,
        );
        let mut parent = Some(bone);
        while let Some(index) = parent {
            orientation = product(model.bones[index].rotation, orientation);
            parent = model.bones[index].parent;
        }
    }
    ensure!(
        position.iter().all(|v| v.is_finite()),
        "Invalid driver marker position"
    );
    Ok((position, rig::quaternion(orientation)?))
}

fn rewrite(
    owner: &mut Payload,
    definition: usize,
    bones: &Value,
    source: ([f32; 3], [f32; 4]),
    entry: bool,
) -> Result<Vec<Value>> {
    let (offset, stride, class) = if entry {
        (0x188, 80, 0x80803d6f)
    } else {
        (0x108, 64, 0x80803d70)
    };
    let rows = owner.array(definition + offset + 0x50, stride, Some(class))?;
    ensure!(
        !rows.is_empty() && (entry || rows.len() == 1),
        "Unsupported native driver marker count"
    );
    let mut report = Vec::new();
    for row in rows {
        let bone = usize::try_from(owner.u32(row + 4)?)?;
        let inverse = bones["bones"][bone]["inverse_object_transform"]
            .as_array()
            .context("Native driver marker inverse transform")?;
        ensure!(inverse.len() == 8, "Native driver inverse transform length");
        let values = inverse
            .iter()
            .map(|v| {
                v.as_f64()
                    .map(|n| n as f32)
                    .context("Native driver inverse transform number")
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values.iter().all(|v| v.is_finite()) && (values[7] - 1.).abs() < 0.00001,
            "Scaled native driver marker frames are unsupported"
        );
        let q = rig::quaternion(values[..4].try_into()?)?;
        let transform = rig::transform(values[4..7].try_into()?, q);
        let position = rig::point(&transform, source.0, true);
        let orientation = rig::quaternion(product(q, source.1))?;
        for (index, value) in orientation
            .into_iter()
            .chain(position)
            .chain([1.])
            .enumerate()
        {
            put(&mut owner.0, row + 0x10 + index * 4, &value.to_le_bytes())?;
        }
        report.push(json!({"row":row,"body_group":owner.u32(row)?,"bone":bone,
            "name":format!("{:08X}",owner.u32(row+0x30)?),"position":position,"orientation":orientation,
            "source_position":source.0,"source_orientation":source.1}));
    }
    Ok(report)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    bones: &Value,
    bytes: &mut [u8],
    patches: &mut Vec<Value>,
) -> Result<Option<Value>> {
    let Some(seats) = scene.object.gameplay["seats"].as_array() else {
        return Ok(None);
    };
    let drivers = seats
        .iter()
        .filter(|seat| seat["flags"].as_u64().is_some_and(|f| f & 4 != 0))
        .collect::<Vec<_>>();
    if drivers.is_empty() {
        return Ok(None);
    }
    ensure!(
        drivers.len() == 1,
        "Source vehicle has multiple driver seats"
    );
    let source = drivers[0];
    let entity = Payload(bytes.to_vec());
    let mut owners = Vec::new();
    for row in entity.array(16, 12, None)? {
        let tag = entity.u32(row)?;
        if tag == u32::MAX {
            continue;
        }
        let owner = native.tag(tag, Some(0x80809c36))?;
        let definition = owner.pointer(24)?;
        if owner.u32(definition - 4)? == 0x80803d7d {
            ensure!(
                owner.u32(owner.pointer(16)? - 4)? == 0x808042ad,
                "Unsupported native driver instance"
            );
            owners.push((tag, owner, definition));
        }
    }
    if owners.is_empty() {
        return Ok(None);
    }
    ensure!(
        owners.len() == 1,
        "Native vehicle has multiple driver components"
    );
    let (tag, original, definition) = owners.pop().unwrap();
    let mut owner = Payload(original.0.clone());
    let mut report = Vec::new();
    for (field, entry) in [("marker", false), ("entry_marker", true)] {
        let name = source[field]
            .as_str()
            .context("Source driver marker name")?;
        ensure!(!name.is_empty(), "Source driver marker name is empty");
        let source = marker(&scene.models[0].model, name)?;
        report.push(json!({"role":field,"source":name,"markers":rewrite(&mut owner, definition, bones, source, entry)?}));
    }
    let mut relocations = Vec::new();
    for at in (0..owner.0.len().saturating_sub(3)).step_by(4) {
        if owner.u32(at)? == tag {
            ensure!(
                owner.u32(at + 4)? & 0xffff0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "Untyped driver component self-reference"
            );
            put(&mut owner.0, at, &u32::MAX.to_le_bytes())?;
            relocations.push(json!({"offset":at,"symbol":"driver-seat"}));
        }
    }
    graph.add("driver-seat", tag, &owner.0, None, relocations)?;
    for at in entity::owner_slots(&entity, &original, tag)? {
        put(bytes, at, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":at,"symbol":"driver-seat"}));
    }
    entity::reject_stale_owner(&Payload(bytes.to_vec()), tag)?;
    Ok(Some(
        json!({"source_owner":tag,"owner":"driver-seat","class":0x80803d7du32,
        "markers":report,"gameplay_verified":false,
        "limits":["The native driver pose and seat policies are retained. Passenger seats are not translated."]}),
    ))
}
