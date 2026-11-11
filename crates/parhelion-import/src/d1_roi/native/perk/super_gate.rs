//! Talents active while a super of one damage type is equipped, as a subclass damage-type gate.
//!
//! Rise of Iron Arc, Solar and Void Armor activate when one listed super joins the ability roster
//! (condition kind 10, class `808031BF`) and end when it leaves (kind 11, `8080308E`). Each lists
//! the supers of every class's subclass of one damage type, which Shadowkeep does not have. The
//! program instead names that damage type, and the build expands it over the installed
//! subclasses' supers, stock or authored.
//!
//! No source subclass record holds its damage type. A super's own outgoing area profiles do:
//! every super that deals damage names profiles of one damage type, beside a Kinetic contact
//! profile. Sunsinger's Radiance and Defender's Ward of Dawn deal none. A list therefore takes
//! the one damage type its damaging supers share, and every super in it must be a subclass's. It
//! must also hold every subclass super that deals that type. Any other list names particular
//! supers, and its roster conditions keep them through `counterpart`.
use super::*;

/// The source area profile definition class, damage type at definition `+8C`.
const PROFILE: u32 = 0x80AA_C28B;
/// Graph and component owner classes the closure walk follows.
const WALKED: [u32; 2] = [0x8080_0734, 0x8080_0861];
const DEPTH: usize = 6;

/// The patterns a condition list names, when every condition is the roster event `class`/`kind`.
fn roster(
    data: &Payload,
    descriptor: usize,
    class: u32,
    kind: u8,
) -> Result<Option<BTreeSet<u32>>> {
    let mut patterns = BTreeSet::new();
    for row in data.array(descriptor, 8, None)? {
        let at = data.pointer(row)?;
        if data.u32(at - 4)? != class || data.u8(at + 5)? != kind {
            return Ok(None);
        }
        patterns.insert(data.u32(at + 16)?);
    }
    Ok((!patterns.is_empty()).then_some(patterns))
}

/// The non-Kinetic damage types of the area profiles a pattern's graph closure names.
fn damage_types(source: &Source, pattern: u32) -> Result<BTreeSet<u8>> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![(pattern, 0)];
    let mut types = BTreeSet::new();
    while let Some((tag, depth)) = stack.pop() {
        if depth > DEPTH || !seen.insert(tag) {
            continue;
        }
        let Ok(payload) = source.read(tag) else {
            continue;
        };
        if source.reference(tag)? == 0x8080_0861
            && let Ok(root) = payload.pointer(24)
            && root >= 4
            && payload.u32(root - 4).ok() == Some(PROFILE)
        {
            let damage = payload.u32(root + 0x8C)?;
            ensure!(damage <= 3, "Source super profile damage type is unknown");
            if damage != 0 {
                types.insert(u8::try_from(damage)?);
            }
        }
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            let word = payload.u32(at)?;
            if (0x8080_0000..0x8180_0000).contains(&word)
                && !seen.contains(&word)
                && source
                    .reference(word)
                    .is_ok_and(|class| WALKED.contains(&class))
            {
                stack.push((word, depth + 1));
            }
        }
    }
    Ok(types)
}

/// The damage type of the subclass gate an action's conditions form, in the damage-profile
/// encoding, when its activation is the roster addition of supers and its removal the roster
/// removal of the same supers.
pub(super) fn read(source: &Source, data: &Payload) -> Result<Option<(u8, BTreeSet<u32>)>> {
    let Some(added) = roster(data, 0x20, 0x8080_31BF, 10)? else {
        return Ok(None);
    };
    let Some(removed) = roster(data, 0x48, 0x8080_308E, 11)? else {
        return Ok(None);
    };
    if added != removed || data.u64(0x58)? != 0 {
        return Ok(None);
    }
    let assignments = source.runtime_assignments(&mut BTreeMap::new())?;
    let supers = source
        .subclass_choices()?
        .into_iter()
        .filter_map(|choice| assignments.get(&choice.pattern).copied())
        .collect::<BTreeSet<_>>();
    if !added.iter().all(|pattern| supers.contains(pattern)) {
        return Ok(None);
    }
    let mut types = BTreeSet::new();
    for &pattern in &added {
        let own = damage_types(source, pattern)?;
        ensure!(
            own.len() <= 1,
            "Source super {pattern:08X} deals more than one damage type"
        );
        types.extend(own);
    }
    let mut types = types.into_iter();
    let (Some(damage), None) = (types.next(), types.next()) else {
        // A list without one shared damage type names particular supers, which the roster
        // conditions select through their Shadowkeep versions.
        return Ok(None);
    };
    // Only the whole damage type's list stands for that type. A shorter list, such as Elusive
    // Shadow's Shadowshot alone, names those supers.
    for &pattern in &supers {
        if !added.contains(&pattern) && damage_types(source, pattern)?.contains(&damage) {
            return Ok(None);
        }
    }
    Ok(Some((damage, added)))
}
