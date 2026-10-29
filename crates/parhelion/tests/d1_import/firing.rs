//! Compare selected original firing curves with independently followed staged runtime inputs.
use super::*;
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};

fn timing(component: u32, input: u32) -> bool {
    matches!((component, input), (2, 0..=5 | 22..=25) | (4, 0..=1))
}

fn selected(p: &Payload, key: u32, source: bool) -> Result<usize> {
    let d = p.pointer(24)?;
    let (tables, keys) = if source { (0x60, 0x288) } else { (0x48, 0x238) };
    let rows = p.array(
        d + tables,
        48,
        Some(if source { 0x80802945 } else { 0x80803975 }),
    )?;
    let keys = p.array(
        d + keys,
        40,
        Some(if source { 0x808020A3 } else { 0x808038C7 }),
    )?;
    ensure!(
        !rows.is_empty() && rows.len() == keys.len(),
        "Firing table population differs"
    );
    let index = keys
        .iter()
        .position(|&r| p.u32(r + 16).ok() == Some(key))
        .unwrap_or(0);
    Ok(rows[index])
}

// Evaluate each stored tier independently. This oracle does not consume converter reports,
// patch offsets or its channel correspondence. Original clients implement logic zero as
// 1 / operand1 minus operand2 / operand0 for multi-bullet bursts with a positive rate.
fn values(p: &Payload, table: usize, old: bool) -> Result<BTreeMap<(u32, u32), Vec<u32>>> {
    let mut result = BTreeMap::new();
    let translations = p.array(table, 56, Some(if old { 0x80802A32 } else { 0x80803979 }))?;
    let operand = |at: usize| -> Result<Vec<f32>> {
        ensure!(
            p.u32(at)? == 0,
            "Firing oracle needs a translated output or constant"
        );
        let channel = p.u32(at + 12)?;
        if channel == 0 {
            return Ok(vec![p.f32(at + 16)?; 11]);
        }
        let index = channel
            .checked_sub(0x1E00)
            .context("Firing output number")?;
        let translation = *translations
            .get(index as usize / 32)
            .context("Firing translation")?;
        ensure!(
            p.bytes::<8>(translation + 0x28)? == [0, 0, 0, 0, 0, 0, 200, 66],
            "Firing stat domain is not 0 to 100"
        );
        let tiers = p.array(translation + 16, 16, None)?;
        ensure!(
            matches!(tiers.len(), 1 | 11),
            "Firing oracle needs one constant or eleven tiers"
        );
        let mut samples = tiers
            .into_iter()
            .map(|r| {
                let cells = p.array(r, 4, Some(0x8080000F))?;
                p.f32(
                    *cells
                        .get(index as usize % 32)
                        .context("Firing curve column")?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        if samples.len() == 1 {
            samples.resize(11, samples[0]);
        }
        Ok(samples)
    };
    for r in p.array(
        table + 16,
        80,
        Some(if old { 0x80802013 } else { 0x80803981 }),
    )? {
        let key = (p.u32(r)?, p.u32(r + 4)?);
        if !timing(key.0, key.1) {
            continue;
        }
        ensure!(p.u32(r + 12)? == 4, "Firing oracle needs a direct route");
        ensure!(
            result
                .insert(
                    key,
                    operand(r + 16)?.into_iter().map(f32::to_bits).collect()
                )
                .is_none(),
            "Repeated firing writer"
        );
    }
    for r in p.array(
        table + 32,
        40,
        Some(if old { 0x8080201E } else { 0x8080397C }),
    )? {
        let outputs = p.array(r, 12, None)?;
        if !outputs.iter().any(|&o| {
            timing(
                p.u32(o).unwrap_or(u32::MAX),
                p.u32(o + 4).unwrap_or(u32::MAX),
            )
        }) {
            continue;
        }
        ensure!(p.u32(r + 32)? == 0, "Firing oracle needs timing logic zero");
        let inputs = p.array(r + 16, 20, None)?;
        ensure!(inputs.len() == 3, "Firing timing operand count");
        let [rate, cadence, count] = [
            operand(inputs[0])?,
            operand(inputs[1])?,
            operand(inputs[2])?,
        ];
        let computed = (0..11)
            .map(|i| {
                let mut v = if cadence[i] == 0.0 {
                    0.0
                } else {
                    1.0 / cadence[i]
                };
                if cadence[i] != 0.0 && count[i] > 1.0 && rate[i] > 0.0 {
                    v -= (1.0 / rate[i]) * count[i];
                }
                v.to_bits()
            })
            .collect::<Vec<_>>();
        for o in outputs {
            let key = (p.u32(o)?, p.u32(o + 4)?);
            if timing(key.0, key.1) {
                ensure!(
                    result.insert(key, computed.clone()).is_none(),
                    "Repeated timing writer"
                );
            }
        }
    }
    ensure!(!result.is_empty(), "Empty firing contract");
    Ok(result)
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
    let report = &receipt["graph"]["firing_translation"];
    let native_manager = sundial::package_authoring::PackageManager::new(
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
    let runtime = runtime::load_weapon_runtime_entity_for_item_with_manager(
        &staged_manager,
        number(&receipt["item"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, 0x2F981564).map_err(anyhow::Error::msg)?;
    ensure!(bindings.len() == 1, "Staged translator binding differs");
    let owner = staged.tag(bindings[0].owner_tag, Some(0x80809C36))?;
    let donor = runtime::load_weapon_runtime_entity_for_item_with_manager(
        &native_manager,
        number(&receipt["donor"])?,
    )
    .map_err(anyhow::Error::msg)?;
    let donor_binding =
        weapon_component_bindings(&donor.payload, 0x2F981564).map_err(anyhow::Error::msg)?;
    ensure!(donor_binding.len() == 1, "Stock translator binding differs");
    let original = stock.tag(donor_binding[0].owner_tag, Some(0x80809C36))?;
    ensure!(
        staged.tag(donor_binding[0].owner_tag, Some(0x80809C36))?.0 == original.0,
        "Firing translation changed the stock translator"
    );
    preserve(
        &original,
        &owner,
        donor.weapon_translation_group_hash,
        report["timing"]["status"] == "translated",
    )?;
    if report["timing"]["status"] != "translated" {
        ensure!(
            receipt["case"]["required_firing_timing"] != true,
            "Required source firing timing did not translate"
        );
        return Ok(
            json!({"timing":"unsupported","reason":report["timing"]["reason"],"native_curves_preserved":true,"stock_owner_preserved":true}),
        );
    }
    let source_item = number(&receipt["source"])?;
    let weapon = source
        .weapons()?
        .into_iter()
        .find(|w| w.hash == source_item)
        .context("Source weapon")?;
    let tables = source.classes(0x8080339A);
    ensure!(tables.len() == 1, "Ambiguous source pattern table");
    let patterns = source.read(tables[0])?;
    let row = patterns.array(8, 32, Some(0x808033BC))?[usize::from(weapon.pattern)];
    let key = patterns.u32(row + 8)?;
    let original = &receipt["graph"]["rendering"][0]["source_runtime"];
    let root = u32::from_str_radix(
        original["runtime_entity"]
            .as_str()
            .context("Source runtime")?,
        16,
    )?;
    let entity = source.read(root)?;
    let mut owners = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(r)?;
        let p = source.read(tag)?;
        if selected(&p, key, true).is_ok() {
            owners.push((tag, p));
        }
    }
    ensure!(
        owners.len() == 1,
        "Source timing owner is missing or ambiguous"
    );
    let (source_tag, source_owner) = &owners[0];
    let expected = values(source_owner, selected(source_owner, key, true)?, true)?;
    let actual = values(
        &owner,
        selected(&owner, runtime.weapon_translation_group_hash, false)?,
        false,
    )?;
    ensure!(
        expected == actual,
        "Staged firing inputs differ from the selected source table"
    );
    Ok(
        json!({"source_owner":source_tag,"staged_owner":bindings[0].owner_tag,
        "source_translation_key":key,"staged_translation_key":runtime.weapon_translation_group_hash,
        "inputs":actual.into_iter().map(|((component,input),bits)|json!({"component":component,"input":input,"tier_bits":bits})).collect::<Vec<_>>(),
        "gameplay_verified":false}),
    )
}

// Read the curves through every native table and its consumers. A refusal preserves all
// calibration, while translated timing must preserve non-firing inputs and other modes.
fn preserve(stock: &Payload, emitted: &Payload, key: u32, translated: bool) -> Result<()> {
    let from = stock.array(stock.pointer(24)? + 0x48, 48, Some(0x80803975))?;
    let to = emitted.array(emitted.pointer(24)? + 0x48, 48, Some(0x80803975))?;
    ensure!(
        from.len() == to.len() && !from.is_empty(),
        "Native firing modes changed"
    );
    let active = selected(stock, key, false)?;
    for (s, n) in from.into_iter().zip(to) {
        let mut protected = BTreeSet::new();
        if translated && s == active {
            for row in stock.array(s + 16, 80, Some(0x80803981))? {
                if !timing(stock.u32(row)?, stock.u32(row + 4)?) {
                    for operand in [row + 16, row + 36, row + 56] {
                        if stock.u32(operand)? == 0 {
                            protected.insert(stock.u32(operand + 12)?);
                        }
                    }
                }
            }
            for row in stock.array(s + 32, 40, Some(0x8080397C))? {
                let destinations = stock.array(row, 12, Some(0x80803987))?;
                if destinations.iter().any(|&d| {
                    !timing(
                        stock.u32(d).unwrap_or(u32::MAX),
                        stock.u32(d + 4).unwrap_or(u32::MAX),
                    )
                }) {
                    for operand in stock.array(row + 16, 20, Some(0x80803986))? {
                        if stock.u32(operand)? == 0 {
                            protected.insert(stock.u32(operand + 12)?);
                        }
                    }
                }
            }
        }
        let sc = stock.array(s, 56, Some(0x80803979))?;
        let nc = emitted.array(n, 56, Some(0x80803979))?;
        ensure!(sc.len() == nc.len(), "Native stat inputs changed");
        for (index, (sr, nr)) in sc.into_iter().zip(nc).enumerate() {
            ensure!(
                stock.bytes::<24>(sr + 32)? == emitted.bytes::<24>(nr + 32)?,
                "Native stat selectors changed"
            );
            let ss = stock.array(sr + 16, 16, None)?;
            let ns = emitted.array(nr + 16, 16, None)?;
            ensure!(ss.len() == ns.len(), "Native stat tier count changed");
            for (ss, ns) in ss.into_iter().zip(ns) {
                let sv = stock.array(ss, 4, Some(0x8080000F))?;
                let nv = emitted.array(ns, 4, Some(0x8080000F))?;
                ensure!(sv.len() == nv.len(), "Native output count changed");
                for (column, (sv, nv)) in sv.into_iter().zip(nv).enumerate() {
                    let channel = 0x1E00 + u32::try_from(index * 32 + column)?;
                    if !translated || s != active || protected.contains(&channel) {
                        ensure!(
                            stock.u32(sv)? == emitted.u32(nv)?,
                            "Non-firing input or unselected mode changed"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
