//! Author native effect sequences from an explicit tree and checked event envelopes.
//! Source adapters supply event bodies and resources after topology is allocated.
pub(crate) mod particle;
pub(crate) mod spawn;
use super::{allocation::Row, payload::Payload};
use crate::presentation::{append, put};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Default)]
pub(crate) struct Timing {
    pub delay: [f32; 2],
    pub duration: [f32; 2],
    pub flags: u32,
}

pub(crate) enum Kind {
    Serial(Vec<Node>),
    Repeat(Vec<Node>),
    Parallel(Vec<Node>),
    /// Repeating native scalar gate. Children retire when the input leaves the range.
    Range {
        input: u32,
        bounds: [f32; 2],
        children: Vec<Node>,
    },
    Weighted(Vec<(f32, Node)>),
    Event(u32),
}

pub(crate) struct Node {
    pub name: u32,
    pub timing: Timing,
    pub kind: Kind,
}

pub(crate) struct Written {
    /// Event names are unique and resolve to their definition offsets.
    pub events: BTreeMap<u32, usize>,
}

pub(crate) fn relative(payload: &mut Payload, field: usize, target: usize) -> Result<()> {
    put(
        &mut payload.0,
        field,
        &(i64::try_from(target)? - i64::try_from(field)?).to_le_bytes(),
    )
}

pub(crate) fn object(payload: &mut Payload, class: u32, size: usize) -> Result<usize> {
    let at = payload
        .0
        .len()
        .checked_add(19)
        .context("Sequence object alignment overflow")?
        & !15;
    let end = at
        .checked_add(size)
        .context("Sequence object size overflow")?;
    payload.0.resize(end, 0);
    put(&mut payload.0, at - 4, &class.to_le_bytes())?;
    Ok(at)
}

fn array(
    payload: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    count: usize,
) -> Result<Vec<usize>> {
    let size = stride
        .checked_mul(count)
        .context("Sequence array size overflow")?;
    append(
        &mut payload.0,
        field,
        u64::from(class),
        &vec![0; size],
        stride,
    )?;
    payload.array(field, stride, Some(class))
}

fn pair(
    payload: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    ic: u32,
    dc: u32,
) -> Result<()> {
    for (at, class, twin) in [(instance, dc, definition), (definition, ic, instance)] {
        put(&mut payload.0, at, &owner.to_le_bytes())?;
        put(&mut payload.0, at + 4, &class.to_le_bytes())?;
        put(&mut payload.0, at + 8, &u64::try_from(twin)?.to_le_bytes())?;
    }
    Ok(())
}

fn common(
    payload: &mut Payload,
    at: usize,
    node: &Node,
    parent: Option<usize>,
    kind: u32,
) -> Result<()> {
    ensure!(
        node.timing.flags & !0x1f == 0,
        "Sequence node flags are unsupported"
    );
    ensure!(
        node.timing
            .delay
            .iter()
            .chain(&node.timing.duration)
            .all(|v| v.is_finite() && *v >= 0.),
        "Sequence timing is negative or nonfinite"
    );
    let parent = match parent {
        Some(index) => u32::try_from(index)? << 16,
        None => 0xffff00ff,
    };
    put(&mut payload.0, at + 0x10, &node.name.to_le_bytes())?;
    put(&mut payload.0, at + 0x14, &parent.to_le_bytes())?;
    put(&mut payload.0, at + 0x18, &node.timing.flags.to_le_bytes())?;
    for (offset, value) in (0x1c..0x2c)
        .step_by(4)
        .zip(node.timing.delay.iter().chain(&node.timing.duration))
    {
        put(&mut payload.0, at + offset, &value.to_le_bytes())?;
    }
    put(&mut payload.0, at + 0x30, &kind.to_le_bytes())
}

struct Control<'a> {
    node: &'a Node,
    parent: Option<usize>,
    children: Vec<(bool, usize)>,
}

struct Event<'a> {
    node: &'a Node,
    parent: usize,
    class: u32,
}

