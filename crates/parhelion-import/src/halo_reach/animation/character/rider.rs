//! Select complete source driver actions from the seat's animation role.
use super::*;
use crate::halo_reach::Scene;

pub(super) fn defaults(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    carrier: Option<u32>,
) -> Result<Option<Options>> {
    const CHARACTER: &str = "objects\\characters\\spartans\\spartans";
    let Some(seats) = scene.object.gameplay["seats"].as_array() else {
        return Ok(None);
    };
    let drivers = seats
        .iter()
        .filter(|seat| seat["flags"].as_u64().is_some_and(|f| f & 4 != 0))
        .collect::<Vec<_>>();
    if drivers.is_empty() {
        return Ok(None);
    }
    ensure!(drivers.len() == 1, "Source rider has multiple driver roles");
    let role = drivers[0]["animation"]
        .as_str()
        .filter(|v| !v.is_empty())
        .context("Source driver animation role")?;
    if !["mode", "jmad"].iter().all(|group| {
        cache
            .tags
            .iter()
            .flatten()
            .any(|tag| tag.group == *group && tag.path == CHARACTER)
    }) {
        return Ok(None);
    }
    let graph = crate::halo_reach::behavior::animation_data(cache, cache.find("jmad", CHARACTER)?)?;
    let names = graph["clips"]
        .as_array()
        .context("Source rider actions")?
        .iter()
        .filter_map(|clip| clip["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    // Seats Reach gives no Spartan actions, such as a Pelican or Phantom pilot that only AI
    // flies, keep the carrier's native driver pose, as when the graph is absent.
    let rider = if names
        .iter()
        .any(|name| name.starts_with(&format!("{role}:")))
    {
        Some(transitions(&names, role)?)
    } else {
        None
    };
    // Each seat that rides as a seated child takes its role's complete source actions. A role
    // without a unique entry, mounted pose and exit keeps the stock seat's pose.
    let mut passengers = Vec::new();
    let mut roles = std::collections::BTreeSet::new();
    for mount in crate::halo_reach::native::seat::mounts(cache, scene)? {
        let Some(role) = mount.seat["animation"].as_str().filter(|v| !v.is_empty()) else {
            continue;
        };
        if roles.insert(role.to_owned())
            && let Ok(rider) = transitions(&names, role)
        {
            passengers.push(Passenger {
                role: role.to_owned(),
                rider,
            });
        }
    }
    let reference = match (&rider, passengers.first()) {
        (Some(rider), _) => rider.idle.clone(),
        (None, Some(passenger)) => passenger.rider.idle.clone(),
        (None, None) => return Ok(None),
    };
    let Action::Clip(reference) = reference else {
        anyhow::bail!("Source rider idle is not a single action");
    };
    let native_controller = Controller {
        skeleton: 0x80C22410,
        controls: 0x80BFCB38,
        lookup: 0x80EF62E5,
        pose: 0x8161D5F7,
    };
    // The private driver role inherits the carrier seat's role in this controller's vehicle
    // role group. A carrier whose role the group lacks, such as the tank's tank_driver, keeps
    // its native driver pose, and so do its seated children.
    if let Some(carrier) = carrier
        && !carrier_role_known(native, carrier, native_controller.lookup)?
    {
        return Ok(None);
    }
    Ok(Some(Options {
        graph: CHARACTER.into(),
        model: CHARACTER.into(),
        reference,
        actions: BTreeMap::new(),
        stance: BTreeMap::new(),
        rider,
        passengers,
        locomotion: BTreeMap::new(),
        bone_aliases: BTreeMap::new(),
        native: native_controller,
    }))
}

/// A role's source entry, mounted pose and exit. An action names its role, then any number of
/// `any` weapon wildcards, then its state.
fn transitions(names: &std::collections::BTreeSet<&str>, role: &str) -> Result<Rider> {
    let prefix = format!("{role}:");
    let select = |state: &str| -> Result<Action> {
        let candidates = names
            .iter()
            .filter(|name| {
                name.strip_prefix(&prefix)
                    .and_then(|rest| rest.strip_suffix(state))
                    .is_some_and(|wildcards| {
                        wildcards.split_terminator(':').all(|level| level == "any")
                            && (wildcards.is_empty() || wildcards.ends_with(':'))
                    })
            })
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "Source rider role {role} has no unique {state} action"
        );
        Ok(Action::Clip(candidates[0].to_string()))
    };
    Ok(Rider {
        enter: select("enter")?,
        idle: select("idle").or_else(|_| class_idle(names, role))?,
        exit: select("exit")?,
    })
}

/// The mounted pose of a role that idles only with a weapon in hand, as a Reach passenger does
/// (`warthog_p:rifle:idle`, `warthog_p:pistol:idle` and others, with no `warthog_p:idle`): the idle
/// of the weapon class the role gives the most actions, so its most complete seated pose.
fn class_idle(names: &std::collections::BTreeSet<&str>, role: &str) -> Result<Action> {
    let prefix = format!("{role}:");
    let mut classes = BTreeMap::<&str, usize>::new();
    for name in names {
        if let Some((class, _)) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.split_once(':'))
            && class != "any"
        {
            *classes.entry(class).or_default() += 1;
        }
    }
    let idles = classes
        .into_iter()
        .filter(|(class, _)| names.contains(format!("{prefix}{class}:idle").as_str()))
        .collect::<Vec<_>>();
    let most = idles.iter().map(|&(_, count)| count).max().unwrap_or(0);
    let best = idles
        .iter()
        .filter(|&&(_, count)| count == most)
        .collect::<Vec<_>>();
    let [&(class, _)] = best[..] else {
        anyhow::bail!("Source rider role {role} has no unique idle");
    };
    Ok(Action::Clip(format!("{prefix}{class}:idle")))
}

/// Whether the driver role of `carrier`'s seat is in the vehicle role group of the controller
/// `lookup` names. The seat's override name at +0xEC replaces its role at +0xE8 when set.
fn carrier_role_known(native: &mut Reader, carrier: u32, lookup: u32) -> Result<bool> {
    let entity = (*native.tag(carrier, Some(0x80809c0f))?).clone();
    let mut roles = Vec::new();
    for row in entity.array(16, 12, None)? {
        let tag = entity.u32(row)?;
        if tag == u32::MAX {
            continue;
        }
        let owner = native.tag(tag, Some(0x80809c36))?;
        let Ok(definition) = owner.pointer(24) else {
            continue;
        };
        if owner.u32(definition.saturating_sub(4))? != 0x80803d7d
            || owner.u32(definition + 0xe0)? & 0x0e != 0x0e
        {
            continue;
        }
        let name = owner.u32(definition + 0xec)?;
        roles.push(if name == 0x811c9dc5 {
            owner.u32(definition + 0xe8)?
        } else {
            name
        });
    }
    ensure!(
        roles.len() == 1,
        "Carrier driver seat is absent or ambiguous"
    );
    let lookup = native.tag(lookup, Some(0x80809c36))?;
    let at = lookup.pointer(24)?;
    let dictionary = native.tag(lookup.u32(at + 0x94)?, Some(0x80808ee1))?;
    crate::tiger::animation::state::parameter::contains(&dictionary, 2, roles[0])
}
