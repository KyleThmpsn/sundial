//! Follow item selection and private response operands without using prepared patch offsets.
use super::*;
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};
mod bank;
mod parameters;
mod selection;

type Destination = (usize, u32, usize, u8);
type Operands = BTreeMap<Destination, Vec<u8>>;

fn materials(
    p: &Payload,
    old: bool,
    mut name: impl FnMut(u32) -> Result<u32>,
) -> Result<Vec<Vec<u32>>> {
    let d = p.pointer(24)?;
    p.array(
        d + if old { 0x220 } else { 0x218 },
        if old { 0x60 } else { 0x58 },
        None,
    )?
    .into_iter()
    .map(|group| {
        let mut tags = p
            .array(group + if old { 0x20 } else { 0x18 }, 16, None)?
            .into_iter()
            .map(|row| p.u32(row + 8))
            .collect::<Result<Vec<_>>>()?;
        tags.push(p.u32(group + if old { 0x38 } else { 0x30 })?);
        tags.into_iter()
            .map(|tag| if tag == u32::MAX { Ok(tag) } else { name(tag) })
            .collect()
    })
    .collect()
}

fn content_graph(reader: &mut Reader, entity: &[u8], key: u32) -> Result<Option<u32>> {
    let bindings = weapon_component_bindings(entity, 0x5F0DD954).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1,
        "Projectile oracle needs one content owner"
    );
    let p = reader.tag(bindings[0].owner_tag, Some(0x80809C36))?;
    let d = p.pointer(24)?;
    let rows = p.array(d + 0x240, 0x1C0, Some(0x80803ACF))?;
    let selected = rows
        .into_iter()
        .filter(|&r| p.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() <= 1,
        "Ambiguous native projectile content key"
    );
    let row = selected.first().copied().unwrap_or(d + 0x80);
    let graph = p.u32(row + 0xF0)?;
    if !matches!(graph, 0 | u32::MAX) {
        return Ok(Some(graph));
    }
    // Use the public Barrel slot decoder so both launch slots keep their format checks.
    let barrels = weapon_component_bindings(
        entity,
        sundial::package_authoring::entity::WEAPON_BARREL_COMPONENT_KEY,
    )
    .map_err(anyhow::Error::msg)?;
    ensure!(barrels.len() == 1, "Projectile oracle needs one Barrel");
    let owner = reader.tag(barrels[0].owner_tag, None)?;
    Ok(
        sundial::package_authoring::entity::barrel::projectile_slots(&owner.0, barrels[0])
            .map_err(anyhow::Error::msg)?
            .into_iter()
            .find_map(|(_, graph)| graph),
    )
}

fn movement(source: &Source, graph: u32) -> Result<(u32, Payload)> {
    let p = source.read(graph)?;
    let mut found = Vec::new();
    for r in p.array(32, 12, Some(0x80800715))? {
        let tag = p.u32(r)?;
        let owner = source.read(tag)?;
        if owner.u32(owner.pointer(24)? - 4)? == 0x808020AC {
            found.push((tag, owner));
        }
    }
    ensure!(found.len() == 1, "Source projectile movement is ambiguous");
    found.pop().context("Source movement")
}

fn operands(p: &Payload, old: bool) -> Result<Operands> {
    let d = p.pointer(24)?;
    let mut values = BTreeMap::new();
    for (group, g) in p
        .array(
            d + if old { 0x220 } else { 0x218 },
            if old { 0x60 } else { 0x58 },
            None,
        )?
        .into_iter()
        .enumerate()
    {
        for n in p.array(
            g + if old { 0x50 } else { 0x48 },
            if old { 0x1D8 } else { 0x188 },
            None,
        )? {
            let name = p.u32(n + if old { 24 } else { 16 })?;
            for (ordinal, row) in p
                .array(n + if old { 0x1C8 } else { 0x178 }, 16, None)?
                .into_iter()
                .enumerate()
            {
                let selector = p.u8(row + 8)?;
                let parameter = p.pointer(row)?;
                let width = match selector {
                    2 => 20,
                    10 => 8,
                    _ => continue,
                };
                let bytes =
                    p.0.get(parameter..parameter + width)
                        .context("Response parameter extent")?
                        .to_vec();
                ensure!(
                    values
                        .insert((group, name, ordinal, selector), bytes)
                        .is_none(),
                    "Duplicate response destination"
                );
            }
        }
    }
    ensure!(!values.is_empty(), "Empty response operand oracle");
    Ok(values)
}

