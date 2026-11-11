//! A source projectile that detonates when its firing trigger is released, on the native remote
//! grenade.
//!
//! Reach tethers a projectile fired from a trigger of behavior Latch-Tether (3) once the trigger
//! has been held for the projectile's Maximum Latch Time To Arm (`proj +0x19C`). Projectile flag
//! 21, Supports Tethering, makes a tethered projectile wait for the trigger to unlatch and then
//! detonate at once, wherever it is. Maximum Latch Time To Detonate (`proj +0x198`) bounds the
//! hold, and zero sets no bound. Material Response flags 5 and 6 restrict a row to tethered or
//! untethered contacts. A projectile with the flag fired from such a trigger detonates on release.
//!
//! Shadowkeep's remote grenade `80BBB91F` holds the native counterpart. Its value provider, class
//! `8080378A` with key `2F4A6E81` (the FNV-1 hash of `remote_detonation`) at definition `+0x98`,
//! publishes the name at `+0x90`, `CD7422C6`, into the projectile's bank. Its movement reads that
//! name in two gated nodes. At or above 1 a contact bounces (selector 2), and through the gate
//! `1 - CD7422C6` a contact finishes through the graph at movement definition `+0x180` (selector
//! 4). Gates are read only for contacts. Apart from its movement, bank, provider and one further
//! component, its components are those of the ordinary impact grenade `80BBB917`.
use super::*;

/// The native remote grenade carrier.
pub(super) const CARRIER: u32 = 0x80bb_b91f;
/// The ordinary impact grenade, the carrier an earlier automatic selection chose for these sources.
pub(super) const ORDINARY: u32 = 0x80bb_b917;
const PROVIDER: u32 = 0x8080_378a;
/// FNV-1 of `remote_detonation`.
const KEY: u32 = 0x2f4a_6e81;
const MOVEMENT: u32 = 0x8080_388f;
/// Reach trigger behavior Latch-Tether.
const LATCH_TETHER: i64 = 3;
/// Reach projectile flag 21, Supports Tethering.
const SUPPORTS_TETHERING: u64 = 1 << 21;
/// Material Response flags 5 and 6, Only When Tethered and Only When Not Tethered.
const ONLY_TETHERED: u64 = 0x20;
const ONLY_UNTETHERED: u64 = 0x40;
/// The held gate passes the provider's value, and the release gate one minus it. Both pass at or
/// above node `+0x150`, 1.0 on the stock nodes.
const HELD: [u8; 4] = [0x3c, 1, 0x3e, 0];
const RELEASED: [u8; 7] = [0x34, 0, 0x3c, 1, 0x02, 0x3e, 0];
const BOUNCE: u8 = 2;
const FINISH: u8 = 4;

/// The source trigger and barrel that tether a projectile which detonates on release.
pub(super) struct Tether {
    pub trigger: usize,
    pub barrel: usize,
}

/// The trigger of behavior Latch-Tether whose primary barrel fires `projectile`, when the
/// projectile also supports tethering. A projectile only one of the two names keeps its ordinary
/// contact rules.
pub(super) fn tethered(behavior: &Value, projectile: &Value) -> Result<Option<Tether>> {
    let flags = projectile["behavior"]["flags"]
        .as_u64()
        .context("Source projectile flags")?;
    if flags & SUPPORTS_TETHERING == 0 {
        return Ok(None);
    }
    let id = projectile["tag"]["datum"]
        .as_u64()
        .context("Source projectile identity")?;
    let Some(triggers) = super::gameplay(behavior)["triggers"].as_array() else {
        return Ok(None);
    };
    for (trigger, row) in triggers.iter().enumerate() {
        if row["behavior"].as_i64() != Some(LATCH_TETHER) {
            continue;
        }
        let Some(barrel) = row["primary_barrel"]
            .as_i64()
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        if super::barrel_projectile(behavior, barrel) == Some(id) {
            return Ok(Some(Tether { trigger, barrel }));
        }
    }
    Ok(None)
}

