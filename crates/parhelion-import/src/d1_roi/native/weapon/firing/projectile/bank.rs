//! Replace a qualified carrier's bank and rebuild its typed component connections.
//!
//! The private bank holds every source declaration, then the carrier's own declarations. Each
//! carrier connection keeps its consumer and channel name, now addressed in the private bank.
//! An unnamed settings input keeps the source's selection instead.
use super::*;
use crate::d1_roi::native::component::Component;
use crate::tiger::entity::{compiled, links::NativeRows};
use sundial::package_authoring::{ability_damage, sandbox_perk::program::NativeAssetResourcePatch};

fn names(owner: &graph::Owner) -> Result<Vec<u32>> {
    owner
        .payload
        .array(owner.definition + 0xD8, 112, None)?
        .into_iter()
        .map(|at| owner.payload.u32(at))
        .collect()
}

fn validate(source: &graph::Owner, native: &graph::Owner) -> Result<()> {
    // The shared bank writer does not translate component activation or host references.
    // Both ends must use the neutral base before substituting its native bank template.
    for (offset, value) in [
        (0x18, 0),
        (0x20, u64::from(u32::MAX)),
        (0x28, 0),
        (0x30, u64::from(u32::MAX)),
        (0x38, u64::from(u32::MAX)),
        (0x40, 0),
        (0x48, u64::from(u32::MAX)),
        (0x50, 0),
        (0x58, 0),
    ] {
        ensure!(
            source.payload.u64(source.definition + offset)? == value,
            "Source projectile bank has additional activation or host state"
        );
    }
    for (offset, value) in [
        (0x10, 0),
        (0x18, u64::MAX),
        (0x20, 0),
        (0x28, u64::MAX),
        (0x30, 0),
        (0x38, 0),
        (0x40, 0),
    ] {
        ensure!(
            native.payload.u64(native.definition + offset)? == value,
            "Native projectile bank has additional activation or host state"
        );
    }
    ensure!(
        source.payload.u64(source.instance + 0x70)? == 0,
        "Private projectile bank requires callback history translation"
    );
    for row in source
        .payload
        .array(source.definition + 0xD8, 112, Some(0x80800883))?
    {
        ensure!(
            source.payload.bytes::<2>(row + 104)? == [255, 254]
                && source.payload.u64(row + 96)? == 0,
            "Private projectile bank requires callback or interpolation translation"
        );
    }
    Ok(())
}

/// The name a carrier bank getter reads, from the incoming lookup at definition `+118`.
fn getter_name(native: &graph::Owner, offset: usize) -> Result<u32> {
    let inputs = native
        .payload
        .array(native.definition + 0xC8, 32, Some(0x808097A8))?;
    let ordinal = inputs
        .iter()
        .position(|&at| at == offset)
        .context("Private bank input is not a native getter")?;
    let lookup = native.payload.array(native.definition + 0x118, 12, None)?;
    let found = lookup
        .into_iter()
        .filter(|&at| native.payload.u32(at + 8).ok() == Some(ordinal as u32))
        .collect::<Vec<_>>();
    ensure!(found.len() == 1, "Private bank incoming name is ambiguous");
    native.payload.u32(found[0] + 4)
}

/// The private getter a carrier getter connection reads. A connection from a replaced receiver row
/// reads the getter of `renamed`, the source row's name.
fn incoming(
    source: &graph::Owner,
    native: &graph::Owner,
    bank: &Component,
    offset: usize,
    renamed: Option<u32>,
) -> Result<usize> {
    // A getter for a name only the carrier declares was rebuilt from the carrier's own.
    if renamed.is_none()
        && let Some(&(_, _, translated)) = bank
            .incoming
            .iter()
            .find(|(owner, old, _)| *owner == native.tag && *old == offset)
    {
        return Ok(translated);
    }
    let name = match renamed {
        Some(name) => name,
        None => getter_name(native, offset)?,
    };
    let source_names = names(source)?;
    let source_ordinal = source_names
        .iter()
        .position(|&value| value == name)
        .context("Native incoming getter has no source channel")?;
    let source_inputs = source
        .payload
        .array(source.definition + 0xC8, 40, Some(0x80800668))?;
    let definition = *source_inputs
        .get(source_ordinal)
        .context("Source incoming getter has another ordinal")?;
    let found = bank
        .incoming
        .iter()
        .filter(|(owner, old, _)| *owner == source.tag && *old == definition)
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Private incoming getter was not translated"
    );
    Ok(found[0].2)
}