type ResponseGroup = (u32, Vec<u32>, Vec<u32>);
type NodeShape = (u32, u64, Vec<u8>);
type GroupShape = (u32, Vec<NodeShape>);
type ScalarProgram = (Vec<u8>, Vec<u8>, Vec<u32>);

fn response_groups(p: &Payload, definition: usize, old: bool) -> Result<Vec<usize>> {
    p.array(
        definition + if old { 0x220 } else { 0x218 },
        if old { 0x60 } else { 0x58 },
        None,
    )
}

fn response_nodes(p: &Payload, group: usize, old: bool) -> Result<Vec<usize>> {
    p.array(
        group + if old { 0x50 } else { 0x48 },
        if old { 0x1D8 } else { 0x188 },
        None,
    )
}

fn response_actions(p: &Payload, node: usize, old: bool) -> Result<Vec<usize>> {
    p.array(node + if old { 0x1C8 } else { 0x178 }, 16, None)
}

/// Each response group's name, the named inputs of its gating scalar and the graphs its
/// selector 4 actions spawn. Group names can repeat, so the groups stay in tree order.
fn groups(p: &Payload, old: bool) -> Result<Vec<ResponseGroup>> {
    let d = p.pointer(24)?;
    let scalars = p.array(
        d + if old { 0x230 } else { 0x228 },
        if old { 0x58 } else { 0x50 },
        None,
    )?;
    let mut result = Vec::new();
    for group in response_groups(p, d, old)? {
        let index = p.u32(group + if old { 0x48 } else { 0x40 })?;
        let inputs = if index == u32::MAX {
            Vec::new()
        } else {
            let scalar = *scalars.get(index as usize).context("Group scalar index")?;
            // The named inputs follow the scalar's expression program.
            p.array(
                scalar + if old { 24 } else { 16 } + 48,
                if old { 48 } else { 40 },
                Some(if old { 0x80800927 } else { 0x80809789 }),
            )?
            .into_iter()
            .map(|row| p.u32(row + if old { 40 } else { 32 }))
            .collect::<Result<Vec<_>>>()?
        };
        let mut spawned = Vec::new();
        for node in response_nodes(p, group, old)? {
            for row in response_actions(p, node, old)? {
                if p.u8(row + 8)? == 4 {
                    spawned.push(p.u32(p.pointer(row)? + 16)?);
                }
            }
        }
        result.push((
            p.u32(group + if old { 0x18 } else { 0x10 })?,
            inputs,
            spawned,
        ));
    }
    Ok(result)
}