fn flatten<'a>(
    node: &'a Node,
    parent: Option<usize>,
    controls: &mut Vec<Control<'a>>,
    events: &mut Vec<Event<'a>>,
    names: &mut BTreeSet<u32>,
) -> Result<(bool, usize)> {
    ensure!(names.insert(node.name), "Sequence node identities collide");
    if let Kind::Event(class) = node.kind {
        let index = events.len();
        ensure!(
            index < i16::MAX as usize,
            "Sequence event count exceeds its selector"
        );
        events.push(Event {
            node,
            parent: parent.context("Sequence root must be a control")?,
            class,
        });
        return Ok((true, index));
    }
    let index = controls.len();
    ensure!(
        index < i16::MAX as usize,
        "Sequence control count exceeds its selector"
    );
    controls.push(Control {
        node,
        parent,
        children: Vec::new(),
    });
    let children = match &node.kind {
        Kind::Serial(children) | Kind::Repeat(children) | Kind::Parallel(children) => {
            children.iter().collect::<Vec<_>>()
        }
        Kind::Range {
            bounds, children, ..
        } => {
            ensure!(
                bounds.iter().all(|v| v.is_finite()) && bounds[0] <= bounds[1],
                "Sequence scalar gate range is invalid"
            );
            children.iter().collect::<Vec<_>>()
        }
        Kind::Weighted(children) => {
            let total = children.iter().map(|(weight, _)| *weight).sum::<f32>();
            ensure!(
                children
                    .iter()
                    .all(|(weight, _)| weight.is_finite() && *weight >= 0.)
                    && total.is_finite()
                    && total > 0.,
                "Sequence choice has invalid weights"
            );
            children.iter().map(|(_, node)| node).collect()
        }
        Kind::Event(_) => unreachable!(),
    };
    ensure!(!children.is_empty(), "Sequence control has no children");
    let mut selectors = Vec::new();
    for child in children {
        selectors.push(flatten(child, Some(index), controls, events, names)?);
    }
    controls[index].children = selectors;
    Ok((false, index))
}

fn event_layout(class: u32) -> Result<(u32, usize, usize, u32)> {
    Ok(match class {
        0x808093cb => (0x808093ca, 0x38, 0x40, 0),
        0x808093c4 => (0x808093c3, 0x80, 0x40, 2),
        0x80806cc6 => (0x80806cc5, 0x128, 0x50, 4),
        0x80806b38 => (0x80806b37, 0x78, 0x60, 6),
        0x80808881 => (0x80808880, 0xe0, 0x70, 15),
        0x80803fe9 => (0x80803fe8, 0x88, 0x60, 18),
        0x80804595 => (0x80804594, 0x38, 0x50, 19),
        0x80804ab0 => (0x80804aaf, 0x48, 0x40, 38),
        _ => anyhow::bail!("Unsupported native authored event {class:08X}"),
    })
}

fn node_allocation(class: u32) -> Row {
    Row::named(
        0x811c9dc5,
        u32::MAX,
        0,
        vec![Row::named(0x3988f7d4, class, 1, Vec::new())],
    )
}

fn event_defaults(envelopes: &[Payload], needed: &BTreeSet<u32>) -> Result<BTreeMap<u32, Vec<u8>>> {
    let mut defaults = BTreeMap::new();
    for template in envelopes {
        let d = template.pointer(24)?;
        ensure!(
            template.u32(d - 4)? == 0x808084e9,
            "Event envelope is not a sequence"
        );
        for row in template.array(d + 0x168, 24, Some(0x808093e6))? {
            let at = template.pointer(row + 16)?;
            let class = template.u32(at - 4)?;
            if !needed.contains(&class) {
                continue;
            }
            let (ic, _, size, kind) = event_layout(class)?;
            let i = usize::try_from(template.u64(at + 8)?)?;
            ensure!(
                template.u32(i - 4)? == ic
                    && template.u32(i + 4)? == class
                    && template.u64(i + 8)? == at as u64
                    && template.u32(at + 0x30)? == kind,
                "Native event envelope pair differs"
            );
            let bytes = template
                .0
                .get(i..i + size)
                .context("Native event instance extent")?
                .to_vec();
            defaults.entry(class).or_insert(bytes);
        }
    }
    ensure!(
        defaults.len() == needed.len(),
        "A requested event has no qualified native instance"
    );
    Ok(defaults)
}

