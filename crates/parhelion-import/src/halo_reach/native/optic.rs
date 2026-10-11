use super::*;
use crate::halo_reach::native_material::Lens;

pub(super) struct Prepared {
    pub lenses: BTreeMap<u32, Lens>,
    bounds: Option<[[f32; 3]; 2]>,
    rear: [f32; 3],
    front: [f32; 3],
}

pub(super) fn select(
    entry: &ImportRequest,
    mesh: &Mesh,
    materials: &BTreeMap<u32, material::Material>,
) -> Result<Option<Prepared>> {
    ensure!(
        entry.optic.is_none() || entry.sights.is_none(),
        "Choose a lens optic or iron sights, not both"
    );
    match (&entry.optic, &entry.sights) {
        (Some(config), _) => prepare(config, mesh, materials).map(Some),
        (_, Some(config)) => sights(config, mesh).map(Some),
        _ => Ok(None),
    }
}

pub(super) fn prepare(
    config: &Optic,
    mesh: &Mesh,
    materials: &BTreeMap<u32, material::Material>,
) -> Result<Prepared> {
    ensure!(
        config.eye_relief.is_finite() && (0.01..=0.5).contains(&config.eye_relief),
        "Invalid optic eye relief"
    );
    ensure!(
        !config.lenses.is_empty() && config.lenses.contains(&config.aperture),
        "Aperture must be a selected lens material"
    );
    let resolve = |path: &str| {
        materials
            .iter()
            .find_map(|(&key, m)| m.tag.as_ref().filter(|t| t.path == path).map(|_| key))
            .with_context(|| format!("Optic source material {path} is absent"))
    };
    let aperture = resolve(&config.aperture)?;
    let vertices = mesh
        .groups
        .iter()
        .filter(|(key, _)| *key == aperture)
        .flat_map(|(_, faces)| faces.iter().flatten())
        .map(|index| mesh.positions[*index as usize])
        .collect::<Vec<_>>();
    ensure!(!vertices.is_empty(), "Optic aperture has no drawn geometry");
    let low = std::array::from_fn(|a| vertices.iter().map(|p| p[a]).fold(f32::INFINITY, f32::min));
    let high = std::array::from_fn(|a| {
        vertices
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let center = std::array::from_fn::<_, 3, _>(|a| (low[a] + high[a]) * 0.5);
    ensure!(
        high[0] > low[0] && high[1] > low[1] && high[2] > low[2],
        "Degenerate sight aperture"
    );
    let radius = ((high[1] - low[1]).min(high[2] - low[2])) * 0.5;
    let mut lenses = BTreeMap::new();
    for path in &config.lenses {
        let key = resolve(path)?;
        ensure!(
            lenses
                .insert(
                    key,
                    Lens {
                        center,
                        rear: low[0],
                        radius,
                        reticle: key == aperture
                    }
                )
                .is_none(),
            "Repeated optic lens material"
        );
    }
    Ok(Prepared {
        lenses,
        bounds: Some([low, high]),
        rear: [low[0] - config.eye_relief, center[1], center[2]],
        front: [high[0], center[1], center[2]],
    })
}

pub(super) fn sights(config: &Sights, mesh: &Mesh) -> Result<Prepared> {
    ensure!(
        config.eye_relief.is_finite() && (0.01..=0.5).contains(&config.eye_relief),
        "Invalid iron sight eye relief"
    );
    ensure!(
        config
            .rear
            .iter()
            .chain(&config.front)
            .all(|v| v.is_finite())
            && config.front[0] > config.rear[0],
        "Invalid iron sight axis"
    );
    ensure!(
        !mesh.positions.is_empty(),
        "Iron sights need source geometry"
    );
    for axis in 0..3 {
        let low = mesh
            .positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::INFINITY, f32::min)
            - 0.02;
        let high = mesh
            .positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max)
            + 0.02;
        ensure!(
            [config.rear[axis], config.front[axis]]
                .iter()
                .all(|v| (low..=high).contains(v)),
            "Iron sight point is outside source geometry bounds"
        );
    }
    let direction = crate::halo_reach::rig::normalize(std::array::from_fn(|axis| {
        config.front[axis] - config.rear[axis]
    }))?;
    Ok(Prepared {
        lenses: BTreeMap::new(),
        bounds: None,
        rear: std::array::from_fn(|axis| config.rear[axis] - direction[axis] * config.eye_relief),
        front: config.front,
    })
}

