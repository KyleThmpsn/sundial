//! Bind private vertex programs and constants to the final Shadowkeep model declaration.
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
        28 => Some(vec![
            position, weights, indices, uv, normal, tangent, detail,
        ]),
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
        "native vertex input repair requires a shader model 5.0 vertex program"
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
    original: &[u8],
    layouts: BTreeSet<i16>,
) -> Result<Option<Vec<u8>>> {
    let mut selected = None;
    for layout in layouts {
        let bytes = program(original, layout)
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
    Ok(selected.filter(|bytes| bytes != original))
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

/// Return repaired payloads by graph symbol without changing the pinned source files.
/// Only private programs and materials reached through supported final model stages are eligible.
pub fn repair(graph: &Value, directory: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let root = directory.canonicalize().context("imported graph folder")?;
    let mut nodes = BTreeMap::new();
    for node in graph["nodes"].as_array().context("graph nodes missing")? {
        let name = node["symbol"].as_str().context("graph symbol missing")?;
        ensure!(
            nodes.insert(name, node).is_none(),
            "duplicate graph symbol {name}"
        );
    }
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
    for (&name, &node) in &nodes {
        if name != "model" && node["model"] != true {
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
    let mut repaired = BTreeMap::new();
    for (name, layouts) in consumers {
        if layouts.iter().all(|layout| elements(*layout).is_none()) {
            continue;
        }
        ensure!(
            layouts.iter().all(|layout| elements(*layout).is_some()),
            "vertex program {name} is also consumed by an unsupported layout"
        );
        let original = read(get(name)?)?;
        if let Some(bytes) = compatible_program(name, &original, layouts)? {
            repaired.insert(name.to_owned(), bytes);
        }
    }
    for (name, roles) in materials {
        if !roles.contains(&true) {
            continue;
        }
        ensure!(
            roles.len() == 1,
            "cloth material {name} is shared by simulation and other draws"
        );
        let node = get(name)?;
        let Some(header) = target(node, 0x48)? else {
            continue;
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
        let original = read(node)?;
        if let Some(bytes) = cloth_scope(name, original)? {
            repaired.insert(name.to_owned(), bytes);
        }
    }
    Ok(repaired)
}
