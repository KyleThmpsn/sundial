//! A source radial detonation through a private copy of the native carrier's own detonation.
//!
//! A native projectile movement names the graph it creates on detonation at definition `+0x180`
//! and passes it creation channels from definition `+0x198`. The carrier's detonation queries
//! are unit spheres scaled by the `detonation_shape_scale` channel, and their damage actions
//! scale the profile amount by an expression over the other channels. The copy keeps the native
//! queries, recipient filters and presentation. It writes the source outer radius into that
//! channel, a constant one into each damage scale, the source falloff into each damage query
//! and a private profile with the source amount and recipient push.
//!
//! A movement that names no detonation graph, as the effect-only carrier's does, takes a private
//! copy of the impact grenade's when the source detonates on contact. Its stopping contacts
//! (selector 3) become detonations (selector 4) whose parameter names the movement's own graph,
//! as the grenade's finishing contacts do. Such a movement passes no creation inputs, so the
//! copy's parameter component (`80808BE6`) holds the source outer radius as its own
//! `detonation_shape_scale` default. A parameter its parent does not pass keeps its default, as
//! the stock graph's `1DC6A44C` keeps 1.25 under the grenade, which never passes it. The other
//! parameters keep their stock defaults of zero, those of a grenade without perks.
use super::{Assets, Payload, Reader, put};
use crate::{halo_reach::blast, tiger::projectile::retag};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// FNV-1 of `detonation_shape_scale`, the creation channel the detonation scales by.
const SHAPE_SCALE: u32 = 0x1289_d0bb;
const MOVEMENT: u32 = 0x8080_388f;
const FLOW: u32 = 0x8080_84e9;
const QUERY: u32 = 0x8080_4ab9;
const PROFILE_ACTION: u32 = 0x8080_4ac9;
const PROFILE: u32 = 0x8080_4b02;
const PUSH: u32 = 0x8080_4b57;
/// The impact grenade's movement, whose detonation graph `80B7CAA6` a movement without one takes.
const IMPACT_MOVEMENT: u32 = 0x8152_8280;
/// Why a movement that names no detonation graph takes none.
const NO_GRAPH: &str = "The native carrier creates no detonation graph";
/// Contact response actions (`808037F7`): selector 3 stops the projectile with an `808037F6`
/// parameter, and selector 4 detonates it through an `808037F4` parameter naming a graph at `+0x10`,
/// `FFFFFFFF` for the movement's own.
const RESPONSE_ACTION: u32 = 0x8080_37f7;
const STOP: u32 = 3;
const STOP_PARAMETER: u32 = 0x8080_37f6;
const DETONATE: u32 = 4;
const DETONATE_PARAMETER: u32 = 0x8080_37f4;
const DETONATE_SIZE: usize = 40;
/// The detonation's parameter component: definition rows (`80808BEC`, `0x40`) at definition
/// `+0xA0` name a parameter at `+0x28` and hold its default at `+0x30`. Each names its instance
/// twin (`80808BEB`, `0x30`, at instance `+0x40`) at `+0x08`, whose `+0x20` holds the value.
const PARAMETERS: u32 = 0x8080_8be6;
const PARAMETER_ROW: u32 = 0x8080_8bec;
const PARAMETER_TWIN: u32 = 0x8080_8beb;
/// Reach damage units per firing-weapon hit, for weapon-relative native profiles. A Needler
/// needle (6) then hits for 0.4 of a donor hit, the share chosen in gameplay after a tenth read
/// as too weak, and the source's own ratio carries its supercombine (350) to about 23 donor hits.
/// This is a gameplay calibration, not a recovered native conversion.
pub(super) const REACH_UNITS_PER_WEAPON_HIT: f32 = 15.;
/// Every input row passes its first channel through unchanged.
const PASS_THROUGH: [u8; 4] = [0x3c, 1, 0x3e, 0];
/// Pushes constant zero and stores it.
const CONSTANT: [u8; 4] = [0x34, 0, 0x3e, 0];

