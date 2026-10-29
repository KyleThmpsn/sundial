//! Private native sight interfaces with source aim poses and no donor lens geometry.
use super::*;

fn template(reader: &mut Reader, art: &Value) -> Result<(Value, u32, Payload)> {
    for parent in art["parents"].as_array().context("Native sight parents")? {
        if parent["placement"]["selector"].as_u64().is_none() {
            continue;
        }
        let bytes = hex::decode(parent["parent_bytes"].as_str().context("Sight parent")?)?;
        let tag = u32::from_le_bytes(bytes.get(16..20).context("Sight entity")?.try_into()?);
        let entity = reader.tag(tag, Some(0x80809C0F))?;
        let mut found = None;
        for component in entity.array(16, 12, Some(0x80809C04))? {
            let owner = reader.tag(entity.u32(component)?, Some(0x80809C36))?;
            if owner.u32(owner.pointer(24)? - 4)? == 0x8080393B {
                ensure!(found.is_none(), "Ambiguous native sight component");
                found = Some(());
            }
        }
        if found.is_some() {
            return Ok((parent.clone(), tag, (*entity).clone()));
        }
    }
    anyhow::bail!("Native carrier has no sight component")
}

/// Sights keep their source eye. A graph can still record a moved eye in
/// `source_optics.framing`, with the optic and its source rear point. A build returns that optic
/// to the source rear point, replacing an already bound payload or reading the node's file. A
/// graph without the record keeps its optic unchanged.
pub fn restore_source_eye(
    graph: &Value,
    folder: &Path,
    bound: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let framing = &graph["source_optics"]["framing"];
    let Some(symbol) = framing["optic"].as_str() else {
        return Ok(());
    };
    let mut owner = match bound.remove(symbol) {
        Some(bytes) => Payload(bytes),
        None => {
            let node = graph["nodes"]
                .as_array()
                .context("Marathon graph nodes")?
                .iter()
                .find(|node| node["symbol"] == symbol)
                .context("Framed optic node")?;
            Payload(fs::read(
                folder.join(node["file"].as_str().context("Framed optic file")?),
            )?)
        }
    };
    let (rear, _, front) = points(&owner)?;
    ensure!(
        front == coordinates(&framing["front"])?,
        "Framed optic differs from its recorded sight"
    );
    write(&mut owner, rear, coordinates(&framing["source_rear"])?)?;
    bound.insert(symbol.to_owned(), owner.0);
    Ok(())
}

/// A private optic's rear position offset, rear point and front point.
fn points(owner: &Payload) -> Result<(usize, [f32; 3], [f32; 3])> {
    let data = owner.pointer(24)?;
    ensure!(
        owner.u32(data - 4)? == 0x8080393B,
        "Framed sight is not a native optic"
    );
    let rear = owner.array(data + 0x198, 64, Some(0x80809C00))?;
    let front = owner.array(data + 0x228, 64, Some(0x80809C00))?;
    ensure!(
        rear.len() == 1 && front.len() == 1,
        "Private optic needs one rear and one front point"
    );
    let point = |at: usize| -> Result<[f32; 3]> {
        Ok([owner.f32(at)?, owner.f32(at + 4)?, owner.f32(at + 8)?])
    };
    Ok((rear[0] + 32, point(rear[0] + 32)?, point(front[0] + 32)?))
}

fn write(owner: &mut Payload, at: usize, point: [f32; 3]) -> Result<()> {
    for (lane, value) in point.iter().enumerate() {
        put(&mut owner.0, at + lane * 4, &value.to_le_bytes())?;
    }
    Ok(())
}

fn coordinates(value: &Value) -> Result<[f32; 3]> {
    let lanes = value
        .as_array()
        .filter(|lanes| lanes.len() == 3)
        .context("Sight framing point")?;
    let mut point = [0.; 3];
    for (lane, value) in point.iter_mut().zip(lanes) {
        *lane = value.as_f64().context("Sight framing coordinate")? as f32;
    }
    Ok(point)
}

fn aligned(original: &Payload, sight: &Sight) -> Result<Payload> {
    let mut owner = original.clone();
    let data = owner.pointer(24)?;
    put(&mut owner.0, data + 0x80, &sight.angle.to_le_bytes())?;
    for (i, descriptor) in [0x198, 0x228].into_iter().enumerate() {
        ensure!(
            owner.u32(data + descriptor - 40)? == 0x80809BE2
                && owner.u32(data + descriptor - 12)? == sight.points[i].name,
            "Native sight provider layout differs"
        );
        let mut row = vec![0; 64];
        for (offset, values) in [
            (16, sight.points[i].orientation),
            (32, sight.points[i].position),
        ] {
            for (lane, value) in values.into_iter().enumerate() {
                put(&mut row, offset + lane * 4, &value.to_le_bytes())?;
            }
        }
        put(&mut row, 48, &sight.points[i].name.to_le_bytes())?;
        append(&mut owner.0, data + descriptor, 0x80809C00, &row, 64)?;
        put(&mut owner.0, data + descriptor - 32, &1u32.to_le_bytes())?;
    }
    Ok(owner)
}

