//! Armed vehicle barrel inputs and checked firing graph substitutions.
use super::{Projectile, Weapons, values};
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{read_u16, read_u32, write_u32},
};
use std::collections::BTreeSet;
use sundial::package_authoring::{
    PackageManager, entity::validate_weapon_entity, runtime::load_weapon_runtime_graph_for_entity,
};
use tiger_pkg::TagHash;

const ENTITY: u32 = 0x8080_9C0F;
const BARREL: u32 = 0x8080_3865;
// Package references at Barrel Data +0xB80/+0xB90 name the normal and alternate firing
// graphs. The later +0x1920/+0x1960 resources are shared response policies, not projectiles.
const FIRING_LANES: [usize; 2] = [0xB80, 0xB90];

fn checked_projectile(manager: &PackageManager, tag: u32) -> AuthoringResult<()> {
    if manager
        .get_entry(TagHash(tag))
        .is_none_or(|entry| entry.reference != ENTITY)
    {
        return Err(invalid(format!(
            "Projectile 0x{tag:08X} is not a live entity graph"
        )));
    }
    let payload = manager
        .read_tag(TagHash(tag))
        .map_err(|e| invalid(format!("Projectile 0x{tag:08X}: {e}")))?;
    validate_weapon_entity(&payload).map_err(invalid)?;
    let graph =
        load_weapon_runtime_graph_for_entity(manager, 0, 0, tag, &payload).map_err(invalid)?;
    if read_u16(&payload, 0x96)? != 18
        || !graph
            .resources
            .iter()
            .any(|r| r.instance.schema == 0x8080_3B73)
    {
        return Err(invalid(format!(
            "Projectile 0x{tag:08X} requires a moving projectile firing graph"
        )));
    }
    Ok(())
}

pub(super) fn projectile(
    manager: &PackageManager,
    choice: &Projectile,
) -> AuthoringResult<Option<u32>> {
    if *choice == Projectile::Stock {
        return Ok(None);
    }
    if let Projectile::Other { entity } = choice {
        let tag = entity.parse_u32().map_err(|e| invalid(e.to_string()))?;
        checked_projectile(manager, tag)?;
        return Ok(Some(tag));
    }
    let source = choice
        .donor()
        .and_then(|s| s.entity().ok().flatten())
        .ok_or_else(|| invalid("Projectile choice has no vehicle donor"))?;
    let payload = manager
        .read_tag(TagHash(source))
        .map_err(|e| invalid(format!("Projectile donor: {e}")))?;
    let graph =
        load_weapon_runtime_graph_for_entity(manager, 0, 0, source, &payload).map_err(invalid)?;
    let mut choices = BTreeSet::new();
    for resource in &graph.resources {
        let Some(root) = resource.definition.as_ref().filter(|r| r.schema == BARREL) else {
            continue;
        };
        if root.byte_size != 0x1DC0 {
            return Err(invalid(
                "Projectile donor has an unsupported barrel definition layout",
            ));
        }
        let owner = manager
            .read_tag(TagHash(resource.owner_tag))
            .map_err(|e| invalid(e.to_string()))?;
        for lane in FIRING_LANES {
            let tag = read_u32(&owner, root.owner_offset as usize + lane)?;
            if !matches!(tag, 0 | u32::MAX | 0x811C_9DC5) {
                choices.insert(tag);
            }
        }
    }
    let choices = choices.into_iter().collect::<Vec<_>>();
    let [tag] = choices.as_slice() else {
        return Err(invalid(
            "This projectile donor has no single primary firing graph. Select Other Projectile with its native graph tag.",
        ));
    };
    let tag = *tag;
    checked_projectile(manager, tag)?;
    Ok(Some(tag))
}

pub(super) fn tune(
    payload: &mut Vec<u8>,
    roots: impl IntoIterator<Item = usize>,
    settings: &Weapons,
    projectile: Option<u32>,
) -> AuthoringResult<()> {
    for root in roots {
        let rate = f32::from(settings.firing_rate_percent) / 100.0;
        // CC2120 initializes 74 numeric inputs at definition +0x10, stride 0x20.
        // Input identities follow the stock modifier and stat-translation contracts.
        for (input, factor, label) in [
            (0, rate, "Firing Rate"),
            (1, rate, "Firing Rate"),
            (2, rate, "Firing Rate"),
            (3, rate, "Firing Rate"),
            (4, rate.recip(), "Firing Rate"),
            (5, rate.recip(), "Firing Rate"),
            (
                32,
                f32::from(settings.damage_percent) / 100.0,
                "Weapon Damage",
            ),
        ] {
            let config = root + 0x10 + input * 0x20;
            values::scale_number(payload, config, factor, label)?;
            values::scale_float(payload, config + 12, factor, label)?;
        }
        if let Some(projectile) = projectile {
            let mut replaced = false;
            for lane in FIRING_LANES {
                let at = root + lane;
                if !matches!(read_u32(payload, at)?, 0 | u32::MAX | 0x811C_9DC5) {
                    write_u32(payload, at, projectile)?;
                    replaced = true;
                }
            }
            if !replaced {
                return Err(invalid(
                    "Projectile swaps require the vehicle barrel's original firing graph",
                ));
            }
        }
    }
    Ok(())
}
