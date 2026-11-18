//! Independently traverse persisted native selectors and compare package-derived source choices.
use crate::*;

fn short(p: &Payload, at: usize) -> Result<i16> {
    Ok(p.u16(at)? as i16)
}

fn choice(
    states: &Payload,
    dictionary: &Payload,
    action: u32,
    inputs: &std::collections::BTreeMap<u8, u32>,
) -> Result<Vec<(u32, f32)>> {
    let name = states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .find(|&at| states.u32(at).ok() == Some(action))
        .context("Native action")?;
    let nodes = states.array(24, 16, Some(0x8080342F))?;
    let row = *nodes
        .get(states.u32(name + 4)? as usize)
        .context("Native node")?;
    let base = states.pointer(row + 8)?;
    let selected = match states.u64(row)? {
        1 => base,
        2 => {
            let records = states.array(base + 32, 28, Some(0x8080347D))?;
            let operations = states.array(base + 48, 4, Some(0x8080347C))?;
            let choices = states.array(base, 16, Some(0x80803437))?;
            let mut operation = 0;
            let mut seen = BTreeSet::new();
            loop {
                ensure!(seen.insert(operation), "Emitted operation cycle");
                let op = states.u32(*operations.get(operation).context("Operation extent")?)?;
                ensure!(
                    op & 0xFF00 == 0 && op & 255 < 24,
                    "Invalid emitted operation"
                );
                let builtin = op as u8;
                let value = inputs.get(&builtin).copied().unwrap_or(0x811C9DC5);
                let group = dictionary.u16(24 + usize::from(builtin) * 2)?;
                let mut mask = [0; 16];
                if group != u16::MAX {
                    let groups = dictionary.array(8, 32, Some(0x80808EE5))?;
                    let at = *groups.get(usize::from(group)).context("Dictionary group")?;
                    for row in dictionary.array(at + 16, 24, Some(0x80808EE8))? {
                        if dictionary.u32(row + 16)? == value {
                            mask = dictionary.bytes::<16>(row)?;
                        }
                    }
                }
                fn visit(
                    p: &Payload,
                    records: &[usize],
                    i: usize,
                    value: u32,
                    mask: &[u8; 16],
                    seen: &mut BTreeSet<usize>,
                ) -> Result<Option<i16>> {
                    ensure!(seen.insert(i), "Emitted record cycle");
                    let at = *records.get(i).context("Record extent")?;
                    let result = if p.u32(at + 16)? == value {
                        Some(short(p, at + 24)?)
                    } else {
                        let m = p.bytes::<16>(at)?;
                        let matches = m[15] & mask[15] & 128 != 0
                            && m.iter().zip(mask).all(|(a, b)| a & !b == 0);
                        let child = short(p, at + 20)?;
                        let alternate = short(p, at + 22)?;
                        ensure!(child >= -1 && alternate >= -1, "Invalid emitted child");
                        let refined = if matches && child >= 0 {
                            visit(p, records, child as usize, value, mask, seen)?
                        } else {
                            None
                        };
                        if matches && refined.is_none() {
                            Some(short(p, at + 24)?)
                        } else {
                            (if alternate >= 0 {
                                visit(p, records, alternate as usize, value, mask, seen)?
                            } else {
                                None
                            })
                            .or(refined)
                        }
                    };
                    seen.remove(&i);
                    Ok(result)
                }
                let root = (op >> 16) as usize;
                let result =
                    visit(states, &records, root, value, &mask, &mut BTreeSet::new())?.unwrap_or(
                        short(states, *records.get(root).context("Root extent")? + 24)?,
                    );
                if result < 0 {
                    break *choices.get((!result) as usize).context("Choice extent")?;
                }
                ensure!(result > 0, "Zero emitted result");
                operation = (result - 1) as usize;
            }
        }
        _ => anyhow::bail!("Unknown emitted node"),
    };
    states
        .array(selected, 8, Some(0x80803439))?
        .into_iter()
        .map(|at| Ok((states.u32(at)?, states.f32(at + 4)?)))
        .collect()
}

pub(super) fn verify(
    directory: &std::path::Path,
    animation: &Value,
    probes: &Value,
) -> Result<Value> {
    let probes = probes.as_array().context("Independent selector probes")?;
    ensure!(!probes.is_empty(), "Required selector probes are empty");
    let read = |name: &str| -> Result<Payload> {
        Ok(Payload(fs::read(directory.join(
            animation["files"][name].as_str().context("Selector file")?,
        ))?))
    };
    let states = read("states")?;
    let dictionary = read("parameters")?;
    let bank = read("converted_bank")?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let mut receipts = Vec::new();
    let mut variants = std::collections::BTreeMap::<u32, BTreeSet<String>>::new();
    for probe in probes {
        let inputs: std::collections::BTreeMap<u8, u32> =
            serde_json::from_value(probe["inputs"].clone())?;
        let action = u32::try_from(probe["action"].as_u64().context("Probe action")?)?;
        let actual = choice(&states, &dictionary, action, &inputs)?
            .into_iter()
            .map(|(d, w)| {
                let row = *descriptors
                    .get(d as usize)
                    .context("Selected descriptor extent")?;
                Ok((bank.u32(row + 16)?, w))
            })
            .collect::<Result<Vec<_>>>()?;
        let expected: Vec<(u32, f32)> = serde_json::from_value(probe["descriptor_names"].clone())?;
        ensure!(
            !expected.is_empty() && actual == expected,
            "Emitted selector differs from independent source route for {action:08X}"
        );
        variants
            .entry(action)
            .or_default()
            .insert(serde_json::to_string(&expected)?);
        receipts.push(json!({"action":action,"inputs":inputs,"descriptor_names":actual}));
    }
    ensure!(
        variants.values().any(|choices| choices.len() > 1),
        "Required selector probes do not exercise a changing action"
    );
    Ok(json!(receipts))
}
