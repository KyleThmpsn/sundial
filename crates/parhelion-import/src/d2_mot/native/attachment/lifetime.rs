//! Prove duration-refresh inputs from the actual connected source providers.
use super::*;

/// Both cleanup callbacks ask every attached sequence for definition flag 02.
/// A graph carrying that flag is retained regardless of its Duration state or
/// the action's parent comparison. Sequence emission preserves the same flag.
pub(super) fn sequence_retention(closure: &closure::Closure) -> Result<BTreeSet<u32>> {
    let mut retained = BTreeSet::new();
    for (&tag, payload) in &closure.graphs {
        let graph = Graph::read(payload, true)?;
        for owner in graph.components {
            let p = closure
                .owners
                .get(&owner)
                .context("cleanup sequence owner is absent")?;
            let d = p.pointer(24)?;
            if p.u32(d - 4)? == 0x80808179 && p.u8(d + 0x3A8)? & 2 != 0 {
                sequence::Sequence::read(p)?;
                retained.insert(tag);
            }
        }
    }
    Ok(retained)
}

fn literal(p: &Payload, at: usize) -> Result<Option<f32>> {
    let code = p.array(at + 16, 1, Some(0x80800009))?;
    let constants = p.array(at + 32, 16, Some(0x80800090))?;
    if code.len() != 4
        || p.bytes::<4>(code[0])? != [0x42, 0, 0x4C, 0]
        || constants.len() != 1
        || p.u64(at + 48)? != 1
        || p.u64(at + 56)? != 1
        || !p.array(at + 64, 40, Some(0x80809591))?.is_empty()
    {
        return Ok(None);
    }
    let value = p.f32(constants[0])?;
    Ok((value.is_finite()
        && (1..4).all(|lane| p.u32(constants[0] + lane * 4).ok() == Some(value.to_bits())))
    .then_some(value))
}

/// A disconnected, constant positive timer retains its creator until that timer
/// retires the entity. Unlike a sequence flag, this needs a nonempty action key
/// so native action cleanup defers to the timer's own retirement callback.
pub(super) fn timer_retention(closure: &closure::Closure) -> Result<BTreeMap<u32, f32>> {
    let mut retained = BTreeMap::new();
    for (&tag, payload) in &closure.graphs {
        let graph = Graph::read(payload, true)?;
        let mut timer = None;
        let mut bounded = true;
        for &owner in &graph.components {
            let p = closure
                .owners
                .get(&owner)
                .context("cleanup timer owner is absent")?;
            let d = p.pointer(24)?;
            match p.u32(d - 4)? {
                // Context, network, named parameters, numeric bank and sequence.
                0x80803F64 | 0x80808EDD | 0x80808757 | 0x80809597 | 0x80808179 => {}
                0x8080B11F if timer.is_none() => {
                    let duration = d + 0x298;
                    let value = literal(p, duration + 0x118)?;
                    if p.u32(d + 0x58)? != 0
                        || p.f32(d + 0x5C)? != 0.0
                        || literal(p, duration + 0x20)? != Some(-1.0)
                        || literal(p, duration + 0x1D8)? != Some(-1.0)
                        || value.is_none_or(|seconds| seconds <= 0.0)
                        || graph
                            .connections
                            .iter()
                            .chain(&graph.named_connections)
                            .any(|edge| {
                                edge.consumer
                                    .object
                                    .is_some_and(|object| object.owner == owner)
                                    && edge.provider.object.is_some()
                                    || edge
                                        .provider
                                        .object
                                        .is_some_and(|object| object.owner == owner)
                                        && edge.consumer.object.is_some()
                            })
                    {
                        bounded = false;
                    }
                    timer = value;
                }
                _ => bounded = false,
            }
        }
        if bounded && let Some(seconds) = timer {
            retained.insert(tag, seconds);
        }
    }
    Ok(retained)
}

fn provider(
    graph: &Graph,
    input: Object,
) -> Result<(&crate::d2_mot::entity::links::Connection, Object)> {
    let edges = graph
        .connections
        .iter()
        .chain(&graph.named_connections)
        .filter(|edge| edge.consumer.object == Some(input))
        .collect::<Vec<_>>();
    ensure!(
        edges.len() == 1 && edges[0].flags == 0,
        "duration input has missing, ambiguous or flagged linkage"
    );
    let edge = edges[0];
    let object = edge
        .provider
        .object
        .context("duration input is disconnected")?;
    ensure!(
        graph.components.contains(&object.owner),
        "duration input is outside its graph"
    );
    Ok((edge, object))
}

