//! Source driver attachments bound to their translated skeleton frames.
use super::*;
use crate::tiger::entity;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DriverPose {
    /// Native metres along the source seat's X forward, Y left and Z up axes.
    pub translation: [f32; 3],
    /// Rotation about local Y. Negative values raise the rider's forward axis.
    pub pitch_degrees: f32,
}

impl DriverPose {
    fn apply(&self, source: ([f32; 3], [f32; 4])) -> Result<([f32; 3], [f32; 4])> {
        ensure!(
            self.translation
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.)
                && self.pitch_degrees.is_finite()
                && self.pitch_degrees.abs() <= 90.,
            "Driver pose exceeds its supported position or pitch range"
        );
        let radians = self.pitch_degrees.to_radians() * 0.5;
        let rotation = [0., radians.sin(), 0., radians.cos()];
        Ok((
            rig::point(&rig::transform(source.0, source.1), self.translation, true),
            rig::quaternion(product(source.1, rotation))?,
        ))
    }
}

fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub(super) fn selected_marker<'a>(
    model: &'a model::Model,
    name: &str,
) -> Result<&'a model::Marker> {
    let mut markers = Vec::new();
    for marker in model.markers.iter().filter(|marker| marker.name == name) {
        if model.marker_selected(marker)? {
            markers.push(marker);
        }
    }
    ensure!(
        markers.len() == 1,
        "Source vehicle marker {name} is absent or ambiguous"
    );
    Ok(markers[0])
}

pub(in crate::halo_reach) fn marker(
    model: &model::Model,
    name: &str,
) -> Result<([f32; 3], [f32; 4])> {
    let marker = selected_marker(model, name)?;
    marker_pose(model, marker)
}

pub(super) fn marker_pose(
    model: &model::Model,
    marker: &model::Marker,
) -> Result<([f32; 3], [f32; 4])> {
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

pub(super) fn frame(
    bones: &Value,
    bone: usize,
    source: ([f32; 3], [f32; 4]),
) -> Result<([f32; 3], [f32; 4])> {
    let inverse = bones["bones"][bone]["inverse_object_transform"]
        .as_array()
        .context("Native vehicle marker inverse transform")?;
    ensure!(
        inverse.len() == 8,
        "Native vehicle inverse transform length"
    );
    let values = inverse
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|n| n as f32)
                .context("Native vehicle inverse transform number")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        values.iter().all(|v| v.is_finite()) && (values[7] - 1.).abs() < 0.00001,
        "Scaled native vehicle marker frames are unsupported"
    );
    let q = rig::quaternion(values[..4].try_into()?)?;
    let transform = rig::transform(values[4..7].try_into()?, q);
    Ok((
        rig::point(&transform, source.0, true),
        rig::quaternion(product(q, source.1))?,
    ))
}

fn rewrite(
    owner: &mut Payload,
    definition: usize,
    bones: &Value,
    source: ([f32; 3], [f32; 4]),
    source_bone: Option<usize>,
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
        let bone = source_bone.unwrap_or(usize::try_from(owner.u32(row + 4)?)?);
        let (position, orientation) = frame(bones, bone, source)?;
        put(&mut owner.0, row + 4, &u32::try_from(bone)?.to_le_bytes())?;
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
    pose: Option<&DriverPose>,
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
        let model = &scene.models[0].model;
        let source_bone = bones["source_bone_first"]
            .as_u64()
            .map(|first| -> Result<usize> {
                Ok(match selected_marker(model, name)?.bone {
                    Some(bone) => usize::try_from(first)?
                        .checked_add(bone)
                        .context("Source driver bone overflow")?,
                    None => 0,
                })
            })
            .transpose()?;
        let mut source = marker(model, name)?;
        if !entry && let Some(pose) = pose {
            source = pose.apply(source)?;
        }
        report.push(json!({"role":field,"source":name,"markers":rewrite(&mut owner, definition, bones, source, source_bone, entry)?}));
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
        "markers":report,"pose_adjustment":pose,"gameplay_verified":false,
        "limits":["Native seat policies are retained. Rider actions are selected separately. Passenger seats are not translated."]}),
    ))
}
