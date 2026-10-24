//! Resolve encounter openings and source gates to the private objects emitted in this build.
use std::collections::{BTreeMap, BTreeSet};

use parhelion_import::halo_reach::level::mission;
use tiger_pkg::TagHash;

use super::{Encounter, Triggers};
use crate::{
    AuthoringResult, NewTagSpec,
    error::invalid,
    map::{MapRecipe, MapSource},
    tag_payload::read_u32,
};

/// Sunrise's generated SDK folds runs of punctuation into one separator.
fn sdk_name(name: &str) -> String {
    let mut name = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .to_ascii_uppercase();
    if name.is_empty() {
        name.push_str("UNNAMED");
    } else if name.as_bytes()[0].is_ascii_digit() {
        name.insert(0, '_');
    }
    name
}

fn object(
    template: TagHash,
    key: u32,
    package: u16,
    tags: &[NewTagSpec],
) -> AuthoringResult<TagHash> {
    let matches = tags
        .iter()
        .enumerate()
        .filter(|(_, spec)| {
            spec.template_tag == template && read_u32(&spec.payload, 0x0C).ok() == Some(key)
        })
        .map(|(index, _)| {
            u16::try_from(index)
                .map(|index| TagHash::new(package, index))
                .map_err(|_| invalid("Mission object entry overflow"))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let [object] = matches.as_slice() else {
        return Err(invalid(
            "Mission controller needs one linked private object",
        ));
    };
    Ok(*object)
}

fn variants(
    opening: &mission::encounter::Opening,
    encounters: &[(usize, Encounter, TagHash)],
) -> AuthoringResult<String> {
    let matches = |source: &str, target: &str| {
        source == target
            || (!source.contains('/')
                && target
                    .strip_prefix(source)
                    .is_some_and(|rest| rest.starts_with('/')))
    };
    // Every complete area variant must represent the same source members. A partially loaded
    // source squad cannot silently become a smaller encounter.
    let mut required = BTreeMap::new();
    for placement in &opening.placements {
        let mut members = BTreeMap::new();
        for (_, encounter, _) in encounters {
            for squad in encounter
                .squads
                .iter()
                .filter(|s| matches(&placement.source, &s.source))
            {
                if let Some(previous) = members.insert(squad.name.clone(), squad.count)
                    && previous != squad.count
                {
                    return Err(invalid(
                        "Source squad membership differs between native areas",
                    ));
                }
            }
        }
        if members.is_empty() {
            return Err(invalid(format!(
                "Source squad {} has no native actor mapping",
                placement.source
            )));
        }
        if placement
            .count
            .is_some_and(|count| members.len() > 1 && count != members.values().sum::<usize>())
        {
            return Err(invalid(
                "A partial source squad needs an explicit member distribution",
            ));
        }
        if required.insert(placement.source.clone(), members).is_some() {
            return Err(invalid("Encounter opening places a source squad twice"));
        }
    }
    let mut variants = Vec::new();
    for (area, encounter, object) in encounters {
        let mut rows = Vec::new();
        let mut used = BTreeSet::new();
        let mut complete = true;
        for placement in &opening.placements {
            let squads = encounter
                .squads
                .iter()
                .filter(|s| matches(&placement.source, &s.source))
                .collect::<Vec<_>>();
            let membership = squads
                .iter()
                .map(|s| (s.name.clone(), s.count))
                .collect::<BTreeMap<_, _>>();
            if membership != required[&placement.source] {
                complete = false;
                break;
            }
            for squad in &squads {
                if !used.insert(&squad.name) {
                    return Err(invalid("Encounter opening aliases a native squad"));
                }
                let count = if squads.len() == 1 {
                    placement.count.unwrap_or(squad.count)
                } else {
                    squad.count
                };
                let name = sdk_name(&squad.name);
                rows.push(format!(
                    "{{name={name:?},qualified={qualified:?},object={object:?},count={count}}}",
                    qualified = format!("{name}_{:08X}", object.0),
                    object = format!("slot/{:08x}/", object.0)
                ));
            }
        }
        if complete {
            variants.push(format!(
                "{{region={},placements={{{}}}}}",
                area * 8,
                rows.join(",")
            ));
        }
    }
    if variants.is_empty() {
        return Err(invalid(
            "No native area contains this complete encounter opening",
        ));
    }
    Ok(format!("{{{}}}", variants.join(",")))
}

pub(super) fn build(
    recipe: &MapRecipe,
    source: &MapSource,
    encounters: Vec<(usize, Encounter)>,
    triggers: Vec<(usize, Triggers)>,
    package: u16,
    tags: &[NewTagSpec],
) -> AuthoringResult<String> {
    if source.encounter_gates.len() > 32
        || source
            .encounter_gates
            .iter()
            .any(|gate| gate.volumes.is_empty() || gate.scripts.is_empty())
    {
        return Err(invalid(
            "Encounter progression needs named gates and scripts within its phase limit",
        ));
    }
    let encounters = encounters
        .into_iter()
        .map(|(area, encounter)| {
            let object = object(encounter.template, encounter.key, package, tags)?;
            Ok((area, encounter, object))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let triggers = triggers
        .into_iter()
        .map(|(area, trigger)| {
            let object = object(trigger.template, trigger.key, package, tags)?;
            Ok((area, trigger, object))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let program = mission::read(&source.maps.join(&source.map))
        .map_err(|error| invalid(format!("Reading encounter scripts: {error:#}")))?;
    let mut phases = Vec::new();
    let mut evidence = Vec::new();
    let mut placed = BTreeSet::new();
    for (index, (names, scripts)) in std::iter::once((&[][..], source.encounter_starts.as_slice()))
        .chain(
            source
                .encounter_gates
                .iter()
                .map(|gate| (gate.volumes.as_slice(), gate.scripts.as_slice())),
        )
        .enumerate()
    {
        let opening = mission::encounter::read(&program, scripts).map_err(|error| {
            invalid(format!("Reading encounter phase {}: {error:#}", index + 1))
        })?;
        for placement in &opening.placements {
            if !placed.insert(placement.source.clone()) {
                return Err(invalid(
                    "Progression places a source squad in more than one phase",
                ));
            }
        }
        let mut gates = Vec::new();
        for name in names {
            let before = gates.len();
            for (area, trigger, object) in &triggers {
                if let Some(slot) = trigger.names.iter().position(|candidate| candidate == name) {
                    let alias = sdk_name(&format!("pt_{name}"));
                    gates.push(format!(
                        "{{region={},object={},key={},slot={},volume={},name={name:?},aliases={{{indexed:?},{qualified:?},{alias:?}}},id={id:?}}}",
                        area * 8,
                        object.0,
                        trigger.key,
                        slot,
                        trigger.names.len() + slot,
                        id = format!("slot/{:08x}/{slot:06x}/{slot:04x}/001f", object.0),
                        indexed = format!("{alias}_{:08X}_{slot:04X}", object.0),
                        qualified = format!("{alias}_{:08X}", object.0),
                    ));
                }
            }
            if gates.len() == before {
                return Err(invalid(format!(
                    "Source gate {name} has no native player sensor"
                )));
            }
        }
        let variants = variants(&opening, &encounters)?;
        phases.push(format!(
            "{{gates={{{}}},variants={variants}}}",
            gates.join(",")
        ));
        evidence.push(serde_json::json!({"scripts":scripts,"gates":names,"opening":opening}));
    }
    let evidence = serde_json::to_string(&serde_json::json!({
        "source_build":program.source_build,"source_sha256":program.source_metadata_sha256,"phases":evidence,
        "limits":"Explicit Normal solo encounter openings at source gates. Each selected script stops at its first wait. Later combat conditions, transport, navigation, objectives, cinematics and mission completion do not execute."
    })).map_err(|error| invalid(error.to_string()))?;
    Ok(format!(
        "-- Source evidence: {evidence}\nlocal ACTIVITY={:?}\nlocal PHASES={{{}}}\n{}",
        recipe.name.to_ascii_uppercase(),
        phases.join(",\n"),
        include_str!("progression.lua")
    ))
}
