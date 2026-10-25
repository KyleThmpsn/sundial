//! Private native pulse damage, finite timing and imported visual dependencies.
use super::*;

type Patches = BTreeMap<u32, Vec<Value>>;

fn visual_table(
    manager: &PackageManager,
    table: &mut [u8],
    visual: &str,
) -> Result<Vec<Value>, String> {
    let original = Payload(table.to_vec());
    let mut patches = Vec::new();
    for branch in [0x80, 0x98, 0xB0] {
        for group in original
            .array(branch, 24, Some(0x80808BD5))
            .map_err(error)?
        {
            for row in original
                .array(group + 8, 32, Some(0x80808BD7))
                .map_err(error)?
            {
                visual_row(manager, &original, table, row, visual, &mut patches)?;
            }
        }
    }
    if patches.is_empty() {
        return Err("Native pulse table has no effect graph".into());
    }
    Ok(patches)
}

fn visual_row(
    manager: &PackageManager,
    original: &Payload,
    table: &mut [u8],
    row: usize,
    visual: &str,
    patches: &mut Vec<Value>,
) -> Result<(), String> {
    let mut written = false;
    for field in original
        .array(row + 8, 4, Some(0x80800014))
        .map_err(error)?
    {
        let target = original.u32(field).map_err(error)?;
        match manager
            .get_entry(TagHash(target))
            .map(|entry| entry.reference)
        {
            Some(0x80809C0F) => {
                write(table, field, &u32::MAX.to_le_bytes())?;
                if !written {
                    patches.push(json!({"offset":field,"symbol":visual}));
                    written = true;
                }
            }
            Some(0x80809802) => write(table, field, &u32::MAX.to_le_bytes())?,
            _ => {}
        }
    }
    Ok(())
}

fn repeat(
    owner: &mut [u8],
    original: &Payload,
    definition: usize,
    count: u8,
) -> Result<(), String> {
    let controls = original
        .array(definition + 0x158, 24, Some(0x808093E6))
        .map_err(error)?;
    let mut loops = 0;
    for row in controls {
        let at = original.pointer(row + 16).map_err(error)?;
        if original.u32(at - 4).map_err(error)? != 0x808093D9 {
            continue;
        }
        let children = original
            .array(at + 0x38, 4, Some(0x808093FB))
            .map_err(error)?;
        if children.len() != 2
            || original.bytes::<4>(children[0]).map_err(error)? != [1, 0, 0, 0]
            || original.bytes::<4>(children[1]).map_err(error)? != [0, 0, 2, 0]
        {
            return Err("Native pulse sequence topology differs".into());
        }
        write(owner, at + 0x31, &[count, 0])?;
        write(owner, children[0], &[0, 0, 2, 0])?;
        write(owner, children[1], &[1, 0, 0, 0])?;
        loops += 1;
    }
    if loops != 1 {
        return Err("Native pulse sequence needs one serial flow".into());
    }
    Ok(())
}

