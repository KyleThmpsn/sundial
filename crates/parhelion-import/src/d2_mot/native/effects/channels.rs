//! Preserve named, live object inputs when a source model needs additional controls.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq)]
struct Channel {
    name: Option<[u8; 8]>,
    resource_name: Option<[u8; 8]>,
    alias: Option<[u8; 8]>,
    declaration: Vec<u8>,
    dependencies: Vec<String>,
    procedure: Option<procedural::Declaration>,
    modern: bool,
    preserved_pointers: Vec<(usize, usize)>,
    resting_procedure: Option<String>,
}

fn resource_index(row: &[u8], named: bool, computed: bool) -> Result<Option<u16>> {
    let index = u16::from_le_bytes(
        row.get(106..108)
            .context("channel resource index")?
            .try_into()?,
    );
    Ok((named || (computed && index != u16::MAX)).then_some(index))
}

fn merge_source_channel(
    channels: &mut BTreeMap<String, Channel>,
    name: String,
    channel: Channel,
) -> Result<()> {
    if channels
        .get(&name)
        .is_some_and(|previous| previous != &channel)
    {
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source models require separate controllers for channel {name}"
        )));
    }
    channels.insert(name, channel);
    Ok(())
}

fn dependency_closure(
    mut channels: BTreeMap<String, Channel>,
    root: &str,
) -> Result<BTreeMap<String, Channel>> {
    let mut needed = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        if !needed.insert(name.clone()) {
            continue;
        }
        let channel = channels
            .get(&name)
            .context("source channel dependency is absent")?;
        pending.extend(channel.dependencies.iter().cloned());
        if let Some(procedure) = &channel.procedure {
            pending.extend(procedure.outputs.iter().cloned());
        }
    }
    channels.retain(|name, _| needed.contains(name));
    Ok(channels)
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn declarations(
    p: &Payload,
    modern: bool,
    only: Option<&str>,
) -> Result<BTreeMap<String, Channel>> {
    let schema = p.pointer(24)?;
    let (field, class, lookup) = if modern {
        (0x148, 0x808095A9, 0x808095A8)
    } else {
        (0xD8, 0x808097A1, 0x808097A0)
    };
    let rows = p.array(schema + field, 112, Some(class))?;
    let names = p.array(schema + field + 0x30, 12, Some(lookup))?;
    let mut result = BTreeMap::new();
    for (i, row) in rows.iter().copied().enumerate() {
        if only.is_some_and(|hash| hash != format!("{:08X}", p.u32(row).unwrap_or(0))) {
            continue;
        }
        ensure!(
            p.u32(row + 8)? == 0 || !modern || only.is_none(),
            "uninspected {} channel {:08X} program length {}",
            if modern { "source" } else { "native" },
            p.u32(row)?,
            p.u32(row + 8)?
        );
        let aliases = names
            .iter()
            .filter(|n| p.u32(**n + 8).ok() == Some(i as u32))
            .copied()
            .collect::<Vec<_>>();
        ensure!(aliases.len() <= 1, "channel has ambiguous names");
        let name = aliases.first().map(|at| p.bytes::<8>(*at)).transpose()?;
        let mut other_names = vec![];
        for delta in [0x20, 0x40] {
            let list = p.array(schema + field + delta, 12, Some(lookup))?;
            let found = list
                .into_iter()
                .filter(|at| p.u32(*at + 8).ok() == Some(i as u32))
                .collect::<Vec<_>>();
            ensure!(found.len() <= 1, "channel resource name is ambiguous");
            other_names.push(found.first().map(|at| p.bytes::<8>(*at)).transpose()?);
        }
        let mut declaration = p.0[row..row + 112].to_vec();
        let mut preserved_pointers = Vec::new();
        if !modern && p.u64(row + 8)? != 0 {
            if p.u64(row + 24)? != 0 {
                p.array(row + 24, 16, Some(0x80800090))?;
                preserved_pointers.push((32, p.pointer(row + 32)?));
            }
            for (field, class, stride) in [(16, 0x80800009, 1usize), (64, 0x80800007, 4)] {
                let target = p.pointer(row + field)?;
                let count = usize::try_from(p.u64(target)?)?;
                ensure!(
                    p.u32(target + 8)? == class
                        && count <= 65536
                        && target + 16 + count * stride <= p.0.len(),
                    "native procedure array differs"
                );
                if field == 64 {
                    ensure!(
                        p.u64(row + 56)? == count as u64,
                        "native procedure output count differs"
                    );
                }
                preserved_pointers.push((field, target));
            }
            if p.u64(row + 96)? != 0 {
                let target = p.pointer(row + 96)?;
                ensure!(
                    target >= 4
                        && matches!(p.u32(target - 4)?, 0x808097A9..=0x808097AF)
                        && target + 16 <= p.0.len(),
                    "native procedure parameter class differs"
                );
                preserved_pointers.push((96, target));
            }
        }
        let procedure = if modern && p.u64(row + 8)? != 0 {
            Some(procedural::Declaration::read(p, row, &rows)?)
        } else {
            None
        };
        // These are native live vector slots, not synthesized constant outputs.
        // Retain authored defaults only when both source and native envelopes
        // initialize them identically. Nonzero or procedural defaults need a
        // separate conversion and must not silently become zero.
        ensure!(
            procedure.is_some()
                || !preserved_pointers.is_empty()
                || declaration[16..72].iter().all(|v| *v == 0),
            "channel has authored nonzero or procedural defaults"
        );
        let vectors = p.array(p.pointer(16)? + 0x50, 16, Some(0x80800090))?;
        let initial = *vectors.get(i).context("channel vector index")?;
        ensure!(
            p.0[initial..initial + 16].iter().all(|v| *v == 0),
            "channel initial vector is not zero"
        );
        let deps = p.array(row + 80, 8, Some(0x8080000B))?;
        if !(!modern || only.is_none() || (deps.len() == 1 && p.u64(deps[0])? == 1u64 << i)) {
            // This arm only inspects the source bank, so the verdict is the
            // same whichever native donor the conversion is using.
            return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
                "channel {:08X} at index {i} has procedural dependencies {:?}",
                p.u32(row)?,
                deps.iter()
                    .map(|at| p.u64(*at))
                    .collect::<Result<Vec<_>>>()?
            )));
        }
        // Storage allocation and dependency pointers belong to the native bank.
        declaration[72..96].fill(0);
        let mut dependencies = vec![];
        ensure!(deps.len() <= 1, "channel dependency mask is too wide");
        if let Some(at) = deps.first() {
            let bits = p.u64(*at)?;
            ensure!(
                rows.len() <= 64 && (rows.len() == 64 || bits >> rows.len() == 0),
                "channel dependency outside declaration table"
            );
            for (index, at) in rows.iter().enumerate() {
                if bits & (1 << index) != 0 {
                    dependencies.push(format!("{:08X}", p.u32(*at)?));
                }
            }
        }
        result.insert(
            format!("{:08X}", p.u32(row)?),
            Channel {
                name,
                resource_name: other_names[0],
                alias: other_names[1],
                declaration,
                dependencies,
                procedure,
                modern,
                preserved_pointers,
                resting_procedure: None,
            },
        );
    }
    Ok(result)
}

