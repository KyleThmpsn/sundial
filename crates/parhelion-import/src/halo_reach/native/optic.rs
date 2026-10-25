use super::material::Lens;
use super::*;
mod bore;
mod scope;

pub(super) struct Source<'a> {
    pub cache: &'a cache::Cache,
    pub pages: &'a mut resource::Pages,
    pub weapon: &'a cache::Tag,
}

pub(super) struct Prepared {
    pub lenses: BTreeMap<u32, Lens>,
    bounds: Option<[[f32; 3]; 2]>,
    rear: [f32; 3],
    front: [f32; 3],
    bore: Option<Value>,
    separate: bool,
    automatic: bool,
}

/// Distance behind the geometry for an automatically placed eye.
const RELIEF: f32 = 0.08;
/// Height of the sightline above the highest vertex.
const CLEARANCE: f32 = 0.005;

/// Aim from behind the geometry along the sight axis, clear of its top.
///
/// With a rear point beyond the muzzle, live aiming left the weapon between the
/// camera and the target. A separate view therefore looks over the geometry, and
/// the axis keeps the lateral position of `through`.
fn separate_eye(
    mesh: &Mesh,
    through: [f32; 3],
    direction: [f32; 3],
    relief: f32,
) -> Result<[f32; 3]> {
    // Source models are Z up. Remove the part of it along a tilted sight axis.
    let vertical = [0., 0., 1.];
    let up = crate::halo_reach::rig::normalize(std::array::from_fn(|a| {
        vertical[a] - direction[a] * direction[2]
    }))?;
    let dot = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f32>();
    let (nearest, highest) = mesh.positions.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(nearest, highest), p| {
            let offset = std::array::from_fn(|a| p[a] - through[a]);
            (
                nearest.min(dot(offset, direction)),
                highest.max(dot(offset, up)),
            )
        },
    );
    ensure!(
        nearest.is_finite() && highest.is_finite(),
        "Separate scope has no source bounds"
    );
    Ok(std::array::from_fn(|a| {
        through[a] + direction[a] * (nearest - relief) + up[a] * (highest + CLEARANCE)
    }))
}

pub(super) fn select(
    entry: &ImportRequest,
    scene: &Scene,
    mesh: &mut Mesh,
    materials: &BTreeMap<u32, crate::halo_reach::material::Material>,
) -> Result<Option<Prepared>> {
    let scoped = scene.object.tag.group == "weap"
        && scene.object.gameplay["zoom_levels"]
            .as_u64()
            .is_some_and(|levels| levels > 0);
    ensure!(
        entry.optic.is_none() || entry.sights.is_none(),
        "Choose a lens optic or iron sights, not both"
    );
    ensure!(
        entry.optic_item.is_none() || entry.optic.is_some() || entry.sights.is_some() || scoped,
        "An explicit sight carrier requires configured sights"
    );
    match (&entry.optic, &entry.sights) {
        (Some(config), _) => prepare(config, mesh, materials).map(Some),
        (_, Some(config)) => sights(config, mesh).map(Some),
        _ if scoped => {
            // Reach hides a zoomed weapon behind its overlay. The overlay stays,
            // and the eye clears the geometry like an unscoped firearm's.
            let mut prepared = unscoped(mesh)?;
            prepared.separate = true;
            Ok(Some(prepared))
        }
        _ if scene.object.tag.group == "weap"
            && !matches!(
                scene.object.gameplay["weapon_class"].as_str(),
                Some("sword" | "hammer")
            ) =>
        {
            unscoped(mesh).map(Some)
        }
        _ => Ok(None),
    }
}

