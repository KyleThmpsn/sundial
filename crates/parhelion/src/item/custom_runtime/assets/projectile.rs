//! Fit a private native projectile to its host's launch input.
use super::*;
use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{Kind, Stored};

pub(in crate::item) fn prepare<M>(
    nodes: &mut [Node<M>],
    symbol: &str,
    manager: &PackageManager,
    host: &[u8],
    group: u32,
    boost: f32,
) -> AuthoringResult<()> {
    let host_graph = crate::weapon::behavior::fired_graph(manager, host, group, &[])?;
    if host_graph.is_some_and(|graph| crate::weapon::behavior::launches_its_own(manager, graph)) {
        return Ok(());
    }
    let root = nodes
        .iter()
        .find(|node| node.symbol == symbol)
        .ok_or_else(|| invalid("Private projectile root is missing"))?;
    let bindings = weapon_component_bindings(&root.payload, 0x0437_756D).map_err(invalid)?;
    let (_, _, rows, class) = array_at(&root.payload, 16)?;
    if class != 0x8080_9C04 {
        return Err(invalid("Private projectile component array differs"));
    }
    let mut edits = BTreeMap::new();
    for binding in bindings {
        if binding.concrete_class != 0x8080_3B73 {
            continue;
        }
        let owner = replication::target(nodes, root, rows + binding.component_index * 12)?;
        let instance = usize::try_from(binding.resource_offset)
            .map_err(|_| invalid("Private projectile instance offset overflow"))?;
        // This reference must stay private when symbolic addresses are assigned.
        let definition_owner = replication::target(nodes, owner, instance)?;
        if definition_owner.symbol != owner.symbol {
            return Err(invalid(
                "Private projectile definition is outside its private owner",
            ));
        }
        let parameter =
            Stored::read(Kind::Speed, &owner.payload, u32::MAX, instance).map_err(invalid)?;
        let raised = crate::weapon::behavior::raised_speed(parameter.original(), boost);
        if raised > parameter.original() {
            let payload = edits
                .entry(owner.symbol.clone())
                .or_insert_with(|| owner.payload.clone());
            parameter.write(payload, raised).map_err(invalid)?;
        }
    }
    for node in nodes.iter_mut() {
        if let Some(payload) = edits.remove(&node.symbol) {
            node.payload = payload;
        }
    }
    Ok(())
}