fn source_input(owner: &Payload, hash: &str) -> Result<Option<usize>> {
    let instance = owner.pointer(16)?;
    ensure!(
        instance >= 4 && owner.u32(instance - 4)? == 0x80806D8A,
        "source object owner differs"
    );
    let inputs = owner.array(instance + 0x180, 48, Some(0x80809590))?;
    for input in inputs {
        ensure!(
            owner.u32(input + 4)? == 0x80809591,
            "source channel input type differs"
        );
        let link = usize::try_from(owner.u64(input + 8)?)?;
        ensure!(
            owner.u32(link + 4)? == 0x80809590
                && owner.u64(link + 8)? == input as u64
                && owner.u64(link + 24)? == 0x808095CE,
            "source vector input lacks reciprocal link"
        );
        if format!("{:08X}", owner.u32(link + 32)?) == hash {
            return Ok(Some(link));
        }
    }
    Ok(None)
}

fn required_source_channels(source: &Source, model: &Value) -> Result<BTreeSet<String>> {
    let mut needed = BTreeSet::new();
    for material in model["materials"]
        .as_array()
        .context("source material list")?
    {
        let tag = material.as_str().context("source material")?;
        if tag == "FFFFFFFF" {
            continue;
        }
        let material = source.raw(tag)?;
        for base in [0x70, 0x2B0] {
            let code = array_bytes(&material, base + 0x20, 1)?;
            for op in program::parse(&code)? {
                if op.op == 0x5C {
                    needed.insert(hex::encode_upper(op.args));
                }
            }
        }
    }
    Ok(needed)
}

fn source_channels(
    source: &Source,
    objects: &BTreeMap<String, u8>,
) -> Result<(BTreeMap<String, Channel>, Option<String>)> {
    let mut result = BTreeMap::new();
    let mut variable = None;
    for model in source.report["models"]
        .as_array()
        .context("source models")?
    {
        let owner_tag = tag(&model["owner"])?;
        let owner = source.raw(model["owner"].as_str().unwrap())?;
        let entity = source.raw(model["entity"].as_str().context("source entity")?)?;
        let connections = entity.array(0x18, 56, Some(0x80809A8F))?;
        for hash in required_source_channels(source, model)? {
            if objects.contains_key(&hash) {
                continue;
            }
            let link = source_input(&owner, &hash)?
                .with_context(|| format!("source owner has no input {hash}"))?;
            let links = connections
                .iter()
                .copied()
                .filter(|at| {
                    entity.u32(*at).ok() == Some(owner_tag)
                        && entity.u32(*at + 4).ok() == Some(0x80809591)
                        && entity.u64(*at + 8).ok() == Some(link as u64)
                })
                .collect::<Vec<_>>();
            ensure!(
                links.len() == 1,
                "source channel lacks one runtime provider"
            );
            let at = links[0];
            let components = entity.array(8, 12, None)?;
            let component = *components
                .get(entity.u32(at + 44)? as usize)
                .context("source channel provider component index")?;
            ensure!(
                entity.u32(at + 28)? == 0x808095CF
                    && entity.u32(component)? == entity.u32(at + 24)?,
                "source channel provider is not a live vector bank"
            );
            let bank = source.raw(&format!("{:08X}", entity.u32(at + 24)?))?;
            let schema = bank.pointer(24)?;
            ensure!(
                entity.u64(at + 32)? == (schema + 0x68) as u64,
                "source channel vector provider offset differs"
            );
            let rows = bank.array(schema + 0x148, 112, Some(0x808095A9))?;
            let row = *rows
                .get(entity.u32(at + 48)? as usize)
                .context("source channel provider index")?;
            ensure!(
                format!("{:08X}", bank.u32(row)?) == hash,
                "source channel provider name differs"
            );
            // Internal channel procedures own their state in the bank. They
            // do not require the separate variable component used by resource
            // feedback loops. Import their complete dependency graph together.
            if bank.u64(bank.pointer(16)? + 0x30)? == 0
                && rows
                    .iter()
                    .any(|at| bank.u64(*at + 8).is_ok_and(|n| n != 0))
            {
                for (name, channel) in dependency_closure(declarations(&bank, true, None)?, &hash)?
                {
                    merge_source_channel(&mut result, name.clone(), channel)?;
                }
                continue;
            }
            if hash == "774501CA" {
                procedural::Declaration::read(&bank, row, &rows)?;
                let index = rows
                    .iter()
                    .position(|at| *at == row)
                    .context("procedure index")?;
                let mut resting = bank.clone();
                put(&mut resting.0, row + 8, &0u32.to_le_bytes())?;
                resting.0[row + 16..row + 72].fill(0);
                resting.0[row + 96..row + 104].fill(0);
                append_array(
                    &mut resting.0,
                    row + 80,
                    0x8080000B,
                    &(1u64 << index).to_le_bytes(),
                    8,
                )?;
                let mut channel = declarations(&resting, true, Some(&hash))?
                    .remove(&hash)
                    .context("resting procedure declaration")?;
                channel.resting_procedure = Some(hash.clone());
                merge_source_channel(&mut result, hash.clone(), channel)?;
                continue;
            }
            if bank.u32(row + 72)? == 0xFFFF {
                let provider = procedural::validate_source(source, &bank, &entity)
                    .with_context(|| format!("source channel {hash}"))?;
                ensure!(
                    variable.as_ref().is_none_or(|v| v == &provider),
                    "source models require different variable providers"
                );
                variable = Some(provider);
                for (name, channel) in declarations(&bank, true, None)? {
                    merge_source_channel(&mut result, name, channel)?;
                }
                continue;
            }
            // Microcosm's numeric output is driven by the same inspected
            // resource procedure used by the deferred barrel animations.
            // Keep its validated authored rest value until that provider is ported.
            let mut bank = bank;
            let mut resting_procedure = None;
            if hash == "59E4FF8D" {
                let deps = bank.array(row + 80, 8, Some(0x8080000B))?;
                let index = rows
                    .iter()
                    .position(|at| *at == row)
                    .context("output index")?;
                let producers = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, at)| bank.u32(**at).ok() == Some(0x774501CA))
                    .collect::<Vec<_>>();
                ensure!(
                    producers.len() == 1 && deps.len() == 1,
                    "resting output producer differs"
                );
                let (producer_index, producer) = producers[0];
                ensure!(
                    bank.u64(deps[0])? == (1u64 << index) | (1u64 << producer_index),
                    "resting output dependencies differ"
                );
                let procedure = procedural::Declaration::read(&bank, *producer, &rows)?;
                ensure!(
                    procedure.outputs == [hash.clone()],
                    "resting output declaration differs"
                );
                put(&mut bank.0, deps[0], &(1u64 << index).to_le_bytes())?;
                resting_procedure = Some("774501CA".to_owned());
            }
            let mut channel = declarations(&bank, true, Some(&hash))?
                .remove(&hash)
                .context("source numeric channel declaration")?;
            channel.resting_procedure = resting_procedure;
            merge_source_channel(&mut result, hash.clone(), channel)?;
        }
    }
    Ok((result, variable))
}