/// The source rows restricted to tethered or untethered contacts.
fn conditional(projectile: &Value) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for row in projectile["behavior"]["responses"]
        .as_array()
        .context("Source material responses")?
    {
        let flags = row["flags"].as_u64().context("Source response flags")?;
        let when = match (flags & ONLY_TETHERED != 0, flags & ONLY_UNTETHERED != 0) {
            (true, false) => "tethered",
            (false, true) => "untethered",
            (false, false) => continue,
            (true, true) => "never",
        };
        rows.push(
            json!({"material":row["material"],"response":row["response"],"when":when,
            "angle_radians":row["angle_radians"],"velocity":row["velocity"]}),
        );
    }
    Ok(rows)
}

fn class(p: &Payload) -> Result<u32> {
    p.u32(p.pointer(24)? - 4)
}

/// The carrier's remote detonation provider and the movement nodes its value gates, or why the
/// carrier has none.
fn gates(owners: &BTreeMap<u32, Payload>) -> Result<std::result::Result<Value, &'static str>> {
    let mut providers = Vec::new();
    for (&tag, p) in owners {
        if class(p)? == PROVIDER && p.u32(p.pointer(24)? + 0x98)? == KEY {
            providers.push((tag, p));
        }
    }
    let [(provider, p)] = providers.as_slice() else {
        return Ok(Err(
            "The carrier holds no single remote detonation provider",
        ));
    };
    let d = p.pointer(24)?;
    let name = p.u32(d + 0x90)?;
    let block = [p.f32(d + 0x9c)?, p.f32(d + 0xa0)?, p.f32(d + 0xa4)?];
    let count = p.u32(d + 0xa8)?;
    let mut movements = Vec::new();
    for p in owners.values() {
        if class(p)? == MOVEMENT {
            movements.push(p);
        }
    }
    let [m] = movements.as_slice() else {
        anyhow::bail!("Projectile movement is absent or ambiguous")
    };
    let d = m.pointer(24)?;
    let (mut held, mut released) = (Vec::new(), Vec::new());
    for group in m.array(d + 0x218, 0x58, Some(0x8080_37cd))? {
        for node in m.array(group + 0x48, 0x188, Some(0x8080_37d0))? {
            let inputs = m
                .array(node + 0x130, 40, Some(0x8080_9789))?
                .into_iter()
                .map(|row| m.u32(row + 32))
                .collect::<Result<Vec<_>>>()?;
            if inputs != [name] {
                continue;
            }
            let code = m
                .array(node + 0x100, 1, Some(0x8080_0009))?
                .into_iter()
                .map(|at| m.u8(at))
                .collect::<Result<Vec<_>>>()?;
            let selectors = m
                .array(node + 0x178, 16, Some(0x8080_37f7))?
                .into_iter()
                .map(|action| m.u8(action + 8))
                .collect::<Result<Vec<_>>>()?;
            let threshold = m.f32(node + 0x150)?;
            let row = json!({"node":format!("{:08X}", m.u32(node + 0x10)?),
                "mask":format!("{:#x}", m.u64(node + 0x28)?),"selectors":selectors,"threshold":threshold});
            if threshold != 1. {
                continue;
            }
            if code == HELD && selectors.contains(&BOUNCE) {
                held.push(row);
            } else if code == RELEASED && selectors.contains(&FINISH) {
                released.push(row);
            }
        }
    }
    if held.is_empty() || released.is_empty() {
        return Ok(Err(
            "The carrier movement does not bounce while the provider's value holds and finish once it falls",
        ));
    }
    Ok(Ok(
        json!({"provider":format!("{provider:08X}"),"name":format!("{name:08X}"),
        "key":format!("{KEY:08X}"),"block":block,"count":count,"held":held,"released":released}),
    ))
}

