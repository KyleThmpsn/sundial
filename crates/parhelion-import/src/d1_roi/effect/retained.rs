//! Open inputs of retained stock sequences.
//!
//! A retained stock sequence evaluates every input its stock graph connects, and the source names
//! only its own. An input left open reads the bank channel of its own name, which the bank
//! inherits from the default namespace when it does not declare it, as stock graph `80B810D2`
//! wires presentation sequence `80FA302E`: each of its seven inputs reads the bank channel of its
//! name, five of them inherited. An input without a provider leaves its channel container's
//! source empty, and the channel consumer faults when the sequence first reads it.
use super::*;

/// A retained sequence's open input: its owner, input definition and name.
type Open = (u32, usize, u32);

/// Connect every open input of a retained stock sequence to the graph's channel bank, which first
/// inherits the names it does not declare and is then sealed.
pub(super) fn connect(
    native: &mut Reader,
    components: &mut [Component],
    rows: &mut Vec<links::Connection>,
) -> Result<()> {
    let open = open_inputs(components, rows)?;
    if open.is_empty() {
        return Ok(());
    }
    let bank = components
        .iter_mut()
        .find(|component| component.family == Family::Channel)
        .context("A retained stock sequence's open inputs need a channel bank")?;
    let missing = inherit(native, bank, &open)?;
    let connected = wire(bank, open, rows)?;
    let sealed = if missing.is_empty() {
        Value::Null
    } else {
        seal(bank)?
    };
    if let Some(evidence) = bank.evidence.as_object_mut() {
        evidence.insert(
            "retained_sequence_inputs".into(),
            json!({"inherited":missing.iter().map(|name| format!("{name:08X}")).collect::<Vec<_>>(),
            "connected":connected,"sealed":sealed}),
        );
    }
    Ok(())
}

/// Every input of a retained stock sequence that no row connects, in component and declaration
/// order, main inputs before auxiliary ones.
fn open_inputs(components: &[Component], rows: &[links::Connection]) -> Result<Vec<Open>> {
    let mut open = Vec::new();
    for component in components.iter().filter(|component| {
        component.family == Family::Sequence
            && component.evidence.get("retained_stock_owner").is_some()
    }) {
        let (_, definition) = pair(&component.payload)?;
        let mut definitions = component
            .payload
            .array(definition + 0x188, 40, Some(0x80809789))?;
        definitions.extend(
            component
                .payload
                .array(definition + 0x2B8, 40, Some(0x80809789))?,
        );
        for definition in definitions {
            let connected = rows.iter().any(|row| {
                row.consumer.object.is_some_and(|object| {
                    object.owner == component.id
                        && object.class == 0x80809789
                        && object.offset == definition as u64
                })
            });
            if !connected {
                open.push((
                    component.id,
                    definition,
                    component.payload.u32(definition + 0x20)?,
                ));
            }
        }
    }
    Ok(open)
}

/// The channel names a bank declares, in declaration order.
fn declared(bank: &Payload) -> Result<Vec<u32>> {
    let definition = bank.pointer(24)?;
    bank.array(definition + 0xD8, 112, Some(0x808097A1))?
        .into_iter()
        .map(|row| bank.u32(row))
        .collect()
}

/// Extend the bank with an inherited declaration for each open name it lacks, in name order, and
/// return those names. Only a private bank can grow.
fn inherit(native: &mut Reader, bank: &mut Component, open: &[Open]) -> Result<Vec<u32>> {
    let names = declared(&bank.payload)?;
    let missing = open
        .iter()
        .map(|&(_, _, name)| name)
        .filter(|name| !names.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        ensure!(
            !bank.symbol.is_empty(),
            "A retained stock bank cannot inherit a retained sequence's open inputs"
        );
        let (tag, allocation) = match &bank.allocation {
            Some((tag, allocation)) => (*tag, allocation.clone()),
            None => {
                let tag = bank.payload.u32(0x44)?;
                (tag, (*native.tag(tag, Some(0x80809BBB))?).clone())
            }
        };
        let (payload, allocation) =
            crate::tiger::channel::inherited::extend(&bank.payload, &allocation, &missing)?;
        bank.payload = payload;
        bank.allocation = Some((tag, allocation));
    }
    Ok(missing)
}

/// A row from each open input to the bank channel of its name, with the evidence of each.
fn wire(
    bank: &Component,
    open: Vec<Open>,
    rows: &mut Vec<links::Connection>,
) -> Result<Vec<Value>> {
    let names = declared(&bank.payload)?;
    let (_, definition) = pair(&bank.payload)?;
    let mut connected = Vec::new();
    for (owner, input, name) in open {
        let channel = names
            .iter()
            .position(|&declared| declared == name)
            .context("Retained sequence input has no bank channel")?;
        rows.push(links::Connection {
            consumer: endpoint(owner, 0x80809789, input)?,
            provider: endpoint(bank.id, 0x808097C2, definition + 0x60)?,
            channel: u32::try_from(channel)?,
            flags: 0,
        });
        connected.push(json!({"input":input,"name":format!("{name:08X}"),"channel":channel}));
    }
    Ok(connected)
}

/// The client copies a bank instance only up to its definition, and the extension appended the
/// grown output and link arrays after it. Without a second definition view past them, live
/// links land in the neighbouring object and the stock provider's forwarder `005814B0` later
/// follows that garbage. Rows keep the original definition offsets, which sealing leaves valid.
fn seal(bank: &mut Component) -> Result<Value> {
    let layout = crate::tiger::instance::LAYOUTS
        .iter()
        .find(|layout| layout.symbol == "object-channels")
        .context("Channel instance layout")?;
    let mut patches = bank
        .patches
        .iter()
        .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
        .collect::<Vec<_>>();
    let sealed = crate::tiger::instance::seal(&mut bank.payload, &mut patches, layout)?
        .unwrap_or(Value::Null);
    bank.patches = patches
        .iter()
        .map(|patch| {
            Ok((
                usize::try_from(patch["offset"].as_u64().context("Bank patch offset")?)?,
                patch["symbol"]
                    .as_str()
                    .context("Bank patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<_>>()?;
    Ok(sealed)
}
