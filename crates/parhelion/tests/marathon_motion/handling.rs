//! Independent source curves followed through the staged item's selected translator.
use super::*;

fn table(
    reader: &mut Reader,
    runtime: &runtime::RuntimeEntitySource,
) -> Result<(u32, Arc<Payload>, usize)> {
    let bindings =
        weapon_component_bindings(&runtime.payload, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
            .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Handling translator binding differs");
    };
    let owner = reader.tag(binding.owner_tag, Some(0x80809C36))?;
    let base = owner.pointer(24)?;
    let groups = owner.array(base + 0x48, 48, Some(0x80803975))?;
    let keys = owner.array(base + 0x238, 40, Some(0x808038C7))?;
    ensure!(
        !groups.is_empty() && groups.len() == keys.len(),
        "Handling groups differ"
    );
    let group = groups[keys
        .iter()
        .position(|&r| owner.u32(r + 16).ok() == Some(runtime.weapon_translation_group_hash))
        .unwrap_or(0)];
    Ok((binding.owner_tag, owner, group))
}

fn overrides(owner: &Payload) -> Result<BTreeMap<u32, BTreeMap<u32, u32>>> {
    let definition = owner.pointer(24)?;
    let globals = owner.array(definition + 0x228, 8, Some(0x808038C9))?;
    owner
        .array(definition + 0x238, 40, Some(0x808038C7))?
        .into_iter()
        .map(|key| {
            let mut values = BTreeMap::new();
            for row in globals
                .iter()
                .copied()
                .chain(owner.array(key, 8, Some(0x808038C9))?)
            {
                values.insert(owner.u32(row)?, owner.u32(row + 4)?);
            }
            Ok((owner.u32(key + 16)?, values))
        })
        .collect()
}

