//! Read selected persisted magazine routes and every curve they actually reference.
use super::*;

fn logic(owner: &Payload, group: usize) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for row in owner.array(group + 32, 40, Some(0x8080397C))? {
        let outputs = owner.array(row, 12, Some(0x80803987))?;
        let magazine = outputs
            .iter()
            .copied()
            .filter(|&at| {
                owner.u32(at).ok() == Some(1) && matches!(owner.u32(at + 4).ok(), Some(0 | 3))
            })
            .collect::<Vec<_>>();
        if magazine.is_empty() {
            continue;
        }
        ensure!(
            owner.u32(row + 32)? == 2 && magazine.len() == outputs.len(),
            "Unresolved magazine logic consumer"
        );
        let operands = owner.array(row + 16, 20, Some(0x80803986))?;
        ensure!(operands.len() == 4, "Magazine logic operands differ");
        let mut values = Vec::new();
        for &at in &operands {
            ensure!(
                owner.f32(at + 16)? != f32::MAX && owner.f32(at + 16)?.is_finite(),
                "Imported magazine logic still reads an unresolved operand"
            );
            values.push(owner.f32(at + 16)?);
        }
        let [reference, rate, base, bullets] = values[..] else {
            unreachable!()
        };
        ensure!(reference > 0., "Magazine reference rate is not positive");
        // The native arithmetic rounds before multiplying. Only integral inputs are accepted
        // here, so this check does not assume a rule for the unresolved native half-way case.
        let count = base * rate / reference;
        ensure!(
            count.fract() == 0.,
            "Magazine logic needs a rounding oracle"
        );
        let value = count * if bullets == 0. { 1. } else { bullets };
        let bytes = operands
            .iter()
            .map(|&at| owner.bytes::<20>(at))
            .collect::<Result<Vec<_>>>()?;
        for output in magazine {
            rows.push(json!({"kind":"logic","input":owner.u32(output+4)?,
                "slot":owner.u32(output+8)?,"order":owner.u32(row+36)?,
                "operands":bytes,"value":value}));
        }
    }
    Ok(rows)
}

fn routes(reader: &mut Reader, entity: &[u8], key: u32) -> Result<Value> {
    let bindings = weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
        .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Magazine translator is ambiguous");
    };
    let owner = reader.tag(binding.owner_tag, Some(0x80809C36))?;
    let base = usize::try_from(owner.u64(binding.resource_offset as usize + 8)?)?;
    let groups = owner.array(base + 0x48, 48, Some(0x80803975))?;
    let keys = owner.array(base + 0x238, 40, Some(0x808038C7))?;
    let selected = keys
        .iter()
        .position(|&r| owner.u32(r + 16).ok() == Some(key))
        .unwrap_or(0);
    let group = *groups.get(selected).context("Magazine group")?;
    let curves = owner.array(group, 56, Some(0x80803979))?;
    let mut out = Vec::new();
    for row in owner.array(group + 16, 80, Some(0x80803981))? {
        if owner.u32(row)? != 1 || ![0, 3].contains(&owner.u32(row + 4)?) {
            continue;
        }
        let mut dependencies = Vec::new();
        let operation = owner.u32(row + 12)?;
        let inputs = match operation {
            0..=3 | 9 | 10 => 2,
            4..=7 => 1,
            _ => anyhow::bail!("Unresolved magazine routing operation {operation}"),
        };
        for operand in [row + 16, row + 36, row + 56].into_iter().take(inputs) {
            let output = owner.u32(operand + 12)?;
            if owner.u32(operand)? != 0 || output < 0x1E00 {
                continue;
            }
            let index = usize::try_from((output - 0x1E00) / 32)?;
            let curve = *curves.get(index).context("Magazine curve")?;
            let values = owner
                .array(curve + 16, 16, Some(0x80809F2A))?
                .into_iter()
                .map(|tier| {
                    owner
                        .array(tier, 4, Some(0x8080000F))?
                        .into_iter()
                        .map(|at| owner.u32(at))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?;
            dependencies.push(json!({"output":output,"domain":hex::encode(owner.bytes::<24>(curve+32)?),"tiers":values}));
        }
        out.push(
            json!({"kind":"route","input":owner.u32(row+4)?,"operation":owner.u32(row+12)?,
            "slot":owner.u32(row+8)?,"order":owner.u32(row+76)?,
            "operands":hex::encode(owner.bytes::<60>(row+16)?),"dependencies":dependencies,
            "literal":owner.f32(row+32)?}),
        );
    }
    out.extend(logic(&owner, group)?);
    ensure!(
        [0, 3]
            .into_iter()
            .all(|input| out.iter().any(|r| r["input"] == input)),
        "Magazine capacity or refill is absent"
    );
    Ok(json!(out))
}

pub(super) fn readback(
    reader: &mut Reader,
    original: &mut Reader,
    actual: &runtime::RuntimeEntitySource,
    donor: &runtime::RuntimeEntitySource,
    expected: &Value,
) -> Result<Value> {
    ensure!(
        !expected.is_null(),
        "Independent magazine expectation is required"
    );
    let observed = routes(
        reader,
        &actual.payload,
        actual.weapon_translation_group_hash,
    )?;
    if expected == "donor" {
        let stock = routes(
            original,
            &donor.payload,
            donor.weapon_translation_group_hash,
        )?;
        ensure!(
            observed == stock,
            "Unsupported source ammo changed the donor magazine"
        );
    } else {
        for row in observed.as_array().context("Magazine rows")? {
            let input = number(&row["input"])?;
            let want = expected[if input == 0 { "capacity" } else { "refill" }]
                .as_f64()
                .context("Required magazine value")?;
            let matches = if row["kind"] == "logic" {
                row["value"].as_f64() == Some(want)
            } else {
                row["operation"] == 4
                    && row["dependencies"] == json!([])
                    && row["literal"].as_f64() == Some(want)
            };
            ensure!(matches, "Conventional source magazine or refill changed");
        }
    }
    Ok(observed)
}
