//! Bind private material programs and scopes to their final Shadowkeep model draws.
//!
//! Native layouts assign registers in stream-element order. Compiling a subset of those
//! parameters produces different registers even when the semantic names still match.
use super::{payload::Payload, shader::dxbc};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod initial;

type Element = (&'static str, u32, u32, u8);

// Shadowkeep render-global declarations, in stream order. The last two values are the
// DXBC component type and available lanes, not the storage format of the vertex element.
fn elements(layout: i16) -> Option<Vec<Element>> {
    let position = ("POSITION", 0, 3, 15);
    let weights = ("BLENDWEIGHT", 0, 3, 15);
    let indices = ("BLENDINDICES", 0, 1, 15);
    let uv = ("TEXCOORD", 0, 3, 3);
    let normal = ("NORMAL", 0, 3, 15);
    let tangent = ("TANGENT", 0, 3, 15);
    let detail = ("TEXCOORD", 2, 3, 3);
    match layout {
        9 => Some(vec![position, weights, indices, uv, normal, tangent]),
        28 => Some(vec![
            position, weights, indices, uv, normal, tangent, detail,
        ]),
        123 => Some(vec![position, uv, normal, tangent]),
        139 => Some(vec![position, uv, normal, tangent, detail]),
        18 => Some(vec![position, normal, tangent, uv, weights, indices]),
        _ => None,
    }
}

fn program(bytes: &[u8], layout: i16) -> Result<Vec<u8>> {
    let elements = elements(layout).context("unsupported vertex input layout")?;
    let code = match dxbc::chunk(bytes, b"SHEX")? {
        Some(range) => range,
        None => dxbc::chunk(bytes, b"SHDR")?.context("vertex program missing")?,
    };
    ensure!(
        Payload(bytes[code].to_vec()).u32(0)? == 0x00010050,
        "native vertex input binding requires a shader model 5.0 vertex program"
    );
    let range = dxbc::chunk(bytes, b"ISGN")?.context("vertex input signature missing")?;
    let signature = Payload(bytes[range].to_vec());
    let mut mapping = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for (index, (name, semantic, register, mask)) in
        dxbc::signature(bytes, b"ISGN")?.into_iter().enumerate()
    {
        let system = signature.u32(8 + index * 24 + 8)?;
        let component = signature.u32(8 + index * 24 + 12)?;
        let name = name.to_ascii_uppercase();
        let (target, expected, available) = if system == 0 {
            let (target, element) = elements
                .iter()
                .enumerate()
                .find(|(_, (n, i, _, _))| *n == name && *i == semantic)
                .with_context(|| format!("layout {layout} has no {name}{semantic} input"))?;
            (target as u32, element.2, element.3)
        } else {
            let offset = match (name.as_str(), system, semantic) {
                ("SV_VERTEXID", 6, 0) => 0,
                ("SV_INSTANCEID", 8, 0) => 1,
                _ => anyhow::bail!("unsupported vertex system input {name}{semantic}"),
            };
            (elements.len() as u32 + offset, 1, 1)
        };
        ensure!(
            component == expected && mask != 0 && mask & !available == 0,
            "layout {layout} input {name}{semantic} has incompatible components"
        );
        ensure!(
            mapping.insert(register, target).is_none() && targets.insert(target),
            "vertex input registers overlap"
        );
    }
    if mapping.iter().all(|(from, to)| from == to) {
        return Ok(bytes.to_vec());
    }
    Ok(dxbc::patch(
        bytes,
        &dxbc::Remap {
            inputs: mapping.into_iter().collect(),
            ..Default::default()
        },
    )?
    .bytecode)
}

fn target(node: &Value, at: usize) -> Result<Option<&str>> {
    let patches = node["patches"]
        .as_array()
        .context("graph node patches missing")?;
    let mut found = None;
    for patch in patches {
        if patch["offset"].as_u64() == Some(at as u64) {
            ensure!(found.is_none(), "duplicate graph patch at {at:X}");
            found = Some(
                patch["symbol"]
                    .as_str()
                    .context("graph patch symbol missing")?,
            );
        }
    }
    Ok(found)
}

fn compatible_program(
    name: &str,
    layouts: BTreeSet<i16>,
    read: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    if layouts.iter().all(|layout| elements(*layout).is_none()) {
        return Ok(None);
    }
    let original = read()?;
    let mut selected = None;
    for &layout in layouts.iter().filter(|layout| elements(**layout).is_some()) {
        let bytes = program(&original, layout)
            .with_context(|| format!("vertex program {name}, layout {layout}"))?;
        if let Some(previous) = &selected {
            ensure!(
                *previous == bytes,
                "vertex program {name} is shared by incompatible layouts"
            );
        } else {
            selected = Some(bytes);
        }
    }
    let bound = selected.filter(|bytes| *bytes != original);
    // An unchanged program also preserves every unhandled consumer. A register
    // rewrite needs a verified mapping for all of them before it can be applied.
    ensure!(
        bound.is_none() || layouts.iter().all(|layout| elements(*layout).is_some()),
        "vertex program {name} is also consumed by an unsupported layout"
    );
    Ok(bound)
}

fn dye_scopes(mut bytes: Vec<u8>) -> Result<Option<Vec<u8>>> {
    let payload = Payload(bytes.clone());
    // Native gear draws gate their pixel dye binders on material +0x18.
    // Earlier imported color programs requested those scopes only at +0x1C.
    // Carry just the three declared dye dependencies, preserving all other bits.
    let required = payload.u32(0x1C)? & 0x1C00_0000;
    let original = payload.u32(0x18)?;
    let selected = original | required;
    if selected == original {
        return Ok(None);
    }
    bytes[0x18..0x1C].copy_from_slice(&selected.to_le_bytes());
    Ok(Some(bytes))
}

fn bind_material(node: &Value, original: Vec<u8>, source: &Value) -> Result<Option<Vec<u8>>> {
    let mut selected = original.clone();
    if source == "d1_roi" {
        for stage in [0x48, 0xE8, 0x188, 0x228, 0x2C8] {
            if target(node, stage)?.is_some() && target(node, stage + 0x84)?.is_none() {
                initial::bind(&mut selected, stage)
                    .with_context(|| format!("material stage {stage:X}"))?;
            }
        }
    }
    if let Some(bytes) = dye_scopes(selected.clone())? {
        selected = bytes;
    }
    if selected == original {
        return Ok(None);
    }
    let length = selected.len() as u64;
    selected[..8].copy_from_slice(&length.to_le_bytes());
    Ok(Some(selected))
}

fn cloth_scope(name: &str, original: Vec<u8>) -> Result<Option<Vec<u8>>> {
    let payload = Payload(original.clone());
    let mut bytes = original.clone();
    // The native simulation path prepares rigid_model (scope 2) at VS b11.
    // A copied weighted material selects skinning (scope 7) at the same slot,
    // whose first four vectors do not supply the simulated vertex transform.
    // Retain all pixel, dye and other scope bits in both material masks.
    for at in [0x18, 0x1C] {
        let mask = payload.u32(at)?;
        ensure!(
            matches!(mask & 0x84, 0x80 | 0x04),
            "cloth material {name} has an unsupported model scope mask {mask:08X}"
        );
        bytes[at..at + 4].copy_from_slice(&((mask & !0x80) | 0x04).to_le_bytes());
    }
    Ok((bytes != original).then_some(bytes))
}

fn frame_exposure(original: &[u8]) -> Result<Option<Vec<u8>>> {
    let range = match dxbc::chunk(original, b"SHEX")? {
        Some(range) => range,
        None => dxbc::chunk(original, b"SHDR")?.context("pixel program missing")?,
    };
    ensure!(
        Payload(original[range].to_vec()).u32(0)? == 0x00000050,
        "native frame exposure binding requires a shader model 5.0 pixel program"
    );
    // Stock Shadowkeep transparent emission multiplies frame vector 1's W and Z lanes.
    // The modern shader uses W and X. Reuse the particle adapter's bounded instruction
    // rewrite, retaining other frame reads and shaders already using the native lane.
    let patched = dxbc::patch(
        original,
        &dxbc::Remap {
            exposure: Some((13, 1)),
            ..Default::default()
        },
    )?;
    Ok((patched.changes != 0).then_some(patched.bytecode))
}

fn exposed_program(
    name: &str,
    draws: BTreeSet<(&str, usize)>,
    read: impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    if !draws.iter().any(|(_, stage)| *stage == 7) {
        return Ok(None);
    }
    let original = read(name)?;
    let Some(bytes) =
        frame_exposure(&original).with_context(|| format!("transparent pixel program {name}"))?
    else {
        return Ok(None);
    };
    for (material, stage) in draws {
        let payload = Payload(read(material)?);
        ensure!(
            stage == 7
                && payload.u32(8)? == 1
                && payload.u8(0x20)? & 0x7F == 8
                && payload.u32(0x18)? & 0x2001 == 0x2001
                && payload.u32(0x1C)? & 0x2001 == 0x2001,
            "pixel program {name} has an unsupported frame exposure consumer {material} at stage {stage}"
        );
    }
    Ok(Some(bytes))
}

fn nodes(graph: &Value) -> Result<BTreeMap<&str, &Value>> {
    let mut nodes = BTreeMap::new();
    for node in graph["nodes"].as_array().context("graph nodes missing")? {
        let name = node["symbol"].as_str().context("graph symbol missing")?;
        ensure!(
            nodes.insert(name, node).is_none(),
            "duplicate graph symbol {name}"
        );
    }
    Ok(nodes)
}

fn bind_cloth<'a>(
    name: &str,
    roles: &BTreeSet<bool>,
    get: impl Fn(&str) -> Result<&'a Value>,
    read: impl Fn(&Value) -> Result<Vec<u8>>,
    bound: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    if !roles.contains(&true) {
        return Ok(());
    }
    ensure!(
        roles.len() == 1,
        "cloth material {name} is shared by simulation and other draws"
    );
    let node = get(name)?;
    let Some(header) = target(node, 0x48)? else {
        return Ok(());
    };
    let data = get(header)?["reference"]
        .as_str()
        .context("private cloth vertex header has no bytecode reference")?;
    let signature = dxbc::signature(&read(get(data)?)?, b"ISGN")?;
    ensure!(
        signature.iter().any(|(semantic, index, _, _)| {
            semantic.eq_ignore_ascii_case("POSITION") && *index == 0
        }) && !signature.iter().any(|(semantic, _, _, _)| {
            semantic.eq_ignore_ascii_case("BLENDWEIGHT")
                || semantic.eq_ignore_ascii_case("BLENDINDICES")
        }),
        "simulated cloth material {name} does not use an unweighted vertex program"
    );
    let original = match bound.get(name) {
        Some(bytes) => bytes.clone(),
        None => read(node)?,
    };
    if let Some(bytes) = cloth_scope(name, original)? {
        bound.insert(name.to_owned(), bytes);
    }
    Ok(())
}

