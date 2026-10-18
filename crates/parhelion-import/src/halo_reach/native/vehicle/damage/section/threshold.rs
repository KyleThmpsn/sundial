//! Native health crossings and source region changes share one response plan.
use super::*;

fn key(section: u32, fraction: f32) -> u32 {
    hash(&format!(
        "reach.section.{section:08x}.ceiling.{:08x}",
        fraction.to_bits()
    ))
}

pub(super) fn validate(template: &[u8; 0xc8]) -> Result<()> {
    let p = Payload(template.to_vec());
    for offset in [0x18, 0x28, 0x38, 0x48, 0x58, 0x68, 0x78] {
        ensure!(
            p.u64(offset)? == 0,
            "Native damage threshold has an active predicate array"
        );
    }
    ensure!(
        p.u64(0x80)? == 0
            && p.u32(0x88)? != u32::MAX
            && p.u32(0x90)? == 0
            && p.u32(0x94)? == 0
            && p.u32(0x98)? == 0xff
            && p.u64(0xa0)? == 0
            && p.u16(0xa8)? == 0x0101,
        "Native damage threshold filter differs"
    );
    Ok(())
}

pub(super) fn ceilings(sections: &[Value]) -> Result<Vec<BTreeSet<u32>>> {
    let mut out = vec![BTreeSet::new(); sections.len()];
    for section in sections {
        for response in section["responses"]
            .as_array()
            .context("Source damage responses")?
        {
            for transfer in response["damage_transfers"]
                .as_array()
                .context("Source response transfers")?
            {
                let target = transfer["section"]
                    .as_i64()
                    .context("Source response target")?;
                if target < 0 {
                    continue;
                }
                ensure!(
                    transfer["function"] == 2 && flags(transfer)? == 0x11 && transfer["seat"] == "",
                    "Source response transfer needs another native operation"
                );
                let fraction = scalar(transfer, "amount")?;
                ensure!(
                    fraction <= 1.,
                    "Source response ceiling exceeds full vitality"
                );
                out.get_mut(usize::try_from(target)?)
                    .context("Source response target exceeds section table")?
                    .insert(fraction.to_bits());
            }
        }
    }
    Ok(out)
}

fn state(payload: &mut Payload, geometry: &Geometry, transition: &Value) -> Result<usize> {
    let name = transition["region"]
        .as_str()
        .context("Source damage region name")?;
    let region = geometry
        .regions
        .iter()
        .position(|r| r.name == name)
        .context("Source response region is not in the selected model")?;
    let ordinal = usize::try_from(
        transition["state"]
            .as_u64()
            .context("Source region state")?,
    )?;
    let state = *super::super::region::STATES
        .get(ordinal)
        .context("Source region state exceeds native table")?;
    let mut bytes = [0; 0x28];
    put(&mut bytes, 4, &hash(name).to_le_bytes())?;
    put(&mut bytes, 8, &state.to_le_bytes())?;
    put(&mut bytes, 0x18, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x20, &u32::try_from(region)?.to_le_bytes())?;
    put(&mut bytes, 0x24, &u32::try_from(ordinal)?.to_le_bytes())?;
    dynamic(payload, 0x80804c3e, &bytes)
}

fn named_ceiling(payload: &mut Payload, target: u32, fraction: f32) -> Result<usize> {
    let mut bytes = [0; 12];
    put(&mut bytes, 4, &target.to_le_bytes())?;
    put(&mut bytes, 8, &key(target, fraction).to_le_bytes())?;
    dynamic(payload, 0x80804c38, &bytes)
}

struct Row {
    name: u32,
    fraction: f32,
    mode: u8,
    targets: Vec<usize>,
}

pub(super) struct Source<'a> {
    pub index: usize,
    pub main: usize,
    pub damage: &'a Value,
    pub names: &'a [u32],
    pub geometry: &'a Geometry,
    pub template: &'a [u8; 0xc8],
    pub ceilings: &'a BTreeSet<u32>,
}