fn unscoped(mesh: &Mesh) -> Result<Prepared> {
    ensure!(
        !mesh.positions.is_empty(),
        "Unscoped aiming needs source geometry"
    );
    let low = std::array::from_fn::<_, 3, _>(|axis| {
        mesh.positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::INFINITY, f32::min)
    });
    let high = std::array::from_fn::<_, 3, _>(|axis| {
        mesh.positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    ensure!(
        low.iter().chain(&high).all(|v| v.is_finite()) && high[0] > low[0],
        "Invalid unscoped aiming bounds"
    );
    // Reach weapons without zoom do not supply an ADS camera. Keep a clear,
    // horizontal view over their geometry instead of retaining the donor's aim
    // markers. Explicit sights can place the eye against a particular rear post.
    let rear = [
        low[0] - RELIEF,
        (low[1] + high[1]) * 0.5,
        high[2] + CLEARANCE,
    ];
    Ok(Prepared {
        lenses: BTreeMap::new(),
        bounds: None,
        rear,
        front: [high[0], rear[1], rear[2]],
        bore: None,
        separate: false,
        automatic: true,
    })
}

pub(super) fn prepare(
    config: &Optic,
    mesh: &mut Mesh,
    materials: &BTreeMap<u32, crate::halo_reach::material::Material>,
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
    let mut faces = mesh
        .groups
        .iter()
        .filter(|(key, _)| *key == aperture)
        .flat_map(|(_, faces)| faces)
        .map(|face| face.map(|index| mesh.positions[index as usize]))
        .collect::<Vec<_>>();
    ensure!(
        faces.len() >= 2,
        "Optic aperture has insufficient drawn geometry"
    );
    let x = |face: &[[f32; 3]; 3]| face.iter().map(|p| p[0]).sum::<f32>() / 3.;
    faces.sort_by(|a, b| x(a).total_cmp(&x(b)));
    let split = (1..faces.len())
        .max_by(|&a, &b| {
            (x(&faces[a]) - x(&faces[a - 1])).total_cmp(&(x(&faces[b]) - x(&faces[b - 1])))
        })
        .context("Optic lens separation")?;
    let bounds = |faces: &[[[f32; 3]; 3]]| {
        let low = std::array::from_fn::<_, 3, _>(|a| {
            faces
                .iter()
                .flatten()
                .map(|p| p[a])
                .fold(f32::INFINITY, f32::min)
        });
        let high = std::array::from_fn::<_, 3, _>(|a| {
            faces
                .iter()
                .flatten()
                .map(|p| p[a])
                .fold(f32::NEG_INFINITY, f32::max)
        });
        [low, high]
    };
    let rear_bounds = bounds(&faces[..split]);
    let front_bounds = bounds(&faces[split..]);
    let gap = front_bounds[0][0] - rear_bounds[1][0];
    ensure!(
        gap > 2.
            * (rear_bounds[1][0] - rear_bounds[0][0]).max(front_bounds[1][0] - front_bounds[0][0]),
        "Optic aperture must have distinct rear and front lens surfaces"
    );
    let center = |b: [[f32; 3]; 2]| std::array::from_fn::<_, 3, _>(|a| (b[0][a] + b[1][a]) * 0.5);
    let radius = |b: [[f32; 3]; 2]| (b[1][1] - b[0][1]).min(b[1][2] - b[0][2]) * 0.5;
    let rear = center(rear_bounds);
    let front = center(front_bounds);
    let radii = [radius(rear_bounds), radius(front_bounds)];
    ensure!(
        radii.iter().all(|r| r.is_finite() && *r > 0.),
        "Degenerate sight aperture"
    );
    let direction = crate::halo_reach::rig::normalize(std::array::from_fn(|a| front[a] - rear[a]))?;
    let mut lenses = BTreeMap::new();
    for path in &config.lenses {
        let key = resolve(path)?;
        ensure!(
            lenses
                .insert(
                    key,
                    Lens {
                        center: rear,
                        rear: rear_bounds[0][0],
                        radius: radii[0],
                        reticle: key == aperture
                    }
                )
                .is_none(),
            "Repeated optic lens material"
        );
    }
    if config.view == OpticView::Separate {
        let eye = separate_eye(mesh, rear, direction, config.eye_relief)?;
        return Ok(Prepared {
            lenses: BTreeMap::new(),
            bounds: Some(bounds(&faces)),
            rear: eye,
            front: std::array::from_fn(|a| eye[a] + direction[a] * 0.2),
            bore: None,
            separate: true,
            automatic: false,
        });
    }
    let opened = bore::open(
        mesh,
        &lenses,
        rear,
        front,
        radii,
        [rear_bounds[0][0], front_bounds[1][0]],
    )?;
    Ok(Prepared {
        lenses,
        bounds: Some(bounds(&faces)),
        rear: std::array::from_fn(|a| rear[a] - direction[a] * config.eye_relief),
        front,
        bore: Some(
            json!({"rear_lens_bounds":rear_bounds,"front_lens_bounds":front_bounds,"rear_center":rear,"front_center":front,"radii":radii,"clearance":opened}),
        ),
        separate: false,
        automatic: false,
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
    if config.view == OpticView::Separate {
        let eye = separate_eye(mesh, config.rear, direction, config.eye_relief)?;
        return Ok(Prepared {
            lenses: BTreeMap::new(),
            bounds: None,
            rear: eye,
            front: std::array::from_fn(|a| eye[a] + direction[a] * 0.2),
            bore: None,
            separate: true,
            automatic: false,
        });
    }
    Ok(Prepared {
        lenses: BTreeMap::new(),
        bounds: None,
        rear: std::array::from_fn(|axis| config.rear[axis] - direction[axis] * config.eye_relief),
        front: config.front,
        bore: None,
        separate: false,
        automatic: false,
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

type Components = BTreeMap<u32, (String, Payload)>;

fn components(native: &mut Reader, entity: &Payload) -> Result<Components> {
    let mut components = BTreeMap::new();
    for (index, row) in entity
        .array(16, 12, Some(0x80809c04))?
        .into_iter()
        .enumerate()
    {
        let tag = entity.u32(row)?;
        let payload = native.tag(tag, Some(0x80809c36))?;
        // Channel components are immutable shared resources. Keep their native
        // references until an added model input requires a private copy below.
        if payload.u32(payload.pointer(24)? - 4)? == 0x80809790 {
            continue;
        }
        ensure!(
            components
                .insert(tag, (format!("optic-owner-{index}"), (*payload).clone()))
                .is_none(),
            "Repeated optic component"
        );
    }
    Ok(components)
}

/// How the report names the aiming method.
fn alignment(optic: &Prepared) -> &'static str {
    if optic.separate {
        "separate_view"
    } else if optic.bounds.is_some() {
        "lens_aperture"
    } else if optic.automatic {
        "source_bounds"
    } else {
        "iron_sights"
    }
}

/// What the report says the aiming method leaves unverified.
fn limits(optic: &Prepared) -> &'static str {
    if optic.separate {
        "The aiming camera looks over the weapon from behind along the sight axis, preserving the physical lens. Scoped weapons use their source HUD bitmap layers and neutral reticle artwork. Dynamic meter and reticle inputs are reported separately. Source magnification is recorded in source_gameplay.aim. Aim transitions and the final view require gameplay acceptance."
    } else if optic.automatic {
        "The aiming camera clears the selected source bounds along the forward axis. This is an adaptation for a source firearm without zoom, not recovered iron-sight alignment. Live aim transitions and eye relief require gameplay acceptance."
    } else {
        "Object-space sight alignment. Lens optics use a simple illuminated aiming mark. Source magnification is recorded separately for scoped weapons and first-person motion."
    }
}

/// The imported weapon has one fixed sight, even when the native item exposes
/// several plug-selected alternatives. A carrier's position only belongs to
/// that carrier's own list and cannot select the host's active sight position.
fn positions(art: &Value, placement: &Value) -> Result<Vec<usize>> {
    let selector = placement["selector"].as_u64().context("Sight selector")?;
    let mut width = usize::try_from(placement["position"].as_u64().context("Sight position")?)?
        .checked_add(1)
        .context("Sight position overflow")?;
    for row in art["art_rows"].as_array().context("Native art rows")? {
        for slot in row["slots"].as_array().context("Native art slots")? {
            if slot["selector"].as_u64() == Some(selector) {
                width = width.max(
                    slot["assignments"]
                        .as_array()
                        .context("Native sight alternatives")?
                        .len(),
                );
            }
        }
    }
    Ok((0..width).collect())
}

pub(super) fn build(
    native: &mut Reader,
    g: &mut Graph,
    art: &Value,
    optic: &Prepared,
    item: u32,
    source: Option<u32>,
    reach: Source<'_>,
) -> Result<(Value, Value)> {
    let overlay = if optic.separate {
        crate::halo_reach::hud::scope::read(reach.cache, reach.weapon)?
    } else {
        None
    };
    // Automatic sights use the qualified native carrier independently of firing behavior.
    let selected = source
        .or(optic.automatic.then_some(0x4F5CCF1D))
        .map(|item| crate::tiger::shadowkeep::extract(native, item))
        .transpose()?;
    let carrier = selected.as_ref().unwrap_or(art);
    let (parent, entity_tag, original_entity) = template(native, carrier)?;
    let positions = positions(art, &parent["placement"])?;
    let components = components(native, &original_entity)?;
    let mut entity = original_entity.clone();
    let mut scope_report = None;
    let mut scope_target = None;
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
            if overlay.is_some() {
                ensure!(
                    scope_target.is_none(),
                    "Separate scope has multiple render components"
                );
                scope_target = Some((model_symbol.clone(), symbol.clone()));
            } else {
                g.add(&model_symbol, model_tag, &no_draws(&model)?.0, None, vec![])?;
            }
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
    if let (Some(overlay), Some((model, owner))) = (&overlay, scope_target) {
        let name = u32::from_str_radix(&scope::visibility(native)?, 16)?;
        let inputs = crate::presentation::channel::inherit_named(
            native,
            g,
            &[name],
            "optic-entity",
            &owner,
            "optic",
        )?;
        scope_report = Some(scope::build(
            native,
            g,
            &model,
            &inputs,
            scope::Source {
                cache: reach.cache,
                pages: reach.pages,
                overlay,
            },
        )?);
    }
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
    let kept = json!({"assignment":assignment,"key":name_hash(&format!("reach-{item:08X}-optic")),"parent":"optic-parent",
        "native_region":true,"selector":parent["placement"]["selector"],"position":0,"positions":positions});
    ensure!(
        overlay.is_none() || scope_report.is_some(),
        "Separate scope has no render component"
    );
    let report = json!({"source_item":carrier["item"],"source_entity":format!("{entity_tag:08X}"),"placement":parent["placement"],"positions":positions,"aperture_bounds":optic.bounds,"rear":optic.rear,"front":optic.front,"bore":optic.bore,"scope":scope_report,
        "alignment":alignment(optic),
        "lens_materials":optic.lenses.keys().map(|key|format!("{key:08X}")).collect::<Vec<_>>(),"automatic":optic.automatic,"gameplay_verified":false,
        "limits":limits(optic)});
    Ok((kept, report))
}