/// A source projectile damage profile, qualified as one deterministic radial profile.
pub(super) struct Radial<'a> {
    pub source: &'a Value,
    pub radius: [f32; 2],
    pub amount: f32,
    /// The source damage lower bound, which no translation carries yet.
    pub lower: f32,
    pub acceleration: f32,
}

fn number(value: &Value) -> Result<f32> {
    let number = value.as_f64().context("Detonation source scalar")? as f32;
    ensure!(number.is_finite(), "Nonfinite detonation source scalar");
    Ok(number)
}

/// The source damage profile `role` of `projectile`, or the reason it has none a radial
/// translation can carry.
pub(super) fn radial<'a>(
    behavior: &'a Value,
    projectile: &Value,
    role: &str,
) -> Result<std::result::Result<Radial<'a>, &'static str>> {
    let edges = behavior["edges"]
        .as_array()
        .context("Detonation source edges")?;
    let matching = edges
        .iter()
        .filter(|e| {
            e["owner"] == projectile["tag"]["datum"]
                && e["role"] == role
                && e["tag"]["group"] == "jpt!"
        })
        .collect::<Vec<_>>();
    let edge = match matching.as_slice() {
        [] => return Ok(Err("The source projectile has no such damage profile")),
        [edge] => *edge,
        _ => anyhow::bail!("Source projectile has several {role} profiles"),
    };
    let id = edge["tag"]["datum"]
        .as_u64()
        .context("Detonation source identity")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    ensure!(
        node["tag"] == edge["tag"],
        "Detonation source reference is unresolved"
    );
    let d = &node["data"];
    let radius = [number(&d["radius"][0])?, number(&d["radius"][1])?];
    let amount = number(&d["upper_bound"][0])?;
    let acceleration = number(&d["instantaneous_acceleration"])?;
    let lower = number(&d["lower_bound"])?;
    if !(radius[0] >= 0.
        && radius[1] > radius[0]
        && amount > 0.
        && acceleration >= 0.
        && (0. ..=amount).contains(&lower)
        && number(&d["upper_bound"][1])? == amount
        && d["cone_radians"] == json!([0., 0.]))
    {
        return Ok(Err(
            "The source damage is not one deterministic spherical profile",
        ));
    }
    Ok(Ok(Radial {
        source: node,
        radius,
        amount,
        lower,
        acceleration,
    }))
}

/// The amount of the source single-target damage profile `role`, such as `impact_damage`, or the
/// reason it has none a fixed amount can carry.
pub(super) fn contact(
    behavior: &Value,
    projectile: &Value,
    role: &str,
) -> Result<std::result::Result<(f32, Value), &'static str>> {
    Ok(match contact_bounds(behavior, projectile, role)? {
        Ok((upper, lower, _)) if lower != upper => {
            Err("The source damage is not one fixed single-target amount")
        }
        Ok((upper, _, source)) => Ok((upper, source)),
        Err(reason) => Err(reason),
    })
}

/// The upper and lower amounts of the source single-target damage profile `role`. Reach scales a
/// projectile's damage from the upper bound down to the lower over the projectile's damage range.
pub(super) fn contact_bounds(
    behavior: &Value,
    projectile: &Value,
    role: &str,
) -> Result<std::result::Result<(f32, f32, Value), &'static str>> {
    let edges = behavior["edges"]
        .as_array()
        .context("Contact damage source edges")?;
    let matching = edges
        .iter()
        .filter(|e| {
            e["owner"] == projectile["tag"]["datum"]
                && e["role"] == role
                && e["tag"]["group"] == "jpt!"
        })
        .collect::<Vec<_>>();
    let edge = match matching.as_slice() {
        [] => return Ok(Err("The source projectile has no such damage profile")),
        [edge] => *edge,
        _ => anyhow::bail!("Source projectile has several {role} profiles"),
    };
    let id = edge["tag"]["datum"]
        .as_u64()
        .context("Contact damage source identity")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    ensure!(
        node["tag"] == edge["tag"],
        "Contact damage source reference is unresolved"
    );
    let d = &node["data"];
    let amount = number(&d["upper_bound"][0])?;
    let lower = number(&d["lower_bound"])?;
    if !(amount > 0.
        && number(&d["upper_bound"][1])? == amount
        && (0. ..=amount).contains(&lower)
        && d["radius"] == json!([0., 0.])
        && d["cone_radians"] == json!([0., 0.]))
    {
        return Ok(Err("The source damage is not one single-target profile"));
    }
    Ok(Ok((amount, lower, node["tag"].clone())))
}

