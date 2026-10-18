//! Discover attachment dependencies through declared references and typed actions.
use super::*;
use crate::d2_mot::{
    gameplay::perks::{controller as action, lower, translate},
    native::effects::resources::{self, selectors},
};

pub struct Resource {
    pub class: u32,
    pub payload: Payload,
}

/// Planning retains original payloads. Every component occurrence is allocated
/// separately when its enclosing graph is emitted, even when owners are shared.
#[derive(Default)]
pub struct Closure {
    pub graphs: BTreeMap<u32, Payload>,
    pub owners: BTreeMap<u32, Payload>,
    pub resources: BTreeMap<u32, Resource>,
    pub actions: BTreeMap<u32, Payload>,
    pub references: BTreeMap<u32, BTreeMap<usize, u32>>,
    pub parameters: BTreeMap<u32, BTreeMap<u32, [u8; 16]>>,
    pub particles: BTreeSet<u32>,
    pub sounds: BTreeSet<u32>,
}

fn wide(reader: &Reader, payload: &Payload, field: usize) -> Result<Option<u32>> {
    let tag = payload.u32(field)?;
    let form = payload.u32(field + 4)?;
    let hash = payload.u64(field + 8)?;
    if matches!(tag, 0 | u32::MAX) && form <= 2 && hash == 0 {
        return Ok(None);
    }
    let tag = reader.ref64(payload, field)?;
    reader.reference(tag)?;
    Ok(Some(tag))
}