/// Each group's name with its nodes' names, event masks and action selectors, in tree order.
/// Rise of Iron selectors 13, 15 and 16 are numbered 14, 16 and 17 in Shadowkeep.
fn shapes(p: &Payload, old: bool) -> Result<Vec<GroupShape>> {
    let d = p.pointer(24)?;
    response_groups(p, d, old)?
        .into_iter()
        .map(|group| {
            let nodes = response_nodes(p, group, old)?
                .into_iter()
                .map(|node| {
                    let selectors = response_actions(p, node, old)?
                        .into_iter()
                        .map(|row| {
                            let selector = p.u8(row + 8)?;
                            Ok(if old && matches!(selector, 13 | 15 | 16) {
                                selector + 1
                            } else {
                                selector
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok((
                        p.u32(node + if old { 0x18 } else { 0x10 })?,
                        p.u64(node + if old { 0x38 } else { 0x28 })?,
                        selectors,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((p.u32(group + if old { 0x18 } else { 0x10 })?, nodes))
        })
        .collect()
}

/// A filter's label names for match any, require all, exclude any and exclude all together.
type Lists = [Vec<u32>; 4];

/// Each response node's label filter and those of its attachment and spawn actions, in tree order
/// with their offsets. Node filters sit at source `+40` and native `+30`. Selector 8 filters sit at
/// `+10`, Rise of Iron 15 at `+10` and Shadowkeep 16 at `+18`. Rise of Iron 13 and Shadowkeep 14
/// hold one at `+08` in each row. Unnamed source rows carry no label.
fn filters(p: &Payload, old: bool) -> Result<Vec<(usize, Lists)>> {
    let d = p.pointer(24)?;
    let mut result = Vec::new();
    for group in response_groups(p, d, old)? {
        for node in response_nodes(p, group, old)? {
            let mut places = vec![node + if old { 0x40 } else { 0x30 }];
            for row in response_actions(p, node, old)? {
                let parameter = p.pointer(row)?;
                match (old, p.u8(row + 8)?) {
                    (_, 8) | (true, 15) => places.push(parameter + 0x10),
                    (false, 16) => places.push(parameter + 0x18),
                    (true, 13) | (false, 14) => places.extend(
                        p.array(parameter, if old { 0xF8 } else { 0x80 }, None)?
                            .into_iter()
                            .map(|row| row + 8),
                    ),
                    _ => {}
                }
            }
            for at in places {
                let mut lists = Lists::default();
                for (operation, names) in lists.iter_mut().enumerate() {
                    for label in p.array(at + operation * 16, 24, None)? {
                        let name = p.u32(label)?;
                        if name != 0x811C9DC5 {
                            names.push(name);
                        }
                    }
                }
                result.push((at, lists));
            }
        }
    }
    Ok(result)
}

/// Whether the native filter at `at` stores the masks its names compile to in registry
/// `80C70CA1`, where atom `i` sets bit `i` and a group its stored 40-byte mask. Mode 0 keeps match
/// any and exclude any in a `808093F6` record, mode 1 all four in `808093F5`, each with an empty
/// flag per mask after them, and mode 255 none.
fn compiled(p: &Payload, at: usize, lists: &Lists, registry: &Payload) -> Result<bool> {
    let mut masks = BTreeMap::new();
    for (index, row) in registry
        .array(8, 4, Some(0x80800070))?
        .into_iter()
        .enumerate()
    {
        let mut mask = [0u8; 40];
        mask[index / 8] = 1 << (index % 8);
        masks.insert(registry.u32(row)?, mask);
    }
    for row in registry.array(24, 44, Some(0x808094BE))? {
        masks.insert(registry.u32(row)?, registry.bytes::<40>(row + 4)?);
    }
    let mut expected = [[0u8; 40]; 4];
    for (mask, names) in expected.iter_mut().zip(lists) {
        for name in names {
            let bits = masks.get(name).context("Staged label is not registered")?;
            for (out, bit) in mask.iter_mut().zip(bits) {
                *out |= bit;
            }
        }
    }
    let mut stored = [[0u8; 40]; 4];
    let mode = p.u8(at + 0x50)?;
    if mode != 0xFF {
        let record = p.pointer(at + 0x58)?;
        let operations: &[usize] = match (mode, p.u32(record - 4)?) {
            (0, 0x808093F6) => &[0, 2],
            (1, 0x808093F5) => &[0, 1, 2, 3],
            _ => anyhow::bail!("Staged label predicate mode or record differs"),
        };
        for (slot, &operation) in operations.iter().enumerate() {
            if p.u8(record + operations.len() * 40 + slot)? == 0 {
                stored[operation] = p.bytes::<40>(record + slot * 40)?;
            }
        }
    }
    Ok(stored == expected)
}

/// Whether no graph the source movement creates mentions `name`: the graphs its selector 4, 8, 13
/// and 15 responses name and its stop-child and final-impact graphs at definition `+118` and
/// `+180`, with every owner each lists.
fn unread(source: &Source, movement: &Payload, name: u32) -> Result<bool> {
    let d = movement.pointer(24)?;
    let mut graphs = BTreeSet::new();
    for at in [0x118, 0x180] {
        graphs.insert(movement.u32(d + at)?);
    }
    for group in movement.array(d + 0x220, 0x60, None)? {
        for node in movement.array(group + 0x50, 0x1D8, None)? {
            for row in movement.array(node + 0x1C8, 16, None)? {
                let parameter = movement.pointer(row)?;
                match movement.u8(row + 8)? {
                    4 => graphs.insert(movement.u32(parameter + 16)?),
                    8 | 15 => graphs.insert(movement.u32(parameter + 8)?),
                    13 => {
                        for row in movement.array(parameter, 0xF8, None)? {
                            graphs.insert(movement.u32(row + 0xF0)?);
                        }
                        false
                    }
                    _ => false,
                };
            }
        }
    }
    let mentions = |p: &Payload| -> Result<bool> {
        for at in (0..p.0.len().saturating_sub(3)).step_by(4) {
            if p.u32(at)? == name {
                return Ok(true);
            }
        }
        Ok(false)
    };
    for graph in graphs {
        if matches!(graph, 0 | u32::MAX) {
            continue;
        }
        let p = source.read(graph)?;
        if mentions(&p)? {
            return Ok(false);
        }
        for row in p.array(32, 12, None)? {
            if mentions(&source.read(p.u32(row)?)?)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// A profile's category names, source rows at root `+C8` and native rows at root `+60`.
fn categories(p: &Payload, old: bool) -> Result<BTreeSet<u32>> {
    let root = p.pointer(24)?;
    p.array(root + if old { 0xC8 } else { 0x60 }, 24, None)?
        .into_iter()
        .map(|row| p.u32(row))
        .collect()
}

/// The damage profiles a movement names: its direct-hit and secondary profiles, then each
/// selector 9 response's profile in tree order.
fn profiles(p: &Payload, old: bool) -> Result<Vec<u32>> {
    let d = p.pointer(24)?;
    let mut tags = vec![
        p.u32(d + if old { 0x128 } else { 0x118 })?,
        p.u32(d + if old { 0x1E0 } else { 0x1D0 })?,
    ];
    for group in p.array(
        d + if old { 0x220 } else { 0x218 },
        if old { 0x60 } else { 0x58 },
        None,
    )? {
        for node in p.array(
            group + if old { 0x50 } else { 0x48 },
            if old { 0x1D8 } else { 0x188 },
            None,
        )? {
            for row in p.array(node + if old { 0x1C8 } else { 0x178 }, 16, None)? {
                if p.u8(row + 8)? == 9 {
                    tags.push(p.u32(p.pointer(row)? + 8)?);
                }
            }
        }
    }
    Ok(tags)
}

/// Lower and upper amounts, random extent and damage type. Rise of Iron reaches the record through
/// relative definition field `+18`, Shadowkeep stores it inline at `+14`.
fn amount(p: &Payload, old: bool) -> Result<[u8; 13]> {
    let d = p.pointer(24)?;
    p.bytes::<13>(if old { p.pointer(d + 24)? } else { d + 20 })
}

/// The scalar each group gate reads, in tree order, then the computed scalar at source definition
/// `+138` and native `+128`. An absent index reads none.
#[allow(clippy::type_complexity)]
fn gates(
    p: &Payload,
    old: bool,
    scalars: &[ScalarProgram],
) -> Result<Vec<Option<(Vec<u8>, Vec<u8>, Vec<u32>)>>> {
    let d = p.pointer(24)?;
    let mut indices = p
        .array(
            d + if old { 0x220 } else { 0x218 },
            if old { 0x60 } else { 0x58 },
            None,
        )?
        .into_iter()
        .map(|group| p.u32(group + if old { 0x48 } else { 0x40 }))
        .collect::<Result<Vec<_>>>()?;
    indices.push(p.u32(d + if old { 0x138 } else { 0x128 })?);
    indices
        .into_iter()
        .map(|index| {
            Ok(if index == u32::MAX {
                None
            } else {
                Some(
                    scalars
                        .get(index as usize)
                        .context("Scalar index exceeds the scalars")?
                        .clone(),
                )
            })
        })
        .collect()
}

/// Root scalar programs, constants and named inputs in array order.
fn scalars(p: &Payload, old: bool) -> Result<Vec<ScalarProgram>> {
    let d = p.pointer(24)?;
    p.array(
        d + if old { 0x230 } else { 0x228 },
        if old { 0x58 } else { 0x50 },
        None,
    )?
    .into_iter()
    .map(|row| {
        let at = row + if old { 24 } else { 16 };
        let code = p
            .array(at, 1, None)?
            .into_iter()
            .map(|byte| p.u8(byte))
            .collect::<Result<Vec<_>>>()?;
        let mut constants = Vec::new();
        for constant in p.array(at + 16, 16, None)? {
            constants.extend_from_slice(
                p.0.get(constant..constant + 16)
                    .context("Scalar constant")?,
            );
        }
        let names = p
            .array(at + 48, if old { 48 } else { 40 }, None)?
            .into_iter()
            .map(|input| p.u32(input + if old { 40 } else { 32 }))
            .collect::<Result<Vec<_>>>()?;
        Ok((code, constants, names))
    })
    .collect()
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    staged: &mut Reader,
    receipt: &Value,
) -> Result<Value> {
    if receipt["graph"]["kind"] != "weapon" {
        return Ok(Value::Null);
    }
    let status = &receipt["graph"]["firing_translation"]["projectile"]["status"];
    ensure!(
        receipt["case"]["require_private_projectile_bank"] != true
            || (status == "retained_native"
                && receipt["graph"]["firing_translation"]["projectile"]["private_bank"] == true),
        "Required private projectile bank was not prepared"
    );
    if let Some(required) = receipt["case"]["required_projectile"].as_str() {
        ensure!(
            status == required,
            "Required projectile route {required} was not prepared"
        );
    }
    let stock_manager = sundial::package_authoring::PackageManager::new(
        &stock.manager.package_dir,
        stock.manager.version,
        Some(stock.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let staged_manager = sundial::package_authoring::PackageManager::new(
        &staged.manager.package_dir,
        staged.manager.version,
        Some(staged.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let donor = runtime::load_weapon_runtime_entity_for_item_with_manager(
        &stock_manager,
        number(&receipt["donor"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let authored = runtime::load_weapon_runtime_entity_for_item_with_manager(
        &staged_manager,
        number(&receipt["item"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let actual = content_graph(
        staged,
        &authored.payload,
        authored.weapon_content_group_hash,
    )?;
    if status != "retained_native" {
        if receipt["case"]["required_projectile"] == "native_donor" {
            let expected = content_graph(stock, &donor.payload, donor.weapon_content_group_hash)?;
            ensure!(
                actual == expected,
                "Unsupported projectile changed donor selection"
            );
        }
        return Ok(json!({"route":status,"staged_graph":actual}));
    }
    let weapon = source
        .weapons()?
        .into_iter()
        .find(|w| Some(u64::from(w.hash)) == receipt["source"].as_u64())
        .context("Source weapon")?;
    let tables = source.classes(0x8080339A);
    ensure!(tables.len() == 1, "Ambiguous source patterns");
    let patterns = source.read(tables[0])?;
    let pattern = patterns.array(8, 32, Some(0x808033BC))?[usize::from(weapon.pattern)];
    let key = patterns.u32(pattern + 4)?;
    // The configured RoI build declares investment_assets with this generated schema.
    // Other 80800341 maps belong to activities rather than investment assignments.
    let assets = source.classes(0x810621AA);
    ensure!(assets.len() == 1, "Ambiguous source investment assets");
    let assignments = source.read(assets[0])?.u32(0x10)?;
    ensure!(
        source.reference(assignments)? == 0x80800341,
        "Source runtime assignment class differs"
    );
    let assignments = source.read(assignments)?;
    let pattern_key = patterns.u32(pattern)?;
    let roots = assignments
        .array(8, 8, None)?
        .into_iter()
        .filter(|&row| assignments.u32(row).ok() == Some(pattern_key))
        .map(|row| assignments.u32(row + 4))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        roots.len() == 1,
        "Source pattern has no unique runtime entity"
    );
    let root = roots[0];
    let entity = source.read(root)?;
    let (source_graph, source_selection) = selection::graph(source, &entity, key)?;
    if receipt["case"]["require_barrel_selection"] == true {
        ensure!(
            source_selection == "barrel",
            "Source projectile did not use the required Barrel slots"
        );
    }
    let (source_owner, from) = movement(source, source_graph)?;
    let carrier =
        content_graph(stock, &donor.payload, key)?.context("Retained native selection")?;
    let emitted = actual.context("Staged firing graph is missing")?;
    ensure!(
        emitted != carrier,
        "Retained projectile was not made private"
    );
    let original = stock.tag(carrier, Some(0x80809C0F))?;
    ensure!(
        staged.tag(carrier, Some(0x80809C0F))?.0 == original.0,
        "Stock projectile graph changed"
    );
    let private = staged.tag(emitted, Some(0x80809C0F))?;
    ensure!(
        receipt["graph"]["firing_translation"]["projectile"]["private_bank"] == true,
        "Retained projectile has no private bank"
    );
    let bindings = weapon_component_bindings(&private.0, 0x0437756D).map_err(anyhow::Error::msg)?;
    let original_bindings =
        weapon_component_bindings(&original.0, 0x0437756D).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1 && original_bindings.len() == 1,
        "Private projectile movement binding differs"
    );
    ensure!(
        bindings[0].owner_tag != original_bindings[0].owner_tag,
        "Movement owner is not private"
    );
    let to = staged.tag(bindings[0].owner_tag, Some(0x80809C36))?;
    let expected_materials = materials(&from, true, |tag| {
        ensure!(
            source.reference(tag)? == 0x80800519,
            "Source physical material class differs"
        );
        source.read(tag)?.u32(8)
    })?;
    let actual_materials = materials(&to, false, |tag| {
        let material = staged.tag(tag, Some(0x80808BCB))?;
        ensure!(
            stock.tag(tag, Some(0x80808BCB))?.0 == material.0,
            "Stock physical material changed"
        );
        material.u32(8)
    })?;
    ensure!(
        expected_materials == actual_materials,
        "Private projectile material selection differs from source"
    );
    let original_movement = stock.tag(original_bindings[0].owner_tag, Some(0x80809C36))?;
    let child_parameters = parameters::check(&from, &original_movement, &to, |name| {
        unread(source, &from, name)
    })?;
    // Inputs of a source child parameter left out for want of a reader are not bank consumers.
    let omitted = child_parameters["omitted_unread"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            row["name"]
                .as_u64()
                .and_then(|name| u32::try_from(name).ok())
        })
        .collect::<Vec<_>>();
    let bank = bank::check(
        source,
        stock,
        staged,
        (source_graph, &omitted),
        &private,
        &original,
    )?;
    let original_materials = materials(&original_movement, false, |tag| {
        stock.tag(tag, Some(0x80808BCB))?.u32(8)
    })?;
    if receipt["case"]["require_material_retarget"] == true {
        ensure!(
            actual_materials != original_materials,
            "Required material selection was not changed"
        );
    }
    let expected = operands(&from, true)?;
    let actual = operands(&to, false)?;
    ensure!(
        expected == actual,
        "Private projectile response operands differ from source"
    );
    // A named group must keep the scalar gate the source reads, and spawn the stated native graph
    // where the source spawns its own. The spawned graph stays a stock resource.
    let (source_groups, staged_groups) = (groups(&from, true)?, groups(&to, false)?);
    // Groups only the carrier has are left out, so the staged tree pairs one to one with the
    // source's. An unnamed source group pairs by position.
    ensure!(
        source_groups.len() == staged_groups.len()
            && source_groups
                .iter()
                .zip(&staged_groups)
                .all(|(source, staged)| source.0 == staged.0 || source.0 == 0x811C9DC5),
        "Staged response groups do not pair with the source's"
    );
    // Every source root scalar reaches the staged movement with its source program, constants
    // and inputs. Rows that only left-out carrier groups read may remain, and a source forward the
    // carrier has no row for is appended. Each group gate and the computed scalar then read a
    // scalar with the source's content, following the stored indices.
    let (source_scalars, staged_scalars) = (scalars(&from, true)?, scalars(&to, false)?);
    let mut remaining = staged_scalars.clone();
    ensure!(
        source_scalars.iter().all(|scalar| {
            remaining
                .iter()
                .position(|staged| staged == scalar)
                .map(|at| remaining.remove(at))
                .is_some()
        }),
        "Staged root scalars do not carry the source programs and constants"
    );
    ensure!(
        gates(&from, true, &source_scalars)? == gates(&to, false, &staged_scalars)?,
        "Staged group gates or computed scalar read other scalars than the source's"
    );
    // Every staged node carries the source's name, event mask and action selectors in order.
    let (source_shape, staged_shape) = (shapes(&from, true)?, shapes(&to, false)?);
    ensure!(
        source_shape.len() == staged_shape.len()
            && source_shape
                .iter()
                .zip(&staged_shape)
                .all(|(source, staged)| {
                    (source.0 == staged.0 || source.0 == 0x811C9DC5) && source.1 == staged.1
                }),
        "Staged response nodes differ from the source's names, masks or actions"
    );
    // Every node, attachment and spawn filter names the source's labels, and its stored predicate
    // holds the masks those names compile to in the native registry.
    let (source_filters, staged_filters) = (filters(&from, true)?, filters(&to, false)?);
    ensure!(
        source_filters.len() == staged_filters.len()
            && source_filters
                .iter()
                .zip(&staged_filters)
                .all(|(source, staged)| source.1 == staged.1),
        "Staged response filters name other labels than the source's"
    );
    let registry = staged.tag(0x80C70CA1, None)?;
    for (at, lists) in &staged_filters {
        ensure!(
            compiled(&to, *at, lists, &registry)?,
            "Staged response filter masks differ from its labels"
        );
    }
    // The movement allocation keeps one child list per staged group, counting its nodes.
    let definition = to.pointer(24)?;
    let nodes = to
        .array(definition + 0x218, 0x58, Some(0x808037CD))?
        .into_iter()
        .map(|group| Ok(to.array(group + 0x48, 0x188, None)?.len()))
        .collect::<Result<Vec<_>>>()?;
    let allocation = staged.tag(to.u32(0x44)?, None)?;
    let member = allocation
        .array(0x20, 40, None)?
        .into_iter()
        .find(|&row| allocation.u32(row).ok() == Some(0x5680393D))
        .context("Movement allocation has no group member")?;
    let lists = allocation.array(member + 24, 40, None)?;
    ensure!(
        allocation.u32(member + 20)? as usize == nodes.len() && lists.len() == nodes.len(),
        "Movement allocation group count differs from the staged groups"
    );
    // An empty group has no node list, as stock rocket group `984AF7B1` has none.
    for (list, &count) in lists.into_iter().zip(&nodes) {
        let children = allocation.array(list + 24, 40, None)?;
        ensure!(
            (count == 0 && children.is_empty())
                || (children.len() == 1
                    && allocation.u32(children[0])? == 0x5B2D36EB
                    && allocation.u32(children[0] + 20)? as usize == count),
            "Movement allocation node list differs from its staged group"
        );
    }
    let private_profiles = check_profiles(
        source,
        (stock, staged),
        (&from, &to, &original_movement),
        receipt["case"]["require_private_profile"] == true,
    )?;
    for entry in receipt["case"]["required_projectile_groups"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let field = |key: &str| -> Result<u32> {
            Ok(u32::from_str_radix(
                entry[key].as_str().context("Required group field")?,
                16,
            )?)
        };
        let (group, input, child) = (field("group")?, field("input")?, field("native_child")?);
        let gated = |groups: &[ResponseGroup]| {
            groups
                .iter()
                .filter(|(name, inputs, _)| *name == group && inputs.contains(&input))
                .map(|(_, _, spawned)| spawned.clone())
                .collect::<Vec<_>>()
        };
        ensure!(
            gated(&source_groups)
                .iter()
                .any(|spawned| !spawned.is_empty())
                && gated(&staged_groups)
                    .iter()
                    .any(|spawned| spawned.contains(&child))
                && stock.tag(child, Some(0x80809C0F))?.0 == staged.tag(child, None)?.0,
            "Required response group {group:08X} lost its {input:08X} gate or spawned graph"
        );
    }
    for row in original.array(16, 12, Some(0x80809C04))? {
        let tag = original.u32(row)?;
        ensure!(
            stock.tag(tag, None)?.0 == staged.tag(tag, None)?.0,
            "Stock projectile component changed"
        );
    }
    // A Rise of Iron `808030BA` provider's block of 0.3, zero, its value and a count at
    // definition `+B0` reaches the staged `8080378A` provider at `+9C`.
    let block = |p: &Payload, at: usize| -> Result<[u8; 16]> { p.bytes::<16>(p.pointer(24)? + at) };
    let class = |p: &Payload| -> Result<u32> { p.u32(p.pointer(24)? - 4) };
    let source_root = source.read(source_graph)?;
    let mut expected_providers = Vec::new();
    for row in source_root.array(32, 12, None)? {
        let owner = source.read(source_root.u32(row)?)?;
        if class(&owner)? == 0x808030BA {
            expected_providers.push(block(&owner, 0xB0)?);
        }
    }
    let mut staged_providers = Vec::new();
    for row in private.array(16, 12, Some(0x80809C04))? {
        let owner = staged.tag(private.u32(row)?, Some(0x80809C36))?;
        if class(&owner)? == 0x8080378A {
            staged_providers.push(block(&owner, 0x9C)?);
        }
    }
    ensure!(
        expected_providers.is_empty() || expected_providers == staged_providers,
        "Staged provider values differ from the source's"
    );
    Ok(
        json!({"route":"retained_native","source_graph":source_graph,"source_owner":source_owner,"source_selection":source_selection,
        "carrier_graph":carrier,"staged_graph":emitted,"staged_movement":bindings[0].owner_tag,
        "responses":actual.into_iter().map(|((group,node,ordinal,selector),bytes)|json!({"group":group,"node":node,"ordinal":ordinal,"selector":selector,"bytes":bytes})).collect::<Vec<_>>(),
        "source_materials":expected_materials,"carrier_materials":original_materials,"staged_materials":actual_materials,
        "child_parameters":child_parameters,
        "private_profiles":private_profiles,
        "bank":bank,
        "stock_graph_and_owners_preserved":true,"gameplay_verified":false}),
    )
}

fn check_profiles(
    source: &Source,
    (stock, staged): (&mut Reader, &mut Reader),
    (from, to, original_movement): (&Payload, &Payload, &Payload),
    required: bool,
) -> Result<Vec<Value>> {
    // Each staged profile deals its source profile's amounts and damage type. A profile the
    // carrier does not name is private, and the carrier's own stay stock.
    let source_profiles = profiles(from, true)?;
    let staged_profiles = profiles(to, false)?;
    let carrier_profiles = profiles(original_movement, false)?;
    ensure!(
        source_profiles.len() == staged_profiles.len(),
        "Staged profiles do not pair with the source's"
    );
    let mut private_profiles = Vec::new();
    for (&from_profile, &staged_profile) in source_profiles.iter().zip(&staged_profiles) {
        if matches!(from_profile, 0 | u32::MAX) {
            ensure!(
                from_profile == staged_profile,
                "Staged profile presence differs from the source's"
            );
            continue;
        }
        let (from, to) = (
            source.read(from_profile)?,
            staged.tag(staged_profile, Some(0x80809C36))?,
        );
        let actual = amount(&to, false)?;
        ensure!(
            amount(&from, true)? == actual,
            "Staged profile {staged_profile:08X} differs from the source amount or damage type"
        );
        ensure!(
            categories(&from, true)? == categories(&to, false)?,
            "Staged profile {staged_profile:08X} differs from the source categories"
        );
        if !carrier_profiles.contains(&staged_profile) {
            private_profiles.push(
                json!({"source":from_profile,"staged":staged_profile,"damage_type":actual[12]}),
            );
        }
    }
    for &profile in &carrier_profiles {
        if !matches!(profile, 0 | u32::MAX) {
            ensure!(
                stock.tag(profile, None)?.0 == staged.tag(profile, None)?.0,
                "Stock projectile profile changed"
            );
        }
    }
    ensure!(
        !required || !private_profiles.is_empty(),
        "Required private projectile profile was not staged"
    );
    Ok(private_profiles)
}