/// Reach damage side effect 3, EMP, which stalls vehicles and drains shields.
const EMP: i64 = 3;

/// A source damage profile's side effect at `+0x10`, from an older receipt's raw bytes when it
/// has no decoded field.
fn side_effect(node: &Value) -> Result<Option<i64>> {
    if let Some(value) = node["data"]["side_effect"].as_i64() {
        return Ok(Some(value));
    }
    let Some(raw) = node["data"]["raw"].as_str() else {
        return Ok(None);
    };
    let raw = hex::decode(raw)?;
    Ok(raw
        .get(0x10..0x12)
        .map(|b| i64::from(i16::from_le_bytes([b[0], b[1]]))))
}

/// Every damage profile of the weapon's source projectiles whose side effect is EMP, and why none
/// is carried. The profiles' amounts still reach a translated detonation where its report records
/// one.
pub(super) fn emp(behavior: &Value, projectiles: &Value) -> Result<Value> {
    let edges = behavior["edges"].as_array().context("Behavior edges")?;
    let mut sources = Vec::new();
    for projectile in projectiles.as_array().context("Projectile presentations")? {
        for edge in edges
            .iter()
            .filter(|e| e["owner"] == projectile["tag"]["datum"] && e["tag"]["group"] == "jpt!")
        {
            let id = edge["tag"]["datum"]
                .as_u64()
                .context("Damage source identity")?;
            let node = &behavior["nodes"][format!("{id:08X}")];
            if node["tag"] != edge["tag"] || side_effect(node)? != Some(EMP) {
                continue;
            }
            sources.push(json!({"projectile":projectile["tag"],"role":edge["role"],
                "source":node["tag"],"radius":node["data"]["radius"],
                "upper_bound":node["data"]["upper_bound"]}));
        }
    }
    if sources.is_empty() {
        return Ok(json!({"status":"not_applicable",
            "reason":"No damage profile of the weapon's source projectiles has the EMP side effect"}));
    }
    Ok(json!({"status":"omitted","sources":sources,
        "reason":"No native counterpart is established. The client registry names no class binding or member under the FNV-1 hash of EMP, stun, disable, suppression, shield drain or vehicle ejection names, and none of the fifteen response classes a native damage profile's response list can hold has a recovered name or a stock use established as a vehicle or shield effect.",
        "limits":["A profile's amount reaches the translated detonation where the detonation report records it. Its vehicle stall and shield drain do not."],
        "gameplay_verified":false}))
}

fn class(p: &Payload) -> Result<u32> {
    p.u32(p.pointer(24)? - 4)
}

/// Makes the movement's `detonation_shape_scale` creation input a constant `radius`.
fn shape_scale(p: &mut Payload, definition: usize, radius: f32) -> Result<()> {
    let rows = p
        .array(definition + 0x198, 0x78, Some(0x8080_851c))?
        .into_iter()
        .filter(|&row| p.u32(row + 0x70).ok() == Some(SHAPE_SCALE))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        anyhow::bail!("Native detonation has no single shape-scale creation input")
    };
    let expression = row + 0x20;
    let code = p.array(expression, 1, Some(0x8080_0009))?;
    ensure!(
        code.len() == PASS_THROUGH.len()
            && code.iter().zip(PASS_THROUGH).all(|(&at, b)| p.0[at] == b),
        "Native detonation shape scale has another program"
    );
    // The bound channel providers stay, so the input count does too.
    let inputs = p.u64(expression + 32)?;
    super::program::write(p, expression, &CONSTANT, &[radius], inputs)
}