fn written_parameters(closure: &closure::Closure) -> Result<BTreeSet<u32>> {
    let mut written = BTreeSet::new();
    for owner in closure.owners.values() {
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? != 0x80808179 {
            continue;
        }
        for row in owner.array(d + 0x1D8, 24, Some(0x808091F1))? {
            let event = owner.pointer(row + 16)?;
            if owner.u32(event - 4)? == 0x8080847E {
                written.insert(owner.u32(event + 0x68)?);
            }
        }
    }
    Ok(written)
}

/// A zero serialized parameter can still produce a positive timer after the
/// creation context is applied. Admit the complete, bounded max(input, floor)
/// program, rather than substituting a guessed value for its input.
fn positive_floor(closure: &closure::Closure, input: Object) -> Result<bool> {
    let p = closure
        .owners
        .get(&input.owner)
        .context("duration owner is outside its closure")?;
    let d = p.pointer(24)?;
    if p.u32(d - 4)? != 0x8080B11F || input.class != 0x80809591 {
        return Ok(false);
    }
    // Timed status +100, Duration +198, timer +80, scalar definition +98.
    let scalar = d + 0x3B0;
    let inputs = p.array(scalar + 64, 40, Some(0x80809591))?;
    if inputs.as_slice() != [input.offset as usize]
        || p.u64(scalar + 48)? != 2
        || p.u64(scalar + 56)? != 1
    {
        return Ok(false);
    }
    let code = p.array(scalar + 16, 1, Some(0x80800009))?;
    if code.len() != 7 || p.bytes::<7>(code[0])? != [0x4A, 1, 0x42, 0, 9, 0x4C, 0] {
        return Ok(false);
    }
    let constants = p.array(scalar + 32, 16, Some(0x80800090))?;
    let [constant] = constants.as_slice() else {
        return Ok(false);
    };
    let floor = p.f32(*constant)?;
    Ok(floor.is_finite()
        && floor > 0.0
        && (1..4).all(|lane| p.u32(*constant + lane * 4).ok() == Some(floor.to_bits())))
}

