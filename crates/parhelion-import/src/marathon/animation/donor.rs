//! The native donor whose first-person states request the source weapon's own reloads.
//!
//! Native first-person states differ in how they reload. Every examined Shadowkeep weapon type
//! requests `reload_full` and `reload_empty`, except shotguns: 61 of 63 request `reload_enter`, a
//! shell insert and `chamber_1` instead. A source weapon that reloads by magazine on a shell-fed
//! donor never plays its reload. Among donors of the same type, the first that requests every
//! magazine reload the source plays is chosen, keeping the importer's order otherwise.
use super::compile::{PROFILE, RELOAD, RELOADS};
use super::*;

/// The source reloads that carry their own weapon motion: `reload_empty` and `reload_full` whose
/// weapon lookup names a descriptor after them, or whose first absolute arm clip, under the
/// weapon's profile and reload value, has a weapon-local clip within one frame of its length.
pub(in crate::marathon) fn magazine_reloads(
    source: &mut Reader,
    root: u32,
) -> Result<BTreeSet<u32>> {
    let owners = components(source, root)?;
    let Some((_, weapon, base, _)) = owners.iter().find(|o| o.3 == 0x80804A14) else {
        return Ok(BTreeSet::new());
    };
    let (weapon, base) = (weapon.clone(), *base);
    let locals = owners
        .iter()
        .filter(|o| o.3 == controller::LOOKUP)
        .collect::<Vec<_>>();
    let [local] = locals.as_slice() else {
        return Ok(BTreeSet::new());
    };
    let local = controller::Controller::read(source, &local.1.clone())?;
    let profile = weapon.u32(base + 0x314)?;
    let selection = source.tag(source.ref64(&weapon, base + 0x338)?, Some(0x808025AA))?;
    let arms = source.ref64(&selection, 0x10)?;
    let lookups = components(source, arms)?
        .into_iter()
        .filter(|o| o.3 == controller::LOOKUP)
        .collect::<Vec<_>>();
    let [lookup] = lookups.as_slice() else {
        return Ok(BTreeSet::new());
    };
    let arm = controller::Controller::read(source, &lookup.1)?;
    let mut lengths = Vec::new();
    for entry in &local.descriptors {
        if let Some(ordinal) = entry.clip
            && entry.rate == 1.
        {
            lengths.push(
                source
                    .tag(local.clips[ordinal], Some(0x8080AE01))?
                    .u16(0x140)?,
            );
        }
    }
    let mut reloads = BTreeSet::new();
    for (action, value) in RELOADS {
        let name = source::fnv(action);
        if !arm.names.contains_key(&name) {
            continue;
        }
        if local
            .descriptors
            .iter()
            .any(|d| d.name == name && d.clip.is_some())
        {
            reloads.insert(name);
            continue;
        }
        let mut inputs = BTreeMap::from([(PROFILE, profile)]);
        if arm.declares(RELOAD, source::fnv(value)) {
            inputs.insert(RELOAD, source::fnv(value));
        }
        for (descriptor, _) in arm.route(name, &inputs)? {
            let Some(ordinal) = arm.descriptors[descriptor as usize].clip else {
                continue;
            };
            let clip = source.tag(arm.clips[ordinal], Some(0x8080AE01))?;
            if clip.u8(0x181)? != 0 {
                continue;
            }
            let frames = clip.u16(0x140)?;
            if lengths.iter().any(|f| f.abs_diff(frames) <= 1) {
                reloads.insert(name);
            }
            break;
        }
    }
    Ok(reloads)
}

/// The index of the donor to build on: the first, in the given order, whose first-person states
/// request every magazine reload the source plays, else the first. Returns the choice's report.
pub(in crate::marathon) fn choose(
    source: &mut Reader,
    native: &mut Reader,
    root: u32,
    donors: &[(u32, String)],
) -> Result<(usize, Value)> {
    let reloads = magazine_reloads(source, root)?;
    let names = reloads
        .iter()
        .map(|r| format!("{r:08X}"))
        .collect::<Vec<_>>();
    if reloads.is_empty() || donors.len() < 2 {
        return Ok((
            0,
            json!({"source_magazine_reloads":names,"chosen_by":"order"}),
        ));
    }
    let mut rejected = Vec::new();
    for (index, (hash, name)) in donors.iter().enumerate() {
        let requested = crate::tiger::shadowkeep::item_tag(native, *hash)
            .and_then(|tag| native::requested_actions(native, tag));
        match requested {
            Ok(actions) if reloads.is_subset(&actions) => {
                return Ok((
                    index,
                    json!({"source_magazine_reloads":names,"chosen_by":"reloads","rejected":rejected}),
                ));
            }
            Ok(_) => rejected.push(json!({"donor":format!("{hash:08X}"),"name":name,
                "reason":"Does not request every source magazine reload"})),
            Err(error) => rejected.push(json!({"donor":format!("{hash:08X}"),"name":name,
                "reason":format!("{error:#}")})),
        }
    }
    Ok((
        0,
        json!({"source_magazine_reloads":names,"chosen_by":"order","rejected":rejected,
            "reason":"No donor of this type requests every source magazine reload"}),
    ))
}