/// A private copy of the native damage profile with the source amount and recipient push. The
/// profile's other responses stay. A profile without a recipient push carries none.
pub(super) fn profile(
    native: &mut Reader,
    assets: &mut Assets,
    symbol: &str,
    template: u32,
    amount: f32,
    push: f32,
) -> Result<(u32, Value)> {
    ensure!(
        push.is_finite() && push >= 0.,
        "Invalid translated detonation push"
    );
    let id = assets.reserve(native, symbol.into())?;
    let original = native.tag(template, Some(0x8080_9c36))?;
    let mut p = retag(&original, id)?;
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == PROFILE,
        "Native detonation damage has another type"
    );
    let native_amount = [p.f32(d + 0x14)?, p.f32(d + 0x18)?, p.f32(d + 0x1c)?];
    let native_mode = p.u8(d + 0x10)?;
    // Mode one would multiply the bounds by the firing weapon's base damage, and flag `+53`
    // adds a clock scale. Projectile profiles still scale the firing weapon's own hit: stock
    // bounds 0 and 1 span none to one full hit, so a Reach amount written as is dealt that many
    // hits, and in game a Needler needle written as 6 hit for about six auto rifle hits. A
    // weapon-relative template takes the source amount at `REACH_UNITS_PER_WEAPON_HIT`, and any
    // other template keeps the amount absolute.
    let relative = native_amount[0] == 0. && native_amount[1] == 1.;
    let written = if relative {
        amount / REACH_UNITS_PER_WEAPON_HIT
    } else {
        amount
    };
    put(&mut p.0, d + 0x10, &[0])?;
    put(&mut p.0, d + 0x53, &[0])?;
    for (field, value) in [(0x14, written), (0x18, written), (0x1c, 0.)] {
        put(&mut p.0, d + field, &value.to_le_bytes())?;
    }
    let pushes = p
        .array(d + 0x78, 8, None)?
        .into_iter()
        .map(|r| p.pointer(r))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&r| p.u32(r.saturating_sub(4)).ok() == Some(PUSH))
        .collect::<Vec<_>>();
    let native_push = match pushes.as_slice() {
        [] => None,
        [response] => {
            let bounds = [p.f32(response + 4)?, p.f32(response + 8)?];
            for field in [4, 8] {
                put(&mut p.0, response + field, &push.to_le_bytes())?;
            }
            Some(bounds)
        }
        _ => anyhow::bail!("Native detonation recipient push is ambiguous"),
    };
    assets.push(id, template, p, None)?;
    Ok((
        id,
        json!({"template":template,"native_mode":native_mode,"native_amount":native_amount,"native_push":native_push,
            "weapon_relative":relative,"written_amount":written,
            "push_carried":native_push.is_some() || push == 0.}),
    ))
}

/// Points the movement in `owners` at a private copy of its detonation graph carrying the
/// source projectile's detonation damage, and reports what was translated.
pub(super) fn apply(
    native: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    behavior: &Value,
    projectile: &Value,
    metres: f32,
) -> Result<Value> {
    let radial = match radial(behavior, projectile, "detonation_damage")? {
        Ok(radial) if radial.lower == 0. => radial,
        Ok(_) => {
            return Ok(
                json!({"status":"native","reason":"The source detonation has a minimum damage"}),
            );
        }
        Err(reason) => return Ok(json!({"status":"native","reason":reason})),
    };
    ensure!(
        metres.is_finite() && metres > 0.,
        "Invalid detonation length scale"
    );
    // A native detonation laid out another way keeps its native damage. The source profile
    // has already qualified, so only the native structure can refuse here.
    let contact = super::effect::contact_response(projectile)? == Some(0);
    let snapshot = (assets.clone(), owners.clone());
    match translate(native, assets, owners, &radial, metres, contact) {
        Ok(report) => Ok(report),
        Err(error) => {
            (*assets, *owners) = snapshot;
            Ok(json!({"status":"native","source":radial.source["tag"],
                "reason":format!("The native detonation has another structure: {error:#}")}))
        }
    }
}

