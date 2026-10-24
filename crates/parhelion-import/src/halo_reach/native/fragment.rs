//! Source presentation on a rigid native object with no weapon or vehicle roles.
use super::*;

pub(super) fn build(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &Scene,
    graph: &mut Graph,
    entity: u32,
    frame: &[f32; 16],
) -> Result<(u32, Value)> {
    let models = runtime_models(native, entity, 6)?;
    let (model, header, row) = carrier(native, &models)?;
    ensure!(
        header.u32(0x40)? == 0,
        "Fragment presentation needs a rigid object frame"
    );
    let bones = json!({"bones":[{"index":0,"name_hash":format!("{:08X}",name_hash("b_handle")),"parent":-1}],
        "static":true,"rigid_uniforms":true});
    let Assembly {
        mut mesh,
        materials,
        palette,
        rig_report,
        ..
    } = assemble(scene, &bones, &BTreeMap::new())?;
    for position in &mut mesh.positions {
        *position = rig::point(frame, *position, false);
    }
    for attributes in &mut mesh.attributes {
        for first in [2, 6] {
            let vector = rig::point(frame, attributes[first..first + 3].try_into()?, false);
            attributes[first..first + 3].copy_from_slice(&vector);
        }
    }
    let stages = materials
        .iter()
        .map(|(&key, material)| {
            (
                key,
                if material.alpha_mode() == "BLEND" {
                    7
                } else {
                    0
                },
            )
        })
        .collect();
    let groups = vec![0; mesh.groups.len()];
    let encoded = geometry::encode_grouped(&mesh, &header, row, &stages, &groups)?;
    buffers(native, graph, &header, row, &encoded)?;
    graph.add(
        "model",
        hash(&model, "model")?,
        &encoded.header,
        None,
        encoded.patches,
    )?;
    let report = super::material::build(
        cache,
        pages,
        native,
        graph,
        &materials,
        super::material::Binding {
            palette: &palette,
            rigid: true,
            objects: &BTreeMap::new(),
            overrides: &BTreeMap::new(),
            lenses: &BTreeMap::new(),
            paint: &BTreeMap::new(),
            lightmap: None,
            vertex_data: None,
            statics: false,
            prefix: "",
        },
    )?;
    Ok((
        hash(&model, "owner")?,
        json!({"source":scene.object.tag,
        "vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),
        "materials":report,"rig":rig_report,"binding":"Rigid object frame"}),
    ))
}