fn hide(
    reader: &mut Reader,
    graph: &mut Graph,
    owner: &mut Payload,
    symbol: &str,
    patches: &mut Vec<Value>,
) -> Result<()> {
    let data = owner.pointer(24)?;
    let class = owner.u32(data - 4)?;
    if matches!(class, 0x808072BD | 0x80807286) {
        let model_tag = owner.u32(data + 0x1DC)?;
        let model = reader.tag(model_tag, Some(0x808073A5))?;
        let mut hidden = model.0.clone();
        for mesh in model.array(16, 136, Some(0x80807378))? {
            hidden[mesh + 24..mesh + 88].fill(0);
            hidden[mesh + 88..mesh + 134].fill(0xFF);
        }
        let model_symbol = format!("{symbol}-model");
        graph.add(&model_symbol, model_tag, &hidden, None, vec![])?;
        put(&mut owner.0, data + 0x1DC, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":data + 0x1DC,"symbol":model_symbol}));
    }
    Ok(())
}

fn relocate(
    owner: &mut Payload,
    components: &BTreeMap<u32, (String, Payload)>,
    patches: &mut Vec<Value>,
) -> Result<()> {
    for at in (0..owner.0.len().saturating_sub(3)).step_by(4) {
        if let Some((target, payload)) = components.get(&owner.u32(at)?) {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < payload.0.len() as u64,
                "Untyped native sight owner reference"
            );
            put(&mut owner.0, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":at,"symbol":target}));
        }
    }
    Ok(())
}

pub(super) fn build(
    reader: &mut Reader,
    art: &Value,
    graph: &mut Graph,
    sight: &Sight,
    item: u32,
) -> Result<Value> {
    let (parent, entity_tag, original) = template(reader, art)?;
    let body = hash(graph.node("entity")?, "template")?;
    let in_body = entity_tag == body;
    let model_owner = hash(graph.node("owner")?, "template")?;
    let mut entity = if in_body {
        graph.read("entity")?
    } else {
        original.clone()
    };
    let components = original
        .array(16, 12, Some(0x80809C04))?
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let tag = original.u32(at)?;
            let payload = reader.tag(tag, Some(0x80809C36))?;
            Ok((
                tag,
                (
                    if in_body
                        && graph.nodes.iter().any(|node| {
                            node["template"].as_u64() == Some(u64::from(tag))
                                && node["symbol"] == "object-channels"
                        })
                    {
                        "object-channels".into()
                    } else if in_body && tag == model_owner {
                        "owner".into()
                    } else {
                        format!("optic-owner-{i}")
                    },
                    (*payload).clone(),
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut entity_patches = if in_body {
        graph.node("entity")?["patches"]
            .as_array()
            .context("Body patches")?
            .clone()
    } else {
        Vec::new()
    };
    for (&tag, (symbol, original_owner)) in &components {
        if in_body && tag == model_owner {
            continue;
        }
        // A live material input can already have a private bank in this body.
        // Keep its scheduler and provider connections rather than replacing that
        // component with another copy of the stock donor bank.
        if in_body
            && graph.nodes.iter().any(|node| {
                node["template"].as_u64() == Some(u64::from(tag))
                    && node["symbol"] == "object-channels"
            })
        {
            continue;
        }
        let mut owner = original_owner.clone();
        let data = owner.pointer(24)?;
        let class = owner.u32(data - 4)?;
        if class == 0x8080393B {
            owner = aligned(original_owner, sight)?;
            crate::tiger::markers::optics::connections::apply(
                &mut entity,
                &mut entity_patches,
                tag,
                symbol,
                original_owner,
                [0xAE7D0AED; 2],
            )?;
        }
        let mut patches = Vec::new();
        hide(reader, graph, &mut owner, symbol, &mut patches)?;
        relocate(&mut owner, &components, &mut patches)?;
        graph.add(symbol, tag, &owner.0, None, patches)?;
        for at in crate::tiger::entity::owner_slots(&original, original_owner, tag)? {
            // Rewritten movement providers no longer reference their donor adapter.
            if entity.u32(at)? != tag {
                continue;
            }
            put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
            entity_patches.push(json!({"offset":at,"symbol":symbol}));
        }
    }
    if in_body {
        graph.replace("entity", &entity.0, entity_patches)?;
        return Ok(json!([]));
    }
    graph.add("optic-entity", entity_tag, &entity.0, None, entity_patches)?;
    let parent_tag = hash(&parent, "parent")?;
    let mut bytes = reader.tag(parent_tag, None)?.0.clone();
    put(&mut bytes, 16, &u32::MAX.to_le_bytes())?;
    graph.add(
        "optic-parent",
        parent_tag,
        &bytes,
        None,
        vec![json!({"offset":16,"symbol":"optic-entity"})],
    )?;
    let companion = reader.tag(0x81A662DE, None)?;
    graph.add(
        "optic-parent-companion",
        0x81A662DE,
        &companion.0,
        None,
        vec![],
    )?;
    let node = graph.nodes.last_mut().context("Sight companion")?;
    node["shared_owner"] = json!("optic-parent");
    node["source_parent"] = json!(parent_tag);
    let mut placements = Vec::new();
    for alternative in art["parents"]
        .as_array()
        .context("Native sight alternatives")?
    {
        if alternative["placement"]["selector"] != parent["placement"]["selector"] {
            continue;
        }
        let position = alternative["placement"]["position"]
            .as_u64()
            .context("Sight alternative position")?;
        let key = sight_key(item, position);
        placements.push(json!({"assignment":hash(alternative,"assignment")?,"key":key,"parent":"optic-parent","native_region":true,
            "selector":alternative["placement"]["selector"],"position":position}));
    }
    ensure!(!placements.is_empty(), "Missing native sight alternatives");
    Ok(json!(placements))
}

/// The art key of `item`'s sight placement at `position`. A source optic can serve several
/// authored items in the same package batch, so each item keys its own.
pub(in crate::marathon) fn sight_key(item: u32, position: u64) -> u32 {
    let mut key = format!("parhelion/imported-model/{item:08X}/sight-{position}")
        .bytes()
        .fold(0x811C9DC5u32, |h, b| {
            h.wrapping_mul(0x1000193) ^ u32::from(b)
        });
    if [0, u32::MAX, 0x811C9DC5].contains(&key) {
        key ^= 0x10000;
    }
    key
}
