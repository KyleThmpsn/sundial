//! Apply source response structure to a private carrier movement and its allocation.
//!
//! Words, masks and programs change in place. New action lists, parameters, named inputs and their
//! instance twins are appended, copied from the carrier's own records of the same class, and the
//! component is sealed once afterwards. Allocation rows are copied from the carrier's own rows.
use super::*;
use crate::d1_roi::native::array;

/// An action in a rewritten list: a carrier parameter kept by offset, or a new one.
pub(super) enum Act {
    Keep {
        selector: u8,
        parameter: usize,
    },
    /// A parameter body, arrays it owns as (field, class, data, stride), and fields that name a
    /// translated spawned graph, by the source graph it translates.
    New {
        selector: u8,
        class: u32,
        bytes: Vec<u8>,
        arrays: Vec<(usize, u32, Vec<u8>, usize)>,
        spawns: Vec<(usize, u32)>,
    },
}

#[derive(Default)]
pub(super) struct Plan {
    /// Four-byte writes: group and node names, group gates, definition `+128`.
    pub words: Vec<(usize, u32)>,
    /// Node event masks.
    pub masks: Vec<(usize, u64)>,
    /// Node offset and its new action list in source order.
    pub actions: Vec<(usize, Vec<Act>)>,
    /// Carrier constant scalar rows that become a forward of one named input.
    pub scalars: Vec<(usize, u32)>,
    /// Carrier forward scalar rows nothing kept reads, whose one named input takes another name.
    pub renamed: Vec<(usize, u32)>,
    /// Node expressions, at node `+F0`, that become a forward of one named input.
    pub expressions: Vec<(usize, u32)>,
    /// Source groups the carrier lacks.
    pub inserted: Vec<Inserted>,
    /// Source nodes a paired carrier group lacks: carrier group index, then each node with its
    /// source position in the group.
    pub nodes: Vec<(usize, Vec<(usize, InsertedNode)>)>,
    /// Native material filters and fallback for each inserted group, by source position.
    pub materials: BTreeMap<usize, (Vec<u32>, u32)>,
    /// Source scalars no carrier scalar can take, appended after the carrier's scalars.
    pub appended: Vec<Appended>,
    /// Kept carrier response filters that take the source's labels.
    pub labels: Vec<Relabel>,
}

/// A kept carrier response filter rewritten to the source's labels: the inline filter's offset,
/// its lists for the native registry, and the compiled predicate's mode and mask record.
pub(super) struct Relabel {
    pub at: usize,
    pub lists: [Vec<u32>; 4],
    pub mode: u8,
    pub record: Option<(u32, Vec<u8>)>,
}

/// A root scalar appended from the source: its program and the one named input it forwards, if
/// any.
pub(super) struct Appended {
    pub input: Option<u32>,
    pub code: Vec<u8>,
    pub constants: Vec<u8>,
    pub counts: [u64; 2],
}

impl Plan {
    /// Source graphs that created spawns name, which the route translates.
    pub fn spawns(&self) -> BTreeSet<u32> {
        let acts = self
            .actions
            .iter()
            .flat_map(|(_, list)| list)
            .chain(
                self.inserted
                    .iter()
                    .flat_map(|group| &group.nodes)
                    .flat_map(|node| &node.actions),
            )
            .chain(
                self.nodes
                    .iter()
                    .flat_map(|(_, nodes)| nodes)
                    .flat_map(|(_, node)| &node.actions),
            );
        acts.flat_map(|act| match act {
            Act::New { spawns, .. } => spawns.iter().map(|&(_, tag)| tag).collect(),
            Act::Keep { .. } => Vec::new(),
        })
        .collect()
    }

