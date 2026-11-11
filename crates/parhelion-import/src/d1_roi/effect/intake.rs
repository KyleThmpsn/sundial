//! The source effect graph and its owners, read and checked before any native template.
use super::*;

/// A source owner: its ordinal in the source component list, its tag, its payload and its
/// family. An owner without a translation has no family.
pub(super) type Owner = (usize, u32, Payload, Option<Family>);

pub(super) struct Intake {
    pub(super) entity: Payload,
    pub(super) graph: connections::Graph,
    pub(super) owners: Vec<Owner>,
    /// Every translated owner family. Each is held by one owner, except sequences.
    pub(super) families: BTreeSet<Family>,
}

/// Read source entity graph `tag`, which has no named connections or condition state, and
/// classify each of its owners.
pub(super) fn read(source: &Source, tag: u32) -> Result<Intake> {
    ensure!(
        source.reference(tag)? == 0x80800734,
        "Source effect is not an entity graph"
    );
    let entity = source.read(tag)?;
    let graph = connections::Graph::read(&entity)?;
    ensure!(
        graph.named_connections.is_empty()
            && entity
                .array(0x88, 2, Some(0x80800006))?
                .iter()
                .all(|&at| entity.u16(at).ok() == Some(0)),
        "Source effect graph has named connections or condition state"
    );
    let mut owners = Vec::new();
    for (ordinal, &owner) in graph.components.iter().enumerate() {
        ensure!(
            source.reference(owner)? == 0x80800861,
            "Source effect owner class differs"
        );
        let payload = source.read(owner)?;
        let family = source_family(source, &payload)?;
        owners.push((ordinal, owner, payload, family));
    }
    let families = owners
        .iter()
        .filter_map(|(_, _, _, family)| *family)
        .collect::<BTreeSet<_>>();
    for family in &families {
        ensure!(
            *family == Family::Sequence
                || owners
                    .iter()
                    .filter(|owner| owner.3 == Some(*family))
                    .count()
                    == 1,
            "Source effect graph repeats its {} owner",
            family.name()
        );
    }
    Ok(Intake {
        entity,
        graph,
        owners,
        families,
    })
}

impl Intake {
    /// The graph's one modifier bank, when no connection row names it. Such a bank reads and
    /// writes nothing in the graph, so it can live in a graph of its own.
    pub(super) fn unconnected_modifier(&self) -> Option<u32> {
        let banks = self
            .owners
            .iter()
            .filter(|owner| owner.3 == Some(Family::Modifier))
            .collect::<Vec<_>>();
        let [(ordinal, owner, _, _)] = banks.as_slice() else {
            return None;
        };
        self.graph
            .connections
            .iter()
            .all(|row| {
                row.consumer.selector != *ordinal as u64 && row.provider.selector != *ordinal as u64
            })
            .then_some(*owner)
    }

    /// Leave `owner` out of the translated graph, as an owner without a family is.
    pub(super) fn detach(&mut self, owner: u32) {
        for item in &mut self.owners {
            if item.1 == owner {
                item.3 = None;
            }
        }
        self.families = self
            .owners
            .iter()
            .filter_map(|(_, _, _, family)| *family)
            .collect();
    }

    /// Owners the source leaves unconnected are left out only when no row names them.
    pub(super) fn check_unconnected(&self) -> Result<()> {
        for (ordinal, _, payload, family) in &self.owners {
            let class = payload.u32(payload.pointer(24)? - 4)?;
            if family.is_none() && UNCONNECTED.iter().any(|(c, _)| *c == class) {
                ensure!(
                    self.graph.connections.iter().all(|row| {
                        row.consumer.selector != *ordinal as u64
                            && row.provider.selector != *ordinal as u64
                    }),
                    "Source owner class {class:08X} is connected and has no translation"
                );
            }
        }
        Ok(())
    }
}