/// The source's latch times, which the behavior receipt does not hold.
fn latch(cache: &cache::Cache, projectile: &Value) -> Result<Value> {
    let path = projectile["tag"]["path"]
        .as_str()
        .context("Source projectile path")?;
    let at = cache.find("proj", path)?.address()?;
    Ok(json!({"detonate_seconds":cache.f32(at + 0x198)?,"arm_seconds":cache.f32(at + 0x19c)?}))
}

/// The source super detonation, which Reach plays for the tethered projectile's detonation.
fn super_detonation(behavior: &Value, projectile: &Value) -> Result<Value> {
    let edges = behavior["edges"].as_array().context("Behavior edges")?;
    let sources = edges
        .iter()
        .filter(|e| {
            e["owner"] == projectile["tag"]["datum"]
                && matches!(
                    e["role"].as_str(),
                    Some("super_detonation" | "super_damage")
                )
        })
        .map(|e| json!({"role":e["role"],"source":e["tag"]}))
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(Value::Null);
    }
    Ok(json!({"status":"omitted","sources":sources,
        "reason":"A release finishes through the same private detonation as a contact. The source's super detonation effect and damage, which its tethered projectile plays in place of the ordinary detonation, are not carried, and the emp receipt records the side effect of its damage."}))
}

/// What became of the source's release detonation on the converted carrier `owners`.
pub(super) fn receipt(
    cache: &cache::Cache,
    owners: &BTreeMap<u32, Payload>,
    behavior: &Value,
    projectile: &Value,
) -> Result<Value> {
    let rows = conditional(projectile)?;
    let Some(tether) = tethered(behavior, projectile)? else {
        let mut receipt = json!({"status":"not_applicable",
            "reason":"The source projectile does not wait for a tethering trigger's release"});
        if !rows.is_empty() {
            receipt["status"] = json!("omitted");
            receipt["reason"] = json!(
                "The source restricts contact rows to tethered or untethered projectiles, while no tethering trigger fires this projectile with Supports Tethering, so no native remote detonation applies"
            );
            receipt["tether_conditional_rows"] = json!(rows);
        }
        return Ok(receipt);
    };
    let latch =
        latch(cache, projectile).unwrap_or_else(|error| json!({"unread":format!("{error:#}")}));
    let source = json!({"trigger":tether.trigger,"trigger_behavior":"latch_tether",
        "barrel":tether.barrel,"supports_tethering":true,"maximum_latch":latch,
        "tether_conditional_rows":rows});
    let native = match gates(owners)? {
        Ok(native) => native,
        Err(reason) => {
            return Ok(json!({"status":"omitted","source":source,"reason":reason,
                "super_detonation":super_detonation(behavior, projectile)?,"gameplay_verified":false}));
        }
    };
    Ok(json!({"status":"partial","source":source,"native":native,
        "translation":"The projectile flies on the native remote grenade. While the trigger is held the provider's value holds and contacts bounce, and once it falls the next contact finishes through the private detonation",
        "super_detonation":super_detonation(behavior, projectile)?,
        "limits":[
            "Native gates are read only at contacts, so a released projectile detonates at its next contact, not at once in the air. No native component is established that ends a flying projectile on the provider's value. An effect flow on the projectile gated below the held value, as the Rise of Iron release drop uses, would also pass for a tap that never armed, since the native range gate reads a level and keeps no latch.",
            "The provider keeps its stock block. That its first value is the hold that arms the remote detonation and its third the longest hold is inferred from the match with the source's Maximum Latch Time To Arm and from the Scorch Cannon's longer stock block. The source latch times are recorded, not written.",
            "Source rows restricted to tethered or untethered contacts are not mapped. A held projectile takes the remote grenade's own contact responses, and an untethered tap detonates at the first contact the release node accepts, without the source's angle and speed windows or its timer after the first bounce.",
            "That the provider follows the firing weapon's held trigger is inferred from the remote grenade's gates. It has not been observed in game."
        ],
        "gameplay_verified":false}))
}