/// Run source channel/provider checks and the donor's minimum channel-bank
/// requirements before atlas work or shader compilation.
pub(super) fn preflight(source: &Path, native: &Path, owner_tag: u32) -> Result<()> {
    let source = Source::read(source)?;
    let objects = program::native_channels(native, Some(owner_tag))?;
    let (extra, _) = source_channels(&source, &objects)?;
    if extra.is_empty() {
        return Ok(());
    }
    ensure!(
        !objects.is_empty(),
        "native donor has no object channel input to extend"
    );
    let report = load(&native.join("template-report.json"))?;
    let model = report["models"]
        .as_array()
        .context("native models")?
        .iter()
        .find(|m| tag(&m["owner"]).ok() == Some(owner_tag))
        .context("native channel owner model")?;
    let entity = Payload(fs::read(native.join(format!(
        "raw/{}.bin",
        model["entity"].as_str().context("native entity")?
    )))?);
    let manifest = load(&native.join("source-manifest.json"))?;
    let mut banks = Vec::new();
    for at in entity.array(16, 12, Some(0x80809C04))? {
        let tag = entity.u32(at)?;
        if manifest["tags"][format!("{tag:08X}")]["reference"].as_u64() != Some(0x80809C36) {
            continue;
        }
        let candidate = Payload(fs::read(native.join(format!("raw/{tag:08X}.bin")))?);
        let instance = candidate.pointer(16)?;
        if instance >= 4 && candidate.u32(instance - 4)? == 0x8080979F {
            banks.push(candidate);
        }
    }
    ensure!(banks.len() == 1, "native owner has no unique channel bank");
    // Binding later rebuilds this bank around vec4 storage, so its allocation
    // must already be dense vectors. Checking here lets preparation choose a
    // different host before any asset is compiled.
    vector_allocation(&banks[0])?;
    Ok(())
}

/// Every declared channel of the native bank must occupy one dense vec4 slot
/// or none. Other storage layouts cannot be extended by the adapter.
fn vector_allocation(old_bank: &Payload) -> Result<()> {
    let channels = declarations(old_bank, false, None)?;
    let old_schema = old_bank.pointer(24)?;
    let old_rows = old_bank.array(old_schema + 0xD8, 112, Some(0x808097A1))?;
    let storage = old_rows
        .iter()
        .map(|at| old_bank.u32(*at + 72))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        channels.len() == old_rows.len()
            && storage
                .into_iter()
                .filter(|i| *i != 0xFFFF)
                .collect::<BTreeSet<_>>()
                == (0..old_bank.u64(old_bank.pointer(16)? + 0x90)? as u32).collect(),
        "native channel bank has nonvector allocation"
    );
    Ok(())
}

fn patches(g: &Graph, symbol: &str) -> Result<Vec<Value>> {
    Ok(g.node(symbol)?["patches"]
        .as_array()
        .context("node patches")?
        .clone())
}
fn patch(entries: &mut Vec<Value>, at: usize, symbol: &str) {
    entries.retain(|p| p["offset"].as_u64() != Some(at as u64));
    entries.push(json!({"offset":at,"symbol":symbol}));
}

