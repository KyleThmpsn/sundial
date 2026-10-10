//! Fit final firing graphs to the final Barrel, after donor, typed-value and technical edits.
use super::payload::{private_index, read};
use super::*;
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY, barrel_pellets, projectile_trajectory_capacity,
    weapon_component_binding_hashes,
};

const MOVEMENT: u32 = 0x0437_756D;

pub(super) fn fit(
    manager: &PackageManager,
    entity: &mut [u8],
    imported: Option<TagHash>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    if !weapon_component_binding_hashes(entity)
        .map_err(invalid)?
        .contains(&WEAPON_BARREL_COMPONENT_KEY)
    {
        return Ok(());
    }
    let barrels =
        weapon_component_bindings(entity, WEAPON_BARREL_COMPONENT_KEY).map_err(invalid)?;
    let [barrel] = barrels.as_slice() else {
        return Err(invalid(
            "Final weapon must have exactly one Barrel to determine its pellet count",
        ));
    };
    let owner = read(manager, barrel.owner_tag, allocator, tags)?;
    let pellets = barrel_pellets(&owner, *barrel)
        .map_err(|error| invalid(format!("Final Barrel: {error}")))?;
    if pellets <= 1 {
        return Ok(());
    }
    let mut content =
        crate::weapon::behavior::content_with(entity, |tag| read(manager, tag, allocator, tags))?;
    let mut graphs = BTreeMap::<u32, Vec<usize>>::new();
    // A perk can select another variant. Each firing slot in this private content owner must fit.
    for &block in &content.blocks {
        let slot = block + 0xF0;
        let graph = read_u32(&content.owner, slot)?;
        if graph != 0 && graph != u32::MAX {
            graphs.entry(graph).or_default().push(slot);
        }
    }
    let mut changed = false;
    for (graph, slots) in graphs {
        if imported.is_some_and(|tag| tag.0 == graph) {
            return Err(invalid(
                "Pellet barrels with imported firing graphs are not supported by the final trajectory-pool check.",
            ));
        }
        let private = fit_graph(manager, graph, pellets, allocator, tags)
            .map_err(|error| invalid(format!("Final firing graph 0x{graph:08X}: {error}")))?;
        if private.0 != graph {
            for slot in slots {
                write_u32(&mut content.owner, slot, private.0)?;
            }
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    if let Some(index) = private_index(content.owner_tag, allocator, tags)? {
        tags[index].payload = content.owner;
    } else {
        let private = allocator.assigned_tag(
            tags.len(),
            "Final weapon content",
            "runtime component owner",
        )?;
        retarget_weapon_component_owner_payload(
            &mut content.owner,
            entity,
            content.owner_tag,
            private.0,
        )
        .map_err(invalid)?;
        retarget_weapon_component_owner(entity, content.owner_tag, private.0).map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(content.owner_tag),
            payload: content.owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(())
}

/// The graph's one Projectile Movement resource, which must hold a trajectory pool.
fn trajectory_projectile(
    graph: &[u8],
) -> AuthoringResult<sundial::package_authoring::entity::WeaponComponentBinding> {
    let projectiles = weapon_component_bindings(graph, MOVEMENT).map_err(invalid)?;
    let [projectile] = projectiles.as_slice() else {
        return Err(invalid(
            "Multiple Projectile Movement resources make pellet capacity ambiguous",
        ));
    };
    if projectile.concrete_class != 0x8080_3B73 {
        return Err(invalid(
            "Projectile Movement has an unsupported trajectory-pool layout",
        ));
    }
    Ok(*projectile)
}

fn fit_graph(
    manager: &PackageManager,
    graph: u32,
    pellets: u16,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let mut payload = read(manager, graph, allocator, tags)?;
    if !weapon_component_binding_hashes(&payload)
        .map_err(invalid)?
        .contains(&MOVEMENT)
    {
        return Ok(TagHash(graph));
    }
    let projectile = trajectory_projectile(&payload)?;
    let mut owner = read(manager, projectile.owner_tag, allocator, tags)?;
    let instance = usize::try_from(projectile.resource_offset)
        .map_err(|_| invalid("Projectile offset overflow"))?;
    let capacity =
        projectile_trajectory_capacity(&owner, projectile.owner_tag, instance).map_err(invalid)?;
    if capacity >= usize::from(pellets) {
        return Ok(TagHash(graph));
    }
    let Some(index) = private_index(graph, allocator, tags)? else {
        return append_private_graph_edit(
            manager,
            TagHash(graph),
            &[],
            &BTreeSet::new(),
            Some(pellets),
            allocator,
            tags,
        );
    };
    // This edited owner may also serve another graph. Give this graph its own grown copy,
    // preserving the existing values without moving objects under any other references.
    let template = private_index(projectile.owner_tag, allocator, tags)?
        .map_or(TagHash(projectile.owner_tag), |index| {
            tags[index].template_tag
        });
    sundial::package_authoring::entity::grow_projectile_trajectories(
        &mut payload,
        projectile.owner_tag,
        &mut owner,
        instance,
        usize::from(pellets),
    )
    .map_err(invalid)?;
    let private = allocator.assigned_tag(
        tags.len(),
        "Final projectile owner",
        "runtime component owner",
    )?;
    retarget_weapon_component_owner_payload(&mut owner, &payload, projectile.owner_tag, private.0)
        .map_err(invalid)?;
    retarget_weapon_component_owner(&mut payload, projectile.owner_tag, private.0)
        .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: template,
        payload: owner,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    tags[index].payload = payload;
    Ok(TagHash(graph))
}