pub(super) fn stable(
    closure: &closure::Closure,
    graph: &Graph,
    inputs: &[Object],
    specialized: &BTreeMap<u32, Payload>,
) -> Result<()> {
    let written = written_parameters(closure)?;
    for &input in inputs {
        let (edge, object) = provider(graph, input)?;
        ensure!(
            object.class == 0x808095CF,
            "duration input does not read a channel bank"
        );
        let bank = &closure.owners[&object.owner];
        let d = bank.pointer(24)?;
        ensure!(
            bank.u32(d - 4)? == 0x80809597 && object.offset == (d + 0x68) as u64,
            "duration channel provider is not the bank's numeric interface"
        );
        let declarations = bank.array(d + 0x148, 112, Some(0x808095A9))?;
        let row = *declarations
            .get(edge.channel as usize)
            .context("duration channel is outside the bank")?;
        ensure!(
            bank.u64(row + 8)? == 0 && bank.u64(row + 96)? == 0,
            "duration refresh depends on a computed or interpolated bank channel"
        );
        let names = |field| -> Result<Vec<usize>> {
            bank.array(d + field, 12, Some(0x808095A8))?
                .into_iter()
                .filter_map(|name| match bank.u32(name + 8) {
                    Ok(channel) if channel == edge.channel => Some(Ok(name)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        };
        // +48 is a numeric storage slot and +6A indexes the publication table.
        // Neither is an ordinal in the inherited or connected-name tables.
        let inherited = names(0x178)?;
        let incoming = names(0x188)?;
        ensure!(
            incoming.len() + inherited.len() == 1,
            "duration channel has missing or competing input names"
        );
        let name_row = *incoming.first().or_else(|| inherited.first()).unwrap();
        ensure!(
            bank.u32(name_row)? == 0x811C9DC5 && bank.u32(name_row + 4)? == bank.u32(row)?,
            "duration channel and input name disagree"
        );
        if !incoming.is_empty() {
            let physical = bank.array(d + 0x138, 32, Some(0x808095B0))?;
            let port = *physical
                .get(edge.channel as usize)
                .context("duration channel has no connected input")?;
            let (edge, value) = provider(
                graph,
                Object {
                    owner: object.owner,
                    class: 0x808095B0,
                    offset: port as u64,
                },
            )?;
            ensure!(
                edge.channel == 0 && value.class == 0x808098D2,
                "duration input is not a named vector parameter"
            );
            let p = specialized
                .get(&value.owner)
                .unwrap_or(&closure.owners[&value.owner]);
            let pd = p.pointer(24)?;
            ensure!(
                p.u32(pd - 4)? == 0x80808757,
                "duration input parameter component differs"
            );
            let rows = p.array(pd + 0xB8, 80, Some(0x8080875D))?;
            let parameter = rows
                .into_iter()
                .find(|&r| value.offset == (r + 16) as u64)
                .context("duration parameter provider is not declared")?;
            ensure!(
                p.u32(parameter + 48)? == bank.u32(row)? && p.u32(parameter + 52)? == 1,
                "duration parameter name or storage mode differs"
            );
            ensure!(
                !written.contains(&p.u32(parameter + 48)?)
                    && graph
                        .connections
                        .iter()
                        .chain(&graph.named_connections)
                        .all(|edge| edge
                            .consumer
                            .object
                            .is_none_or(|object| object.owner != value.owner)),
                "duration parameter has a writer after creation"
            );
            let raw = p.bytes::<16>(parameter + 64)?;
            let first = f32::from_le_bytes(raw[..4].try_into()?);
            ensure!(
                first.is_finite() && raw.chunks_exact(4).all(|lane| lane == &raw[..4]),
                "duration parameter default is not a finite scalar broadcast"
            );
            ensure!(
                first > 0.0 || positive_floor(closure, input)?,
                "duration parameter needs a positive default or a positive timer floor"
            );
            // Native sequence 58AFD0 builds the creation context before
            // 56DE00 constructs the entity. Its parameter constructor runs
            // before the separate post-creation interface pass. Matching
            // refresh invokes the existing receiver without a new argument,
            // so the expression may vary between instances while remaining
            // fixed within each one. Keep the creation expression intact.
        } else {
            ensure!(
                matches!(bank.u32(row)?, 0xE0862153 | 0x48E16077),
                "duration refresh reads an unqualified inherited property"
            );
            // The admitted names are is_player and activity_mode_pvp. Their
            // producer and attachment-parent routes still require validation
            // by the entry point before this graph can be declared installable.
        }
    }
    Ok(())
}

pub(super) fn parameters(closure: &closure::Closure) -> Result<BTreeMap<u32, Payload>> {
    let mut output = BTreeMap::new();
    let written = written_parameters(closure)?;
    for (&graph_tag, values) in &closure.parameters {
        if values.is_empty() {
            continue;
        }
        ensure!(
            values.keys().all(|name| !written.contains(name)),
            "creation parameter is also written by an attachment sequence"
        );
        let graph = Graph::read(&closure.graphs[&graph_tag], true)?;
        let candidates = graph
            .components
            .iter()
            .filter(|&&owner| {
                let p = &closure.owners[&owner];
                p.pointer(24).ok().and_then(|d| p.u32(d - 4).ok()) == Some(0x80808757)
            })
            .copied()
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "creation parameters need one private parameter component"
        );
        let owner = candidates[0];
        ensure!(
            graph
                .connections
                .iter()
                .chain(&graph.named_connections)
                .all(|edge| edge
                    .consumer
                    .object
                    .is_none_or(|object| object.owner != owner)),
            "creation parameter component has an external writer"
        );
        let specialized = controller::parameters::with_defaults(&closure.owners[&owner], values)?;
        if let Some(previous) = output.insert(owner, specialized.clone()) {
            ensure!(
                previous.0 == specialized.0,
                "shared parameter owner has incompatible creation values"
            );
        }
        for (&other, payload) in &closure.graphs {
            if other != graph_tag && Graph::read(payload, true)?.components.contains(&owner) {
                ensure!(
                    closure.parameters.get(&other) == Some(values),
                    "parameter owner is shared with a graph using different creation defaults"
                );
            }
        }
    }
    Ok(output)
}
