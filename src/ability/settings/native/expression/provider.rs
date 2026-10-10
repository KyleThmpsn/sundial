//! Resolve the expression's actual bank output and its typed producer.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Provider {
    bank: u32,
    bank_definition: usize,
    ordinal: usize,
    owner: u32,
    definition: usize,
    source_class: u32,
    interface: u32,
    pub variation: bool,
}

struct Destination {
    owner: u32,
    class: u32,
    offset: usize,
    argument: u64,
}

fn wire(entity: &[u8], owner: u32, at: usize, class: u32) -> Result<Destination, String> {
    let mut data = Data::new(entity, 0)?;
    let components = data.array(16, 0x8080_9C04)?;
    let events = data.array(32, 0x8080_9BC9)?;
    let mut found = Vec::new();
    for row in events {
        if u32_at(entity, row + 8)? != owner
            || u32_at(entity, row + 12)? != class
            || u64_at(entity, row + 16)? != at as u64
        {
            continue;
        }
        for end in [row + 8, row + 40] {
            let index = usize::try_from(u64_at(entity, end + 16)?)
                .map_err(|_| "Provider component index overflows")?;
            let component = *components
                .get(index)
                .ok_or("Provider component is outside the graph")?;
            if u32_at(entity, component)? != u32_at(entity, end)? {
                return Err("Provider endpoint names a different component".into());
            }
        }
        found.push(Destination {
            owner: u32_at(entity, row + 40)?,
            class: u32_at(entity, row + 44)?,
            offset: usize::try_from(u64_at(entity, row + 48)?)
                .map_err(|_| "Provider endpoint overflows")?,
            argument: u64_at(entity, row + 64)?,
        });
    }
    if found.len() != 1 {
        return Err("Expression provider has no unique connection".into());
    }
    Ok(found.remove(0))
}

/// The connected producer is a member of the graph's declared parent arrays, rather than an
/// unrelated reciprocal pair that happens to sit in the same owner.
fn enclosed(
    graph: &WeaponRuntimeGraph,
    data: &mut Data<'_>,
    at: usize,
    definition: u32,
    source: u32,
) -> Result<bool, String> {
    for (owner, root) in roots(graph) {
        if owner != data.owner {
            continue;
        }
        let parent = root.owner_offset as usize;
        if definition == 0x8080_388F && root.schema == definition && parent == at {
            data.pair(at, definition, source)?;
            return Ok(true);
        }
        let (parent_source, arrays): (_, &[(usize, usize)]) = match root.schema {
            0x8080_8BF7 => (0x8080_8BF5, &[(0x78, 0x30)]),
            0x8080_8BE6 => (0x8080_8BF1, &[(0x90, 0x30), (0xA0, 0x40), (0xB0, 0x50)]),
            _ => continue,
        };
        let state = data.pair(parent, root.schema, parent_source)?;
        for &(d, s) in arrays {
            let Ok(definitions) = data.array(parent + d, definition) else {
                continue;
            };
            let Some(index) = definitions.iter().position(|row| *row == at) else {
                continue;
            };
            let sources = data.array(state + s, source)?;
            if definitions.len() != sources.len()
                || data.pair(at, definition, source)? != sources[index]
            {
                return Err("Expression producer arrays disagree".into());
            }
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn join(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
    entity: &[u8],
    owner: u32,
    declaration: usize,
    name: u32,
) -> Result<Provider, String> {
    let output = wire(entity, owner, declaration, 0x8080_9789)?;
    if output.class != 0x8080_97C2 {
        return Err("Expression provider is not a bank output".into());
    }
    let bytes = manager
        .read_tag(output.owner)
        .map_err(|error| error.to_string())?;
    let mut bank = Data::new(&bytes, output.owner)?;
    bytes_at::<24>(&bytes, output.offset)?;
    let definition = relative_offset(output.offset, 0, i64_at(&bytes, output.offset)?)?;
    let source = bank.pair(definition, 0x8080_9790, 0x8080_979F)?;
    let outputs = bank.array(definition + 0xD8, 0x8080_97A1)?;
    let vectors = bank.array(source + 0x50, 0x8080_0090)?;
    let inputs = bank.array(definition + 0xC8, 0x8080_97A8)?;
    let states = bank.array(source + 0x30, 0x8080_97A7)?;
    let ordinal = output.argument as u32 as i32;
    if !(0..64).contains(&ordinal) || outputs.len() != vectors.len() || inputs.len() != states.len()
    {
        return Err("Expression bank output has an incompatible ordinal".into());
    }
    let ordinal = ordinal as usize;
    let channel = *outputs
        .get(ordinal)
        .ok_or("Expression bank output is missing")?;
    let input = *inputs
        .get(ordinal)
        .ok_or("Expression bank input is missing")?;
    let dependencies = bank.array(channel + 80, 0x8080_000B)?;
    if u32_at(&bytes, channel)? != name
        || u64_at(&bytes, channel + 8)? != 0
        || u16::from_le_bytes(bytes_at(&bytes, channel + 0x48)?) != u16::MAX
        || dependencies.len() != 1
        || u64_at(&bytes, dependencies[0])? != 1_u64 << ordinal
        || bank.pair(input, 0x8080_97A8, 0x8080_97A7)? != states[ordinal]
    {
        return Err("Expression bank does not forward the selected producer".into());
    }
    let target = wire(entity, output.owner, input, 0x8080_97A8)?;
    if target.class != 0x8080_9AE1 {
        return Err("Expression input has an unsupported getter".into());
    }
    let producer = manager
        .read_tag(target.owner)
        .map_err(|error| error.to_string())?;
    let mut data = Data::new(&producer, target.owner)?;
    bytes_at::<24>(&producer, target.offset)?;
    let at = relative_offset(target.offset, 0, i64_at(&producer, target.offset)?)?;
    let paired =
        usize::try_from(u64_at(&producer, at + 8)?).map_err(|_| "Producer pair overflows")?;
    let sh = u32_at(&producer, at + 4)?;
    let dh = u32_at(&producer, paired + 4)?;
    let (method, variation) = match (dh, sh) {
        (0x8080_8BFA, 0x8080_8BF9) | (0x8080_8BEC, 0x8080_8BEB) => (1, false),
        (0x8080_388F, 0x8080_3B73) => (25, true),
        _ => return Err("Expression producer has no verified getter".into()),
    };
    data.pair(at, dh, sh)?;
    if !enclosed(graph, &mut data, at, dh, sh)?
        || !variation && u32_at(&producer, at + 0x28)? != name
    {
        return Err("Expression producer does not belong to the selected channel".into());
    }
    let interface = u32_at(&producer, target.offset + 8)?;
    let methods = manager
        .read_tag(interface)
        .map_err(|error| error.to_string())?;
    let mut interface_data = Data::new(&methods, 0)?;
    if u32_at(&methods, 8)? != 0x8080_9AE1 || u32_at(&methods, 12)? != 0x8080_9AE2 {
        return Err("Expression producer interface is incompatible".into());
    }
    let rows = interface_data.array(16, 0x8080_9C56)?;
    if rows.len() != 1
        || u32_at(&methods, rows[0])? != sh
        || u32_at(&methods, rows[0] + 4)? != method
    {
        return Err("Expression producer dispatches another method".into());
    }
    Ok(Provider {
        bank: output.owner,
        bank_definition: definition,
        ordinal,
        owner: target.owner,
        definition: at,
        source_class: sh,
        interface,
        variation,
    })
}