pub(super) fn build(
    payload: &mut Payload,
    section: usize,
    source: Source<'_>,
    ordinal: &mut usize,
    effects: &mut effect::Effects,
) -> Result<Value> {
    let Source {
        index,
        main,
        damage: source,
        names,
        geometry,
        template,
        ceilings,
    } = source;
    let mut rows = Vec::new();
    let mut report = Vec::new();
    for (source_index, response) in source["responses"]
        .as_array()
        .context("Source damage responses")?
        .iter()
        .enumerate()
    {
        let flags = flags(response)?;
        let fraction = scalar(response, "threshold")?;
        ensure!(
            fraction <= 1.
                && response["custom_behavior"] == 0
                && scalar(response, "delay_seconds")? == 0.,
            "Source delayed or labelled damage response needs explicit scheduling"
        );
        if flags & 0x10000 != 0 {
            report.push(json!({"source_index":source_index,"source":response,"status":"Special death branch requires a native selection condition"}));
            continue;
        }
        let mut targets = Vec::new();
        targets.extend(effects.responses(payload, index, source_index)?);
        for transition in response["regions"]
            .as_array()
            .context("Source damage region transitions")?
        {
            targets.push(state(payload, geometry, transition)?);
        }
        for transfer in response["damage_transfers"]
            .as_array()
            .context("Source response damage transfers")?
        {
            let target = transfer["section"]
                .as_i64()
                .context("Source response target")?;
            if target >= 0 {
                let destination = names[usize::try_from(target)?];
                targets.push(named_ceiling(
                    payload,
                    destination,
                    scalar(transfer, "amount")?,
                )?);
            }
        }
        if flags & 1 != 0 && !(index == main && fraction == 0.) {
            targets.push(dynamic(payload, 0x80804c39, &[0, 0x21])?);
        }
        let name = hash(&format!(
            "reach.section.{:08x}.response.{source_index}",
            names[index]
        ));
        rows.push(Row {
            name,
            fraction,
            mode: if flags & 0x1000000 == 0 { 1 } else { 0 },
            targets,
        });
        let mut pending = Vec::new();
        for field in [
            "generic_effect",
            "specific_effect",
            "damage_effect",
            "looping_effect",
            "delay_effect",
        ] {
            if !response[field].is_null() {
                pending.push(json!({"field":field,"source":response[field]}));
            }
        }
        report.push(json!({"source_index":source_index,"threshold_name":format!("{name:08X}"),
            "fraction":fraction,"regions":response["regions"],"transfers":response["damage_transfers"],
            "source_skip_fraction":response["skip_fraction"],"source_flags":flags,
            "pending_effects":pending,"chance_translated":scalar(response,"skip_fraction")? == 0.}));
    }
    for value in source["recharge_fractions"]
        .as_array()
        .context("Source recharge fractions")?
    {
        let fraction = value.as_f64().context("Source recharge checkpoint")? as f32;
        ensure!(
            fraction.is_finite() && (0.0..=1.0).contains(&fraction),
            "Source recharge checkpoint is invalid"
        );
        if fraction >= 1. {
            continue;
        }
        rows.push(Row {
            name: hash(&format!(
                "reach.section.{:08x}.recovery.{:08x}",
                names[index],
                fraction.to_bits()
            )),
            fraction,
            mode: 1,
            targets: vec![dynamic(payload, 0x80804c39, &[0, 4])?],
        });
    }
    for &bits in ceilings {
        let fraction = f32::from_bits(bits);
        rows.push(Row {
            name: key(names[index], fraction),
            fraction,
            mode: 0,
            targets: Vec::new(),
        });
    }
    if index == main {
        let mut targets = effects.stop(payload)?;
        targets.push(dynamic(payload, 0x80804c39, &[0, 0x21])?);
        rows.push(Row {
            name: hash("reach.primary.depleted"),
            fraction: 0.,
            mode: 1,
            targets,
        });
    }
    ensure!(
        rows.iter().map(|r| r.name).collect::<BTreeSet<_>>().len() == rows.len(),
        "Source threshold identities collide"
    );
    let native_rows = array(payload, section + 0x78, 0x80804bb9, 0xc8, rows.len())?;
    for (&at, row) in native_rows.iter().zip(rows) {
        put(&mut payload.0, at, template)?;
        put(&mut payload.0, at, &row.name.to_le_bytes())?;
        put(&mut payload.0, at + 4, &(row.fraction * 100.).to_le_bytes())?;
        put(&mut payload.0, at + 8, &row.fraction.to_le_bytes())?;
        put(&mut payload.0, at + 12, &[row.mode, 0, 0, 0])?;
        put(
            &mut payload.0,
            at + 0xc0,
            &u32::try_from(*ordinal)?.to_le_bytes(),
        )?;
        put(&mut payload.0, at + 0xc4, &u32::MAX.to_le_bytes())?;
        pointers(payload, at + 0xb0, 0x80804c2c, &row.targets)?;
        *ordinal += 1;
    }
    Ok(json!(report))
}
