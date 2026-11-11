//! A private carrier and the graphs it creates fly unreplicated, as the effect-only carrier and
//! stock unreplicated carriers such as `80BBB0DB` do.
//!
//! The modeled grenade carriers `80BBB917`, `80BBB91F` and `815282E1` name a replication record at
//! entity `+0x88`, and all three carry the network component `80C70EDD` that reads it. Their
//! private copies took a compiled record whose component rows hold zero in the trailing word stock
//! records fill per allocation. In game such a projectile kept one entity per weapon alive after
//! its shot ended, with its model drawn where its last round ended, while stock grenades from the
//! same carriers and Reach rounds on the unreplicated effect-only carrier took a fresh entity for
//! every shot. The private carrier and its detonation name no record and leave the network
//! component out, the only reader of `+0x88`, whose initialization faults on `FFFFFFFF`. Replication
//! alone did not end the reported Concussion Rifle body, and the compiled entity's condition words
//! were the remaining departure from stock (`tiger::entity::compiled::condition`).
use super::*;
use crate::tiger::entity::links;
use assembly::Assembly;

/// The network component's definition class.
const NETWORK: u32 = 0x80c7_0edd;

/// Leaves the replication record and every network component out of `assembly`, and reports
/// what it removed.
pub(super) fn omit(reader: &mut Reader, assembly: &mut Assembly) -> Result<Value> {
    let record = assembly.entity.u32(0x88)?;
    let (entity, network) = strip(reader, &assembly.entity, &assembly.owners)?;
    if record == u32::MAX && network.is_empty() {
        return Ok(json!({"status":"native",
            "reason":"The carrier has no replication record or network component"}));
    }
    for tag in &network {
        assembly
            .owners
            .remove(tag)
            .context("Omitted network component is absent")?;
    }
    assembly.entity = entity;
    Ok(json!({"status":"translated",
        "native_record":(record != u32::MAX).then(|| format!("{record:08X}")),
        "omitted_components":network.iter().map(|tag| format!("{tag:08X}")).collect::<Vec<_>>(),
        "reason":"A replicated private carrier kept one entity per weapon alive after its shot, so the projectile flies unreplicated as the effect-only carrier does",
        "gameplay_verified":false,
        "limits":["The projectile exists only on the firing client. Its contacts, detonation and damage are local, as for stock unreplicated carriers."]}))
}

/// `entity` rebuilt without its network components, which `owners` holds among every component's
/// payload, and with no replication record, and the tags it left out. Fails when a connection
/// reaches a network component. Endpoints outside the graph keep their rows, as when a component
/// is hosted.
pub(in crate::halo_reach) fn strip(
    reader: &mut Reader,
    entity: &Payload,
    owners: &BTreeMap<u32, Payload>,
) -> Result<(Payload, BTreeSet<u32>)> {
    let mut network = BTreeSet::new();
    for (&tag, p) in owners {
        if Assembly::class(p)? == NETWORK {
            network.insert(tag);
        }
    }
    let mut entity = if network.is_empty() {
        entity.clone()
    } else {
        without(reader, entity, owners, &network)?
    };
    put(&mut entity.0, 0x88, &u32::MAX.to_le_bytes())?;
    Ok((entity, network))
}

fn without(
    reader: &mut Reader,
    entity: &Payload,
    owners: &BTreeMap<u32, Payload>,
    omitted: &BTreeSet<u32>,
) -> Result<Payload> {
    let mut graph = links::Graph::read(entity, false)?;
    ensure!(
        !graph
            .connections
            .iter()
            .chain(&graph.named_connections)
            .any(|row| [row.consumer.object, row.provider.object]
                .into_iter()
                .flatten()
                .any(|object| omitted.contains(&object.owner))),
        "A connection reaches the carrier's network component"
    );
    graph.components.retain(|tag| !omitted.contains(tag));
    let mut endpoints = owners
        .iter()
        .filter(|(tag, _)| !omitted.contains(tag))
        .map(|(&tag, p)| (tag, p.clone()))
        .collect::<BTreeMap<_, _>>();
    for object in graph.objects() {
        if let std::collections::btree_map::Entry::Vacant(entry) = endpoints.entry(object.owner) {
            entry.insert((*reader.tag(object.owner, Some(0x80809c36))?).clone());
        }
    }
    let rows = graph.native_rows(
        &graph.components,
        &endpoints,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let components = graph
        .components
        .iter()
        .map(|tag| {
            Ok(compiled::Component {
                owner: *tag,
                payload: owners.get(tag).context("Kept component is absent")?,
                template: owners.get(tag).context("Kept component is absent")?,
                entity,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    compiled::emit(entity, &components, &rows)
}
