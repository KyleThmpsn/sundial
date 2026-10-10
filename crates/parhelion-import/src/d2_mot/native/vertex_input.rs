//! Bind private vertex programs to the final Shadowkeep model declaration.
//!
//! Native layouts assign registers in stream-element order. Compiling a subset of those
//! parameters produces different registers even when the semantic names still match.
use super::{shader::dxbc, *};

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

/// Return repaired payloads by graph symbol without changing the pinned source files.
/// Only private vertex programs reached through supported final model stages are eligible.
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
        let mut selected = None;
        for layout in layouts {
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
        if let Some(bytes) = selected
            && bytes != original
        {
            repaired.insert(name.to_owned(), bytes);
        }
    }
    Ok(repaired)
}