fn publish(assembly: &mut Assembly<'_>, component: &mut Component) -> Result<()> {
    let symbol = component.symbol.clone();
    for at in (0..component.payload.0.len().saturating_sub(15)).step_by(4) {
        if component.payload.u32(at)? == component.id {
            ensure!(
                component.payload.u32(at + 4)? & 0xFF800000 == 0x80800000
                    && component.payload.u64(at + 8)? < component.payload.0.len() as u64,
                "Private projectile owner has an untyped identity reference"
            );
            component.patch(at, &symbol)?;
        }
    }
    let patches = component
        .patches
        .iter()
        .map(|p| {
            Ok((
                usize::try_from(
                    p["offset"]
                        .as_u64()
                        .context("Projectile relocation offset")?,
                )?,
                p["symbol"]
                    .as_str()
                    .context("Projectile relocation symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    assembly.node(
        &symbol,
        component.template_tag,
        &component.payload.0,
        None,
        &patches,
    )?;
    Ok(())
}

/// Connect the release flow's inputs to the private bank by name, and the bank's added getter to
/// the remote detonation provider. Rows copy a carrier row of the same consumer and provider
/// classes.
fn release_rows(
    carrier: &Payload,
    connections: &mut NativeRows,
    plan: &release::Plan,
    (bank, bank_index, bank_definition, bank_names): (&Component, usize, usize, &[u32]),
    (flow, flow_index): (&Component, usize),
    (provider, provider_index): (u32, usize),
) -> Result<()> {
    let template = |consumer: u32, producer: u32| -> Result<[u8; 72]> {
        for at in carrier.array(32, 72, Some(0x80809BC9))? {
            if carrier.u32(at + 12)? == consumer && carrier.u32(at + 44)? == producer {
                return carrier.bytes::<72>(at);
            }
        }
        anyhow::bail!("Carrier has no {consumer:08X} connection from {producer:08X}")
    };
    let input = template(0x80809789, 0x808097C2)?;
    for &(at, name) in &plan.inputs {
        let channel = bank_names
            .iter()
            .position(|&value| value == name)
            .context("Release flow input is absent from the private bank")?;
        let mut row = input;
        put(&mut row, 8, &flow.id.to_le_bytes())?;
        put(&mut row, 16, &(at as u64).to_le_bytes())?;
        put(&mut row, 24, &(flow_index as u64).to_le_bytes())?;
        put(&mut row, 40, &bank.id.to_le_bytes())?;
        put(
            &mut row,
            48,
            &((bank_definition + 0x60) as u64).to_le_bytes(),
        )?;
        put(&mut row, 56, &(bank_index as u64).to_le_bytes())?;
        put(&mut row, 64, &u32::try_from(channel)?.to_le_bytes())?;
        connections.connections.push(row.to_vec());
    }
    let getter = bank
        .incoming
        .iter()
        .find(|(owner, key, _)| *owner == plan.provider && *key == plan.getter)
        .map(|entry| entry.2)
        .context("Private bank has no release getter")?;
    let mut row = template(0x808097A8, 0x80809AE1)?;
    put(&mut row, 8, &bank.id.to_le_bytes())?;
    put(&mut row, 16, &(getter as u64).to_le_bytes())?;
    put(&mut row, 24, &(bank_index as u64).to_le_bytes())?;
    put(&mut row, 40, &provider.to_le_bytes())?;
    put(&mut row, 48, &(plan.getter as u64).to_le_bytes())?;
    put(&mut row, 56, &(provider_index as u64).to_le_bytes())?;
    put(&mut row, 64, &0u32.to_le_bytes())?;
    connections.connections.push(row.to_vec());
    Ok(())
}

/// Apply checked response, scalar and material patches to the private movement.
fn edit_movement(
    movement: &mut Component,
    native_movement: &graph::Owner,
    (edits, material_edits): (&[Value], &[NativeAssetResourcePatch]),
) -> Result<()> {
    let mut writes = BTreeSet::new();
    let mut patches = edits
        .iter()
        .cloned()
        .map(serde_json::from_value::<NativeAssetResourcePatch>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    patches.extend_from_slice(material_edits);
    for patch in patches {
        ensure!(
            patch.binding_hash == MOVEMENT
                && patch.resource_index == 0
                && patch.imported_particle.is_none()
                && patch.expected.len() == patch.bytes.len(),
            "Unqualified private movement edit"
        );
        let at = native_movement
            .instance
            .checked_add(patch.offset as usize)
            .context("Movement edit overflow")?;
        let end = at
            .checked_add(patch.bytes.len())
            .context("Movement edit extent overflow")?;
        ensure!(
            movement.payload.0.get(at..end) == Some(patch.expected.as_slice())
                && (at..end).all(|at| writes.insert(at)),
            "Private movement edit overlaps or its source changed"
        );
        put(&mut movement.payload.0, at, &patch.bytes)?;
    }
    Ok(())
}

/// Apply the response tree plan and leave out carrier-only groups, with a private allocation.
/// Returns each created named input's offset and channel name.
fn reshape(
    assembly: &mut Assembly<'_>,
    native: &graph::Graph,
    movement: &mut Component,
    (paired, dropped): (&[Option<usize>], &[usize]),
    plan: &edit::Plan,
) -> Result<Vec<(usize, u32)>> {
    let native_movement = native.owner(graph::Role::Movement)?;
    // Patches and plan edits address the carrier layout, so groups are left out after them.
    let mut created = Vec::new();
    if !dropped.is_empty() || !plan.is_empty() {
        ensure!(
            native.payload.u32(0x88)? == u32::MAX,
            "A replicated carrier cannot change its response tree"
        );
        let groups = native_movement
            .payload
            .array(native_movement.definition + 0x218, 0x58, None)?
            .len();
        let tag = movement.payload.u32(0x44)?;
        let mut allocation = (*assembly.native.tag(tag, None)?).clone();
        let length = movement.payload.0.len();
        let rebuilt;
        let spawned;
        (created, rebuilt, spawned) = edit::apply(
            plan,
            &mut movement.payload,
            (
                movement.id,
                native_movement.definition,
                native_movement.instance,
            ),
            &mut allocation,
            paired,
        )?;
        // A rebuild already leaves unpaired carrier groups out.
        if !rebuilt && !dropped.is_empty() {
            tree::drop_groups(
                &mut movement.payload,
                movement.id,
                native_movement.definition,
                native_movement.instance,
                dropped,
            )?;
            tree::drop_allocation(&mut allocation, groups, dropped)?;
        }
        // A created spawn names its translated graph's root.
        for (at, tag) in spawned {
            movement.patch(at, &spawn_symbol(tag))?;
        }
        if movement.payload.0.len() != length {
            movement.seal()?;
        }
        let symbol = "projectile-movement-allocation";
        assembly.node(symbol, tag, &allocation.0, None, &[])?;
        movement.patch(0x44, symbol)?;
    }
    Ok(created)
}

/// A private profile copy of `tag` with the source's damage type and category names, named
/// `symbol`. Its root and paired block name the copy itself.
fn private_profile(
    assembly: &mut Assembly<'_>,
    tag: u32,
    private: &Private,
    symbol: &str,
) -> Result<()> {
    let mut payload = (*assembly.native.tag(tag, Some(0x80809C36))?).clone();
    let shape = ability_damage::profile(tag, &payload.0)
        .context("Retyped projectile profile shape differs")?;
    if let Some(mode) = private.damage_type {
        payload.0[shape.root + 0x20] = mode;
    }
    // Native category rows of `808094B3` name the native dictionary at `+10`.
    if let Some(names) = &private.categories {
        let mut rows = Vec::new();
        for name in names {
            let mut row = [0u8; 0x18];
            row[..4].copy_from_slice(&name.to_le_bytes());
            row[0x10..0x14].copy_from_slice(&NATIVE_CATEGORIES.to_le_bytes());
            rows.extend(row);
        }
        crate::presentation::append(&mut payload.0, shape.root + 0x60, 0x808094B3, &rows, 0x18)?;
    }
    let mut relocations = Vec::new();
    for at in [shape.root, shape.paired] {
        put(&mut payload.0, at, &u32::MAX.to_le_bytes())?;
        relocations.push((at, symbol.to_owned()));
    }
    assembly.node(symbol, tag, &payload.0, None, &relocations)?;
    Ok(())
}

/// Give each empty movement profile field the source fills its own private profile, named in both
/// definition copies a sealed movement has. The field held no profile in the carrier.
fn fill_profiles(
    assembly: &mut Assembly<'_>,
    native_movement: &graph::Owner,
    movement: &mut Component,
    fills: &[(usize, u32, Private)],
) -> Result<Vec<Value>> {
    let mut report = Vec::new();
    for (field, tag, private) in fills {
        let symbol = format!("projectile-profile-{tag:08X}-field-{field:X}");
        private_profile(assembly, *tag, private, &symbol)?;
        let mut places = BTreeSet::new();
        for definition in [native_movement.definition, movement.payload.pointer(24)?] {
            places.insert(definition + field);
        }
        for &at in &places {
            ensure!(
                movement.payload.u32(at)? == u32::MAX,
                "Filled projectile profile field is not empty"
            );
            movement.patch(at, &symbol)?;
        }
        report.push(json!({"native":tag,"symbol":symbol,"field":field,
            "damage_type":private.damage_type,"categories":private.categories,"references":places}));
    }
    Ok(report)
}

/// Give each carrier profile whose source deals another damage type or declares other categories
/// a private copy with the source's, and point the private movement at it. Only the direct-hit
/// and secondary profile fields and selector 9 parameters, class `808037F3` with the profile at
/// `+08`, may name it.
fn retype_profiles(
    assembly: &mut Assembly<'_>,
    native_movement: &graph::Owner,
    movement: &mut Component,
    retyped: &BTreeMap<u32, Private>,
) -> Result<Vec<Value>> {
    let mut report = Vec::new();
    for (&tag, private) in retyped {
        let symbol = format!("projectile-profile-{tag:08X}");
        private_profile(assembly, tag, private, &symbol)?;
        // Sealing a reshaped movement keeps the original records and appends a copy with the
        // definition view, so both definitions name the profile.
        let definitions = [native_movement.definition, movement.payload.pointer(24)?];
        let mut places = Vec::new();
        for at in (0..movement.payload.0.len().saturating_sub(3)).step_by(4) {
            if movement.payload.u32(at)? != tag {
                continue;
            }
            ensure!(
                definitions
                    .iter()
                    .any(|&definition| at == definition + 0x118 || at == definition + 0x1D0)
                    || (at >= 12 && movement.payload.u32(at - 12)? == 0x808037F3),
                "Retyped projectile profile {tag:08X} has an unqualified reference at {at:X}"
            );
            places.push(at);
        }
        ensure!(
            !places.is_empty(),
            "Retyped projectile profile is not referenced"
        );
        for &at in &places {
            movement.patch(at, &symbol)?;
        }
        report.push(
            json!({"native":tag,"symbol":symbol,"damage_type":private.damage_type,
            "categories":private.categories,"references":places}),
        );
    }
    Ok(report)
}

/// Give the private receiver the source's rows where the carrier's differ, and return each
/// replaced row's name with the offset of the provider at its `+10`.
fn replace_receiver_rows(
    source: &graph::Graph,
    native: &graph::Graph,
    retained: &mut BTreeMap<u32, Component>,
) -> Result<BTreeMap<u32, usize>> {
    let replaced = source.receiver_rows(native)?;
    let mut providers = BTreeMap::new();
    if replaced.is_empty() {
        return Ok(providers);
    }
    let owner = native.owner(graph::Role::Receiver)?;
    let rows = owner
        .payload
        .array(owner.definition + 0x78, 0x40, Some(0x80808BFA))?;
    let receiver = retained
        .get_mut(&owner.tag)
        .context("Private projectile receiver")?;
    for (index, (name, flag, default)) in replaced {
        let row = *rows.get(index).context("Projectile receiver row")?;
        put(&mut receiver.payload.0, row + 0x28, &name.to_le_bytes())?;
        put(&mut receiver.payload.0, row + 0x2C, &flag.to_le_bytes())?;
        put(&mut receiver.payload.0, row + 0x30, &default)?;
        providers.insert(name, row + 0x10);
    }
    Ok(providers)
}

/// Each named input the plan created reads its bank channel like the carrier's own inputs.
fn add_created(
    connections: &mut NativeRows,
    (movement, bank): (u32, u32),
    bank_names: &[u32],
    created: &[(usize, u32)],
) -> Result<()> {
    if !created.is_empty() {
        let template = connections
            .connections
            .iter()
            .find(|row| {
                row[8..12] == movement.to_le_bytes()
                    && row[12..16] == 0x80809789u32.to_le_bytes()
                    && row[40..44] == bank.to_le_bytes()
            })
            .context("Carrier has no named movement input to copy")?
            .clone();
        for &(offset, name) in created {
            let ordinal = bank_names
                .iter()
                .position(|&value| value == name)
                .context("Created input reads no private bank channel")?;
            // A renamed carrier input keeps its connection row, which now reads the new channel.
            if let Some(row) = connections.connections.iter_mut().find(|row| {
                row[8..12] == movement.to_le_bytes() && row[16..24] == (offset as u64).to_le_bytes()
            }) {
                ensure!(
                    row[40..44] == bank.to_le_bytes(),
                    "Renamed projectile input reads another provider"
                );
                put(row, 64, &u32::try_from(ordinal)?.to_le_bytes())?;
                continue;
            }
            let mut row = template.clone();
            put(&mut row, 16, &(offset as u64).to_le_bytes())?;
            put(&mut row, 64, &u32::try_from(ordinal)?.to_le_bytes())?;
            connections.connections.push(row);
        }
    }
    Ok(())
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn prepare(
    assembly: &mut Assembly<'_>,
    source: &graph::Graph,
    native: &graph::Graph,
    entity_tag: u32,
    (edits, material_edits, (retyped, fills)): ResponseEdits<'_>,
    (paired, dropped): (&[Option<usize>], &[usize]),
    (plan, values): (&edit::Plan, &[BankValue]),
) -> Result<Value> {
    let start = assembly.nodes.len();
    let source_bank = source.owner(graph::Role::Bank)?;
    let native_bank = native.owner(graph::Role::Bank)?;
    validate(source_bank, native_bank)?;
    let native_movement = native.owner(graph::Role::Movement)?;
    let order = native.ordered()?;
    let bank_index = order
        .iter()
        .position(|o| o.tag == native_bank.tag)
        .context("Carrier bank order")?;
    let movement_index = order
        .iter()
        .position(|o| o.tag == native_movement.tag)
        .context("Carrier movement order")?;
    // Rows a left-out component takes part in leave with it. A bank getter it supplies names a
    // carrier-only declaration that the private bank leaves out too, as the comparison checked.
    let left_out = native
        .left_out
        .iter()
        .map(|owner| owner.tag)
        .collect::<BTreeSet<_>>();
    let mut omitted = BTreeSet::new();
    for field in [32, 48] {
        for at in native.payload.array(field, 72, Some(0x80809BC9))? {
            if left_out.contains(&native.payload.u32(at + 40)?)
                && native.payload.u32(at + 8)? == native_bank.tag
            {
                omitted.insert(getter_name(
                    native_bank,
                    usize::try_from(native.payload.u64(at + 16)?)?,
                )?);
            }
        }
    }
    // Names a replaced receiver row moves between the creator and the row, which the graph
    // comparison qualified.
    let crossed = source
        .receiver_swaps(native)?
        .into_iter()
        .flat_map(|(carrier, source)| [carrier, source])
        .collect::<BTreeSet<_>>();
    let mut declared = names(source_bank)?;
    declared.extend(names(native_bank)?);
    let release = release::plan(&mut assembly.native, native, &declared)?;
    let extra = release
        .iter()
        .map(|plan| (plan.name, plan.provider, plan.getter))
        .collect::<Vec<_>>();
    let (mut bank, bank_names) = assembly.retained_bank(
        &source_bank.payload,
        (&native_bank.payload, &omitted, &crossed),
        &extra,
        "projectile-bank",
        bank_index,
    )?;
    assembly.projectile_names = bank_names.iter().copied().collect();
    let mut movement = assembly.component(
        native_movement.tag,
        entity_tag,
        "projectile-movement",
        movement_index,
    )?;
    // Replication owns one allocation row for every private component. Retained
    // implementations therefore also need private owners, even without payload edits.
    let mut retained = BTreeMap::new();
    for (index, owner) in order.iter().enumerate() {
        if owner.tag != native_bank.tag && owner.tag != native_movement.tag {
            retained.insert(
                owner.tag,
                assembly.component(
                    owner.tag,
                    entity_tag,
                    &format!("projectile-component-{index}"),
                    index,
                )?,
            );
        }
    }
    // A retained variant's private copy takes the source's values.
    for (tag, at, bytes) in values {
        let component = retained
            .get_mut(tag)
            .context("Projectile owner with source values is not kept")?;
        put(&mut component.payload.0, *at, bytes)?;
    }
    let receiver_providers = replace_receiver_rows(source, native, &mut retained)?;
    edit_movement(&mut movement, native_movement, (edits, material_edits))?;
    let created = reshape(assembly, native, &mut movement, (paired, dropped), plan)?;
    let mut profiles = retype_profiles(assembly, native_movement, &mut movement, retyped)?;
    profiles.extend(fill_profiles(
        assembly,
        native_movement,
        &mut movement,
        fills,
    )?);
    let native_names = names(native_bank)?;
    let settings = source.settings()?;
    ensure!(
        settings.keys().eq(native.settings()?.keys()),
        "Projectile settings inputs differ"
    );
    let bank_definition = usize::try_from(bank.payload.u64(bank.payload.pointer(16)? + 8)?)?;
    let mut connections = NativeRows {
        connections: Vec::new(),
        named_connections: Vec::new(),
    };
    for (field, rows) in [
        (32, &mut connections.connections),
        (48, &mut connections.named_connections),
    ] {
        for at in native.payload.array(field, 72, Some(0x80809BC9))? {
            if left_out.contains(&native.payload.u32(at + 8)?)
                || left_out.contains(&native.payload.u32(at + 40)?)
            {
                continue;
            }
            let mut row = native.payload.bytes::<72>(at)?;
            // Component selectors count the kept owners only.
            for (owner, selector) in [(8, 24), (40, 56)] {
                if native.payload.u64(at + selector)? < 0xFFFF {
                    let tag = native.payload.u32(at + owner)?;
                    let index = order
                        .iter()
                        .position(|kept| kept.tag == tag)
                        .context("Connection names an owner that is not kept")?;
                    put(&mut row, selector, &(index as u64).to_le_bytes())?;
                }
            }
            // A getter the private receiver now supplies reads its row's provider instead of
            // the carrier context. Both providers have class `80809AE1`.
            if native.payload.u32(at + 8)? == native_bank.tag
                && native
                    .owners
                    .get(&graph::Role::Context)
                    .is_some_and(|context| native.payload.u32(at + 40).ok() == Some(context.tag))
            {
                let name =
                    getter_name(native_bank, usize::try_from(native.payload.u64(at + 16)?)?)?;
                if let Some(&provider) = receiver_providers.get(&name) {
                    let receiver = native.owner(graph::Role::Receiver)?;
                    ensure!(
                        native.payload.u32(at + 44)? == 0x80809AE1,
                        "Carrier context provider class differs"
                    );
                    let index = order
                        .iter()
                        .position(|owner| owner.tag == receiver.tag)
                        .context("Carrier receiver order")?;
                    put(&mut row, 40, &receiver.tag.to_le_bytes())?;
                    put(&mut row, 48, &(provider as u64).to_le_bytes())?;
                    put(&mut row, 56, &(index as u64).to_le_bytes())?;
                }
            }
            if native.payload.u32(at + 8)? == native_bank.tag {
                ensure!(
                    native.payload.u32(at + 12)? == 0x808097A8,
                    "Private bank consumer class differs"
                );
                // A replaced receiver row's provider now supplies the source row's name.
                let renamed = match native.owners.get(&graph::Role::Receiver) {
                    Some(receiver) if native.payload.u32(at + 40)? == receiver.tag => {
                        let provider = usize::try_from(native.payload.u64(at + 48)?)?;
                        receiver_providers
                            .iter()
                            .find(|(_, offset)| **offset == provider)
                            .map(|(&name, _)| name)
                    }
                    _ => None,
                };
                let offset = incoming(
                    source_bank,
                    native_bank,
                    &bank,
                    usize::try_from(native.payload.u64(at + 16)?)?,
                    renamed,
                )?;
                put(&mut row, 8, &bank.id.to_le_bytes())?;
                put(&mut row, 16, &(offset as u64).to_le_bytes())?;
            }
            if native.payload.u32(at + 40)? == native_bank.tag {
                ensure!(
                    native.payload.u32(at + 44)? == 0x808097C2
                        && native.payload.u64(at + 48)? == (native_bank.definition + 0x60) as u64,
                    "Private bank output is not its typed vector provider"
                );
                let ordinal = if native.payload.u32(at + 12)? == 0x80809BD9 {
                    // Source declarations keep their ordinals in the private bank.
                    let tag = native.payload.u32(at + 8)?;
                    let role = native
                        .owners
                        .iter()
                        .find(|(_, owner)| owner.tag == tag)
                        .map(|(role, _)| *role)
                        .context("Native settings input owner")?;
                    *settings
                        .get(&role)
                        .context("Source settings input is unbound")? as usize
                } else {
                    let name = *native_names
                        .get(native.payload.u32(at + 64)? as usize)
                        .context("Native output ordinal")?;
                    bank_names
                        .iter()
                        .position(|&value| value == name)
                        .context("Native output is absent from the private bank")?
                };
                put(&mut row, 40, &bank.id.to_le_bytes())?;
                put(
                    &mut row,
                    48,
                    &((bank_definition + 0x60) as u64).to_le_bytes(),
                )?;
                put(&mut row, 64, &(ordinal as u32).to_le_bytes())?;
            }
            // Read the edited row, so a getter moved to the receiver names the private receiver.
            for offset in [8, 40] {
                let owner = u32::from_le_bytes(row[offset..offset + 4].try_into()?);
                if owner == native_movement.tag {
                    put(&mut row, offset, &movement.id.to_le_bytes())?;
                } else if let Some(component) = retained.get(&owner) {
                    put(&mut row, offset, &component.id.to_le_bytes())?;
                }
            }
            rows.push(row.to_vec());
        }
    }
    add_created(
        &mut connections,
        (movement.id, bank.id),
        &bank_names,
        &created,
    )?;
    let mut release = match release {
        Some(plan) => {
            let flow = release::component(assembly, &plan, order.len())?;
            let provider = order
                .iter()
                .position(|owner| owner.tag == plan.provider)
                .context("Remote detonation provider is not kept")?;
            release_rows(
                &native.payload,
                &mut connections,
                &plan,
                (&bank, bank_index, bank_definition, &bank_names),
                (&flow.component, order.len()),
                (retained[&plan.provider].id, provider),
            )?;
            Some(flow)
        }
        None => None,
    };
    let parts = order
        .iter()
        .map(|owner| {
            if owner.tag == native_bank.tag {
                compiled::Component {
                    owner: bank.id,
                    payload: &bank.payload,
                    template: &bank.template,
                    entity: &bank.entity,
                }
            } else if owner.tag == native_movement.tag {
                compiled::Component {
                    owner: movement.id,
                    payload: &movement.payload,
                    template: &movement.template,
                    entity: &movement.entity,
                }
            } else {
                let component = &retained[&owner.tag];
                compiled::Component {
                    owner: component.id,
                    payload: &component.payload,
                    template: &component.template,
                    entity: &component.entity,
                }
            }
        })
        .chain(release.iter().map(|flow| compiled::Component {
            owner: flow.component.id,
            payload: &flow.component.payload,
            template: &flow.component.template,
            entity: &flow.component.entity,
        }))
        .collect::<Vec<_>>();
    let mut entity = compiled::emit(&native.payload, &parts, &connections)?;
    let root_symbol = "projectile-entity";
    let mut relocations = Vec::new();
    // Stock carriers such as `80BBB0DB` do not replicate, and neither does their private copy.
    // A replicated carrier gets a private record that names the translated bank allocation.
    let replication_template = native.payload.u32(0x88)?;
    let replication = if replication_template == u32::MAX {
        None
    } else {
        assembly
            .native
            .tag(replication_template, Some(0x80809BB6))?;
        let mut allocations = BTreeMap::from([(bank.id, bank.template.u32(0x44)?)]);
        if let Some(flow) = &release {
            allocations.insert(flow.component.id, flow.allocation);
        }
        let mut replication = compiled::replication::emit_authored(u32::MAX, &parts, &allocations)?;
        let allocation_symbol = bank
            .patches
            .iter()
            .find(|patch| patch["offset"].as_u64() == Some(0x44))
            .and_then(|patch| patch["symbol"].as_str())
            .context("Private bank allocation symbol")?
            .to_owned();
        let rows = replication.array(0x30, 32, Some(0x80809BB8))?;
        let mut patches = vec![(rows[bank_index] + 4, allocation_symbol)];
        if release.is_some() {
            patches.push((rows[order.len()] + 4, release::ALLOCATION.to_owned()));
        }
        for (row, _) in &patches {
            put(&mut replication.0, *row, &u32::MAX.to_le_bytes())?;
        }
        put(&mut entity.0, 0x88, &u32::MAX.to_le_bytes())?;
        relocations.push((0x88, "projectile-replication".to_owned()));
        Some((replication, patches))
    };
    for at in (0..entity.0.len().saturating_sub(3)).step_by(4) {
        let id = entity.u32(at)?;
        if let Some(component) = [&bank, &movement]
            .into_iter()
            .chain(retained.values())
            .chain(release.iter().map(|flow| &flow.component))
            .find(|c| c.id == id)
        {
            put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
            relocations.push((at, component.symbol.clone()));
        }
    }
    publish(assembly, &mut bank)?;
    publish(assembly, &mut movement)?;
    for component in retained.values_mut() {
        publish(assembly, component)?;
    }
    if let Some(flow) = &mut release {
        publish(assembly, &mut flow.component)?;
    }
    let replicated = replication.is_some();
    if let Some((replication, patches)) = replication {
        let patches = std::iter::once((8, root_symbol.to_owned()))
            .chain(patches)
            .collect::<Vec<_>>();
        assembly.node(
            "projectile-replication",
            replication_template,
            &replication.0,
            None,
            &patches,
        )?;
    }
    let root = assembly.node(root_symbol, entity_tag, &entity.0, None, &relocations)?;
    let nodes = assembly.nodes.drain(start..).collect::<Vec<_>>();
    Ok(json!({"installable":true,"root":root,"nodes":nodes,
        "conversion":{"launch_policy":"retained_native_trajectory","source_bank":source_bank.tag,
        "carrier_bank":native_bank.tag,"declarations":bank_names,"replicated":replicated,
        "private_profiles":profiles,"release":release.map(|flow| flow.report),
        "gameplay_verified":false}}))
}

type ResponseEdits<'a> = (
    &'a [Value],
    &'a [NativeAssetResourcePatch],
    (&'a BTreeMap<u32, Private>, &'a [(usize, u32, Private)]),
);
type BankValue = (u32, usize, Vec<u8>);