fn template(native: &mut Reader, art: &Value) -> Result<(Value, u32, Payload)> {
    for parent in art["parents"].as_array().context("Optic art parents")? {
        if parent["placement"]["selector"].as_u64().is_none() {
            continue;
        }
        let bytes = hex::decode(
            parent["parent_bytes"]
                .as_str()
                .context("Native parent bytes")?,
        )?;
        let tag = u32::from_le_bytes(
            bytes
                .get(16..20)
                .context("Native parent entity")?
                .try_into()?,
        );
        let entity = native.tag(tag, Some(0x80809c0f))?;
        let mut count = 0;
        for row in entity.array(16, 12, Some(0x80809c04))? {
            let owner = native.tag(entity.u32(row)?, Some(0x80809c36))?;
            count += usize::from(owner.u32(owner.pointer(24)? - 4)? == 0x8080393b);
        }
        ensure!(count <= 1, "Ambiguous native optic part");
        if count == 1 {
            return Ok((parent.clone(), tag, (*entity).clone()));
        }
    }
    anyhow::bail!("The selected donor has no native optic part")
}

fn no_draws(model: &Payload) -> Result<Payload> {
    let mut out = model.clone();
    for mesh in model.array(16, 136, Some(0x80807378))? {
        out.0[mesh + 24..mesh + 40].fill(0);
        out.0[mesh + 40..mesh + 88].fill(0);
        out.0[mesh + 88..mesh + 134].fill(0xff);
    }
    Ok(out)
}

pub(super) fn build(
    native: &mut Reader,
    g: &mut Graph,
    art: &Value,
    optic: &Prepared,
    item: u32,
) -> Result<(Value, Value)> {
    let (parent, entity_tag, original_entity) = template(native, art)?;
    let mut components = BTreeMap::new();
    for (index, row) in original_entity
        .array(16, 12, Some(0x80809c04))?
        .into_iter()
        .enumerate()
    {
        let tag = original_entity.u32(row)?;
        let payload = native.tag(tag, Some(0x80809c36))?;
        ensure!(
            components
                .insert(tag, (format!("optic-owner-{index}"), (*payload).clone()))
                .is_none(),
            "Repeated optic component"
        );
    }
    let mut entity = original_entity.clone();
    let mut entity_patches = Vec::new();
    for (&tag, (symbol, original)) in &components {
        let schema = original.pointer(24)?;
        let class = original.u32(schema - 4)?;
        let mut payload = if class == 0x8080393b {
            crate::tiger::markers::optics::aligned(original, optic.rear, optic.front)?
        } else {
            original.clone()
        };
        let mut patches = Vec::new();
        if matches!(class, 0x808072bd | 0x80807286) {
            let model_tag = original.u32(schema + 0x1dc)?;
            let model = native.tag(model_tag, Some(0x808073a5))?;
            let model_symbol = format!("{symbol}-model");
            g.add(&model_symbol, model_tag, &no_draws(&model)?.0, None, vec![])?;
            put(&mut payload.0, schema + 0x1dc, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":schema+0x1dc,"symbol":model_symbol}));
        }
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            if let Some((target, owner)) = components.get(&payload.u32(at)?) {
                ensure!(
                    payload.u32(at + 4)? & 0xffff0000 == 0x80800000
                        && payload.u64(at + 8)? < owner.0.len() as u64,
                    "Optic component has an untyped owner reference"
                );
                put(&mut payload.0, at, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":at,"symbol":target}));
            }
        }
        g.add(symbol, tag, &payload.0, None, patches)?;
        for at in crate::tiger::entity::owner_slots(&original_entity, original, tag)? {
            put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
            entity_patches.push(json!({"offset":at,"symbol":symbol}));
        }
    }
    g.add("optic-entity", entity_tag, &entity.0, None, entity_patches)?;
    let parent_tag = hash(&parent, "parent")?;
    let mut bytes = native.tag(parent_tag, None)?.0.clone();
    put(&mut bytes, 16, &u32::MAX.to_le_bytes())?;
    g.add(
        "optic-parent",
        parent_tag,
        &bytes,
        None,
        vec![json!({"offset":16,"symbol":"optic-entity"})],
    )?;
    let companion = native.tag(0x81a662de, None)?;
    g.add(
        "optic-parent-companion",
        0x81a662de,
        &companion.0,
        None,
        vec![],
    )?;
    let node = g.nodes.last_mut().context("Optic companion")?;
    node["shared_owner"] = json!("optic-parent");
    node["source_parent"] = json!(parent_tag);
    let assignment = hash(&parent, "assignment")?;
    let kept = json!({"assignment":assignment,"key":name_hash(&format!("reach-{item:08X}-optic")),"parent":"optic-parent"});
    let report = json!({"source_entity":format!("{entity_tag:08X}"),"placement":parent["placement"],"aperture_bounds":optic.bounds,"rear":optic.rear,"front":optic.front,
        "alignment":if optic.bounds.is_some() { "lens_aperture" } else { "iron_sights" },
        "lens_materials":optic.lenses.keys().map(|key|format!("{key:08X}")).collect::<Vec<_>>(),"gameplay_verified":false,
        "limits":"Object-space sight alignment. Lens optics use a simple illuminated aiming mark. Source scope overlays and magnification are not reproduced."});
    Ok((kept, report))
}
