//! Scripted scenery poses as rigid render entities. Their collision already belongs to the
//! area's world compound, so these entities contain only the render component.
use super::*;
use crate::halo_reach::model::Primitive;
use crate::tiger::entity::{compiled, links::NativeRows, owner_slots};

#[derive(Clone, Copy, serde::Serialize)]
pub struct Templates {
    pub entity: u32,
    pub render: u32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Prop {
    pub entity: String,
    pub position: [f32; 3],
    pub bsps: u32,
}

fn entity(
    native: &mut Reader,
    graph: &mut Graph,
    templates: Templates,
    name: &str,
    model: &str,
    sphere: [f32; 4],
) -> Result<String> {
    let entity = native.tag(templates.entity, Some(0x8080_9C0F))?;
    entity
        .array(0x10, 12, None)?
        .into_iter()
        .find(|&row| entity.u32(row).ok() == Some(templates.render))
        .context("The prop template does not name its render owner")?;
    let template = native.tag(templates.render, Some(0x8080_9C36))?;
    let resource = template.pointer(0x18)?;
    ensure!(
        template.u32(resource - 4)? == 0x8080_72BD,
        "The prop owner is not a render component"
    );
    // Rigid scenery materials are compiled without object channels. The donor's channel
    // bank is not retained, so its consumed inputs and allocation must also be removed.
    let allocation_tag = template.u32(0x44)?;
    let allocation = native.tag(allocation_tag, Some(0x8080_9BBB))?;
    let (owner, allocation) =
        crate::presentation::channel::without_object_inputs(&template, &allocation)?;
    let allocation_symbol = format!("{name}-allocation");
    graph.add(
        &allocation_symbol,
        allocation_tag,
        &allocation.0,
        None,
        Vec::new(),
    )?;
    let render_symbol = format!("{name}-render");
    let mut bytes = owner.0.clone();
    let mut patches = vec![
        json!({"offset":resource+0x1dc,"symbol":model}),
        json!({"offset":0x44,"symbol":allocation_symbol}),
    ];
    // Render owners can name themselves in their channel definitions.
    for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
        if bytes[at..at + 4] == templates.render.to_le_bytes() {
            patches.push(json!({"offset":at,"symbol":render_symbol}));
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
    }
    for (axis, value) in sphere.into_iter().enumerate() {
        put(&mut bytes, resource + 0xa0 + axis * 4, &value.to_le_bytes())?;
    }
    put(&mut bytes, 0x44, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, resource + 0x1dc, &u32::MAX.to_le_bytes())?;
    graph.add(&render_symbol, templates.render, &bytes, None, patches)?;
    // A component subset also needs new interface spans, ordinals and replication
    // rows. Retaining the donor graph would bind its removed gameplay components.
    let components = [compiled::Component {
        owner: templates.render,
        payload: &owner,
        template: &template,
        entity: &entity,
    }];
    let mut assembled = compiled::emit(
        &entity,
        &components,
        &NativeRows {
            connections: Vec::new(),
            named_connections: Vec::new(),
        },
    )?;
    let replication_template = entity.u32(0x88)?;
    let replication_symbol = format!("{name}-replication");
    let symbol = format!("{name}-entity");
    let mut patches = owner_slots(&assembled, &owner, templates.render)?
        .into_iter()
        .map(|at| json!({"offset":at,"symbol":render_symbol}))
        .collect::<Vec<_>>();
    // A render-only donor may have no replication definition. Preserve that absence.
    // A present definition must describe the retained component set, not the donor's.
    if replication_template != u32::MAX {
        native.tag(replication_template, Some(0x8080_9BB6))?;
        patches.push(json!({"offset":0x88,"symbol":replication_symbol}));
    }
    for patch in &patches {
        put(
            &mut assembled.0,
            patch["offset"].as_u64().context("Prop entity patch")? as usize,
            &u32::MAX.to_le_bytes(),
        )?;
    }
    graph.add(&symbol, templates.entity, &assembled.0, None, patches)?;
    if replication_template != u32::MAX {
        let mut replication = compiled::replication::emit(templates.entity, &components)?;
        let allocation_field = replication.array(0x30, 32, Some(0x8080_9BB8))?[0] + 4;
        put(&mut replication.0, 8, &u32::MAX.to_le_bytes())?;
        put(
            &mut replication.0,
            allocation_field,
            &u32::MAX.to_le_bytes(),
        )?;
        graph.add(
            &replication_symbol,
            replication_template,
            &replication.0,
            None,
            vec![
                json!({"offset":8,"symbol":symbol}),
                json!({"offset":allocation_field,"symbol":allocation_symbol}),
            ],
        )?;
    }
    Ok(symbol)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &super::Templates,
    placements: &[Vec<Primitive>],
    materials: &[Material],
    offset: [f32; 3],
    report: &mut dyn FnMut(&str),
) -> Result<(Vec<Prop>, Value)> {
    if placements.is_empty() {
        return Ok((Vec::new(), json!([])));
    }
    let mut used = BTreeMap::new();
    for primitive in placements.iter().flatten() {
        let index = primitive
            .material
            .context("A posed prop has no source material")?;
        used.insert(
            u32::try_from(index)?,
            materials
                .get(index)
                .context("Prop material outside the level")?
                .clone(),
        );
    }
    let material_report = native::material::build_with_progress(
        cache,
        pages,
        native,
        graph,
        &used,
        native::material::Binding {
            palette: &[0],
            rigid: true,
            objects: &BTreeMap::new(),
            overrides: &BTreeMap::new(),
            lenses: &BTreeMap::new(),
            paint: &BTreeMap::new(),
            lightmap: None,
            vertex_data: None,
            statics: false,
            prefix: "prop-",
        },
        report,
    )?;
    let stages = used
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
    let template = native.tag(templates.model, Some(MODEL_CLASS))?;
    let row = *template
        .array(16, 136, Some(MESH_CLASS))?
        .first()
        .context("Prop template has no mesh")?;
    let mut result = Vec::new();
    for (index, primitives) in placements.iter().enumerate() {
        let name = format!("prop{index}");
        let (low, high) = bounds(
            primitives
                .iter()
                .flat_map(|primitive| primitive.vertices.iter().map(|v| v.position)),
        );
        let center: [f32; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
        let mut mesh = Mesh {
            positions: Vec::new(),
            attributes: Vec::new(),
            groups: Vec::new(),
            weights: Vec::new(),
            bones: 0,
        };
        let mut bsps = 0;
        for primitive in primitives {
            bsps |= primitive.bsps;
            let base = u32::try_from(mesh.positions.len())?;
            for vertex in &primitive.vertices {
                mesh.positions.push(std::array::from_fn(|axis| {
                    vertex.position[axis] - center[axis]
                }));
                mesh.attributes.push([
                    vertex.uv[0],
                    vertex.uv[1],
                    vertex.normal[0],
                    vertex.normal[1],
                    vertex.normal[2],
                    0.,
                    vertex.tangent[0],
                    vertex.tangent[1],
                    vertex.tangent[2],
                    vertex.tangent[3],
                ]);
            }
            mesh.groups.push((
                u32::try_from(primitive.material.context("Prop material")?)?,
                primitive
                    .indices
                    .chunks_exact(3)
                    .map(|face| [face[0] + base, face[1] + base, face[2] + base])
                    .collect(),
            ));
        }
        let mut encoded = geometry::encode_with_stages(&mesh, &template, row, &stages)?;
        // A prop's single detail level remains visible at every native distance band.
        let rows = Payload(encoded.header.clone()).array(0xc8, 32, None)?;
        for row in rows {
            encoded.header[row + 0x1b] = 2;
        }
        for (kind, at, bytes) in [
            ("positions", 0, &encoded.streams[0]),
            ("attributes", 4, &encoded.streams[1]),
            ("indices", 16, &encoded.streams[2]),
        ] {
            let mut tag = template.u32(row + at)?;
            if tag == u32::MAX && at != 16 {
                tag = template.u32(row + if at == 0 { 4 } else { 0 })?;
            }
            let mut header = native.tag(tag, None)?.0.clone();
            if at == 16 {
                put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
                header[1] = 0;
            } else {
                put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
                put(
                    &mut header,
                    4,
                    &(if at == 0 { 8u16 } else { 24u16 }).to_le_bytes(),
                )?;
            }
            let symbol = format!("{name}-{kind}-header");
            let data = format!("{name}-{kind}-data");
            graph.add(&symbol, tag, &header, Some(&data), vec![])?;
            graph.add(&data, native.reference(tag)?, bytes, Some(&symbol), vec![])?;
        }
        for patch in &mut encoded.patches {
            let symbol = patch["symbol"].as_str().context("Prop mesh patch")?;
            patch["symbol"] = json!(if symbol.starts_with("material-") {
                format!("prop-{symbol}")
            } else {
                format!("{name}-{symbol}")
            });
        }
        let model = format!("{name}-model");
        let header = Payload(encoded.header.clone());
        let sphere = [
            header.f32(0x80)?,
            header.f32(0x84)?,
            header.f32(0x88)?,
            header.f32(0x8c)?,
        ];
        graph.add(
            &model,
            templates.model,
            &encoded.header,
            None,
            encoded.patches,
        )?;
        result.push(Prop {
            entity: entity(native, graph, templates.props, &name, &model, sphere)?,
            position: std::array::from_fn(|axis| center[axis] + offset[axis]),
            bsps,
        });
    }
    Ok((result, json!(material_report)))
}