fn sequence(
    manager: &PackageManager,
    directory: &Path,
    nodes: &mut Vec<Value>,
    owner: &mut [u8],
    source: &source::Source,
    visual: &str,
) -> Result<Vec<Value>, String> {
    let original = Payload(owner.to_vec());
    let definition = original.pointer(24).map_err(error)?;
    repeat(owner, &original, definition, source.pulses)?;
    let events = original
        .array(definition + 0x168, 24, Some(0x808093E6))
        .map_err(error)?;
    let mut patches = Vec::new();
    let mut delays = 0;
    let mut tables = 0;
    for row in events {
        let at = original.pointer(row + 16).map_err(error)?;
        match original.u32(at - 4).map_err(error)? {
            0x808093CB => {
                write(owner, at + 0x18, &0u32.to_le_bytes())?;
                for field in [0x1C, 0x20] {
                    write(owner, at + field, &source.interval.to_le_bytes())?;
                }
                for field in [0x24, 0x28] {
                    write(owner, at + field, &0f32.to_le_bytes())?;
                }
                delays += 1;
            }
            0x80804AB4 => {
                let tag = original.u32(at + 0x50).map_err(error)?;
                let mut table = checked_tag(manager, tag, 0x80808BCD)?;
                let table_patches = visual_table(manager, &mut table, visual)?;
                let symbol = format!("pulse-table-{tag:08X}");
                node(directory, nodes, &symbol, tag, &table, table_patches)?;
                write(owner, at + 0x50, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":at+0x50,"symbol":symbol}));
                tables += 1;
            }
            0x80804AB9 => {}
            _ => return Err("Native pulse sequence has an unsupported event".into()),
        }
    }
    if delays != 1 || tables != 1 {
        return Err("Native pulse delay or visual table differs".into());
    }
    Ok(patches)
}

fn damage(
    manager: &PackageManager,
    directory: &Path,
    nodes: &mut Vec<Value>,
    graph: &[u8],
    owners: &mut BTreeMap<u32, Vec<u8>>,
) -> Result<Patches, String> {
    let mut patches = Patches::new();
    let mut profiles = BTreeMap::new();
    for (place, profile) in ability_damage::references(manager, ROOT, graph)? {
        let symbol = format!("pulse-damage-{:08X}", place.graph);
        if let std::collections::btree_map::Entry::Vacant(entry) = profiles.entry(place.graph) {
            let mut bytes = manager.read_tag(TagHash(place.graph))?;
            ability_damage::retype(&mut bytes, profile, u32::MAX, ability_damage::KINETIC)?;
            node(
                directory,
                nodes,
                &symbol,
                place.graph,
                &bytes,
                vec![
                    json!({"offset":profile.root,"symbol":symbol}),
                    json!({"offset":profile.paired,"symbol":symbol}),
                ],
            )?;
            entry.insert(symbol.clone());
        }
        let bindings = weapon_component_bindings(graph, place.binding_hash)?;
        let binding = bindings
            .iter()
            .find(|binding| {
                binding.owner_tag == place.owner
                    && binding.resource_index == usize::from(place.resource_index)
            })
            .ok_or("Damage profile resource binding is missing")?;
        let offset = usize::try_from(binding.resource_offset)
            .map_err(error)?
            .checked_add(usize::try_from(place.offset).map_err(error)?)
            .ok_or("Damage profile offset overflows")?;
        let owner = owners
            .get_mut(&place.owner)
            .ok_or("Damage profile belongs to an unexpected pulse owner")?;
        if read_u32(owner, offset).map_err(error)? != place.graph {
            return Err("Damage profile resource points elsewhere".into());
        }
        write(owner, offset, &u32::MAX.to_le_bytes())?;
        patches
            .entry(place.owner)
            .or_default()
            .push(json!({"offset":offset,"symbol":symbol}));
    }
    if profiles.is_empty() {
        return Err("Pulse graph has no native damage profile".into());
    }
    Ok(patches)
}

fn relocate(
    bytes: &mut [u8],
    names: &BTreeMap<u32, String>,
    patches: &mut Vec<Value>,
) -> Result<(), String> {
    for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
        if let Some(symbol) = names.get(&read_u32(bytes, at).map_err(error)?) {
            write(bytes, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":at,"symbol":symbol}));
        }
    }
    Ok(())
}

pub(super) fn prepare(
    manager: &PackageManager,
    directory: &Path,
    manifest: &mut Value,
    source: &source::Source,
) -> Result<String, String> {
    let visual = manifest["attachments"]["roots"][0]
        .as_str()
        .ok_or("Imported visual root is missing")?
        .to_owned();
    let mut graph = checked_tag(manager, ROOT, 0x80809C0F)?;
    let (count, _, rows, class) = array_at(&graph, 16).map_err(error)?;
    if class != 0x80809C04 || count == 0 {
        return Err("Pulse graph has no component table".into());
    }
    let mut owners = BTreeMap::new();
    let mut names = BTreeMap::new();
    for index in 0..count {
        let tag = read_u32(&graph, rows + index * 12).map_err(error)?;
        owners.insert(tag, checked_tag(manager, tag, 0x80809C36)?);
        names.insert(tag, format!("pulse-owner-{tag:08X}"));
    }
    let nodes = manifest["attachments"]["nodes"]
        .as_array_mut()
        .ok_or("Imported attachment nodes are missing")?;
    let mut patches = damage(manager, directory, nodes, &graph, &mut owners)?;
    let mut sequences = 0;
    for (&tag, owner) in &mut owners {
        let definition = relative_target(owner, 24).map_err(error)?;
        if read_u32(owner, definition - 4).map_err(error)? == 0x808084E9 {
            patches
                .entry(tag)
                .or_default()
                .extend(sequence(manager, directory, nodes, owner, source, &visual)?);
            sequences += 1;
        }
    }
    if sequences != 1 {
        return Err("Pulse graph needs one native sequence".into());
    }
    for (&tag, owner) in &mut owners {
        let mut edits = patches.remove(&tag).unwrap_or_default();
        relocate(owner, &names, &mut edits)?;
        node(directory, nodes, &names[&tag], tag, owner, edits)?;
    }
    let mut edits = Vec::new();
    relocate(&mut graph, &names, &mut edits)?;
    let symbol = "pulse-root";
    node(directory, nodes, symbol, ROOT, &graph, edits)?;
    manifest["attachments"]["roots"]
        .as_array_mut()
        .ok_or("attachment roots")?
        .push(json!(symbol));
    Ok(symbol.into())
}
