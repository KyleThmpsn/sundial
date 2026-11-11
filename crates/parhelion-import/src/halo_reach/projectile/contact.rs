//! The source contact policy on a carrier without shards: its direct hit, finish and surface timer.
//!
//! An ordinary carrier is a native grenade movement. Its contact responses deal a hit, spawn the
//! finish graph at definition `+0x180` through selector 4, and stick to surfaces through a node
//! with a placement (selector 1) and a timer (selector 0). The source supplies what each of those
//! does: its own contact damage, no finish when it has no detonation, and its own rest timer.
use super::*;
use assembly::Assembly;

/// A stock direct-hit profile with bounds 0 and 1, which spans none to one hit of the firing
/// weapon. The shard movement names it. Its private copy deals exactly one hit.
const RELATIVE_HIT: u32 = 0x8157_9426;
/// Reach's Detonation Timer Starts value When At Rest.
const AT_REST: i64 = 2;

fn views(p: &Payload) -> Result<BTreeSet<usize>> {
    Ok(BTreeSet::from([
        p.pointer(24)?,
        usize::try_from(p.u64(p.pointer(16)? + 8)?)?,
    ]))
}

/// Applies the source's direct hit, finish and rest timer to the one movement in `owners`.
/// `detonation` is the detonation receipt, which says whether the source detonation was translated.
pub(super) fn ordinary(
    reader: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    behavior: &Value,
    projectile: &Value,
    detonation: &Value,
) -> Result<Value> {
    let hit = direct(reader, assets, owners, behavior, projectile, detonation)?;
    let mut movements = owners
        .values_mut()
        .filter(|p| Assembly::class(p).ok() == Some(0x8080388f))
        .collect::<Vec<_>>();
    let [p] = movements.as_mut_slice() else {
        anyhow::bail!("Projectile movement is absent or ambiguous")
    };
    let finish = finish(p, detonation)?;
    let timer = timer(p, projectile)?;
    Ok(json!({"direct_hit":hit,"finish":finish,"rest_timer":timer,"gameplay_verified":false}))
}

/// Makes a contact deal one hit of the firing weapon, as the weapon's primary projectile. The
/// movement's direct-hit profile at `+0x118`, and every contact damage action (selector 9,
/// `808037F3 +08`) that names it or names none, take a private weapon-relative profile of exactly
/// one hit. A grenade carrier's own profile, bounds 10 and 10, would deal ten. A translated source
/// detonation keeps the carrier's direct hit, since its own calibration was set against it.
fn direct(
    reader: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    behavior: &Value,
    projectile: &Value,
    detonation: &Value,
) -> Result<Value> {
    if detonation["status"] == "translated" {
        return Ok(json!({"status":"native",
            "reason":"The translated source detonation was calibrated against the carrier's own direct hit"}));
    }
    let (upper, lower, source) =
        match super::detonation::contact_bounds(behavior, projectile, "impact_damage")? {
            Ok(found) => found,
            Err(reason) => return Ok(json!({"status":"native","reason":reason})),
        };
    let mut movements = owners
        .values_mut()
        .filter(|p| Assembly::class(p).ok() == Some(0x8080388f))
        .collect::<Vec<_>>();
    let [p] = movements.as_mut_slice() else {
        anyhow::bail!("Projectile movement is absent or ambiguous")
    };
    let views = views(p)?;
    let native = p.u32(p.pointer(24)? + 0x118)?;
    let snapshot = assets.clone();
    // A relative template writes the amount over the Reach units per hit, so this is one hit.
    let (profile, report) = match super::detonation::profile(
        reader,
        assets,
        "reach-projectile-impact-damage",
        RELATIVE_HIT,
        super::detonation::REACH_UNITS_PER_WEAPON_HIT,
        0.,
    ) {
        Ok(created) => created,
        Err(error) => {
            *assets = snapshot;
            return Ok(json!({"status":"native",
                "reason":format!("The weapon-relative hit profile has another structure: {error:#}")}));
        }
    };
    let mut actions = 0;
    for &view in &views {
        put(&mut p.0, view + 0x118, &profile.to_le_bytes())?;
        for group in p.array(view + 0x218, 88, Some(0x8080_37cd))? {
            for node in p.array(group + 0x48, 392, Some(0x8080_37d0))? {
                for action in p.array(node + 0x178, 16, Some(0x8080_37f7))? {
                    let parameter = p.pointer(action)?;
                    if p.u8(action + 8)? != 9 || p.u32(parameter - 4)? != 0x8080_37f3 {
                        continue;
                    }
                    let target = p.u32(parameter + 8)?;
                    if target == u32::MAX || target == native {
                        put(&mut p.0, parameter + 8, &profile.to_le_bytes())?;
                        actions += 1;
                    }
                }
            }
        }
    }
    let mut limits = vec![
        "A contact deals one hit of the firing weapon, the donor's own damage per shot. The source amount is recorded. Its impact acceleration is not carried. Target resistance, power and difficulty scaling stay native.",
    ];
    if lower < upper {
        limits.push("The source damage falls from its upper to its lower amount over the projectile's damage range, which a native hit does not.");
    }
    Ok(
        json!({"status":"translated","source":source,"source_amount":upper,"source_lower":lower,
        "weapon_hits":1.0,"native":format!("{native:08X}"),"template":format!("{RELATIVE_HIT:08X}"),
        "profile":report,"contact_actions":actions,"gameplay_verified":false,"limits":limits}),
    )
}

