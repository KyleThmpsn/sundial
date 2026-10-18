//! Match every staged damage face and its activation condition to raw MCC geometry.
use super::*;

type Condition = (u32, Vec<u32>);
type Coverage = BTreeMap<Condition, BTreeMap<[u32; 3], usize>>;

fn condition(render: &Payload, group: usize) -> Result<Condition> {
    let mut region = None;
    let mut states = Vec::new();
    for pair in render.array(group + 8, 8, Some(0x80809c31))? {
        match render.u32(pair)? {
            0x51e7a18d => {
                ensure!(
                    region.replace(render.u32(pair + 4)?).is_none(),
                    "Two damage regions"
                );
            }
            0x6a807a1a => states.push(render.u32(pair + 4)?),
            _ => anyhow::bail!("Unrecognized damage geometry condition"),
        }
    }
    states.sort_unstable();
    ensure!(!states.is_empty(), "Damage geometry has no state condition");
    Ok((region.context("Damage geometry has no region")?, states))
}

fn expected(primitives: &[Value]) -> Result<Coverage> {
    let mut coverage = Coverage::new();
    let mut first = 0u32;
    for primitive in primitives {
        let region = hash(primitive["region"].as_str().context("Source mesh region")?);
        let mut states = primitive["states"]
            .as_array()
            .context("Source mesh states")?
            .iter()
            .map(|state| {
                STATES
                    .get(state.as_u64().context("Source mesh state")? as usize)
                    .copied()
                    .context("Source mesh state out of range")
            })
            .collect::<Result<Vec<_>>>()?;
        states.sort_unstable();
        let vertices = primitive["vertices"]
            .as_array()
            .context("Source mesh vertices")?;
        let indices = primitive["indices"]
            .as_array()
            .context("Source mesh indices")?;
        ensure!(
            !vertices.is_empty() && !indices.is_empty() && indices.len().is_multiple_of(3),
            "Empty or incomplete source mesh"
        );
        let faces = coverage.entry((region, states)).or_default();
        for face in indices.chunks_exact(3) {
            let mut triangle = [0; 3];
            for (i, value) in face.iter().enumerate() {
                let index = usize::try_from(value.as_u64().context("Source mesh index")?)?;
                ensure!(index < vertices.len(), "Source mesh index escapes vertices");
                triangle[i] = first
                    .checked_add(u32::try_from(index)?)
                    .context("Source mesh index overflow")?;
            }
            *faces.entry(triangle).or_default() += 1;
        }
        first = first
            .checked_add(u32::try_from(vertices.len())?)
            .context("Source vertex count overflow")?;
    }
    ensure!(
        !coverage.is_empty(),
        "Empty required damage geometry witness"
    );
    Ok(coverage)
}

pub(super) fn verify(staged: &mut Reader, runtime: &Value, witness: &Value) -> Result<Value> {
    let primitives = witness["geometry"]["models"][0]["model"]["primitives"]
        .as_array()
        .context("Independent damage geometry")?;
    let expected = expected(primitives)?;
    let vertices = primitives
        .iter()
        .flat_map(|p| p["vertices"].as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    let render = component(staged, runtime, "808072BD")?;
    let definition = render.pointer(24)?;
    let groups = render.array(definition + 0x38, 24, Some(0x80809c2c))?;
    let tag = render.u32(definition + 0x1dc)?;
    let model = staged.tag(tag, Some(0x808073a5))?;
    let meshes = model.array(16, 136, Some(0x80807378))?;
    ensure!(
        meshes.len() == 1,
        "Damage geometry requires one complete mesh"
    );
    let mesh = meshes[0];
    let positions = staged.tag(staged.reference(model.u32(mesh)?)?, None)?;
    ensure!(
        positions.0.len() == vertices.len() * 16,
        "Staged damage vertex coverage differs"
    );
    verify_skin(
        &positions,
        &model,
        &vertices,
        number(&witness["geometry_scale"])?,
    )?;
    let indices = staged.tag(staged.reference(model.u32(mesh + 16)?)?, None)?;
    let mut actual = Coverage::new();
    for stage in [0, 7] {
        let parts = model.array(mesh + 24, 32, Some(0x8080737e))?;
        let range = usize::from(model.u16(mesh + 40 + stage * 2)?)
            ..usize::from(model.u16(mesh + 42 + stage * 2)?);
        for &part in parts
            .get(range)
            .context("Damage draw stage escapes parts")?
        {
            let group = *groups
                .get(usize::from(model.u16(part + 20)?))
                .context("Damage draw has no condition")?;
            let faces = actual.entry(condition(&render, group)?).or_default();
            let first = usize::try_from(model.u32(part + 8)?)?;
            let count = usize::try_from(model.u32(part + 12)?)?;
            ensure!(
                count > 0 && count.is_multiple_of(4),
                "Incomplete staged damage faces"
            );
            for index in (first..first + count).step_by(4) {
                let triangle = [
                    indices.u32(index * 4)?,
                    indices.u32(index * 4 + 4)?,
                    indices.u32(index * 4 + 8)?,
                ];
                ensure!(
                    indices.u32(index * 4 + 12)? == u32::MAX,
                    "Damage face lacks restart"
                );
                *faces.entry(triangle).or_default() += 1;
            }
        }
    }
    ensure!(
        actual == expected,
        "Staged damage mesh topology or region/state membership differs from MCC source"
    );
    Ok(
        json!({"model":format!("{tag:08X}"),"vertices":vertices.len(),"conditions":actual.len(),
        "triangles":actual.values().flat_map(|faces| faces.values()).sum::<usize>(),"raw_source_geometry_verified":true}),
    )
}
