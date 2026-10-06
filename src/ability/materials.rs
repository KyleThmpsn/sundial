//! The render materials an entity's component owners reach through declared references, other
//! than through particle systems, which `palette::particle_sites` reads: models, lights, lens
//! flares, decals and other effect resources each name the materials they draw with. A route runs
//! from a word in a bound owner resource through the resources between, each naming the next at
//! declared fields, to a material. Copying every resource on a route, each naming the next copy,
//! gives the entity a private material where the stock one was.
use std::collections::{BTreeMap, BTreeSet};

use super::palette::{MATERIAL_CLASS, PARTICLE_SYSTEM_CLASS};
use super::spawns::is_table;
use crate::entity::{
    WEAPON_ENTITY_CLASS, weapon_component_binding_hashes, weapon_component_bindings,
};
use crate::package_runtime::reader::PackageManager;
use crate::package_runtime::references::declared_fields;

/// The most resources a route holds, the material among them.
const ROUTE_LENGTH: usize = 4;

/// One way an entity's component owner reaches a render material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialRoute {
    pub owner: u32,
    pub binding_hash: u32,
    pub resource_index: u16,
    /// Byte offset of the first resource's tag from the start of its owner resource.
    pub offset: u32,
    /// The resources from the owner's word to the material, the material last. Each but the last
    /// comes with the declared fields of its payload that name the next.
    pub chain: Vec<(u32, Vec<usize>)>,
}

impl MaterialRoute {
    /// The material it reaches.
    #[must_use]
    pub fn material(&self) -> u32 {
        self.chain.last().map_or(0, |(tag, _)| *tag)
    }
}

/// Every route from `entity`'s component owners to a render material, other than those through
/// particle systems, entity graphs and impact tables. A route passes only through type-8
/// resources, so no loading owner with a companion of its own is copied.
pub fn material_routes(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<MaterialRoute>, String> {
    let mut resources = BTreeMap::<u32, Vec<(u64, u32, u16)>>::new();
    for binding in weapon_component_binding_hashes(entity)? {
        for resource in weapon_component_bindings(entity, binding)? {
            let index = u16::try_from(resource.resource_index)
                .map_err(|_| "A component binding selects too many resources".to_owned())?;
            resources.entry(resource.owner_tag).or_default().push((
                resource.resource_offset,
                binding,
                index,
            ));
        }
    }
    let mut fields = BTreeMap::<u32, Vec<(usize, u32)>>::new();
    let mut routes = Vec::new();
    for (owner, mut starts) in resources {
        starts.sort_unstable();
        starts.dedup_by_key(|(start, ..)| *start);
        for (at, tag) in declared(manager, &mut fields, owner)? {
            if tag == entity_tag || tag == owner {
                continue;
            }
            // A word before the owner's first resource belongs to no binding a patch can name.
            let Some(&(start, binding_hash, resource_index)) =
                starts.iter().rev().find(|(start, ..)| *start <= at as u64)
            else {
                continue;
            };
            let offset = u32::try_from(at as u64 - start)
                .map_err(|_| "A material route starts too far into its resource".to_owned())?;
            let mut found = Vec::new();
            follow(manager, &mut fields, (tag, Vec::new()), &mut found)?;
            routes.extend(found.into_iter().map(|chain| MaterialRoute {
                owner,
                binding_hash,
                resource_index,
                offset,
                chain,
            }));
        }
    }
    Ok(routes)
}

/// `tag`'s declared reference fields, read once.
fn declared(
    manager: &PackageManager,
    fields: &mut BTreeMap<u32, Vec<(usize, u32)>>,
    tag: u32,
) -> Result<Vec<(usize, u32)>, String> {
    if let Some(found) = fields.get(&tag) {
        return Ok(found.clone());
    }
    let found = declared_fields(manager, tag)?;
    fields.insert(tag, found.clone());
    Ok(found)
}

/// Every chain from `tag`, with the resources before it in `chain`, to a material.
fn follow(
    manager: &PackageManager,
    fields: &mut BTreeMap<u32, Vec<(usize, u32)>>,
    (tag, chain): (u32, Vec<(u32, Vec<usize>)>),
    found: &mut Vec<Vec<(u32, Vec<usize>)>>,
) -> Result<(), String> {
    let Some(entry) = manager.get_entry(tag) else {
        return Ok(());
    };
    if entry.file_type != 8 || chain.iter().any(|(each, _)| *each == tag) {
        return Ok(());
    }
    if entry.reference == MATERIAL_CLASS {
        let mut route = chain;
        route.push((tag, Vec::new()));
        found.push(route);
        return Ok(());
    }
    if matches!(entry.reference, WEAPON_ENTITY_CLASS | PARTICLE_SYSTEM_CLASS)
        || is_table(manager, tag)
        || chain.len() + 2 > ROUTE_LENGTH
    {
        return Ok(());
    }
    let mut next = BTreeMap::<u32, Vec<usize>>::new();
    for (at, child) in declared(manager, fields, tag)? {
        if child != tag {
            next.entry(child).or_default().push(at);
        }
    }
    let seen = chain.iter().map(|(each, _)| *each).collect::<BTreeSet<_>>();
    for (child, offsets) in next {
        if seen.contains(&child) {
            continue;
        }
        let mut longer = chain.clone();
        longer.push((tag, offsets));
        follow(manager, fields, (child, longer), found)?;
    }
    Ok(())
}