/// Removes the native finish when the source has no detonation damage, so a contact ends the
/// projectile without the carrier's own explosion. A translated source detonation keeps its
/// private finish.
fn finish(p: &mut Payload, detonation: &Value) -> Result<Value> {
    if detonation["status"] == "translated" {
        return Ok(
            json!({"status":"translated","reason":"The private finish carries the source detonation"}),
        );
    }
    if detonation["reason"] != "The source projectile has no such damage profile" {
        return Ok(json!({"status":"native","reason":detonation["reason"]}));
    }
    let mut removed = Vec::new();
    for view in views(p)? {
        let native = p.u32(view + 0x180)?;
        if native != u32::MAX {
            removed.push(format!("{native:08X}"));
            put(&mut p.0, view + 0x180, &u32::MAX.to_le_bytes())?;
        }
    }
    removed.dedup();
    Ok(json!({"status":"translated","removed":removed,
        "reason":"The source has no detonation, so a contact or expired timer ends the projectile without the carrier's finish graph",
        "limits":["Source detonation effects without damage play through the contact host, not a finish."]}))
}

/// Gives the movement's surface attachment the source rest timer. A node that places the projectile
/// (selector 1) and times it (selector 0, `808037E1`) reads its expiry from a root scalar named by
/// parameter `+0x30`, as the shard path writes.
fn timer(p: &mut Payload, projectile: &Value) -> Result<Value> {
    let behavior = &projectile["behavior"];
    let seconds = behavior["timer_seconds"][1].as_f64().unwrap_or(0.) as f32;
    if behavior["timer_start"].as_i64() != Some(AT_REST) || !(seconds.is_finite() && seconds > 0.) {
        return Ok(
            json!({"status":"native","reason":"The source has no timer that starts when the projectile comes to rest"}),
        );
    }
    let instance = p.pointer(16)?;
    let programs = p.array(instance + 0x40, 48, Some(0x8080_89f7))?;
    let mut written = BTreeSet::new();
    for view in views(p)? {
        for group in p.array(view + 0x218, 88, Some(0x8080_37cd))? {
            for node in p.array(group + 0x48, 392, Some(0x8080_37d0))? {
                let actions = p.array(node + 0x178, 16, Some(0x8080_37f7))?;
                let selectors = actions
                    .iter()
                    .map(|&action| p.u8(action + 8))
                    .collect::<Result<Vec<_>>>()?;
                if !selectors.contains(&1) {
                    continue;
                }
                for (&action, &selector) in actions.iter().zip(&selectors) {
                    let parameter = p.pointer(action)?;
                    if selector != 0 || p.u32(parameter - 4)? != 0x8080_37e1 {
                        continue;
                    }
                    let Ok(index) = usize::try_from(p.u32(parameter + 0x30)?) else {
                        continue;
                    };
                    let Some(&expression) = programs.get(index) else {
                        continue;
                    };
                    let definition = usize::try_from(p.u64(expression + 8)?)?;
                    ensure!(
                        p.u64(definition + 8)? == expression as u64,
                        "Rest timer program reciprocal differs"
                    );
                    if written.insert(definition) {
                        program::constant(p, definition + 0x10, seconds)?;
                    }
                }
            }
        }
    }
    if written.is_empty() {
        return Ok(
            json!({"status":"native","reason":"The movement has no timed surface attachment to carry the source rest timer"}),
        );
    }
    Ok(
        json!({"status":"translated","seconds":seconds,"programs":written.len(),
        "limits":["The source timer's lower bound is not carried. The native timer starts when the projectile attaches."]}),
    )
}