fn translate(
    native: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    radial: &Radial<'_>,
    metres: f32,
    contact: bool,
) -> Result<Value> {
    let movements = owners
        .iter()
        .filter(|(_, p)| class(p).ok() == Some(MOVEMENT))
        .map(|(&tag, _)| tag)
        .collect::<Vec<_>>();
    let [movement] = movements.as_slice() else {
        anyhow::bail!("Detonation translation requires one projectile movement")
    };
    let movement = *movement;
    let definition = owners[&movement].pointer(24)?;
    let stock = owners[&movement].u32(definition + 0x180)?;
    let graft = if stock == 0 || stock == u32::MAX {
        match graft(native, &owners[&movement], contact)? {
            Ok(graft) => Some(graft),
            Err(reason) => {
                return Ok(
                    json!({"status":"native","source":radial.source["tag"],"reason":reason}),
                );
            }
        }
    } else {
        None
    };
    let target = graft.as_ref().map_or(stock, |graft| graft.graph);
    let radius = radial.radius[1] * metres;
    let start = radial.radius[0] / radial.radius[1];
    let push = crate::halo_reach::rig::push_strength(radial.acceleration, metres);
    // The unreplicated carrier creates its detonation, which flies unreplicated as well.
    let mut copy = blast::Copy::local(native, assets, target, "reach-projectile-detonation")?;
    // Every damage action of the detonation names one native profile, retargeted together.
    let mut templates = BTreeSet::new();
    let mut damage = Vec::new();
    for (&tag, p) in &copy.converted {
        if class(p)? != FLOW {
            continue;
        }
        let d = p.pointer(24)?;
        for event in p.array(d + 0x168, 24, Some(0x8080_93e6))? {
            let q = p.pointer(event + 16)?;
            if p.u32(q - 4)? != QUERY {
                continue;
            }
            let mut actions = Vec::new();
            for row in p.array(q + 0x178, 0x58, Some(0x8080_4abf))? {
                let a = p.pointer(row + 0x50)?;
                if p.u32(a - 4)? == PROFILE_ACTION {
                    templates.insert(p.u32(a + 8)?);
                    actions.push(a);
                }
            }
            if !actions.is_empty() {
                damage.push((tag, q, actions));
            }
        }
    }
    let templates = templates.into_iter().collect::<Vec<_>>();
    let &[template] = templates.as_slice() else {
        anyhow::bail!("Native detonation damage needs exactly one outgoing profile")
    };
    let (profile_id, profile_report) = profile(
        native,
        assets,
        "reach-projectile-detonation-damage",
        template,
        radial.amount,
        push,
    )?;
    let mut scales = Vec::new();
    for (tag, q, actions) in &damage {
        let p = copy
            .converted
            .get_mut(tag)
            .context("Detonation flow owner")?;
        let shape = blast::sphere(p, *q)?;
        ensure!(
            p.f32(shape + 4)? == 1.,
            "Native detonation damage query is not a unit sphere"
        );
        blast::falloff(p, *q, start, None)?;
        for &a in actions {
            put(&mut p.0, a + 8, &profile_id.to_le_bytes())?;
            // The damage scale reads the detonation's creation channels, which the source
            // amount replaces, so the profile amount applies as it is.
            let expression = a + 0x28;
            let inputs = p.u64(expression + 32)?;
            scales.push(json!({"query":q,"inputs":inputs}));
            super::program::write(p, expression, &CONSTANT, &[1.], inputs)?;
        }
    }
    if graft.is_some() {
        hold(&mut copy.converted, radius)?;
    }
    let root = copy.finish(native, assets, "reach-projectile-detonation")?;
    let p = owners.get_mut(&movement).context("Projectile movement")?;
    let grafted = match &graft {
        None => {
            shape_scale(p, definition, radius)?;
            Value::Null
        }
        Some(graft) => {
            let actions = detonate(p, &graft.parameter)?;
            json!({"donor_movement":format!("{IMPACT_MOVEMENT:08X}"),"detonating_actions":actions,
                "shape_scale":"The private graph's parameter component holds the radius as its default, since the movement passes no creation inputs",
                "basis":"The source detonates on contact, so each stopping contact detonates through the movement's own graph, as the impact grenade's finishing contacts do"})
        }
    };
    put(&mut p.0, definition + 0x180, &root.to_le_bytes())?;
    let mut limits = vec![
        json!(
            "The radius reaches the queries through the native detonation_shape_scale channel, which also sizes the native presentation. That the channel scales the unit query spheres is inferred from their stored unit radius and the channel name."
        ),
        json!(
            "Push strength scales source instantaneous acceleration by the gameplay calibration that gives the Concussion Rifle the strength of Chosen of the Warmind's stronger push. Physical equivalence is not established."
        ),
        json!(
            "The source exponent falloff, damage core radius, owner and friendly rules and source detonation effects are not translated."
        ),
    ];
    if graft.is_some() {
        limits.push(json!("That an unpassed parameter keeps its default at runtime is inferred from the stock graph's 1DC6A44C, which the grenade never passes and which holds 1.25 in both its definition and instance rows."));
    }
    Ok(
        json!({"status":"translated","source":radial.source["tag"],"native_graph":target,
        "graft":grafted,
        "radius_metres":radius,"falloff_start":start,"amount":radial.amount,
        "source_acceleration":radial.acceleration,"native_push_strength":push,
        "damage_queries":damage.len(),"damage_scales":scales,"profile":profile_report,
        "falloff":"One inside the source inner radius, linear to zero at the outer radius",
        "replication":"none",
        "retained_native_behavior":["Detonation recipient filters, attachments and presentation","Direct contact damage","Target resistance and difficulty scaling"],
        "limits":limits,
        "gameplay_verified":false}),
    )
}