/// Rebuild only the control and event storage. Locator and input declarations
/// remain owned by the caller and keep their existing allocation rows.
#[allow(clippy::cognitive_complexity)]
pub(crate) fn write(
    payload: &mut Payload,
    allocation: &mut Payload,
    root: &Node,
    envelopes: &[Payload],
) -> Result<Written> {
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    ensure!(
        payload.u32(instance - 4)? == 0x808084d7 && payload.u32(definition - 4)? == 0x808084e9,
        "Sequence envelope classes differ"
    );
    let owner = payload.u32(instance)?;
    ensure!(
        payload.u32(definition)? == owner
            && payload.u64(instance + 8)? == definition as u64
            && payload.u64(definition + 8)? == instance as u64,
        "Sequence root pair differs"
    );
    let mut controls = Vec::new();
    let mut events = Vec::new();
    flatten(root, None, &mut controls, &mut events, &mut BTreeSet::new())?;
    let needed = events
        .iter()
        .map(|event| event.class)
        .collect::<BTreeSet<_>>();
    let defaults = event_defaults(envelopes, &needed)?;
    let mut new_allocations = Vec::new();
    let ci = array(payload, instance + 0xa0, 0x808093e5, 48, controls.len())?;
    let cd = array(payload, definition + 0x158, 0x808093e6, 24, controls.len())?;
    let mut children = Vec::new();
    for (index, control) in controls.iter().enumerate() {
        let (dc, ic, size, kind) = match control.node.kind {
            Kind::Serial(_) | Kind::Repeat(_) => (0x808093d9, 0x808093d8, 0x58, 0),
            Kind::Parallel(_) => (0x808093d7, 0x808093d6, 0x58, 1),
            Kind::Range { .. } => (0x808093cd, 0x808093cc, 0x68, 6),
            Kind::Weighted(_) => (0x808093d5, 0x808093d4, 0x68, 2),
            Kind::Event(_) => unreachable!(),
        };
        let i = object(payload, ic, 0x60)?;
        let d = object(payload, dc, size)?;
        pair(payload, owner, ci[index], cd[index], 0x808093e5, 0x808093e6)?;
        relative(payload, ci[index] + 16, instance)?;
        pair(payload, owner, i, d, ic, dc)?;
        relative(payload, i + 16, ci[index])?;
        relative(payload, ci[index] + 32, i)?;
        relative(payload, cd[index] + 16, d)?;
        common(payload, d, control.node, control.parent, kind)?;
        if matches!(control.node.kind, Kind::Repeat(_)) {
            // 00588020 reaches 00588172 after serial-control retirement.
            // Byte +32 requests another activation through 0058DF80, unless
            // the caller is shutting the parent down.
            payload.0[d + 0x32] = 1;
        }
        if let Kind::Range { input, bounds, .. } = control.node.kind {
            ensure!(
                u64::from(input) < payload.u64(definition + 0x188)?,
                "Sequence scalar gate input is undeclared"
            );
            payload.0[d + 0x32] = 1;
            put(&mut payload.0, d + 0x58, &bounds[0].to_le_bytes())?;
            put(&mut payload.0, d + 0x5c, &bounds[1].to_le_bytes())?;
            put(&mut payload.0, d + 0x60, &input.to_le_bytes())?;
        }
        let selectors = array(payload, d + 0x38, 0x808093fb, 4, control.children.len())?;
        for (&at, &(event, index)) in selectors.iter().zip(&control.children) {
            put(&mut payload.0, at, &u16::from(event).to_le_bytes())?;
            put(&mut payload.0, at + 2, &u16::try_from(index)?.to_le_bytes())?;
        }
        if let Kind::Weighted(weights) = &control.node.kind {
            put(&mut payload.0, i + 0x50, &u32::MAX.to_le_bytes())?;
            for (&at, (weight, _)) in array(payload, d + 0x58, 0x8080000f, 4, weights.len())?
                .iter()
                .zip(weights)
            {
                put(&mut payload.0, at, &weight.to_le_bytes())?;
            }
        }
        children.push(node_allocation(ic));
    }
    new_allocations.push(Row::named(0xc78e66c7, 0x808093e5, controls.len(), children));
    let ei = array(payload, instance + 0xb0, 0x808093e5, 48, events.len())?;
    let ed = array(payload, definition + 0x168, 0x808093e6, 24, events.len())?;
    let mut children = Vec::new();
    let mut written = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        let (ic, size, isize, kind) = event_layout(event.class)?;
        let i = object(payload, ic, isize)?;
        let d = object(payload, event.class, size)?;
        put(&mut payload.0, i, &defaults[&event.class])?;
        pair(payload, owner, ei[index], ed[index], 0x808093e5, 0x808093e6)?;
        relative(payload, ei[index] + 16, instance)?;
        pair(payload, owner, i, d, ic, event.class)?;
        relative(payload, i + 16, ei[index])?;
        relative(payload, ei[index] + 32, i)?;
        relative(payload, ed[index] + 16, d)?;
        common(payload, d, event.node, Some(event.parent), kind)?;
        // Bodyless ability-completion records end at +38. Their selected slot is runtime
        // state supplied by the controller, so no definition selector exists to initialize.
        if size > 0x38 && kind != 0 {
            put(&mut payload.0, d + 0x38, &0xffu32.to_le_bytes())?;
        }
        written.insert(event.node.name, d);
        children.push(node_allocation(ic));
    }
    new_allocations.push(Row::named(0xe3cc5c98, 0x808093e5, events.len(), children));
    for (at, field, name, count) in [
        (
            instance + 0xe8,
            definition + 0x198,
            0xe3ea46ed,
            controls.len(),
        ),
        (
            instance + 0x100,
            definition + 0x19c,
            0x5c5bd59a,
            events.len(),
        ),
    ] {
        append(&mut payload.0, at, 0x8080000a, &vec![0xff; count * 2], 2)?;
        put(&mut payload.0, field, &u32::try_from(count)?.to_le_bytes())?;
        new_allocations.push(Row::named(name, 0x8080000a, count, Vec::new()));
    }
    // Preserve independent input, locator and presentation allocations. Their
    // nested relative pointers are relocated when the root array is copied.
    let replaced = [0xc78e66c7, 0xe3cc5c98, 0xe3ea46ed, 0x5c5bd59a];
    let retained = allocation
        .array(32, 40, Some(0x80808852))?
        .into_iter()
        .filter(|&at| {
            allocation
                .u32(at)
                .is_ok_and(|name| !replaced.contains(&name))
        })
        .collect::<Vec<_>>();
    let old_rows = retained
        .iter()
        .map(|&at| allocation.bytes::<40>(at))
        .collect::<Result<Vec<_>>>()?;
    Row::write(&new_allocations, allocation, 32)?;
    let new_rows = allocation.array(32, 40, Some(0x80808852))?;
    let mut joined = new_rows
        .iter()
        .map(|&at| (at, allocation.bytes::<40>(at)))
        .map(|(at, bytes)| bytes.map(|bytes| (at, bytes)))
        .collect::<Result<Vec<_>>>()?;
    joined.extend(retained.into_iter().zip(old_rows));
    let rows = array(allocation, 32, 0x80808852, 40, joined.len())?;
    for (&to, (from, bytes)) in rows.iter().zip(joined) {
        put(&mut allocation.0, to, &bytes)?;
        for delta in [8, 32] {
            let offset = i64::from_le_bytes(bytes[delta..delta + 8].try_into()?);
            if offset != 0 {
                let target = usize::try_from(i64::try_from(from + delta)? + offset)?;
                relative(allocation, to + delta, target)?;
            }
        }
    }
    put(&mut payload.0, definition + 0x2d0, &0u64.to_le_bytes())?;
    put(&mut payload.0, definition + 0x2d8, &0u64.to_le_bytes())?;
    for p in [payload, allocation] {
        let length = u64::try_from(p.0.len())?;
        put(&mut p.0, 0, &length.to_le_bytes())?;
    }
    Ok(Written { events: written })
}
