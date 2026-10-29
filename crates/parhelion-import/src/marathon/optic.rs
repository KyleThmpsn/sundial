//! Source optic defaults in the same object-space frame as the static source assembly.
use super::*;
use crate::tiger::markers::{self, Marker};
use std::collections::BTreeMap;
mod native;
pub use native::restore_source_eye;
pub(super) use native::sight_key;

pub(super) struct Sight {
    pub entity: u32,
    pub owner: u32,
    pub angle: f32,
    pub points: [Marker; 2],
}

// Convert the checked rigid placement's rotation to a quaternion, then compose the source row.
fn placed(mut marker: Marker, matrix: &[f32; 16]) -> Result<Marker> {
    ensure!(
        matrix.iter().all(|v| v.is_finite()) && matrix[12..] == [0., 0., 0., 1.],
        "Invalid sight placement"
    );
    for a in 0..3 {
        for b in 0..3 {
            let dot = (0..3)
                .map(|i| matrix[a * 4 + i] * matrix[b * 4 + i])
                .sum::<f32>();
            ensure!(
                (dot - if a == b { 1. } else { 0. }).abs() < 0.0001,
                "Sight placement is not rigid"
            );
        }
    }
    let determinant = matrix[0] * (matrix[5] * matrix[10] - matrix[6] * matrix[9])
        - matrix[1] * (matrix[4] * matrix[10] - matrix[6] * matrix[8])
        + matrix[2] * (matrix[4] * matrix[9] - matrix[5] * matrix[8]);
    ensure!(
        (determinant - 1.).abs() < 0.0001,
        "Sight placement reflects its axes"
    );
    let mut q = [0.; 4];
    let trace = matrix[0] + matrix[5] + matrix[10];
    if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        q = [
            (matrix[9] - matrix[6]) / s,
            (matrix[2] - matrix[8]) / s,
            (matrix[4] - matrix[1]) / s,
            s / 4.,
        ];
    } else {
        let i = (0..3)
            .max_by(|&a, &b| matrix[a * 5].total_cmp(&matrix[b * 5]))
            .unwrap();
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let s = (1. + matrix[i * 5] - matrix[j * 5] - matrix[k * 5]).sqrt() * 2.;
        q[i] = s / 4.;
        q[j] = (matrix[j * 4 + i] + matrix[i * 4 + j]) / s;
        q[k] = (matrix[k * 4 + i] + matrix[i * 4 + k]) / s;
        q[3] = (matrix[k * 4 + j] - matrix[j * 4 + k]) / s;
    }
    let [x, y, z, w] = q;
    let [a, b, c, d] = marker.orientation;
    marker.orientation = [
        w * a + x * d + y * c - z * b,
        w * b - x * c + y * d + z * a,
        w * c + x * b - y * a + z * d,
        w * d - x * a - y * b - z * c,
    ];
    let p = marker.position;
    for i in 0..3 {
        marker.position[i] =
            (0..3).map(|j| matrix[i * 4 + j] * p[j]).sum::<f32>() + matrix[i * 4 + 3];
    }
    marker.binding = [0, 0];
    Ok(marker)
}

fn movement(reader: &mut Reader, entity: &Payload, tag: u32, owner: &Payload) -> Result<()> {
    let base = owner.pointer(24)?;
    let components = entity.array(8, 12, Some(0x8080BAA2))?;
    let index = components
        .iter()
        .position(|&at| entity.u32(at).ok() == Some(tag))
        .context("Sight component index")?;
    for offset in [0x290, 0x330] {
        let events = entity
            .array(24, 56, Some(0x8080BA61))?
            .into_iter()
            .filter(|&at| {
                entity.u32(at).ok() == Some(tag)
                    && entity.u32(at + 4).ok() == Some(0x8080BA6F)
                    && entity.u64(at + 8).ok() == Some((base + offset) as u64)
            })
            .collect::<Vec<_>>();
        ensure!(
            events.len() == 1,
            "Marathon aim input has no unique movement provider"
        );
        let at = events[0];
        ensure!(
            entity.u32(at + 16)? == 0x811C9DC5
                && entity.u32(at + 20)? as usize == index
                && entity.u32(at + 40)? == 0x811C9DC5,
            "Unsupported Marathon sight movement owner"
        );
        let adapter = reader.tag(entity.u32(at + 24)?, None)?;
        movement_provider(entity, &components, at, &adapter)?;
    }
    Ok(())
}

fn movement_provider(
    entity: &Payload,
    components: &[usize],
    at: usize,
    provider: &Payload,
) -> Result<()> {
    let data = provider.pointer(24)?;
    match provider.u32(data - 4)? {
        0x8080B61F => ensure!(
            entity.u32(at + 28)? == 0x8080BA7B
                && entity.u64(at + 32)? == (data + 16) as u64
                && entity.u32(at + 44)? == 0x20000
                && entity.u64(at + 48)? == 0,
            "Unsupported Marathon sight adapter connection"
        ),
        0x80809FB7 => {
            // A local sight can consume its own skeleton's root object transform.
            // Its checked identity bind is already included in assembly placement.
            ensure!(
                components
                    .iter()
                    .any(|&row| { entity.u32(row).ok() == entity.u32(at + 24).ok() })
                    && entity.u32(at + 28)? == 0x8080AF37
                    && entity.u64(at + 32)? == (data + 0x38) as u64
                    && entity.bytes::<12>(at + 44)? == [0; 12],
                "Unsupported Marathon skeleton sight connection"
            );
            let nodes = provider.array(data + 0xB0, 16, Some(0x8080AF42))?;
            ensure!(
                !nodes.is_empty() && provider.u32(nodes[0] + 4)? == u32::MAX,
                "Skeleton sight has no root bone"
            );
            for descriptor in [0xC0, 0xD0] {
                let rows = provider.array(data + descriptor, 32, Some(0x8080BF47))?;
                ensure!(
                    rows.len() == nodes.len(),
                    "Skeleton sight bind count differs"
                );
                for (lane, value) in [0., 0., 0., 1., 0., 0., 0., 1.].into_iter().enumerate() {
                    ensure!(
                        (provider.f32(rows[0] + lane * 4)? - value).abs() < 0.00001,
                        "Skeleton sight root bind is not identity"
                    );
                }
            }
        }
        _ => anyhow::bail!("Unsupported Marathon sight movement provider"),
    }
    Ok(())
}