/// Modern owners enumerate their wide reference fields in header +60. These
/// pointer rows are declarations, unlike an aligned-word search for tag values.
fn declared(reader: &Reader, payload: &Payload) -> Result<BTreeMap<usize, u32>> {
    let mut fields = BTreeSet::new();
    let mut references = BTreeMap::new();
    for row in payload.array(0x60, 8, Some(0x8080906E))? {
        let field = payload.pointer(row)?;
        ensure!(
            fields.insert(field),
            "owner declares one resource field twice"
        );
        if let Some(tag) = wide(reader, payload, field)? {
            references.insert(field, tag);
        }
    }
    let definition = payload.pointer(24)?;
    let mut direct = |field: usize, class: u32| -> Result<()> {
        let tag = payload.u32(field)?;
        if matches!(tag, 0 | u32::MAX) {
            return Ok(());
        }
        ensure!(
            reader.reference(tag)? == class,
            "declared direct attachment dependency class differs"
        );
        if let Some(previous) = references.insert(field, tag) {
            ensure!(
                previous == tag,
                "direct and wide attachment references disagree"
            );
        }
        Ok(())
    };
    match payload.u32(definition - 4)? {
        0x80803FFC => {
            for section in payload.array(definition + 0x210, 368, Some(0x80804091))? {
                direct(section + 0x160, resources::slot::MODERN_CLASS)?;
            }
        }
        0x80808179 => {
            for row in payload.array(definition + 0x1D8, 24, Some(0x808091F1))? {
                let event = payload.pointer(row + 16)?;
                match payload.u32(event - 4)? {
                    0x808066AD => direct(event + 0x24, 0x80806CEF)?,
                    0x808091CF => direct(event + 0x28, 0x80806BB7)?,
                    0x80806A52 => direct(event + 0x24, 0x80806C71)?,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(references)
}

fn hash_map(reader: &Reader) -> BTreeMap<u64, u32> {
    reader
        .manager
        .lookup
        .tag64_entries
        .iter()
        .map(|(&hash, entry)| (hash, entry.hash32.0))
        .collect()
}

fn condition_resources(
    payload: &Payload,
    condition: &action::Condition,
    resolver: &selectors::ResourceResolver,
    output: &mut BTreeSet<u32>,
) -> Result<()> {
    if condition.class == 0x8080B7B3 {
        output.extend(selectors::inspect_resources(
            payload,
            condition.offset + 0x138,
            resolver,
        )?);
    }
    for child in &condition.children {
        condition_resources(payload, child, resolver, output)?;
    }
    Ok(())
}

fn action_resources(
    reader: &Reader,
    payload: &Payload,
    resolver: &selectors::ResourceResolver,
    parameters: &mut BTreeMap<u32, BTreeMap<u32, [u8; 16]>>,
) -> Result<BTreeSet<u32>> {
    let parsed = action::read(payload)?;
    let mut output = BTreeSet::new();
    for state in &parsed.states {
        for transition in &state.transitions {
            for condition in &transition.conditions {
                condition_resources(payload, condition, resolver, &mut output)?;
            }
        }
        for at in translate::state_effects(payload, state)? {
            match payload.u32(at - 4)? {
                0x8080B7C6 => {
                    let attachment = lower::attachment::read(reader, payload, at)?;
                    if let Some(previous) =
                        parameters.insert(attachment.graph, attachment.parameters.clone())
                    {
                        ensure!(
                            previous == attachment.parameters,
                            "shared attachment graph has differing creation parameters"
                        );
                    }
                    output.insert(attachment.graph);
                }
                0x80803128 => {
                    output.insert(lower::spawn::read(reader, payload, at)?.graph);
                }
                // This lowerer checks both selectors are empty and admits no
                // attachment-bearing assignment lists.
                0x8080B4E1 => {
                    lower::event_modifier(payload, at)?;
                }
                class => anyhow::bail!(
                    "attachment controller effect {class:08X} requires dependency discovery"
                ),
            }
        }
    }
    Ok(output)
}

pub fn read(reader: &mut Reader, roots: &[u32]) -> Result<Closure> {
    ensure!(!roots.is_empty(), "attachment closure has no roots");
    let hashes = hash_map(reader);
    let resolver = selectors::ResourceResolver {
        hashes: hashes.clone(),
        source_classes: reader
            .classes(selectors::MODERN_CLASS)
            .into_iter()
            .map(|tag| (tag, selectors::MODERN_CLASS))
            .collect(),
        native_classes: BTreeMap::new(),
    };
    let mut output = Closure::default();
    let mut pending = roots.iter().copied().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    while let Some(tag) = pending.pop_first() {
        if !seen.insert(tag) {
            continue;
        }
        ensure!(
            seen.len() <= 16384,
            "attachment dependency closure exceeds capacity"
        );
        let class = reader.reference(tag)?;
        let payload = (*reader.tag(tag, Some(class))?).clone();
        match class {
            0x80809AD8 => {
                let graph = Graph::read(&payload, true)?;
                ensure!(
                    !graph.components.is_empty(),
                    "source attachment graph is empty"
                );
                pending.extend(&graph.components);
                output.graphs.insert(tag, payload);
            }
            0x80809B06 => {
                let references = declared(reader, &payload)?;
                pending.extend(references.values());
                output.references.insert(tag, references);
                let definition = payload.pointer(24)?;
                if payload.u32(definition - 4)? == 0x80808179 {
                    event_resources(reader, &payload, &mut output.particles, &mut output.sounds)?;
                }
                output.owners.insert(tag, payload);
            }
            0x8080B835 => {
                pending.extend(action_resources(
                    reader,
                    &payload,
                    &resolver,
                    &mut output.parameters,
                )?);
                output.actions.insert(tag, payload);
            }
            resources::MODERN_CLASS => {
                pending.extend(
                    resources::source_references(&payload, &hashes)?
                        .into_iter()
                        .map(|row| row.source),
                );
                output.resources.insert(tag, Resource { class, payload });
            }
            resources::slot::MODERN_CLASS => {
                let references = resources::slot::source_references(&payload, &hashes)?;
                for reference in &references {
                    if reference.offset <= 20 {
                        let expected = if reference.offset == 20 {
                            resources::MODERN_CLASS
                        } else {
                            resources::slot::MODERN_CLASS
                        };
                        ensure!(
                            reader.reference(reference.source)? == expected,
                            "material slot inheritance or response class differs"
                        );
                    }
                }
                pending.extend(references.into_iter().map(|row| row.source));
                output.resources.insert(tag, Resource { class, payload });
            }
            selectors::MODERN_CLASS => {
                pending.extend(selectors::inspect_resources(&payload, 8, &resolver)?);
                output.resources.insert(tag, Resource { class, payload });
            }
            0x80809738 => {
                output.sounds.insert(tag);
                output.resources.insert(tag, Resource { class, payload });
            }
            0x808031DE | 0x808031D8 | 0x808031C2 | 0x808031D3 | 0x808031C7 => {
                let parsed = resources::programs::emit(&payload, class, &BTreeMap::new())?;
                pending.extend(
                    parsed
                        .references
                        .iter()
                        .map(|row| row.source)
                        .filter(|tag| !matches!(*tag, 0 | u32::MAX)),
                );
                output.resources.insert(tag, Resource { class, payload });
            }
            // Metadata and presentation resources remain classified. Assembly
            // must convert every used reference or record a qualified omission.
            _ => {
                output.resources.insert(tag, Resource { class, payload });
            }
        }
    }
    for graph in output.graphs.values() {
        Graph::read(graph, true)?.validate_owners(&output.owners)?;
    }
    Ok(output)
}
