//! The private resources of a translated effect graph, with their pending symbol patches.
use super::*;

pub(super) struct Emitted {
    /// Every component's temporary or retained stock identity, in source order.
    pub(super) ids: Vec<u32>,
    /// The graph first, then its replication resource, then each private owner's allocation
    /// before the owner itself.
    pub(super) pending: Vec<Pending>,
    /// Each component's translation evidence.
    pub(super) owners: Vec<Value>,
}

/// Lower the rows and emit the graph, its replication resource and every private owner.
pub(super) fn resources(
    template: &Template,
    components: Vec<Component>,
    rows: Vec<links::Connection>,
    symbol: &str,
) -> Result<Emitted> {
    let ids = components
        .iter()
        .map(|component| component.id)
        .collect::<Vec<_>>();
    let mut pending = graph(template, &components, &ids, rows, symbol)?;
    let evidence = owners(components, &mut pending)?;
    Ok(Emitted {
        ids,
        pending,
        owners: evidence,
    })
}

/// Retained stock owners keep their stock tags. Only private owners take symbols.
fn private(component: &Component) -> bool {
    !component.symbol.is_empty()
}

/// The private graph and, when the template has one, its replication resource, graph first.
/// A template without a replication resource, such as a created presentation graph, gives its
/// private copy none either.
fn graph(
    template: &Template,
    components: &[Component],
    ids: &[u32],
    rows: Vec<links::Connection>,
    symbol: &str,
) -> Result<Vec<Pending>> {
    let payloads = components
        .iter()
        .map(|component| (component.id, component.payload.clone()))
        .collect::<BTreeMap<_, _>>();
    let lowered = links::Graph {
        components: ids.to_vec(),
        connections: rows,
        named_connections: Vec::new(),
    }
    .native_rows(
        ids,
        &payloads,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let parts = components
        .iter()
        .map(|component| compiled::Component {
            owner: component.id,
            payload: &component.payload,
            template: &component.template,
            entity: &component.entity,
        })
        .collect::<Vec<_>>();
    let emitted = compiled::emit_authored(&template.graph, &parts, &lowered, &BTreeMap::new())?;
    let mut graph_patches = Vec::new();
    for component in components.iter().filter(|component| private(component)) {
        graph_patches.extend(words(&emitted, component.id, &component.symbol)?);
    }
    let mut pending = Vec::new();
    if template.graph.u32(0x88)? != u32::MAX {
        let replication =
            compiled::replication::emit_authored(template.tag, &parts, &BTreeMap::new())?;
        let replication_symbol = format!("{symbol}-replication");
        graph_patches.insert(0, (0x88, replication_symbol.clone()));
        let mut replication_patches = vec![(8, symbol.to_owned())];
        for (component, row) in
            components
                .iter()
                .zip(replication.array(0x30, 32, Some(0x80809BB8))?)
        {
            if component.allocation.is_some() {
                replication_patches.push((row + 4, format!("{}-allocation", component.symbol)));
            }
        }
        pending.push(Pending {
            symbol: replication_symbol,
            template: template.graph.u32(0x88)?,
            payload: replication,
            patches: replication_patches,
        });
    }
    pending.insert(
        0,
        Pending {
            symbol: symbol.to_owned(),
            template: template.tag,
            payload: emitted,
            patches: graph_patches,
        },
    );
    Ok(pending)
}

/// Append each private owner, after its allocation when it has a private one, and return every
/// component's evidence in source order.
fn owners(components: Vec<Component>, pending: &mut Vec<Pending>) -> Result<Vec<Value>> {
    let mut evidence = Vec::new();
    for component in components {
        if !private(&component) {
            evidence.push(
                json!({"ordinal":component.ordinal,"family":component.family.name(),
                "translation":component.evidence}),
            );
            continue;
        }
        let mut patches = component.patches;
        patches.extend(words(&component.payload, component.id, &component.symbol)?);
        if let Some((allocation_template, allocation)) = component.allocation {
            let allocation_symbol = format!("{}-allocation", component.symbol);
            patches.push((0x44, allocation_symbol.clone()));
            pending.push(Pending {
                symbol: allocation_symbol,
                template: allocation_template,
                payload: allocation,
                patches: Vec::new(),
            });
        }
        evidence.push(
            json!({"ordinal":component.ordinal,"family":component.family.name(),
            "symbol":component.symbol,"template":component.template_tag,
            "translation":component.evidence}),
        );
        pending.push(Pending {
            symbol: component.symbol,
            template: component.template_tag,
            payload: component.payload,
            patches,
        });
    }
    Ok(evidence)
}