fn owner_input(entity: &Payload, patches: &[Value], at: usize) -> Result<bool> {
    // Light components use this same vector interface. Only replace the
    // shader owner's inputs, retaining the other components' connections.
    Ok(entity.u32(at + 12)? == 0x80809789
        && patches
            .iter()
            .any(|p| p["offset"].as_u64() == Some((at + 8) as u64) && p["symbol"] == "owner"))
}

fn static_channel(hash: &str, channel: &mut Channel) -> Result<()> {
    // Source declaration validation already checked that the authored vector
    // starts at zero. Give that resting value a regular native numeric slot,
    // without the variable component's resource feedback or tick callbacks.
    let mut name = [0u8; 8];
    name[..4].copy_from_slice(&0x811C9DC5u32.to_le_bytes());
    name[4..].copy_from_slice(&u32::from_str_radix(hash, 16)?.to_le_bytes());
    channel.name = Some(name);
    channel.resource_name = None;
    channel.alias = None;
    channel.declaration[8..104].fill(0);
    channel.declaration[106..108].copy_from_slice(&u16::MAX.to_le_bytes());
    channel.dependencies = vec![hash.to_owned()];
    channel.procedure = None;
    channel.preserved_pointers.clear();
    Ok(())
}

fn input_allocation(
    p: &mut Payload,
    descriptor: usize,
    before: usize,
    after: usize,
) -> Result<usize> {
    let mut changed = 0;
    for row in p.array(descriptor, 40, Some(0x80808852))? {
        if p.u32(row + 16)? == 0x80809788 {
            ensure!(
                p.u32(row)? == 0xFC3956AD && p.u32(row + 20)? as usize == before,
                "native owner input allocation differs"
            );
            put(&mut p.0, row + 20, &u32::try_from(after)?.to_le_bytes())?;
            changed += 1;
        }
        if p.u64(row + 24)? != 0 {
            changed += input_allocation(p, row + 24, before, after)?;
        }
    }
    Ok(changed)
}

