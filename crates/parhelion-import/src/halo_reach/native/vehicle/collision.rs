//! Source damage forms, solid bodies and surface hit meshes in native frames.
mod hull;
mod shot;
#[cfg(test)]
mod tests;
use super::{assembly::Assembly, body, damage};
use crate::{
    halo_reach::{Scene, rig},
    presentation::{Graph, put},
    tiger::{
        physics::{Hull, Shape},
        reader::Reader,
    },
};
use anyhow::{Context, Result, bail, ensure};
pub(super) use hull::build as hulls;
use serde_json::{Value, json};

fn vector<const N: usize>(value: &Value) -> Result<[f32; N]> {
    let values = value
        .as_array()
        .filter(|v| v.len() == N)
        .context("Collision vector length")?;
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = value.as_f64().context("Collision vector number")? as f32;
        ensure!(out.is_finite(), "Nonfinite collision vector");
    }
    Ok(result)
}

fn condition(
    groups: &mut Vec<(String, Vec<u8>)>,
    value: Option<&(String, Vec<u8>)>,
) -> Result<u32> {
    let Some(value) = value else {
        return Ok(0);
    };
    if let Some(index) = groups.iter().position(|group| group == value) {
        return Ok(u32::try_from(index + 1)?);
    }
    groups.push(value.clone());
    Ok(u32::try_from(groups.len())?)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
) -> Result<Value> {
    let mut selected = body::physical(scene)?;
    // The initial active body keeps the carrier's primary name. All solid rows
    // share its bone so the native constructor combines their enabled shapes.
    selected.sort_by_key(|part| {
        (
            !part
                .condition
                .as_ref()
                .is_none_or(|(_, states)| states.contains(&0)),
            part.body,
        )
    });
    let mut hulls = Vec::new();
    let mut shape_groups = Vec::new();
    let mut groups = Vec::new();
    let mut solids = Vec::new();
    let mut initial_mass = 0.;
    for part in selected {
        let properties = body::properties(scene, part.body)?;
        if part
            .condition
            .as_ref()
            .is_none_or(|(_, states)| states.contains(&0))
        {
            initial_mass += properties.mass;
        }
        let shapes = hull::build(scene, bones, part.body)?;
        let group = condition(&mut groups, part.condition.as_ref())?;
        let share = properties.share(shapes.len())?;
        // Native dynamic assembly rejects a nested hkpListShape after its
        // first body. Expose each convex leaf directly, retaining the source
        // condition and distributing its physical properties exactly once.
        for shape in shapes {
            shape_groups.push(vec![hulls.len()]);
            hulls.push(shape);
            solids.push((part.body, group, properties.mass, share.clone()));
        }
    }
    ensure!(
        initial_mass.is_finite() && initial_mass > 0.,
        "Source intact vehicle has no mass"
    );
    let hits = shot::build(scene, bones)?;
    let hit_groups = hits
        .iter()
        .map(|mesh| condition(&mut groups, Some(&mesh.condition)))
        .collect::<Result<Vec<_>>>()?;
    let meshes = hits
        .iter()
        .map(|mesh| mesh.triangles.clone())
        .collect::<Vec<_>>();
    let owner = assembly.owner(0x808092d8)?;
    let (allocation_tag, mut layout) = assembly.allocation(native, graph, owner)?;
    let payload = assembly.payloads.get_mut(&owner).unwrap();
    let (instance, definition) = damage::definition(payload, 0x808092d8, 0x80808a0c)?;
    let old_rows = payload.array(definition + 0x388, 0x90, Some(0x80808c07))?;
    let primary = body::primary(payload)?;
    ensure!(
        (payload.u32(primary + 0x18)? >> 8) & 0xff == 1 && payload.u32(primary + 0x80)? == 1,
        "Native solid-body filter or motion contract differs"
    );
    let hit = old_rows
        .iter()
        .copied()
        .find(|&row| {
            payload.u32(row + 0x18).is_ok_and(|v| (v >> 8) & 0xff == 2)
                && payload.u32(row + 0x80).ok() == Some(4)
                && payload.f32(row + 0x60).ok() == Some(0.)
        })
        .context("Native animated hit-body contract")?;
    let solid_template = payload.bytes::<0x90>(primary)?;
    let hit_template = payload.bytes::<0x90>(hit)?;
    let target_mass = payload.f32(primary + 0x60)?;
    ensure!(
        (target_mass - initial_mass).abs() <= initial_mass * 0.00001,
        "Vehicle motion and physical parts disagree on initial mass"
    );
    let count = solids.len() + hits.len();
    let rows = damage::array(payload, definition + 0x388, 0x80808c07, 0x90, count)?;
    let damage_source = &scene.object.gameplay["damage"];
    let main = usize::try_from(
        damage_source["indirect_section"]
            .as_u64()
            .context("Source indirect damage section")?,
    )?;
    let main_name = damage_source["sections"][main]["name"]
        .as_str()
        .context("Source primary damage section")?;
    let mut report = Vec::new();
    for (index, (&row, (source, group, source_mass, properties))) in
        rows.iter().zip(&solids).enumerate()
    {
        put(&mut payload.0, row, &solid_template)?;
        if index != 0 {
            put(
                &mut payload.0,
                row,
                &damage::hash(&format!("reach_body_{source}_{index}")).to_le_bytes(),
            )?;
        }
        put(
            &mut payload.0,
            row + 4,
            &damage::hash(main_name).to_le_bytes(),
        )?;
        put(&mut payload.0, row + 8, &0u32.to_le_bytes())?;
        put(&mut payload.0, row + 12, &group.to_le_bytes())?;
        put(
            &mut payload.0,
            row + 0x88,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        body::write(payload, row, bones, properties)?;
        report.push(json!({"source_body":source,"row":row,"group":group,"shape":index,"source_mass":source_mass,
            "mass":properties.mass,"bone":0,"section":main_name,"hulls":shape_groups[index]}));
    }
    let mut hit_report = Vec::new();
    for (index, mesh) in hits.iter().enumerate() {
        let row = rows[solids.len() + index];
        put(&mut payload.0, row, &hit_template)?;
        for (offset, value) in [
            (0, damage::hash(&mesh.name)),
            (4, damage::hash(&mesh.section)),
            (8, u32::try_from(mesh.bone)?),
            (12, hit_groups[index]),
            (0x88, u32::try_from(solids.len() + index)?),
        ] {
            put(&mut payload.0, row + offset, &value.to_le_bytes())?;
        }
        hit_report.push(json!({"name":mesh.name,"section":mesh.section,"bone":mesh.bone,"group":hit_groups[index],
            "shape":solids.len()+index,"row":row,"triangles":mesh.triangles.len()}));
    }
    damage::region::parts(payload, definition, &groups)?;
    for (field, declared, allocation) in [(0x180, 0x378, 0x2565fa53), (0x1a0, 0x37c, 0xead45d3b)] {
        damage::array(payload, instance + field, 0x80800007, 4, count)?;
        put(
            &mut payload.0,
            definition + declared,
            &u32::try_from(count)?.to_le_bytes(),
        )?;
        damage::allocation(&mut layout, &[allocation], 0x80800007, count)?;
    }
    // The carrier's cloth hull belongs to its original silhouette.
    put(&mut payload.0, definition + 0x3b8, &[0; 16])?;
    let previous = payload.u32(definition + 0x380)?;
    let end = payload.0.len();
    let template = native.tag(0x80b822ba, None)?;
    let bytes = crate::tiger::physics::vehicle_shapes(&template.0, &hulls, &shape_groups, &meshes)?;
    graph.add("vehicle-collision", previous, &bytes, None, Vec::new())?;
    assembly.runtime_end(owner, end)?;
    assembly.set_allocation(graph, owner, allocation_tag, &layout)?;
    assembly.resource(owner, definition + 0x380, "vehicle-collision".into())?;
    Ok(
        json!({"owner":format!("{owner:08X}"),"source":scene.object.gameplay["rigid_body"]["tag"],
        "solid_bodies":report,"hit_bodies":hit_report,"groups":groups,"native_bodies":count,
        "source_initial_mass":initial_mass,"native_initial_mass":target_mass,
        "root_bone":0,"solid_shape":"convex_leaf","child_filter":"inherit_live_parent","gameplay_verified":false,
        "limits":["Source cooked masses, inertia, solid forms and separate hit geometry are authored. Live impacts, hit routing, state changes and wreck behavior require gameplay acceptance."]}),
    )
}