pub(super) fn readback(
    readers: (&mut Reader, &mut Reader),
    runtimes: (&runtime::RuntimeEntitySource, &runtime::RuntimeEntitySource),
    item: u32,
    expected: &Value,
) -> Result<Value> {
    let (reader, original) = readers;
    let (runtime, donor) = runtimes;
    let score = stat_score(reader, item, 0x383D6DBC)?;
    ensure!(
        Some(i64::from(score)) == expected["handling_stat"].as_i64(),
        "Persisted Handling lost its source score"
    );
    let (tag, owner, group) = table(reader, runtime)?;
    let (stock_tag, stock, old_group) = table(original, donor)?;
    let effective_overrides = overrides(&owner)?;
    let mut expected_overrides = overrides(&stock)?;
    let selected = expected_overrides
        .get_mut(&runtime.weapon_translation_group_hash)
        .context("Selected Handling override key")?;
    selected.retain(|input, _| ![14, 15, 16].contains(input));
    ensure!(
        effective_overrides == expected_overrides,
        "Source Handling is overridden or another mode's effective overrides changed"
    );
    let curves = owner.array(group, 56, Some(0x80803979))?;
    let old_curves = stock.array(old_group, 56, Some(0x80803979))?;
    ensure!(
        curves.len() >= old_curves.len(),
        "Private curves lost a native ordinal"
    );
    let mut retained_cells = 0;
    for (&row, &old) in curves.iter().zip(&old_curves) {
        ensure!(
            owner.bytes::<24>(row + 32)? == stock.bytes::<24>(old + 32)?,
            "Private handling changed a native curve's sampling domain"
        );
        let tiers = owner.array(row + 16, 16, Some(0x80809F2A))?;
        let old_tiers = stock.array(old + 16, 16, Some(0x80809F2A))?;
        ensure!(tiers.len() == old_tiers.len(), "Native tier count changed");
        for (tier, old_tier) in tiers.into_iter().zip(old_tiers) {
            let cells = owner.array(tier, 4, Some(0x8080000F))?;
            let old_cells = stock.array(old_tier, 4, Some(0x8080000F))?;
            ensure!(
                cells.len() >= old_cells.len(),
                "Private curves removed a native column"
            );
            for (cell, old_cell) in cells.into_iter().zip(old_cells) {
                // Reload and handling retain the original native columns beside their private
                // replacements. Other runtime edits may change zoom or cadence in place.
                let original = if [9, 14, 15, 16].contains(&stock.u32(old + 32)?) {
                    stock.u32(old_cell)?
                } else if cell != old_cell {
                    owner.u32(old_cell)?
                } else {
                    continue;
                };
                ensure!(
                    owner.u32(cell)? == original,
                    "Private handling overwrote a native sample"
                );
                retained_cells += 1;
            }
        }
    }
    let routes = owner.array(group + 16, 80, Some(0x80803981))?;
    let old_routes = stock.array(old_group + 16, 80, Some(0x80803981))?;
    let wanted = expected["handling_curves"]
        .as_array()
        .context("Independent handling fixture")?;
    ensure!(!wanted.is_empty(), "Required handling corpus is empty");
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for want in wanted {
        let component = number(&want["component"])?;
        let input = number(&want["input"])?;
        ensure!(
            seen.insert((component, input)),
            "Repeated handling expectation"
        );
        let writers = routes
            .iter()
            .copied()
            .filter(|&r| {
                owner.u32(r).ok() == Some(component) && owner.u32(r + 4).ok() == Some(input)
            })
            .collect::<Vec<_>>();
        let [row] = writers.as_slice() else {
            anyhow::bail!("Handling has no unique writer");
        };
        ensure!(
            owner.u32(row + 8)? == u32::MAX
                && owner.u32(row + 12)? == 4
                && owner.bytes::<12>(row + 16)? == [0; 12]
                && owner.u32(row + 32)? == 0x7F7FFFFF,
            "Handling no longer reads a dynamic unnamed curve"
        );
        let channel = owner
            .u32(row + 28)?
            .checked_sub(0x1E00)
            .context("Handling channel")? as usize;
        let curve = *curves.get(channel / 32).context("Handling curve extent")?;
        let old_writers = old_routes
            .iter()
            .copied()
            .filter(|&r| {
                stock.u32(r).ok() == Some(component) && stock.u32(r + 4).ok() == Some(input)
            })
            .collect::<Vec<_>>();
        let [old_row] = old_writers.as_slice() else {
            anyhow::bail!("Native handling writer differs");
        };
        let old_channel = stock
            .u32(old_row + 28)?
            .checked_sub(0x1E00)
            .context("Native handling channel")? as usize;
        let old_curve = *old_curves
            .get(old_channel / 32)
            .context("Native handling curve")?;
        ensure!(
            owner.u32(curve + 32)? == stock.u32(old_curve + 32)?
                && owner.u32(curve + 36)? == 0
                && owner.u32(curve + 48)? == 1
                && Some(f64::from(owner.f32(curve + 40)?)) == want["low"].as_f64()
                && Some(f64::from(owner.f32(curve + 44)?)) == want["high"].as_f64(),
            "Handling sampling domain differs from source"
        );
        let samples = owner
            .array(curve + 16, 16, Some(0x80809F2A))?
            .into_iter()
            .map(|tier| {
                let cells = owner.array(tier, 4, Some(0x8080000F))?;
                owner.f32(*cells.get(channel % 32).context("Handling column")?)
            })
            .collect::<Result<Vec<_>>>()?;
        let expected = want["samples"]
            .as_array()
            .context("Source handling samples")?
            .iter()
            .map(|x| {
                x.as_f64()
                    .context("Source handling value")
                    .map(|x| x as f32)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            samples == expected,
            "Staged handling {component}.{input} lost its source curve"
        );
        results.push(json!({"component":component,"input":input,"channel":channel+0x1E00,
            "stat_input":owner.u32(curve+32)?,"range":[owner.f32(curve+40)?,owner.f32(curve+44)?],"samples":samples}));
    }
    ensure!(
        seen == BTreeSet::from([(0, 0), (0, 2), (0, 4), (0, 5), (0, 6), (7, 0), (7, 1)]),
        "Handling must cover timing and the remaining consumers of the shared stat"
    );
    Ok(
        json!({"score":score,"owner":tag,"sha256":digest(&owner.0),"stock_owner":stock_tag,
        "selected_key":runtime.weapon_translation_group_hash,
        "stock_sha256":digest(&stock.0),"retained_native_cells":retained_cells,"curves":results,
        "effective_overrides":effective_overrides}),
    )
}