/// Existing owner input and link records give the record layout for channels a
/// native donor does not already expose. A donor without object channel inputs
/// has no layout to copy, so it is rejected instead of indexed.
fn templates(
    owner: &Payload,
    inputs: &[usize],
    links: &[usize],
    count: usize,
) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        count <= inputs.len() || (!inputs.is_empty() && !links.is_empty()),
        "native donor has no object channel input to extend"
    );
    Ok((
        inputs
            .first()
            .map_or_else(Vec::new, |at| owner.0[*at..at + 96].to_vec()),
        links
            .first()
            .map_or_else(Vec::new, |at| owner.0[*at..at + 40].to_vec()),
    ))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub(super) fn bind(c: &mut Effect, prepared: &Path) -> Result<()> {
    if c.graph.manifest.get("object_channel_adapter").is_some() {
        c.objects = program::object_channel_map(&c.graph.read("owner")?.0)?;
        return Ok(());
    }
    let (mut extra, variable) = source_channels(&c.source, &c.objects)?;
    let deferred = extra
        .iter()
        .filter_map(|(hash, channel)| {
            channel
                .resting_procedure
                .as_ref()
                .map(|procedure| json!({"output":hash,"procedure":procedure}))
        })
        .collect::<Vec<_>>();
    for (hash, channel) in &mut extra {
        if channel.resting_procedure.is_some() {
            static_channel(hash, channel)?;
        }
    }
    if !deferred.is_empty() {
        c.graph.manifest["resting_source_procedures"] = json!({"enabled":false,"mode":"validated authored initial vectors","procedures":deferred});
    }
    if let Some(variable) = variable {
        // TODO: Restore the procedural barrel animation for Insidious,
        // Forbearance and Cataclysmic after validating native resources and
        // callbacks and verifying animated previews in game.
        for (hash, channel) in &mut extra {
            static_channel(hash, channel)?;
        }
        c.graph.manifest["source_procedural_adapter"] = json!({
            "source": variable,
            "enabled": false,
            "mode": "static authored initial vectors",
            "reason": "Barrel animation deferred",
            "channels": extra.keys().collect::<Vec<_>>(),
            "gameplay_verified": false
        });
    }
    c.variable = None;
    if extra.is_empty() {
        return Ok(());
    }
    let entity = c.graph.read("entity")?;
    let native_manifest = load(&prepared.join("native/source-manifest.json"))?;
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let mut banks = vec![];
    for at in &components {
        let tag = entity.u32(*at)?;
        if native_manifest["tags"][format!("{tag:08X}")]["reference"].as_u64() != Some(0x80809C36) {
            continue;
        }
        let candidate = Payload(fs::read(
            prepared.join(format!("native/raw/{tag:08X}.bin")),
        )?);
        let instance = candidate.pointer(16)?;
        if instance >= 4 && candidate.u32(instance - 4)? == 0x8080979F {
            banks.push(tag);
        }
    }
    ensure!(banks.len() == 1, "native owner has no unique channel bank");
    let old_tag = banks[0];
    let old_bank = Payload(fs::read(
        prepared.join(format!("native/raw/{old_tag:08X}.bin")),
    )?);
    let has_procedure = extra.values().any(|v| v.procedure.is_some());
    let computed_outputs = extra
        .values()
        .filter_map(|v| v.procedure.as_ref())
        .flat_map(|p| p.outputs.iter().cloned())
        .collect::<BTreeSet<_>>();
    let old_resources = old_bank.u64(old_bank.pointer(16)? + 0x80)? as usize;
    let mut channels = declarations(&old_bank, false, None)?;
    let old_schema = old_bank.pointer(24)?;
    let old_rows = old_bank.array(old_schema + 0xD8, 112, Some(0x808097A1))?;
    let mut bank_order = old_rows
        .iter()
        .map(|at| Ok(format!("{:08X}", old_bank.u32(*at)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut storage = old_rows
        .iter()
        .map(|at| old_bank.u32(*at + 72))
        .collect::<Result<Vec<_>>>()?;
    vector_allocation(&old_bank)?;
    ensure!(
        c.objects.keys().all(|k| channels.contains_key(k)),
        "native owner and channel bank names differ"
    );
    // Preserve the old shader input order so all earlier material adapters and
    // stock auxiliary passes continue to address the same inputs.
    let mut ordered = c
        .objects
        .iter()
        .map(|(h, i)| (*i as usize, h.clone()))
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    let mut names = ordered.into_iter().map(|(_, h)| h).collect::<Vec<_>>();
    for (hash, channel) in extra {
        if !channels.contains_key(&hash) {
            ensure!(
                channel.name.is_some()
                    || computed_outputs.contains(&hash)
                    || (has_procedure
                        && (channel.resource_name.is_some() || channel.alias.is_some())),
                "additional source channel {hash} has no public name"
            );
            storage.push(if channel.name.is_some() {
                storage.iter().filter(|i| **i != 0xFFFF).count() as u32
            } else {
                0xFFFF
            });
            bank_order.push(hash.clone());
            channels.insert(hash.clone(), channel);
        } else if channel.procedure.is_some() || computed_outputs.contains(&hash) {
            let previous = &channels[&hash];
            ensure!(
                previous.name == channel.name,
                "computed source and native channel namespaces differ"
            );
            channels.insert(hash.clone(), channel);
        }
        if !names.contains(&hash) {
            names.push(hash);
        }
    }
    let resource_slots = channels
        .iter()
        .filter(|(_, c)| c.modern && c.procedure.is_some())
        .enumerate()
        .map(|(index, (hash, _))| Ok((hash.clone(), u16::try_from(old_resources + index)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let resource_count = old_resources + resource_slots.len();
    let count = names.len();
    let bank_count = bank_order.len();
    let numeric_count = storage.iter().filter(|i| **i != 0xFFFF).count();
    let mut sorted_names = bank_order
        .iter()
        .enumerate()
        .filter_map(|(i, h)| channels[h].name.map(|name| (name, i)))
        .collect::<Vec<_>>();
    sorted_names.sort_by_key(|(n, _)| {
        (
            u32::from_le_bytes(n[..4].try_into().unwrap()),
            u32::from_le_bytes(n[4..].try_into().unwrap()),
        )
    });
    ensure!(
        sorted_names.len() == numeric_count,
        "native numeric input names and capacity differ"
    );
    // The input-state slots are ordered by the two-part property name. Preserve
    // the declaration ordinal separately because connections use that ordinal.
    for (slot, (_, ordinal)) in sorted_names.iter().enumerate() {
        storage[*ordinal] = slot as u32;
    }
    let first_global = sorted_names
        .iter()
        .position(|(name, _)| name[..4] == 0x811C9DC5u32.to_le_bytes())
        .context("native bank has no unqualified names")?;
    ensure!(
        sorted_names[first_global..]
            .iter()
            .all(|(n, _)| n[..4] == 0x811C9DC5u32.to_le_bytes()),
        "native channel namespace order differs"
    );
    ensure!(
        bank_count <= 64,
        "native channel dependency mask exceeds one word"
    );
    let bank_tag = old_tag;
    let instance = old_bank.pointer(16)?;
    let schema = old_schema;
    ensure!(
        old_bank.u32(instance - 4)? == 0x8080979F && old_bank.u32(schema - 4)? == 0x80809790,
        "native channel bank types differ"
    );
    let mut bank = old_bank.clone();
    let mut interpolation = array_bytes(&old_bank, instance + 0x60, 16)?;
    let old_interpolation_count = interpolation.len() / 16;
    let mut interpolation_slots = BTreeMap::new();
    for (name, channel) in &channels {
        if let Some(state) = channel
            .procedure
            .as_ref()
            .and_then(|p| p.interpolation_state)
        {
            interpolation_slots.insert(name.clone(), u32::try_from(interpolation.len() / 16)?);
            interpolation.extend(state);
        }
    }
    let interpolation_count = interpolation.len() / 16;
    if interpolation_count != old_interpolation_count {
        append_array(&mut bank.0, instance + 0x60, 0x80800090, &interpolation, 16)?;
        put(
            &mut bank.0,
            schema + 0x138,
            &u32::try_from(interpolation_count)?.to_le_bytes(),
        )?;
    }
    let mut row_data = vec![];
    let mut dependency_data = vec![];
    for (i, hash) in bank_order.iter().enumerate() {
        let channel = &channels[hash];
        ensure!(
            channel.procedure.is_some()
                || !channel.preserved_pointers.is_empty()
                || [16, 24, 32, 96]
                    .iter()
                    .all(|at| channel.declaration[*at..*at + 8].iter().all(|v| *v == 0)),
            "native channel contains an unconverted procedure"
        );
        let mut row = channel.declaration.clone();
        put(&mut row, 72, &storage[i].to_le_bytes())?;
        if let Some(index) = if channel.modern {
            resource_index(
                &row,
                channel.resource_name.is_some() || channel.alias.is_some(),
                computed_outputs.contains(hash),
            )?
        } else {
            None
        } {
            ensure!(
                channel.procedure.is_some() && index != u16::MAX,
                "source resource channel lacks an internal procedure"
            );
            put(
                &mut row,
                106,
                &resource_slots
                    .get(hash)
                    .context("source procedure state slot")?
                    .to_le_bytes(),
            )?;
        }
        row_data.extend(row);
        let mut mask = 0u64;
        for dependency in &channel.dependencies {
            mask |= 1u64
                << bank_order
                    .iter()
                    .position(|h| h == dependency)
                    .context("missing channel dependency")?;
        }
        dependency_data.push(if channel.dependencies.is_empty() {
            vec![]
        } else {
            mask.to_le_bytes().to_vec()
        });
    }
    append_array(&mut bank.0, schema + 0xD8, 0x808097A1, &row_data, 112)?;
    let rows = bank.array(schema + 0xD8, 112, Some(0x808097A1))?;
    for (row, dependencies) in rows.iter().zip(dependency_data) {
        append_array(&mut bank.0, row + 80, 0x8080000B, &dependencies, 8)?;
    }
    for (i, row) in rows.iter().enumerate() {
        for &(field, target) in &channels[&bank_order[i]].preserved_pointers {
            put(
                &mut bank.0,
                row + field,
                &(target as i64 - (row + field) as i64).to_le_bytes(),
            )?;
        }
        if let Some(procedure) = &channels[&bank_order[i]].procedure {
            procedure.write(
                &mut bank.0,
                *row,
                &bank_order,
                interpolation_slots.get(&bank_order[i]).copied(),
            )?;
        }
    }
    let mut cached = array_bytes(&old_bank, instance + 0x50, 16)?;
    ensure!(
        cached.len() == old_rows.len() * 16,
        "native cached channel count differs"
    );
    cached.resize(bank_count * 16, 0);
    append_array(&mut bank.0, instance + 0x50, 0x80800090, &cached, 16)?;
    let mut states = array_bytes(&old_bank, instance + 0x90, 16)?;
    ensure!(
        !states.is_empty() && states.iter().all(|v| *v == 0xFF),
        "native channel input initial state differs"
    );
    states.resize(numeric_count * 16, 0xFF);
    append_array(&mut bank.0, instance + 0x90, 0x8080979C, &states, 16)?;
    for (at, value) in [
        (schema + 0x134, bank_count),
        (schema + 0x140, numeric_count),
    ] {
        put(&mut bank.0, at, &u32::try_from(value)?.to_le_bytes())?;
    }
    put(
        &mut bank.0,
        schema + 0x12A,
        &u16::try_from(numeric_count - 1)?.to_le_bytes(),
    )?;
    put(
        &mut bank.0,
        schema + 0x128,
        &u16::try_from(first_global)?.to_le_bytes(),
    )?;
    let lookup = sorted_names
        .iter()
        .flat_map(|(name, ordinal)| name.iter().copied().chain((*ordinal as u32).to_le_bytes()))
        .collect::<Vec<_>>();
    append_array(&mut bank.0, schema + 0x108, 0x808097A0, &lookup, 12)?;
    if has_procedure {
        for (delta, names) in [
            (
                0xF8,
                channels
                    .iter()
                    .filter_map(|(h, c)| c.resource_name.map(|n| (n, h)))
                    .collect::<Vec<_>>(),
            ),
            (
                0x118,
                channels
                    .iter()
                    .filter_map(|(h, c)| c.alias.map(|n| (n, h)))
                    .collect::<Vec<_>>(),
            ),
        ] {
            let mut names = names;
            names.sort_by_key(|(n, _)| {
                (
                    u32::from_le_bytes(n[..4].try_into().unwrap()),
                    u32::from_le_bytes(n[4..].try_into().unwrap()),
                )
            });
            let mut data = vec![];
            for (n, h) in names {
                data.extend(n);
                data.extend(
                    u32::try_from(
                        bank_order
                            .iter()
                            .position(|v| v == h)
                            .context("resource name declaration")?,
                    )?
                    .to_le_bytes(),
                );
            }
            append_array(&mut bank.0, schema + delta, 0x808097A0, &data, 12)?;
        }
        procedural::internal_resources(&mut bank, schema, instance, resource_count)?;
    }
    let metadata_tag = old_bank.u32(0x44)?;
    let mut metadata = Payload(fs::read(c.refs.join(format!(
        "native-channel-defaults/{metadata_tag:08X}/raw/{metadata_tag:08X}.bin"
    )))?);
    let allocation = metadata.array(0x20, 40, Some(0x80808852))?;
    for (name, class, before, after) in [
        (0xFC2F3D6F, 0x80800090, old_rows.len(), bank_count),
        (
            0x3A55A801,
            0x8080979C,
            old_bank.u64(instance + 0x90)? as usize,
            numeric_count,
        ),
    ] {
        let entries = allocation
            .iter()
            .copied()
            .filter(|at| metadata.u32(*at).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(
            entries.len() == 1
                && metadata.u32(entries[0] + 16)? == class
                && metadata.u32(entries[0] + 20)? as usize == before,
            "native channel allocation schema differs"
        );
        put(
            &mut metadata.0,
            entries[0] + 20,
            &u32::try_from(after)?.to_le_bytes(),
        )?;
    }
    if has_procedure {
        procedural::internal_allocation(c, &mut metadata, resource_count)?;
    }
    if interpolation_count != old_interpolation_count {
        procedural::interpolation_allocation(
            &mut metadata,
            old_interpolation_count,
            interpolation_count,
        )?;
    }
    c.graph.add(
        "object-channel-allocation",
        metadata_tag as u64,
        &metadata.0,
        None,
        vec![],
    )?;
    put(&mut bank.0, 0x44, &u32::MAX.to_le_bytes())?;
    let mut bank_patches = vec![json!({"offset":0x44,"symbol":"object-channel-allocation"})];
    for at in (0..bank.0.len().saturating_sub(15)).step_by(4) {
        if bank.u32(at)? == bank_tag {
            ensure!(
                bank.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && bank.u64(at + 8)? < bank.0.len() as u64,
                "untyped native bank self-reference"
            );
            put(&mut bank.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut bank_patches, at, "object-channels");
        }
    }
    c.graph.add(
        "object-channels",
        bank_tag as u64,
        &bank.0,
        None,
        bank_patches,
    )?;

    let owner = c.graph.read("owner")?;
    let owner_instance = owner.pointer(16)?;
    let parent = owner_instance + 0x100;
    let schema_inputs = usize::try_from(owner.u64(parent + 8)?)? + 0x48;
    let old_inputs = owner.array(owner_instance + 0x120, 96, Some(0x80809788))?;
    let old_links = owner.array(schema_inputs, 40, Some(0x80809789))?;
    ensure!(
        old_inputs.len() == c.objects.len() && old_links.len() == c.objects.len(),
        "native input allocation differs"
    );
    let (input_template, link_template) = templates(&owner, &old_inputs, &old_links, count)?;
    let mut owner_bytes = owner.0.clone();
    let mut input_data = vec![];
    let mut link_data = vec![];
    for i in 0..count {
        input_data.extend(if i < old_inputs.len() {
            &owner.0[old_inputs[i]..old_inputs[i] + 96]
        } else {
            &input_template
        });
        link_data.extend(if i < old_links.len() {
            &owner.0[old_links[i]..old_links[i] + 40]
        } else {
            &link_template
        });
    }
    append_array(
        &mut owner_bytes,
        owner_instance + 0x120,
        0x80809788,
        &input_data,
        96,
    )?;
    append_array(&mut owner_bytes, schema_inputs, 0x80809789, &link_data, 40)?;
    let inputs = Payload(owner_bytes.clone()).array(owner_instance + 0x120, 96, None)?;
    let links = Payload(owner_bytes.clone()).array(schema_inputs, 40, None)?;
    let mut owner_patches = patches(&c.graph, "owner")?;
    let allocation_tag = owner.u32(0x44)?;
    let mut allocation = Payload(fs::read(c.refs.join(format!(
        "native-owner-allocations/{allocation_tag:08X}/raw/{allocation_tag:08X}.bin"
    )))?);
    ensure!(
        input_allocation(&mut allocation, 0x20, old_inputs.len(), count)? == 1,
        "native owner lacks a unique input allocation"
    );
    c.graph.add(
        "object-channel-input-allocation",
        allocation_tag as u64,
        &allocation.0,
        None,
        vec![],
    )?;
    put(&mut owner_bytes, 0x44, &u32::MAX.to_le_bytes())?;
    patch(&mut owner_patches, 0x44, "object-channel-input-allocation");
    for i in 0..count {
        let input = inputs[i];
        let link = links[i];
        put(&mut owner_bytes, input, &u32::MAX.to_le_bytes())?;
        put(&mut owner_bytes, input + 8, &(link as u64).to_le_bytes())?;
        put(
            &mut owner_bytes,
            input + 16,
            &(parent as i64 - (input + 16) as i64).to_le_bytes(),
        )?;
        put(&mut owner_bytes, link, &u32::MAX.to_le_bytes())?;
        put(&mut owner_bytes, link + 8, &(input as u64).to_le_bytes())?;
        put(
            &mut owner_bytes,
            link + 32,
            &u32::from_str_radix(&names[i], 16)?.to_le_bytes(),
        )?;
        patch(&mut owner_patches, input, "owner");
        patch(&mut owner_patches, link, "owner");
    }
    c.graph.write("owner", &owner_bytes)?;
    c.graph.node_mut("owner")?["patches"] = json!(owner_patches);

    let mut entity_bytes = entity.0.clone();
    let mut entity_patches = patches(&c.graph, "entity")?;
    for at in (0..entity_bytes.len().saturating_sub(15)).step_by(4) {
        if entity.u32(at)? != old_tag {
            continue;
        }
        if !components.contains(&at) {
            let target = usize::try_from(entity.u64(at + 8)?)?;
            ensure!(
                entity.u32(at + 4)? & 0xFFFF0000 == 0x80800000 && target + 16 <= old_bank.0.len(),
                "untyped or invalid native bank reference"
            );
            // Cloning this exact bank leaves all original embedded resources at
            // their original offsets, including internal non-shader interfaces.
        }
        put(&mut entity_bytes, at, &u32::MAX.to_le_bytes())?;
        patch(&mut entity_patches, at, "object-channels");
    }
    let connections = entity.array(0x20, 72, Some(0x80809BC9))?;
    let template = connections
        .iter()
        .copied()
        .find(|at| owner_input(&entity, &entity_patches, *at).unwrap_or(false))
        .context("native vector connection")?;
    let mut connection_data = vec![];
    let mut new_patches = vec![];
    for at in connections {
        if owner_input(&entity, &entity_patches, at)? {
            continue;
        }
        let base = connection_data.len();
        connection_data.extend(&entity_bytes[at..at + 72]);
        for p in &entity_patches {
            let offset = number(&p["offset"])?;
            if (at..at + 72).contains(&offset) {
                new_patches.push((
                    base + offset - at,
                    p["symbol"].as_str().context("connection patch")?.to_owned(),
                ));
            }
        }
    }
    for (i, link) in links.iter().enumerate() {
        let base = connection_data.len();
        connection_data.extend(&entity_bytes[template..template + 72]);
        put(
            &mut connection_data,
            base + 16,
            &(*link as u64).to_le_bytes(),
        )?;
        put(
            &mut connection_data,
            base + 48,
            &((schema + 0x60) as u64).to_le_bytes(),
        )?;
        put(
            &mut connection_data,
            base + 64,
            &u32::try_from(
                bank_order
                    .iter()
                    .position(|h| *h == names[i])
                    .context("bank input index")?,
            )?
            .to_le_bytes(),
        )?;
        new_patches.push((base + 8, "owner".to_owned()));
        new_patches.push((base + 40, "object-channels".to_owned()));
    }
    append_array(&mut entity_bytes, 0x20, 0x80809BC9, &connection_data, 72)?;
    let start = Payload(entity_bytes.clone()).pointer(0x28)? + 16;
    for (at, symbol) in new_patches {
        patch(&mut entity_patches, start + at, &symbol);
    }
    if c.variable.is_some() {
        procedural::attach(
            c,
            &mut entity_bytes,
            &mut entity_patches,
            schema,
            &bank_order,
            components
                .iter()
                .position(|at| entity.u32(*at).ok() == Some(old_tag))
                .context("native bank component index")?,
        )?;
    }
    c.graph.write("entity", &entity_bytes)?;
    c.graph.node_mut("entity")?["patches"] = json!(entity_patches);
    c.objects = program::object_channel_map(&owner_bytes)?;
    c.graph.manifest["object_channel_adapter"] = json!({"template":format!("{bank_tag:08X}"),"inputs":c.objects,"native_input_order_preserved":true,"source_names_and_authored_initial_values_preserved":true,"binding":"live native float4 channel bank","gameplay_verified":false});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn donor_without_object_channels_is_rejected_rather_than_indexed() {
        // Appetence's Coldheart donor exposes no object channel inputs, so no
        // record layout exists for the source channels the conversion needs.
        let owner = Payload(vec![0xA5; 256]);
        assert!(templates(&owner, &[], &[], 1).is_err());
        let (input, link) = templates(&owner, &[], &[], 0).unwrap();
        assert!(input.is_empty() && link.is_empty());
        let (input, link) = templates(&owner, &[8], &[8], 3).unwrap();
        assert_eq!((input.len(), link.len()), (96, 40));
    }

    #[test]
    fn vector_rebuild_preserves_native_light_connections() {
        // Forbearance's native grenade-launcher donor has four shader-owner
        // inputs and two lights sharing the vector-bank interface.
        let mut row = vec![0u8; 72];
        row[12..16].copy_from_slice(&0x80809789u32.to_le_bytes());
        for (component, replaced) in [
            (0x8161F4A2u32, true),
            (0x8161F4A6u32, false),
            (0x8161F4AAu32, false),
        ] {
            row[8..12].copy_from_slice(&component.to_le_bytes());
            let patches = if replaced {
                row[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
                vec![json!({"offset":8,"symbol":"owner"})]
            } else {
                vec![]
            };
            assert_eq!(
                owner_input(&Payload(row.clone()), &patches, 0).unwrap(),
                replaced
            );
        }
    }

    #[test]
    fn deferred_animation_uses_numeric_resting_channels() {
        let mut declaration = vec![0; 112];
        declaration[..4].copy_from_slice(&0x6E8F4D11u32.to_le_bytes());
        declaration[8] = 4;
        declaration[16..104].fill(0xA5);
        declaration[104..106].copy_from_slice(&0xFEFFu16.to_le_bytes());
        let mut channel = Channel {
            name: None,
            resource_name: Some([1; 8]),
            alias: Some([2; 8]),
            declaration,
            dependencies: vec!["64590948".to_owned()],
            procedure: None,
            modern: true,
            preserved_pointers: vec![(16, 128)],
            resting_procedure: None,
        };
        static_channel("6E8F4D11", &mut channel).unwrap();
        assert_eq!(
            channel.name.unwrap(),
            [0xC5, 0x9D, 0x1C, 0x81, 0x11, 0x4D, 0x8F, 0x6E]
        );
        assert!(channel.resource_name.is_none() && channel.alias.is_none());
        assert!(channel.declaration[8..104].iter().all(|v| *v == 0));
        assert_eq!(&channel.declaration[104..108], &[0xFF, 0xFE, 0xFF, 0xFF]);
        assert_eq!(
            resource_index(&channel.declaration, false, false).unwrap(),
            None
        );
        assert_eq!(channel.dependencies, ["6E8F4D11"]);
        assert!(channel.preserved_pointers.is_empty());
    }

    #[test]
    fn import_keeps_required_channel_cycles_without_unrelated_controllers() {
        let channel = |dependencies: &[&str]| Channel {
            name: None,
            resource_name: None,
            alias: None,
            declaration: vec![0; 112],
            dependencies: dependencies.iter().map(|s| (*s).to_owned()).collect(),
            procedure: None,
            modern: true,
            preserved_pointers: vec![],
            resting_procedure: None,
        };
        let channels = BTreeMap::from([
            ("ROOT".to_owned(), channel(&["INPUT"])),
            ("INPUT".to_owned(), channel(&["ROOT", "INPUT"])),
            ("UNUSED".to_owned(), channel(&["UNUSED"])),
        ]);
        let mut merged = BTreeMap::new();
        merge_source_channel(&mut merged, "ROOT".to_owned(), channels["ROOT"].clone()).unwrap();
        merge_source_channel(&mut merged, "ROOT".to_owned(), channels["ROOT"].clone()).unwrap();
        let error =
            merge_source_channel(&mut merged, "ROOT".to_owned(), channels["UNUSED"].clone())
                .unwrap_err();
        assert!(crate::d2_mot::is_source_limit(&error));
        assert_eq!(merged["ROOT"].dependencies, ["INPUT"]);
        let selected = dependency_closure(channels.clone(), "ROOT").unwrap();
        assert_eq!(
            selected.keys().map(String::as_str).collect::<Vec<_>>(),
            ["INPUT", "ROOT"]
        );
        assert!(dependency_closure(channels, "MISSING").is_err());
        let dangling = BTreeMap::from([("ROOT".to_owned(), channel(&["MISSING"]))]);
        assert!(dependency_closure(dangling, "ROOT").is_err());
    }
    #[test]
    fn computed_numeric_output_keeps_its_nonresource_sentinel() {
        let mut row = [0u8; 112];
        row[106..108].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(resource_index(&row, false, true).unwrap(), None);
        row[106..108].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(resource_index(&row, false, true).unwrap(), Some(0));
        assert_eq!(resource_index(&row, false, false).unwrap(), None);
        assert_eq!(resource_index(&row, true, false).unwrap(), Some(0));
    }
}