    /// Profiles that created profile responses name.
    pub fn created_profiles(&self) -> BTreeSet<u32> {
        self.actions
            .iter()
            .flat_map(|(_, list)| list)
            .chain(
                self.inserted
                    .iter()
                    .flat_map(|group| &group.nodes)
                    .flat_map(|node| &node.actions),
            )
            .chain(
                self.nodes
                    .iter()
                    .flat_map(|(_, nodes)| nodes)
                    .flat_map(|(_, node)| &node.actions),
            )
            .filter_map(|act| match act {
                Act::New {
                    selector: 9, bytes, ..
                } => bytes
                    .get(8..12)
                    .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes"))),
                _ => None,
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
            && self.masks.is_empty()
            && self.actions.is_empty()
            && self.scalars.is_empty()
            && self.renamed.is_empty()
            && self.expressions.is_empty()
            && self.inserted.is_empty()
            && self.nodes.is_empty()
            && self.appended.is_empty()
            && self.labels.is_empty()
    }

    pub fn report(&self) -> Value {
        json!({"words":self.words.len(),"masks":self.masks.len(),"action_lists":self.actions.len(),
            "converted_scalars":self.scalars.len(),"renamed_scalars":self.renamed.len(),
            "converted_expressions":self.expressions.len(),"appended_scalars":self.appended.iter().map(|scalar| scalar.input).collect::<Vec<_>>(),
            "inserted_groups":self.inserted.iter().map(|group| group.index).collect::<Vec<_>>(),
            "relabeled_filters":self.labels.iter().map(|relabel| json!({"lists":relabel.lists,"mode":relabel.mode})).collect::<Vec<_>>(),
            "inserted_nodes":self.nodes.iter().map(|(group, nodes)| json!({"carrier_group":group,
                "positions":nodes.iter().map(|(position, _)| *position).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
}

fn rows_of(p: &Payload, descriptor: usize, stride: usize) -> Result<(Vec<usize>, u32)> {
    let rows = p.array(descriptor, stride, None)?;
    let class = if rows.is_empty() {
        0
    } else {
        p.u32(p.pointer(descriptor + 8)? + 8)?
    };
    Ok((rows, class))
}

/// The carrier's own named input and input twin rows, used as templates.
struct Templates {
    input: [u8; 40],
    input_class: u32,
    // Native 80809788 occupies 96 bytes, including the state after its 64-byte prefix.
    twin: [u8; 96],
    twin_class: u32,
}

fn templates(p: &Payload, definition: usize) -> Result<Templates> {
    for row in p.array(definition + 0x228, 0x50, Some(0x808089F8))? {
        let (inputs, input_class) = rows_of(p, row + 0x40, 40)?;
        let Some(&input) = inputs.first() else {
            continue;
        };
        let twin = usize::try_from(p.u64(row + 8)?)?;
        let (twins, twin_class) = rows_of(p, twin + 0x20, 96)?;
        let first = *twins.first().context("Carrier scalar input has no twin")?;
        ensure!(
            input_class == 0x80809789
                && twin_class == 0x80809788
                && p.u64(input + 8)? == first as u64
                && p.u64(first + 8)? == input as u64,
            "Carrier scalar input and twin are not reciprocal"
        );
        return Ok(Templates {
            input: p.bytes::<40>(input)?,
            input_class,
            twin: p.bytes::<96>(first)?,
            twin_class,
        });
    }
    anyhow::bail!("Carrier has no named scalar input to copy")
}

/// Turn the program at `at` into a forward of one named input and add that input with its twin.
/// `at` is a scalar row or a node expression, which share the layout. Returns the input offset.
fn forward(p: &mut Payload, at: usize, name: u32, templates: &Templates) -> Result<usize> {
    let code = p.array(at + 0x10, 1, Some(0x80800009))?;
    ensure!(
        code.len() >= 4 && p.array(at + 0x40, 40, None)?.is_empty(),
        "Converted projectile program has inputs or a short code array"
    );
    let header = p.pointer(at + 0x18)?;
    // The physical loader continues after the shorter bytecode envelope.
    p.0[code[0] + 4..code[0] + code.len()].fill(0);
    put(&mut p.0, code[0], &[0x3C, 1, 0x3E, 0])?;
    put(&mut p.0, at + 0x10, &4u64.to_le_bytes())?;
    put(&mut p.0, header, &4u64.to_le_bytes())?;
    put(&mut p.0, at + 0x20, &[0; 16])?;
    put(&mut p.0, at + 0x30, &2u64.to_le_bytes())?;
    put(&mut p.0, at + 0x38, &1u64.to_le_bytes())?;
    let twin = usize::try_from(p.u64(at + 8)?)?;
    ensure!(
        p.array(twin + 0x20, 96, None)?.is_empty() && p.u64(twin + 8)? == at as u64,
        "Converted projectile program twin differs"
    );
    let mut input = templates.input;
    input[32..36].copy_from_slice(&name.to_le_bytes());
    let input_at = array(&mut p.0, at + 0x40, templates.input_class, &input, 40)?;
    let twin_at = array(
        &mut p.0,
        twin + 0x20,
        templates.twin_class,
        &templates.twin,
        96,
    )?;
    put(&mut p.0, input_at + 8, &(twin_at as u64).to_le_bytes())?;
    put(&mut p.0, twin_at + 8, &(input_at as u64).to_le_bytes())?;
    put(
        &mut p.0,
        twin_at + 0x10,
        &(twin as i64 - (twin_at + 0x10) as i64).to_le_bytes(),
    )?;
    Ok(input_at)
}

/// Point a kept inline `808093F3` filter at new `808094B3` label rows over registry `80C70CA1` and
/// a new compiled record. Like the carrier's own filters, it names the registry by tag alone with
/// an empty path at `+40`. Its old rows and record stay unreferenced.
fn relabel_filter(p: &mut Payload, relabel: &Relabel) -> Result<()> {
    const REGISTRY: u32 = 0x80C70CA1;
    let at = relabel.at;
    ensure!(
        p.u64(at + 0x40)? == 0 && p.u32(at + 0x48)? == REGISTRY,
        "Relabeled projectile filter names another registry"
    );
    for (operation, names) in relabel.lists.iter().enumerate() {
        let mut rows = Vec::with_capacity(names.len() * 24);
        for name in names {
            let mut row = [0u8; 24];
            row[..4].copy_from_slice(&name.to_le_bytes());
            row[16..20].copy_from_slice(&REGISTRY.to_le_bytes());
            rows.extend(row);
        }
        array(&mut p.0, at + operation * 16, 0x808094B3, &rows, 24)?;
    }
    put(&mut p.0, at + 0x50, &[relabel.mode])?;
    match &relabel.record {
        Some((class, bytes)) => {
            let record = crate::tiger::sequence::object(p, *class, bytes.len())?;
            put(&mut p.0, record, bytes)?;
            crate::tiger::sequence::relative(p, at + 0x58, record)?;
        }
        None => put(&mut p.0, at + 0x58, &[0; 8])?,
    }
    Ok(())
}

/// Write a node's new action list. Returns each field naming a translated spawned graph.
fn rewrite_actions(p: &mut Payload, node: usize, list: &[Act]) -> Result<Vec<(usize, u32)>> {
    let mut spawned = Vec::new();
    let mut rows = vec![0u8; list.len() * 16];
    for (index, act) in list.iter().enumerate() {
        rows[index * 16 + 8] = match act {
            Act::Keep { selector, .. } | Act::New { selector, .. } => *selector,
        };
    }
    let (_, class) = rows_of(p, node + 0x178, 16)?;
    let class = if class == 0 { 0x808037F7 } else { class };
    let start = array(&mut p.0, node + 0x178, class, &rows, 16)?;
    for (index, act) in list.iter().enumerate() {
        let parameter = match act {
            Act::Keep { parameter, .. } => *parameter,
            Act::New {
                class,
                bytes,
                arrays,
                spawns,
                ..
            } => {
                // Native parameter declarations can require 8- or 16-byte alignment.
                // Keep the class marker immediately before the aligned body.
                let parameter = crate::tiger::sequence::object(p, *class, bytes.len())?;
                put(&mut p.0, parameter, bytes)?;
                for (field, class, data, stride) in arrays {
                    array(&mut p.0, parameter + field, *class, data, *stride)?;
                }
                spawned.extend(spawns.iter().map(|&(field, tag)| (parameter + field, tag)));
                parameter
            }
        };
        let row = start + index * 16;
        put(
            &mut p.0,
            row,
            &(parameter as i64 - row as i64).to_le_bytes(),
        )?;
    }
    Ok(spawned)
}

/// The allocation child rows of `row`, which must be `40`-byte allocation members.
fn children(allocation: &Payload, row: usize) -> Result<Vec<usize>> {
    allocation.array(row + 24, 40, None)
}

fn member(allocation: &Payload, name: u32) -> Result<usize> {
    let rows = allocation
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .filter(|&row| allocation.u32(row).ok() == Some(name))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        anyhow::bail!("Projectile movement allocation member {name:08X} is not unique");
    };
    Ok(*row)
}

/// Give scalar `index` one named input twin in the allocation, copied from another scalar's.
fn allocate_scalar_input(allocation: &mut Payload, index: usize) -> Result<()> {
    let scalars = member(allocation, 0x455355D5)?;
    let wrappers = children(allocation, scalars)?;
    let wrapper = *wrappers.get(index).context("Scalar allocation wrapper")?;
    ensure!(
        children(allocation, wrapper)?.is_empty(),
        "Converted scalar already has allocated inputs"
    );
    let template = wrappers
        .iter()
        .find_map(|&other| {
            let rows = children(allocation, other).ok()?;
            let [row] = rows.as_slice() else { return None };
            (allocation.u32(*row).ok() == Some(0x1D7C9056)
                && allocation.u32(*row + 20).ok() == Some(1))
            .then_some(*row)
        })
        .context("Carrier allocation has no scalar input to copy")?;
    let row = allocation.bytes::<40>(template)?;
    let parent = wrappers
        .iter()
        .copied()
        .find(|&other| children(allocation, other).is_ok_and(|rows| rows.contains(&template)))
        .context("Carrier allocation has no scalar input list")?;
    let class = allocation.u32(allocation.pointer(parent + 32)? + 8)?;
    array(&mut allocation.0, wrapper + 24, class, &row, 40)?;
    Ok(())
}

/// Node input storage as every examined stock movement allocation holds it, `80BBB1BA`,
/// `80BBC07D` and `81529CC8` alike: an empty-named node wrapper, member `4137EF97` and its
/// `C214E9FF` child, each in a `80808852` array. Descriptors at `+18` stay empty.
fn stock_storage() -> ([u8; 40], [u8; 40], [u8; 40]) {
    let row = |name: u32, value: u64| {
        let mut row = [0u8; 40];
        row[..4].copy_from_slice(&name.to_le_bytes());
        row[8..16].copy_from_slice(&value.to_le_bytes());
        row[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        row
    };
    (
        row(0x811C9DC5, 0),
        row(0x4137EF97, 0),
        row(0xC214E9FF, 0x28),
    )
}

/// Give node `node` of group `group` the per-node input storage that carrier nodes with an
/// expression input have: member `4137EF97` holding `C214E9FF`, copied from such a node, or in
/// the stock form when no carrier node has storage, as in Thorn's carrier `80BBC0FE`.
fn allocate_expression_input(
    allocation: &mut Payload,
    group: usize,
    node: usize,
    nodes: usize,
) -> Result<()> {
    let groups = member(allocation, 0x5680393D)?;
    let wrappers = children(allocation, groups)?;
    // Find a node wrapper holding the chain to copy.
    let mut template = None;
    for &wrapper in &wrappers {
        for list in children(allocation, wrapper)? {
            for node_wrapper in children(allocation, list)? {
                let chain = children(allocation, node_wrapper)?;
                if let [first] = chain.as_slice()
                    && allocation.u32(*first)? == 0x4137EF97
                {
                    template = Some((list, node_wrapper, *first));
                }
            }
        }
    }
    let ((wrapper_row, chain_row, inner_row), classes) = match template {
        Some((template_list, template_wrapper, template_chain)) => {
            let inner = children(allocation, template_chain)?;
            let [inner] = inner.as_slice() else {
                anyhow::bail!("Carrier node input storage differs");
            };
            (
                (
                    allocation.bytes::<40>(template_wrapper)?,
                    allocation.bytes::<40>(template_chain)?,
                    allocation.bytes::<40>(*inner)?,
                ),
                (
                    allocation.u32(allocation.pointer(template_list + 32)? + 8)?,
                    allocation.u32(allocation.pointer(template_wrapper + 32)? + 8)?,
                    allocation.u32(allocation.pointer(template_chain + 32)? + 8)?,
                ),
            )
        }
        None => (stock_storage(), (0x80808852, 0x80808852, 0x80808852)),
    };
    let wrapper = *wrappers.get(group).context("Group allocation wrapper")?;
    let lists = children(allocation, wrapper)?;
    let [list] = lists.as_slice() else {
        anyhow::bail!("Group allocation has no single node list");
    };
    let list = *list;
    ensure!(
        allocation.u32(list)? == 0x5B2D36EB && allocation.u32(list + 20)? as usize == nodes,
        "Group allocation node list differs"
    );
    // Node wrappers exist only where some node in the group has storage.
    if children(allocation, list)?.is_empty() {
        let mut empty = wrapper_row;
        empty[24..40].fill(0);
        let rows = empty.repeat(nodes);
        array(&mut allocation.0, list + 24, classes.0, &rows, 40)?;
    }
    let node_wrapper = *children(allocation, list)?
        .get(node)
        .context("Node allocation wrapper")?;
    ensure!(
        children(allocation, node_wrapper)?.is_empty(),
        "Converted node already has input storage"
    );
    let mut chain = chain_row;
    chain[24..40].fill(0);
    let chain_at = array(&mut allocation.0, node_wrapper + 24, classes.1, &chain, 40)?;
    array(&mut allocation.0, chain_at + 24, classes.2, &inner_row, 40)?;
    Ok(())
}

/// Apply every edit. Returns each created named input's offset and channel name, whether the
/// group arrays were rebuilt, which also leaves out unpaired carrier groups, and each field that
/// names a translated spawned graph with its source graph.
#[allow(clippy::type_complexity)]
pub(super) fn apply(
    plan: &Plan,
    p: &mut Payload,
    (owner, definition, instance): (u32, usize, usize),
    allocation: &mut Payload,
    paired: &[Option<usize>],
) -> Result<(Vec<(usize, u32)>, bool, Vec<(usize, u32)>)> {
    let inputs = plan
        .nodes
        .iter()
        .flat_map(|(_, nodes)| nodes.iter().map(|(_, node)| node))
        .chain(plan.inserted.iter().flat_map(|group| &group.nodes))
        .any(|node| node.input.is_some());
    let templates = if plan.scalars.is_empty()
        && plan.expressions.is_empty()
        && plan.appended.is_empty()
        && !inputs
    {
        None
    } else {
        Some(templates(p, definition)?)
    };
    for &(at, value) in &plan.words {
        put(&mut p.0, at, &value.to_le_bytes())?;
    }
    for &(at, value) in &plan.masks {
        put(&mut p.0, at, &value.to_le_bytes())?;
    }
    let groups = p.array(definition + 0x218, 0x58, Some(0x808037CD))?;
    let mut created = Vec::new();
    let scalars = p.array(definition + 0x228, 0x50, Some(0x808089F8))?;
    for &(row, name) in &plan.scalars {
        let index = scalars
            .iter()
            .position(|&at| at == row)
            .context("Converted scalar is not in the carrier array")?;
        let templates = templates.as_ref().context("Input templates")?;
        created.push((forward(p, row, name, templates)?, name));
        allocate_scalar_input(allocation, index)?;
    }
    // A renamed forward keeps its input, twin and allocation, and its connection is rebuilt.
    for &(row, name) in &plan.renamed {
        let inputs = p.array(row + 0x40, 40, None)?;
        let [input] = inputs.as_slice() else {
            anyhow::bail!("Renamed projectile scalar has another input count");
        };
        put(&mut p.0, input + 32, &name.to_le_bytes())?;
        created.push((*input, name));
    }
    for &(at, name) in &plan.expressions {
        let node = at - 0xF0;
        let (group, index, count) = groups
            .iter()
            .enumerate()
            .find_map(|(g, &group)| {
                let nodes = p.array(group + 0x48, 0x188, None).ok()?;
                nodes
                    .iter()
                    .position(|&n| n == node)
                    .map(|i| (g, i, nodes.len()))
            })
            .context("Converted expression is not in a carrier node")?;
        let templates = templates.as_ref().context("Input templates")?;
        created.push((forward(p, at, name, templates)?, name));
        allocate_expression_input(allocation, group, index, count)?;
    }
    for relabel in &plan.labels {
        relabel_filter(p, relabel)?;
    }
    let mut spawned = Vec::new();
    for (node, list) in &plan.actions {
        spawned.extend(rewrite_actions(p, *node, list)?);
    }
    // Node arrays grow after every in-place edit, which address the carrier rows.
    let mut lists = Vec::new();
    for (group, nodes) in &plan.nodes {
        let (actions, inputs) = insert_nodes(
            p,
            (definition, instance),
            allocation,
            (*group, nodes),
            templates.as_ref(),
        )?;
        lists.extend(actions);
        created.extend(inputs);
    }
    for (node, list) in lists {
        spawned.extend(rewrite_actions(p, node, list)?);
    }
    let rebuilt = !plan.inserted.is_empty();
    if rebuilt {
        let (nodes, inputs) = rebuild(
            p,
            (owner, definition, instance),
            allocation,
            paired,
            (&plan.inserted, &plan.materials),
            templates.as_ref(),
        )?;
        created.extend(inputs);
        for (node, list) in nodes {
            spawned.extend(rewrite_actions(p, node, list)?);
        }
    }
    // Scalar rows move last, after every edit that addresses them in place.
    if !plan.appended.is_empty() {
        let templates = templates.as_ref().context("Input templates")?;
        created.extend(append_scalars(
            p,
            (owner, definition, instance),
            allocation,
            &plan.appended,
            templates,
        )?);
    }
    Ok((created, rebuilt, spawned))
}

/// Append root scalars from the source. The definition rows and their `808089F7` instance twins
/// at instance `+40` move to new arrays in order. Moved rows keep their code, constant and input
/// descriptors at `+10`, `+20` and `+40` pointed at their arrays, moved twins keep the instance
/// reference at `+10` and the input twin descriptor at `+20`, and each input twin's `+10` reaches
/// its moved scalar twin. Each new pair copies a carrier forward pair and gains the source
/// program's code, constants and counts, and an input and input twin when it forwards one. The
/// allocation's scalar list under member `455355D5` gains a wrapper for each, with one input child
/// for a forward. Returns each new input's offset and name.
fn append_scalars(
    p: &mut Payload,
    (owner, definition, instance): (u32, usize, usize),
    allocation: &mut Payload,
    names: &[Appended],
    templates: &Templates,
) -> Result<Vec<(usize, u32)>> {
    let rows = p.array(definition + 0x228, 0x50, Some(0x808089F8))?;
    let twins = p.array(instance + 0x40, 0x30, Some(0x808089F7))?;
    ensure!(
        rows.len() == twins.len(),
        "Projectile scalar twins differ from their scalars"
    );
    for (&row, &twin) in rows.iter().zip(&twins) {
        ensure!(
            p.u32(row)? == owner
                && p.u32(twin)? == owner
                && p.u64(row + 8)? == twin as u64
                && p.u64(twin + 8)? == row as u64
                && p.pointer(twin + 0x10)? == instance,
            "Projectile scalar and twin are not reciprocal"
        );
    }
    for at in (0..p.0.len().saturating_sub(15)).step_by(4) {
        if p.u32(at)? == owner && matches!(p.u32(at + 4)?, 0x808089F7 | 0x808089F8) {
            ensure!(
                rows.contains(&at) || twins.contains(&at),
                "Another record addresses a projectile scalar"
            );
        }
    }
    let template = rows
        .iter()
        .zip(&twins)
        .find(|(row, _)| {
            let code = p
                .array(**row + 0x10, 1, Some(0x80800009))
                .and_then(|code| code.iter().map(|&at| p.u8(at)).collect::<Result<Vec<_>>>());
            code.is_ok_and(|code| code == [0x3C, 1, 0x3E, 0])
                && p.u64(**row + 0x20).ok() == Some(0)
                && p.array(**row + 0x40, 40, None)
                    .is_ok_and(|inputs| inputs.len() == 1)
        })
        .map(|(&row, &twin)| (row, twin))
        .context("Carrier has no forward scalar to copy")?;
    let count = rows.len();
    let mut row_bytes = Vec::new();
    let mut twin_bytes = Vec::new();
    for (&row, &twin) in rows
        .iter()
        .zip(&twins)
        .chain(std::iter::repeat_n((&template.0, &template.1), names.len()))
    {
        row_bytes.extend_from_slice(&p.0[row..row + 0x50]);
        twin_bytes.extend_from_slice(&p.0[twin..twin + 0x30]);
    }
    let first_twin = array(&mut p.0, instance + 0x40, 0x808089F7, &twin_bytes, 0x30)?;
    let first_row = array(&mut p.0, definition + 0x228, 0x808089F8, &row_bytes, 0x50)?;
    let mut created = Vec::new();
    for index in 0..count + names.len() {
        let (row, twin) = (first_row + index * 0x50, first_twin + index * 0x30);
        let (from_row, from_twin) = if index < count {
            (rows[index], twins[index])
        } else {
            template
        };
        // Descriptor offsets follow their counts by eight bytes.
        for field in [0x18, 0x28, 0x48] {
            shift_nonzero(p, row + field, from_row as i64 - row as i64)?;
        }
        for field in [0x10, 0x28] {
            shift_nonzero(p, twin + field, from_twin as i64 - twin as i64)?;
        }
        put(&mut p.0, row + 8, &(twin as u64).to_le_bytes())?;
        put(&mut p.0, twin + 8, &(row as u64).to_le_bytes())?;
        if index < count {
            for input_twin in p.array(twin + 0x20, 96, None)? {
                put(
                    &mut p.0,
                    input_twin + 0x10,
                    &(twin as i64 - (input_twin + 0x10) as i64).to_le_bytes(),
                )?;
            }
            continue;
        }
        let scalar = &names[index - count];
        array(&mut p.0, row + 0x10, 0x80800009, &scalar.code, 1)?;
        array(&mut p.0, row + 0x20, 0x80800090, &scalar.constants, 16)?;
        put(&mut p.0, row + 0x30, &scalar.counts[0].to_le_bytes())?;
        put(&mut p.0, row + 0x38, &scalar.counts[1].to_le_bytes())?;
        let Some(name) = scalar.input else {
            put(&mut p.0, row + 0x40, &[0; 16])?;
            put(&mut p.0, twin + 0x20, &[0; 16])?;
            continue;
        };
        let mut input = templates.input;
        input[32..36].copy_from_slice(&name.to_le_bytes());
        let input_at = array(&mut p.0, row + 0x40, templates.input_class, &input, 40)?;
        let input_twin = array(
            &mut p.0,
            twin + 0x20,
            templates.twin_class,
            &templates.twin,
            96,
        )?;
        put(&mut p.0, input_at + 8, &(input_twin as u64).to_le_bytes())?;
        put(&mut p.0, input_twin + 8, &(input_at as u64).to_le_bytes())?;
        put(
            &mut p.0,
            input_twin + 0x10,
            &(twin as i64 - (input_twin + 0x10) as i64).to_le_bytes(),
        )?;
        created.push((input_at, name));
    }
    append_scalar_storage(allocation, count, names)?;
    Ok(created)
}

/// A node of an inserted group, in source terms.
pub(super) struct InsertedNode {
    /// The one named input its program forwards, if any.
    pub input: Option<u32>,
    pub name: u32,
    pub mask: u64,
    pub code: Vec<u8>,
    pub constants: Vec<u8>,
    pub counts: [u64; 2],
    pub actions: Vec<Act>,
}

/// A source group the carrier lacks, at its source position.
pub(super) struct Inserted {
    pub index: usize,
    pub name: u32,
    pub scalar: u32,
    pub nodes: Vec<InsertedNode>,
}

fn shift_nonzero(p: &mut Payload, at: usize, by: i64) -> Result<()> {
    let value = p.u64(at)? as i64;
    if value != 0 {
        put(&mut p.0, at, &(value + by).to_le_bytes())?;
    }
    Ok(())
}

/// A carrier group with one node that reads no input and fires with probability one. Its rows
/// are the template for inserted groups.
fn template_group(
    p: &Payload,
    groups: &[usize],
    twins: &[usize],
) -> Result<(usize, usize, usize, usize)> {
    for (index, (&group, &twin)) in groups.iter().zip(twins).enumerate() {
        let nodes = p.array(group + 0x48, 0x188, Some(0x808037D0))?;
        let node_twins = p.array(twin + 0x20, 0x70, None)?;
        let ([node], [node_twin]) = (nodes.as_slice(), node_twins.as_slice()) else {
            continue;
        };
        let code = p
            .array(node + 0xC0, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| p.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let constants = p.array(node + 0xD0, 16, Some(0x80800090))?;
        let certain = code == [0x34, 0, 0x3E, 0]
            && constants.len() == 1
            && (0..4).all(|lane| p.u32(constants[0] + lane * 4).ok() == Some(0x3F80_0000));
        if certain
            && p.array(node + 0xF0 + 0x40, 40, None)?.is_empty()
            && p.array(*node_twin + 0x60, 96, None)?.is_empty()
        {
            return Ok((index, group, *node, *node_twin));
        }
    }
    anyhow::bail!("Carrier has no single-node response group to copy for insertion")
}

/// Rebuild the group and twin arrays in source order: carrier groups keep their rows, inserted
/// groups take copies of a carrier template with source names, masks, programs and materials.
/// An inserted node that forwards a named input gets the input, its twin and its storage, as an
/// inserted node of a paired group does. Left-out carrier groups are not copied. The allocation's
/// group list follows the same order. Returns each inserted node's offset with its action list,
/// and each created input's offset and name.
#[allow(clippy::type_complexity)]
fn rebuild<'a>(
    p: &mut Payload,
    (owner, definition, instance): (u32, usize, usize),
    allocation: &mut Payload,
    paired: &[Option<usize>],
    (inserted, materials): (&'a [Inserted], &BTreeMap<usize, (Vec<u32>, u32)>),
    templates: Option<&Templates>,
) -> Result<(Vec<(usize, &'a [Act])>, Vec<(usize, u32)>)> {
    let groups = p.array(definition + 0x218, 0x58, Some(0x808037CD))?;
    let twins = p.array(instance + 0x30, 0x30, Some(0x808037CC))?;
    ensure!(
        groups.len() == twins.len(),
        "Projectile group twins differ from their groups"
    );
    validate_group_twins(p, owner, instance, &groups, &twins)?;
    let (template_index, template_group, template_node, template_twin) =
        template_group(p, &groups, &twins)?;
    let node_twin_class = p.u32(p.pointer(twins[template_index] + 0x28)? + 8)?;
    // Slots in source order: a carrier group or an inserted one.
    let mut slots = Vec::with_capacity(paired.len());
    for (index, carrier) in paired.iter().enumerate() {
        slots.push(match carrier {
            Some(carrier) => Ok(*carrier),
            None => Err(inserted
                .iter()
                .find(|group| group.index == index)
                .context("Unpaired source group is not inserted")?),
        });
    }
    let mut group_rows = Vec::new();
    let mut twin_rows = Vec::new();
    for slot in &slots {
        let (group, twin) = match slot {
            Ok(carrier) => (groups[*carrier], twins[*carrier]),
            Err(_) => (template_group, twins[template_index]),
        };
        group_rows.extend_from_slice(&p.0[group..group + 0x58]);
        twin_rows.extend_from_slice(&p.0[twin..twin + 0x30]);
    }
    let first_group = array(&mut p.0, definition + 0x218, 0x808037CD, &group_rows, 0x58)?;
    let first_twin = array(&mut p.0, instance + 0x30, 0x808037CC, &twin_rows, 0x30)?;
    let mut created = Vec::new();
    let mut inputs = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        let (group, twin) = (first_group + index * 0x58, first_twin + index * 0x30);
        let (from_group, from_twin) = match slot {
            Ok(carrier) => (groups[*carrier], twins[*carrier]),
            Err(_) => (template_group, twins[template_index]),
        };
        shift_nonzero(p, group + 0x20, from_group as i64 - group as i64)?;
        shift_nonzero(p, group + 0x50, from_group as i64 - group as i64)?;
        shift_nonzero(p, twin + 0x10, from_twin as i64 - twin as i64)?;
        shift_nonzero(p, twin + 0x28, from_twin as i64 - twin as i64)?;
        put(&mut p.0, group + 8, &(twin as u64).to_le_bytes())?;
        put(&mut p.0, twin + 8, &(group as u64).to_le_bytes())?;
        if let Err(new) = *slot {
            let (filters, fallback) = materials
                .get(&new.index)
                .context("Inserted projectile group has no materials")?;
            let mut rows = Vec::new();
            for &filter in filters {
                rows.extend(0u64.to_le_bytes());
                rows.extend(filter.to_le_bytes());
                rows.extend(0u32.to_le_bytes());
            }
            array(&mut p.0, group + 0x18, 0x80809FC7, &rows, 16)?;
            put(&mut p.0, group + 0x10, &new.name.to_le_bytes())?;
            put(&mut p.0, group + 0x28, &0u64.to_le_bytes())?;
            put(&mut p.0, group + 0x30, &fallback.to_le_bytes())?;
            put(&mut p.0, group + 0x40, &new.scalar.to_le_bytes())?;
            let node_rows = p.0[template_node..template_node + 0x188].repeat(new.nodes.len());
            let twin_rows = p.0[template_twin..template_twin + 0x70].repeat(new.nodes.len());
            let first_node = array(&mut p.0, group + 0x48, 0x808037D0, &node_rows, 0x188)?;
            let first_node_twin = array(&mut p.0, twin + 0x20, node_twin_class, &twin_rows, 0x70)?;
            for (j, source) in new.nodes.iter().enumerate() {
                let node = first_node + j * 0x188;
                fill_node(
                    p,
                    (node, first_node_twin + j * 0x70, twin),
                    template_node,
                    source,
                )?;
                created.push((node, source.actions.as_slice()));
                if let Some(name) = source.input {
                    let templates = templates.context("Input templates")?;
                    let input = forward(p, node + 0xF0, name, templates)?;
                    inputs.push((index, j, new.nodes.len(), input, name));
                }
            }
        } else {
            for node_twin in p.array(twin + 0x20, 0x70, None)? {
                put(
                    &mut p.0,
                    node_twin + 0x10,
                    &(twin as i64 - (node_twin + 0x10) as i64).to_le_bytes(),
                )?;
            }
        }
    }
    // The allocation's group list follows the same slots.
    rebuild_allocation(allocation, &slots, template_index, groups.len())?;
    for &(group, node, nodes, ..) in &inputs {
        allocate_expression_input(allocation, group, node, nodes)?;
    }
    Ok((
        created,
        inputs
            .into_iter()
            .map(|(.., input, name)| (input, name))
            .collect(),
    ))
}

/// Rebuild the allocation's group list in slot order. Inserted groups copy the template group's
/// list row with their own node count.
fn rebuild_allocation(
    allocation: &mut Payload,
    slots: &[std::result::Result<usize, &Inserted>],
    template_index: usize,
    group_count: usize,
) -> Result<()> {
    let member = member(allocation, 0x5680393D)?;
    let wrappers = children(allocation, member)?;
    ensure!(
        wrappers.len() == group_count,
        "Projectile movement allocation differs from its groups"
    );
    let wrapper_class = allocation.u32(allocation.pointer(member + 32)? + 8)?;
    let template_wrapper = wrappers[template_index];
    let template_lists = children(allocation, template_wrapper)?;
    let [template_list] = template_lists.as_slice() else {
        anyhow::bail!("Template group allocation has no single node list");
    };
    ensure!(
        children(allocation, *template_list)?.is_empty(),
        "Template group allocation has node storage"
    );
    let list_class = allocation.u32(allocation.pointer(template_wrapper + 32)? + 8)?;
    let mut rows = Vec::new();
    for slot in slots {
        let from = match slot {
            Ok(carrier) => wrappers[*carrier],
            Err(_) => template_wrapper,
        };
        rows.extend_from_slice(&allocation.0[from..from + 40]);
    }
    let first = array(&mut allocation.0, member + 24, wrapper_class, &rows, 40)?;
    for (index, slot) in slots.iter().enumerate() {
        let wrapper = first + index * 40;
        match slot {
            Ok(carrier) => shift_nonzero(
                allocation,
                wrapper + 32,
                wrappers[*carrier] as i64 - wrapper as i64,
            )?,
            Err(new) => {
                let mut list = allocation.bytes::<40>(*template_list)?;
                list[20..24].copy_from_slice(&u32::try_from(new.nodes.len())?.to_le_bytes());
                list[24..40].fill(0);
                array(&mut allocation.0, wrapper + 24, list_class, &list, 40)?;
            }
        }
    }
    put(
        &mut allocation.0,
        member + 20,
        &u32::try_from(slots.len())?.to_le_bytes(),
    )?;
    Ok(())
}

/// Rebuild a paired carrier group's node and node twin arrays in source order with the inserted
/// nodes copied from the template node. Moved carrier rows keep their arrays: node `+C8`, `+D8`,
/// `+108`, `+118`, `+138` and `+180` and twin `+68` are relative, while node `+08`, `+20` and `+F8`
/// and twin `+08`, `+28` and `+48` name the partner absolutely and twin `+10` reaches the group
/// twin. Twin `+30` and `+50` are relative within the row. Each input twin's `+10` reaches the
/// node twin's expression at `+40`. Nodes must have the empty label filter at `+30..+88`, the
/// examined form, so its lists hold no references. The allocation's node list follows. Returns
/// each inserted node's offset with its action list.
#[allow(clippy::type_complexity)]
fn insert_nodes<'a>(
    p: &mut Payload,
    (definition, instance): (usize, usize),
    allocation: &mut Payload,
    (group_index, inserted): (usize, &'a [(usize, InsertedNode)]),
    templates: Option<&Templates>,
) -> Result<(Vec<(usize, &'a [Act])>, Vec<(usize, u32)>)> {
    let groups = p.array(definition + 0x218, 0x58, Some(0x808037CD))?;
    let twins = p.array(instance + 0x30, 0x30, Some(0x808037CC))?;
    let (group, twin) = (
        *groups.get(group_index).context("Projectile group index")?,
        *twins
            .get(group_index)
            .context("Projectile group twin index")?,
    );
    ensure!(
        p.u64(group + 8)? == twin as u64 && p.u64(twin + 8)? == group as u64,
        "Projectile group and twin are not reciprocal"
    );
    let nodes = p.array(group + 0x48, 0x188, Some(0x808037D0))?;
    let node_twins = p.array(twin + 0x20, 0x70, None)?;
    ensure!(
        !nodes.is_empty() && nodes.len() == node_twins.len(),
        "Projectile node twins differ from their nodes"
    );
    for (&node, &node_twin) in nodes.iter().zip(&node_twins) {
        ensure!(
            p.u64(node + 8)? == node_twin as u64
                && p.u64(node_twin + 8)? == node as u64
                && p.0[node + 0x30..node + 0x78].iter().all(|&b| b == 0)
                && p.u32(node + 0x78)? == 0x80C70CA1
                && p.u8(node + 0x80)? == 0xFF
                && p.u64(node + 0x88)? == 0,
            "Projectile response node has a label filter or another twin"
        );
    }
    let (_, _, template_node, template_twin) = template_group(p, &groups, &twins)?;
    let twin_class = p.u32(p.pointer(twin + 0x28)? + 8)?;
    let total = nodes.len() + inserted.len();
    let mut slots = Vec::with_capacity(total);
    let mut kept = 0;
    for position in 0..total {
        match inserted.iter().find(|(at, _)| *at == position) {
            Some((_, node)) => slots.push(Err(node)),
            None => {
                slots.push(Ok(kept));
                kept += 1;
            }
        }
    }
    ensure!(
        kept == nodes.len(),
        "Inserted projectile node positions differ"
    );
    let mut node_rows = Vec::new();
    let mut twin_rows = Vec::new();
    for slot in &slots {
        let (node, node_twin) = match slot {
            Ok(i) => (nodes[*i], node_twins[*i]),
            Err(_) => (template_node, template_twin),
        };
        node_rows.extend_from_slice(&p.0[node..node + 0x188]);
        twin_rows.extend_from_slice(&p.0[node_twin..node_twin + 0x70]);
    }
    let first_node = array(&mut p.0, group + 0x48, 0x808037D0, &node_rows, 0x188)?;
    let first_twin = array(&mut p.0, twin + 0x20, twin_class, &twin_rows, 0x70)?;
    let mut created = Vec::new();
    let mut inputs = Vec::new();
    for (j, slot) in slots.iter().enumerate() {
        let (node, node_twin) = (first_node + j * 0x188, first_twin + j * 0x70);
        let i = match slot {
            Ok(i) => *i,
            Err(source) => {
                fill_node(p, (node, node_twin, twin), template_node, source)?;
                created.push((node, source.actions.as_slice()));
                if let Some(name) = source.input {
                    let templates = templates.context("Input templates")?;
                    inputs.push((j, forward(p, node + 0xF0, name, templates)?, name));
                }
                continue;
            }
        };
        let (old, old_twin) = (nodes[i], node_twins[i]);
        for field in [0xC8, 0xD8, 0x108, 0x118, 0x138, 0x180] {
            shift_nonzero(p, node + field, old as i64 - node as i64)?;
        }
        shift_nonzero(p, node_twin + 0x68, old_twin as i64 - node_twin as i64)?;
        for (at, value) in [
            (node + 8, node_twin),
            (node + 0x20, node_twin + 0x20),
            (node + 0xF8, node_twin + 0x40),
            (node_twin + 8, node),
            (node_twin + 0x28, node + 0x18),
            (node_twin + 0x48, node + 0xF0),
        ] {
            put(&mut p.0, at, &(value as u64).to_le_bytes())?;
        }
        put(
            &mut p.0,
            node_twin + 0x10,
            &(twin as i64 - (node_twin + 0x10) as i64).to_le_bytes(),
        )?;
        for input_twin in p.array(node_twin + 0x60, 96, None)? {
            put(
                &mut p.0,
                input_twin + 0x10,
                &((node_twin + 0x40) as i64 - (input_twin + 0x10) as i64).to_le_bytes(),
            )?;
        }
    }
    // The group's allocation list counts its nodes and holds a wrapper per node when any node
    // has storage. Inserted nodes take empty wrappers.
    rebuild_node_storage(allocation, group_index, nodes.len(), &slots)?;
    // An inserted node's input gets the storage carrier nodes with inputs have.
    for &(j, ..) in &inputs {
        allocate_expression_input(allocation, group_index, j, total)?;
    }
    Ok((
        created,
        inputs
            .into_iter()
            .map(|(_, input, name)| (input, name))
            .collect(),
    ))
}

/// Give an inserted node copied from the template its own twin, references, name, mask and
/// source program. Its actions are written afterwards.
fn fill_node(
    p: &mut Payload,
    (node, node_twin, twin): (usize, usize, usize),
    template_node: usize,
    source: &InsertedNode,
) -> Result<()> {
    // The probability program keeps the template's arrays.
    for field in [0xC8, 0xD8] {
        shift_nonzero(p, node + field, template_node as i64 - node as i64)?;
    }
    put(&mut p.0, node + 0x10, &source.name.to_le_bytes())?;
    put(&mut p.0, node + 0x28, &source.mask.to_le_bytes())?;
    put(&mut p.0, node + 8, &(node_twin as u64).to_le_bytes())?;
    put(
        &mut p.0,
        node + 0x20,
        &((node_twin + 0x20) as u64).to_le_bytes(),
    )?;
    put(
        &mut p.0,
        node + 0xF8,
        &((node_twin + 0x40) as u64).to_le_bytes(),
    )?;
    array(&mut p.0, node + 0x100, 0x80800009, &source.code, 1)?;
    array(&mut p.0, node + 0x110, 0x80800090, &source.constants, 16)?;
    put(&mut p.0, node + 0x120, &source.counts[0].to_le_bytes())?;
    put(&mut p.0, node + 0x128, &source.counts[1].to_le_bytes())?;
    put(&mut p.0, node + 0x130, &[0; 16])?;
    put(&mut p.0, node + 0x178, &[0; 16])?;
    put(&mut p.0, node_twin + 8, &(node as u64).to_le_bytes())?;
    put(
        &mut p.0,
        node_twin + 0x10,
        &(twin as i64 - (node_twin + 0x10) as i64).to_le_bytes(),
    )?;
    put(
        &mut p.0,
        node_twin + 0x28,
        &((node + 0x18) as u64).to_le_bytes(),
    )?;
    put(
        &mut p.0,
        node_twin + 0x48,
        &((node + 0xF0) as u64).to_le_bytes(),
    )?;
    Ok(())
}

fn append_scalar_storage(allocation: &mut Payload, count: usize, names: &[Appended]) -> Result<()> {
    // The allocation's scalar list gains a wrapper per new scalar, each with one input child.
    let scalars = member(allocation, 0x455355D5)?;
    let wrappers = children(allocation, scalars)?;
    ensure!(
        wrappers.len() == count,
        "Projectile scalar allocation differs from its scalars"
    );
    let (template_wrapper, child) = wrappers
        .iter()
        .find_map(|&wrapper| {
            let rows = children(allocation, wrapper).ok()?;
            let [row] = rows.as_slice() else { return None };
            (allocation.u32(*row).ok() == Some(0x1D7C9056)
                && allocation.u32(*row + 20).ok() == Some(1))
            .then_some((wrapper, *row))
        })
        .context("Carrier allocation has no scalar input to copy")?;
    let wrapper_class = allocation.u32(allocation.pointer(scalars + 32)? + 8)?;
    let child_class = allocation.u32(allocation.pointer(template_wrapper + 32)? + 8)?;
    let child_row = allocation.bytes::<40>(child)?;
    let mut empty = allocation.bytes::<40>(template_wrapper)?;
    empty[24..40].fill(0);
    let mut wrapper_rows = Vec::new();
    for &wrapper in &wrappers {
        wrapper_rows.extend_from_slice(&allocation.0[wrapper..wrapper + 40]);
    }
    for _ in names {
        wrapper_rows.extend_from_slice(&empty);
    }
    let forwards = names
        .iter()
        .map(|scalar| scalar.input.is_some())
        .collect::<Vec<_>>();
    let first = array(
        &mut allocation.0,
        scalars + 24,
        wrapper_class,
        &wrapper_rows,
        40,
    )?;
    for (index, &wrapper) in wrappers.iter().enumerate() {
        let row = first + index * 40;
        shift_nonzero(allocation, row + 32, wrapper as i64 - row as i64)?;
    }
    for (index, _) in (count..count + names.len())
        .zip(&forwards)
        .filter(|(_, forward)| **forward)
    {
        array(
            &mut allocation.0,
            first + index * 40 + 24,
            child_class,
            &child_row,
            40,
        )?;
    }
    put(
        &mut allocation.0,
        scalars + 20,
        &u32::try_from(count + names.len())?.to_le_bytes(),
    )?;
    Ok(())
}

fn validate_group_twins(
    p: &Payload,
    owner: u32,
    instance: usize,
    groups: &[usize],
    twins: &[usize],
) -> Result<()> {
    for (&group, &twin) in groups.iter().zip(twins) {
        ensure!(
            p.u32(group)? == owner
                && p.u32(twin)? == owner
                && p.u64(group + 8)? == twin as u64
                && p.u64(twin + 8)? == group as u64
                && p.pointer(twin + 0x10)? == instance,
            "Projectile group and twin are not reciprocal"
        );
    }
    for at in (0..p.0.len().saturating_sub(15)).step_by(4) {
        if p.u32(at)? == owner && matches!(p.u32(at + 4)?, 0x808037CC | 0x808037CD) {
            ensure!(
                groups.contains(&at) || twins.contains(&at),
                "Another record addresses a projectile response group"
            );
        }
    }
    Ok(())
}

fn rebuild_node_storage(
    allocation: &mut Payload,
    group_index: usize,
    original_count: usize,
    slots: &[std::result::Result<usize, &InsertedNode>],
) -> Result<()> {
    let member = member(allocation, 0x5680393D)?;
    let wrappers = children(allocation, member)?;
    let wrapper = *wrappers
        .get(group_index)
        .context("Group allocation wrapper")?;
    let lists = children(allocation, wrapper)?;
    let [list] = lists.as_slice() else {
        anyhow::bail!("Group allocation has no single node list");
    };
    let list = *list;
    ensure!(
        allocation.u32(list)? == 0x5B2D36EB
            && allocation.u32(list + 20)? as usize == original_count,
        "Group allocation node list differs"
    );
    put(
        &mut allocation.0,
        list + 20,
        &u32::try_from(slots.len())?.to_le_bytes(),
    )?;
    let node_wrappers = children(allocation, list)?;
    if !node_wrappers.is_empty() {
        ensure!(
            node_wrappers.len() == original_count,
            "Group allocation node wrappers differ from its nodes"
        );
        let class = allocation.u32(allocation.pointer(list + 32)? + 8)?;
        let mut empty = allocation.bytes::<40>(node_wrappers[0])?;
        empty[24..40].fill(0);
        let mut rows = Vec::new();
        for slot in slots {
            match slot {
                Ok(i) => {
                    rows.extend_from_slice(&allocation.0[node_wrappers[*i]..node_wrappers[*i] + 40])
                }
                Err(_) => rows.extend_from_slice(&empty),
            }
        }
        let first = array(&mut allocation.0, list + 24, class, &rows, 40)?;
        for (j, slot) in slots.iter().enumerate() {
            if let Ok(i) = slot {
                let row = first + j * 40;
                shift_nonzero(allocation, row + 32, node_wrappers[*i] as i64 - row as i64)?;
            }
        }
    }
    Ok(())
}
