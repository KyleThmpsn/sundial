//! Source graph inspection with component occurrences, provider dispatch and complete reached
//! owner payloads. This is a storage export, not a gameplay or animation graph converter.
use super::*;
use crate::tiger::entity::links::{Graph, Interface, Object};
use std::collections::{BTreeMap, BTreeSet};

fn export(source: &mut Reader, tag: u32, class: u32, directory: &Path) -> Result<Payload> {
    let payload = source.tag(tag, Some(class))?;
    let file = directory.join(format!("{tag:08X}.bin"));
    if file.exists() {
        ensure!(
            fs::read(&file)? == payload.0,
            "Runtime source payload changed during inspection"
        );
    } else {
        fs::write(file, &payload.0)?;
    }
    Ok((*payload).clone())
}

/// Export explicitly selected Marathon entities, their component owners and every connected
/// provider's dispatch metadata. Local component ordinals are checked independently of owner
/// identity, so repeated component templates retain their separate occurrences. External
/// selectors and absent objects remain explicit. The caller owns selection of the entity set.
pub fn inspect(source: &mut Reader, entities: &[u32], directory: &Path) -> Result<Value> {
    ensure!(
        !entities.is_empty(),
        "Required runtime entity corpus is empty"
    );
    ensure!(
        entities.iter().collect::<BTreeSet<_>>().len() == entities.len(),
        "Runtime entity selection contains duplicates"
    );
    let directory = outside(
        directory,
        source.packages().parent().context("Source root")?,
    )?;
    ensure!(
        !directory.exists(),
        "Use a fresh runtime inspection directory"
    );
    fs::create_dir_all(&directory)?;
    let mut reports = Vec::new();
    for &entity in entities {
        crate::cancellation::check()?;
        let payload = export(source, entity, 0x8080BAAD, &directory)?;
        let graph = Graph::read_marathon(&payload)
            .with_context(|| format!("Marathon entity {entity:08X}"))?;
        let components = animation::components(source, entity)?;
        let animation_driver = if components
            .iter()
            .any(|owner| owner.3 == animation::driver::CLASS)
        {
            animation::driver::Driver::read(source, entity, &components)?.report
        } else {
            Value::Null
        };
        let mut owners = BTreeMap::new();
        let mut reached = BTreeSet::from([entity]);
        for owner in graph
            .components
            .iter()
            .copied()
            .chain(graph.objects().map(|o| o.owner))
        {
            if let std::collections::btree_map::Entry::Vacant(entry) = owners.entry(owner) {
                entry.insert(export(source, owner, 0x8080BADB, &directory)?);
                reached.insert(owner);
            }
        }
        graph.validate_owners(&owners)?;
        let mut interfaces = Vec::new();
        let objects = graph
            .connections
            .iter()
            .chain(&graph.named_connections)
            .filter_map(|c| c.provider.object)
            .collect::<BTreeSet<Object>>();
        for object in objects {
            let owner = &owners[&object.owner];
            let at = usize::try_from(object.offset)?;
            let metadata_tag = owner.u32(at + 8)?;
            let metadata = export(source, metadata_tag, 0x8080BAF8, &directory)?;
            reached.insert(metadata_tag);
            let dispatch =
                Interface::read_marathon(owner, object, &metadata).with_context(|| {
                    format!(
                        "Marathon provider {:08X}:{:08X}+{:X}",
                        object.owner, object.class, object.offset
                    )
                })?;
            interfaces.push(json!({"object":object,"dispatch":dispatch}));
        }
        reports.push(json!({"entity":entity,"graph":graph,"interfaces":interfaces,"animation_driver":animation_driver,
            "payloads":reached.into_iter().map(|tag| json!({"tag":tag,"file":format!("{tag:08X}.bin")})).collect::<Vec<_>>() }));
    }
    Ok(
        json!({"revision":2,"entities":reports,"evidence":"source_package_readback",
        "limits":"Typed source references and interface dispatch records only. External selector resolution, source methods, state activation, target publication and gameplay remain unverified."}),
    )
}