/// The impact grenade's detonation graph and its detonating action parameter, for a movement
/// that names no graph. The reason it takes none when the source does not detonate on contact,
/// or when the movement already passes creation inputs that a parameter default would hide.
struct Graft {
    graph: u32,
    parameter: Vec<u8>,
}

fn graft(
    native: &mut Reader,
    movement: &Payload,
    contact: bool,
) -> Result<std::result::Result<Graft, &'static str>> {
    if !contact {
        return Ok(Err(NO_GRAPH));
    }
    let definition = movement.pointer(24)?;
    if !movement
        .array(definition + 0x198, 0x78, Some(0x8080_851c))?
        .is_empty()
    {
        return Ok(Err(
            "The native carrier passes creation inputs but creates no detonation graph",
        ));
    }
    let donor = native.tag(IMPACT_MOVEMENT, Some(0x8080_9c36))?;
    let d = donor.pointer(24)?;
    ensure!(
        donor.u32(d - 4)? == MOVEMENT,
        "The impact grenade's movement has another type"
    );
    let graph = donor.u32(d + 0x180)?;
    ensure!(
        graph != 0 && graph != u32::MAX,
        "The impact grenade's movement creates no detonation graph"
    );
    let mut parameters = BTreeSet::new();
    for action in actions(&donor, d)? {
        if donor.u32(action + 8)? != DETONATE {
            continue;
        }
        let parameter = donor.pointer(action)?;
        ensure!(
            donor.u32(parameter - 4)? == DETONATE_PARAMETER,
            "The impact grenade's detonating action has another parameter"
        );
        let bytes = donor
            .0
            .get(parameter..parameter + DETONATE_SIZE)
            .context("Detonating action parameter extent")?;
        parameters.insert(bytes.to_vec());
    }
    let [parameter] = <[Vec<u8>; 1]>::try_from(parameters.into_iter().collect::<Vec<_>>())
        .ok()
        .context("The impact grenade's detonating actions differ")?;
    ensure!(
        u32::from_le_bytes(parameter[0x10..0x14].try_into()?) == u32::MAX,
        "The impact grenade's detonation names another graph than its movement's"
    );
    Ok(Ok(Graft { graph, parameter }))
}

