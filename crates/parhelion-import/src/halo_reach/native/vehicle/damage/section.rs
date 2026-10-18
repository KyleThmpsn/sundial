//! Checked source health pools, recovery limits and threshold response plans.
mod output;
mod threshold;
mod transfer;

use super::*;
use std::collections::BTreeSet;

pub(super) struct Sections {
    pub count: usize,
    pub outputs: usize,
    pub report: Value,
}

fn scalar(value: &Value, field: &str) -> Result<f32> {
    let number = value[field]
        .as_f64()
        .with_context(|| format!("Source damage {field}"))? as f32;
    ensure!(
        number.is_finite() && number >= 0.,
        "Invalid source damage {field}"
    );
    Ok(number)
}

fn flags(value: &Value) -> Result<u32> {
    Ok(u32::try_from(
        value["flags"].as_u64().context("Source damage flags")?,
    )?)
}

fn name(value: &Value) -> Result<&str> {
    value["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .context("Source damage section name")
}

fn structures(
    payload: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    template: &[u8],
    count: usize,
) -> Result<Vec<usize>> {
    ensure!(
        template.len() == stride,
        "Damage record template size differs"
    );
    let rows = array(payload, field, class, stride, count)?;
    for &row in &rows {
        put(&mut payload.0, row, template)?;
    }
    Ok(rows)
}

/// A typed dynamic record. The relative pointer belongs to the pointer array,
/// while its class marker immediately precedes the separately aligned record.
fn dynamic(payload: &mut Payload, class: u32, bytes: &[u8]) -> Result<usize> {
    let at = payload
        .0
        .len()
        .checked_add(11)
        .context("Damage record size overflow")?
        & !7;
    payload.0.resize(at + bytes.len(), 0);
    put(&mut payload.0, at - 4, &class.to_le_bytes())?;
    put(&mut payload.0, at, bytes)?;
    Ok(at)
}

fn pointers(payload: &mut Payload, field: usize, class: u32, targets: &[usize]) -> Result<()> {
    let rows = array(payload, field, class, 8, targets.len())?;
    for (&row, &target) in rows.iter().zip(targets) {
        put(
            &mut payload.0,
            row,
            &(i64::try_from(target)? - i64::try_from(row)?).to_le_bytes(),
        )?;
    }
    Ok(())
}

pub(super) fn build(
    payload: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    geometry: &Geometry,
    source: &Value,
    effects: &mut effect::Effects,
) -> Result<Sections> {
    let sections = source["sections"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .context("Source vehicle damage sections")?;
    let names = sections
        .iter()
        .map(|row| name(row).map(hash))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        names.iter().copied().collect::<BTreeSet<_>>().len() == names.len(),
        "Source damage section names collide"
    );
    transfer::validate(sections)?;
    let main = usize::try_from(
        source["indirect_section"]
            .as_u64()
            .context("Source indirect damage section")?,
    )?;
    ensure!(
        main < sections.len() && source["shielded_section"] == -1,
        "Source vehicle shield routing needs an explicit translation"
    );
    let base = scalar(source, "vitality")?;
    ensure!(base > 0., "Source vehicle has no vitality");
    let outputs = output::remap(payload, instance, definition, &names, main)?;
    let old = payload.array(definition + 0x1b0, 0x138, Some(0x80804c11))?;
    let primary = old
        .into_iter()
        .filter(|&row| payload.u32(row + 16).ok() == Some(hash("body")))
        .collect::<Vec<_>>();
    ensure!(
        primary.len() == 1,
        "Native health body layout is absent or ambiguous"
    );
    let primary = primary[0];
    let mut template = payload.bytes::<0x138>(primary)?;
    let old_thresholds = payload.array(primary + 0x78, 0xc8, Some(0x80804bb9))?;
    let threshold_template = payload.bytes::<0xc8>(
        *old_thresholds
            .first()
            .context("Native threshold envelope")?,
    )?;
    threshold::validate(&threshold_template)?;
    // Rebuild every source-owned collection. Unused optional expressions retain
    // the native empty sentinels from the checked section envelope.
    for offset in [0x58, 0x68, 0x78, 0x88, 0x98, 0xa8, 0xe8, 0x100, 0x110] {
        put(&mut template, offset, &[0; 16])?;
    }
    let definitions = structures(
        payload,
        definition + 0x1b0,
        0x80804c11,
        0x138,
        &template,
        sections.len(),
    )?;
    let instances = array(payload, instance + 0x708, 0x80804bab, 0x40, sections.len())?;
    let recoveries = array(
        payload,
        definition + 0x368,
        0x80804c5f,
        0x50,
        sections.len(),
    )?;
    let recovery_instances = array(payload, instance + 0x340, 0x80804c5e, 0x70, sections.len())?;
    let fractions = array(payload, instance + 0x310, 0x80804c5c, 4, sections.len())?;
    put(
        &mut payload.0,
        definition + 0x5a0,
        &u32::try_from(sections.len())?.to_le_bytes(),
    )?;
    let lookup = array(payload, definition + 0x328, 0x80804ba9, 12, sections.len())?;
    put(&mut payload.0, definition + 0x1a8, &base.to_le_bytes())?;
    put(&mut payload.0, instance + 0x324, &base.to_le_bytes())?;
    put(
        &mut payload.0,
        definition + 0x1f0,
        &names[main].to_le_bytes(),
    )?;
    array(
        payload,
        instance + 0x3a8,
        0x8080949e,
        8,
        effects.slot_count(),
    )?;
    put(
        &mut payload.0,
        definition + 0x5a8,
        &u32::try_from(effects.slot_count())?.to_le_bytes(),
    )?;
    let ceiling_values = threshold::ceilings(sections)?;
    let mut ordinal = 0usize;
    let mut report = Vec::new();
    for (index, row) in sections.iter().enumerate() {
        let (d, i, rd, ri) = (
            definitions[index],
            instances[index],
            recoveries[index],
            recovery_instances[index],
        );
        reciprocal(payload, owner, i, d, 0x80804bab, 0x80804c11, instance)?;
        reciprocal(payload, owner, ri, rd, 0x80804c5e, 0x80804c5f, instance)?;
        let fraction = scalar(row, "vitality_fraction")?;
        ensure!(fraction > 0., "Source damage section has no capacity");
        let initial = if flags(row)? & 1 == 0 { 1f32 } else { 0f32 };
        put(&mut payload.0, d + 16, &names[index].to_le_bytes())?;
        put(&mut payload.0, d + 20, &(fraction * 100.).to_le_bytes())?;
        put(
            &mut payload.0,
            d + 0xd0,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        put(
            &mut payload.0,
            rd + 0x10,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        put(&mut payload.0, rd + 0x14, &fraction.to_le_bytes())?;
        put(&mut payload.0, rd + 0x18, &initial.to_le_bytes())?;
        // CD7F20 reads current health from the copied runtime row before a
        // reset is guaranteed. Recovery also needs a full ceiling and an
        // inactive timestamp. Keep the replicated fraction consistent.
        put(&mut payload.0, ri + 0x50, &u64::MAX.to_le_bytes())?;
        put(&mut payload.0, ri + 0x58, &1f32.to_le_bytes())?;
        put(&mut payload.0, ri + 0x60, &initial.to_le_bytes())?;
        put(&mut payload.0, fractions[index], &initial.to_le_bytes())?;
        // Native recovery flag 1 uses these local delays and duration, with
        // source recharge checkpoints emitted as threshold ceiling responses.
        put(&mut payload.0, rd + 0x1c, &1u32.to_le_bytes())?;
        let stun = scalar(row, "stun_seconds")?;
        let recharge = scalar(row, "recharge_seconds")?;
        ensure!(
            row["recharge_curves"].as_array().is_some_and(Vec::is_empty),
            "Nonlinear source recharge needs a native curve translation"
        );
        for (offset, value) in [(0x38, stun), (0x3c, stun), (0x40, recharge), (0x48, 1.)] {
            put(&mut payload.0, rd + offset, &value.to_le_bytes())?;
        }
        put(&mut payload.0, rd + 0x44, &u32::MAX.to_le_bytes())?;
        put(&mut payload.0, lookup[index], &names[index].to_le_bytes())?;
        put(
            &mut payload.0,
            lookup[index] + 4,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        put(&mut payload.0, lookup[index] + 8, &u32::MAX.to_le_bytes())?;
        let transfers = transfer::build(payload, d, row, &names)?;
        let thresholds = threshold::build(
            payload,
            d,
            threshold::Source {
                index,
                main,
                damage: row,
                names: &names,
                geometry,
                template: &threshold_template,
                ceilings: &ceiling_values[index],
            },
            &mut ordinal,
            effects,
        )?;
        report.push(json!({"name":name(row)?,"index":index,"definition":d,"instance":i,
            "capacity":base*fraction,"recovery_delay_seconds":stun,"recovery_duration_seconds":recharge,
            "initial_fraction":initial,"recovery_ceiling":1.,
            "thresholds":thresholds,"transfers":transfers}));
    }
    ensure!(
        ordinal <= 64,
        "Native one-shot damage thresholds exceed their state bitmap"
    );
    Ok(Sections {
        count: sections.len(),
        outputs: outputs.len(),
        report: json!({"vitality":base,"main_section":main,"pools":report,
        "threshold_count":ordinal,"outputs":outputs,
        "limits":["Rider transfer, chance-gated responses and special death selection require separate native translation. Source damage effect references remain explicit until their private effects are authored."]}),
    })
}