/// Return bound payloads by graph symbol without changing the pinned source files.
/// Only private programs and materials reached through supported final model stages are eligible.
pub fn bind(graph: &Value, directory: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let root = directory.canonicalize().context("imported graph folder")?;
    let models = crate::model_symbols(graph)?;
    let modern_materials = graph["source_stage_7_adapter"].is_object()
        || graph["rendering"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["source_stage_7_adapter"].is_object())
        });
    let nodes = nodes(graph)?;
    let get = |name: &str| -> Result<&Value> {
        nodes
            .get(name)
            .copied()
            .with_context(|| format!("missing graph symbol {name}"))
    };
    let read = |node: &Value| -> Result<Vec<u8>> {
        let path = root
            .join(node["file"].as_str().context("graph payload missing")?)
            .canonicalize()
            .context("graph payload path")?;
        ensure!(path.starts_with(&root), "graph payload escapes its folder");
        fs::read(&path).with_context(|| format!("read {}", path.display()))
    };
    let mut consumers = BTreeMap::<&str, BTreeSet<i16>>::new();
    let mut materials = BTreeMap::<&str, BTreeSet<bool>>::new();
    let mut pixels = BTreeMap::<&str, BTreeSet<(&str, usize)>>::new();
    for (&name, &node) in &nodes {
        if !models.contains(name) {
            continue;
        }
        let model = Payload(read(node)?);
        for mesh in model.array(0x10, 136, Some(0x80807378))? {
            let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
            for stage in 0..23 {
                let layout = model.i16(mesh + 88 + stage * 2)?;
                if layout < 0 {
                    continue;
                }
                let first = model.u16(mesh + 40 + stage * 2)? as usize;
                let end = model.u16(mesh + 42 + stage * 2)? as usize;
                for &part in parts
                    .get(first..end)
                    .context("model stage exceeds its draw table")?
                {
                    // An unpatched material or shader is an external stock resource.
                    let Some(material) = target(node, part)? else {
                        continue;
                    };
                    // Native float cloth draws distinguish simulation output from the
                    // weighted fallback using the draw flags and detail selectors.
                    let simulated = layout == 18
                        && model.u32(part + 24)? & 8 != 0
                        && model.u8(part + 27)? == 3
                        && model.u8(part + 28)? == 127;
                    materials.entry(material).or_default().insert(simulated);
                    if modern_materials && let Some(header) = target(get(material)?, 0x2C8)? {
                        let data = get(header)?["reference"]
                            .as_str()
                            .context("private pixel header has no bytecode reference")?;
                        pixels.entry(data).or_default().insert((material, stage));
                    }
                    let Some(header) = target(get(material)?, 0x48)? else {
                        continue;
                    };
                    let data = get(header)?["reference"]
                        .as_str()
                        .context("private vertex header has no bytecode reference")?;
                    consumers.entry(data).or_default().insert(layout);
                }
            }
        }
    }
    let mut bound = BTreeMap::new();
    for (name, layouts) in consumers {
        if let Some(bytes) = compatible_program(name, layouts, || read(get(name)?))? {
            bound.insert(name.to_owned(), bytes);
        }
    }
    for (name, draws) in pixels {
        if let Some(bytes) = exposed_program(name, draws, |name| read(get(name)?))? {
            bound.insert(name.to_owned(), bytes);
        }
    }
    for name in materials.keys() {
        let node = get(name)?;
        if let Some(bytes) = bind_material(node, read(node)?, &graph["source"])
            .with_context(|| format!("material {name}"))?
        {
            bound.insert((*name).to_owned(), bytes);
        }
    }
    for (name, roles) in materials {
        bind_cloth(name, &roles, get, read, &mut bound)?;
    }
    Ok(bound)
}