fn defaults(
    owner: &Payload,
    markers: &[Marker],
    matrix: &[f32; 16],
) -> Result<Option<([Marker; 2], bool)>> {
    let base = owner.pointer(24)?;
    let mut points = Vec::new();
    let mut explicit = false;
    for (descriptor, name) in [(0x2D0, 0x8D0DD3CD), (0x370, 0x0B7BA45D)] {
        ensure!(
            owner.u32(base + descriptor - 40)? == 0x8080BA7A
                && owner.u32(base + descriptor - 12)? == name,
            "Marathon sight provider layout differs"
        );
        let rows = markers::read(
            owner,
            base + descriptor,
            markers::Layout {
                row_class: 0x8080BA9E,
                ..markers::SOURCE
            },
        )?;
        ensure!(rows.len() <= 1, "Multiple Marathon sight defaults");
        let mode = owner.u32(base + descriptor - 32)?;
        if let Some(row) = rows.first() {
            ensure!(
                mode == 1 && row.name == name && row.binding == [0, 0],
                "Unsupported Marathon sight binding"
            );
            explicit = true;
            points.push(placed(*row, matrix)?);
        } else {
            ensure!(mode == 0, "Missing explicit Marathon sight default");
            if let Some(row) = markers
                .iter()
                .find(|m| m.name == name && m.binding == [0, 0])
            {
                points.push(placed(*row, matrix)?);
            }
        }
    }
    if points.len() != 2 {
        ensure!(!explicit, "Unresolved Marathon sight default");
        return Ok(None);
    }
    Ok(Some((points.try_into().unwrap(), explicit)))
}

fn collect(
    reader: &mut Reader,
    entity_tag: u32,
    matrix: &[f32; 16],
) -> Result<(Vec<Sight>, Vec<Sight>)> {
    let mut candidates = Vec::new();
    let mut inherited = Vec::new();
    let entity = reader.tag(entity_tag, None)?;
    let mut markers = Vec::new();
    for at in entity.array(8, 12, None)? {
        let tag = entity.u32(at)?;
        let owner = reader.tag(tag, None)?;
        let base = owner.pointer(24)?;
        if owner.u32(base - 4)? == 0x80809F76 {
            markers = markers::read(
                &owner,
                base + 0xA8,
                markers::Layout {
                    row_class: 0x80809F82,
                    ..markers::SOURCE
                },
            )?;
        }
    }
    for at in entity.array(8, 12, None)? {
        let tag = entity.u32(at)?;
        let owner = reader.tag(tag, None)?;
        let base = owner.pointer(24)?;
        if owner.u32(base - 4)? != 0x80803E3C {
            continue;
        }
        let angle = owner.f32(base + 0xE0)?;
        ensure!(
            (-0.018..=std::f32::consts::PI).contains(&angle)
                && owner.bytes::<12>(base + 0xE4)? == [0; 12],
            "Unsupported Marathon sight angular override"
        );
        let Some((points, explicit)) = defaults(&owner, &markers, matrix)? else {
            continue;
        };
        movement(reader, &entity, tag, &owner)?;
        let sight = Sight {
            entity: entity_tag,
            owner: tag,
            angle,
            points,
        };
        if explicit {
            candidates.push(sight);
        } else {
            inherited.push(sight);
        }
    }
    Ok((candidates, inherited))
}

pub(super) fn read(reader: &mut Reader, parts: &[Value]) -> Result<Sight> {
    let mut seen = BTreeMap::new();
    let mut candidates = Vec::new();
    let mut inherited = Vec::new();
    for part in parts {
        let entity_tag = hash(part, "entity")?;
        let matrix: [f32; 16] = serde_json::from_value(part["transform"].clone())?;
        if let Some(previous) = seen.insert(entity_tag, matrix) {
            ensure!(
                previous == matrix,
                "Sight entity has conflicting placements"
            );
            continue;
        }
        let (explicit, fallback) = collect(reader, entity_tag, &matrix)?;
        candidates.extend(explicit);
        inherited.extend(fallback);
    }
    if candidates.is_empty() {
        candidates = inherited;
    }
    ensure!(
        candidates.len() == 1,
        "Marathon assembly needs one unambiguous source sight, found {}",
        candidates.len()
    );
    let sight = candidates.remove(0);
    ensure!(
        sight.points[1].position[0] > sight.points[0].position[0],
        "Marathon sight axis is reversed"
    );
    Ok(sight)
}

pub(super) fn author(
    source: &mut Reader,
    native: &mut Reader,
    template: &Value,
    graph: &mut Graph,
    parts: &[Value],
    item: u32,
) -> Result<(Value, Value)> {
    let sight = read(source, parts)?;
    let kept = native::build(native, template, graph, &sight, item)?;
    let report = json!({"entity":sight.entity,"owner":sight.owner,"angle":sight.angle,
        "points":sight.points.iter().map(|m|json!({"rotation":m.orientation,"position":m.position})).collect::<Vec<_>>(),
        "movement":"assembled_object_space","gameplay_verified":false});
    Ok((kept, report))
}
