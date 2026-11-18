//! Open sights use the source weapon's declared render angle. Scene magnification and the
//! separate source sight angle remain distinct. Aperture/reticle projection keeps its own path.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Angle {
    pub angle: f32,
    pub report: Value,
}

pub(super) fn read(source: &mut Reader, parts: &[Value]) -> Result<Option<Angle>> {
    let entity = hash(parts.first().context("Empty sight assembly")?, "entity")?;
    let entity = source.tag(entity, Some(0x8080BAAD))?;
    let mut found = None;
    for at in entity.array(8, 12, Some(0x8080BAA2))? {
        let tag = entity.u32(at)?;
        let owner = source.tag(tag, Some(0x8080BADB))?;
        let base = owner.pointer(24)?;
        let class = owner.u32(base - 4)?;
        if !source
            .manager
            .get_entry(tiger_pkg::TagHash(class))
            .is_some_and(|entry| entry.reference == 0x80800000)
        {
            continue;
        }
        let schema = source.tag(class, Some(0x80800000))?;
        let mut offsets = BTreeSet::new();
        for field in (0..schema.0.len().saturating_sub(24)).step_by(8) {
            if schema.u32(field + 8)? != 0x8080000F || schema.u32(field + 16)? & 0x10000 == 0 {
                continue;
            }
            let name = schema.pointer(field)?;
            if schema.0.get(name..name + 27) == Some(b"aim_down_sights_render_fov\0") {
                offsets.insert(schema.u32(field + 12)? as usize);
            }
        }
        ensure!(
            offsets.len() <= 1,
            "Ambiguous source weapon render angle field"
        );
        let Some(offset) = offsets.into_iter().next() else {
            continue;
        };
        let angle = owner.f32(base + offset)?;
        ensure!(
            angle.is_finite() && (0.0..std::f32::consts::PI).contains(&angle),
            "Invalid source weapon render angle"
        );
        // Zero selects an untraced source camera fallback, not a zero-degree projection.
        if angle == 0.0 {
            continue;
        }
        ensure!(found.is_none(), "Multiple source weapon render angles");
        found = Some(Angle {
            angle,
            report: json!({"owner":tag,"schema":class,
            "field":"aim_down_sights_render_fov","offset":offset,"radians":angle}),
        });
    }
    Ok(found)
}