/// Every contact response action of movement definition `d`.
fn actions(p: &Payload, d: usize) -> Result<Vec<usize>> {
    let mut actions = Vec::new();
    for group in p.array(d + 0x218, 0x58, Some(0x8080_37cd))? {
        for node in p.array(group + 0x48, 0x188, Some(0x8080_37d0))? {
            actions.extend(p.array(node + 0x178, 16, Some(RESPONSE_ACTION))?);
        }
    }
    Ok(actions)
}

/// Turns every stopping contact action of movement `p` into a detonation with a copy of
/// `parameter`, and returns how many it turned.
fn detonate(p: &mut Payload, parameter: &[u8]) -> Result<usize> {
    let definition = p.pointer(24)?;
    let mut stops = Vec::new();
    for action in actions(p, definition)? {
        if p.u32(action + 8)? == STOP {
            ensure!(
                p.u32(p.pointer(action)? - 4)? == STOP_PARAMETER,
                "Stopping contact action has another parameter"
            );
            stops.push(action);
        }
    }
    ensure!(
        !stops.is_empty(),
        "The movement has no stopping contact to detonate"
    );
    let at = crate::tiger::sequence::object(p, DETONATE_PARAMETER, parameter.len())?;
    put(&mut p.0, at, parameter)?;
    for &action in &stops {
        crate::tiger::sequence::relative(p, action, at)?;
        put(&mut p.0, action + 8, &DETONATE.to_le_bytes())?;
    }
    Ok(stops.len())
}

/// Holds `radius` as the `detonation_shape_scale` default of the detonation copy's one parameter
/// component, in its definition row and that row's instance twin, both stock zero.
fn hold(owners: &mut BTreeMap<u32, Payload>, radius: f32) -> Result<()> {
    let tags = owners
        .iter()
        .filter(|(_, p)| class(p).ok() == Some(PARAMETERS))
        .map(|(&tag, _)| tag)
        .collect::<Vec<_>>();
    let [tag] = tags.as_slice() else {
        anyhow::bail!("Native detonation has no single parameter component")
    };
    let p = owners
        .get_mut(tag)
        .context("Detonation parameter component")?;
    let (row, twin) = shape_scale_rows(p)?;
    put(&mut p.0, row + 0x30, &radius.to_le_bytes())?;
    put(&mut p.0, twin + 0x20, &radius.to_le_bytes())
}

fn shape_scale_rows(p: &Payload) -> Result<(usize, usize)> {
    let definition = p.pointer(24)?;
    let instance = p.pointer(16)?;
    let owner = p.u32(definition)?;
    let twins = p.array(instance + 0x40, 0x30, Some(PARAMETER_TWIN))?;
    let rows = p
        .array(definition + 0xa0, 0x40, Some(PARAMETER_ROW))?
        .into_iter()
        .filter(|&row| p.u32(row + 0x28).ok() == Some(SHAPE_SCALE))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        anyhow::bail!("Native detonation has no single shape-scale parameter")
    };
    let twin = usize::try_from(p.u64(row + 8)?)?;
    ensure!(
        p.u32(*row)? == owner
            && p.u32(row + 4)? == PARAMETER_TWIN
            && twins.contains(&twin)
            && p.u32(twin)? == owner
            && p.u32(twin + 4)? == PARAMETER_ROW
            && p.u64(twin + 8)? == *row as u64,
        "Native detonation shape-scale parameter and its twin are not reciprocal"
    );
    ensure!(
        p.f32(row + 0x30)? == 0. && p.f32(twin + 0x20)? == 0.,
        "Native detonation shape-scale parameter already has a default"
    );
    Ok((*row, twin))
}
